//! The supply daemon: one outbound WebSocket to Motyga, jobs down, results up.
//!
//! The machine is behind a home router, so it dials out and keeps the link open. Everything it will ever be
//! asked to do arrives on that socket as a `job` frame, and a job is a model call — there is no frame that
//! runs a command, touches a file, or opens a listener. That is the security boundary of supply mode and it
//! is enforced simply: this match statement is the entire vocabulary.

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use futures::SinkExt;
use futures::StreamExt;
use serde_json::Value;
use serde_json::json;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

use std::collections::HashMap;

use crate::config::Lane;
use crate::config::SupplyConfig;
use crate::refresh::REFRESH_COOLDOWN;
use crate::refresh::Vendor;
use crate::vendor::ClaudeAdapter;
use crate::vendor::DeltaSink;
use crate::vendor::CodexAdapter;
use crate::vendor::Job;
use crate::vendor::VendorOutcome;

const PROTOCOL_VERSION: u32 = 1;
/// Reconnect backoff bounds. A supplier's link drops for ordinary reasons (sleep, wifi, ISP), so retry
/// briskly at first, but never hammer a backend that is down.
const RECONNECT_MIN: Duration = Duration::from_secs(2);
const RECONNECT_MAX: Duration = Duration::from_secs(120);

pub struct NodeRuntime {
    cfg: SupplyConfig,
    token: String,
    http: reqwest::Client,
    /// One permit per concurrent job the supplier allows on a lane. Held for the length of the vendor call,
    /// so the limit is a real ceiling rather than a number in a config file.
    slots: std::sync::Mutex<HashMap<String, std::sync::Arc<tokio::sync::Semaphore>>>,
    /// Last window utilisation each vendor reported, which is how this machine knows when the slice it
    /// agreed to give is gone. Reported by the vendor on its own responses — nothing we compute.
    window: std::sync::Mutex<HashMap<String, u8>>,
}

impl NodeRuntime {
    pub fn new(cfg: SupplyConfig, token: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(600))
            .build()
            .unwrap_or_default();
        Self { cfg, token, http, slots: Default::default(), window: Default::default() }
    }

    /// The permit pool for one lane, created on first use at the supplier's configured concurrency.
    fn slot(&self, vendor: &str, model: &str, max: u8) -> std::sync::Arc<tokio::sync::Semaphore> {
        let key = format!("{vendor}/{model}");
        // A poisoned lock means some other task panicked while holding it. The map behind it is a plain
        // counter table, not an invariant that a panic could have half-updated, so recovering the guard is
        // strictly better than bringing a working node down over it.
        let mut slots = self.slots.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        std::sync::Arc::clone(
            slots
                .entry(key)
                .or_insert_with(|| std::sync::Arc::new(tokio::sync::Semaphore::new(max.max(1) as usize))),
        )
    }

    /// Has this vendor's window passed the share the supplier offered? Unknown reads as NOT spent: a vendor
    /// that never reports utilisation must not silently take the machine offline.
    fn window_spent(&self, vendor: &str, share_pct: u8) -> bool {
        self.window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(vendor)
            .is_some_and(|used| *used >= share_pct)
    }

    fn note_window(&self, vendor: &str, used: Option<u8>) {
        if let Some(pct) = used {
            self.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(vendor.to_string(), pct);
        }
    }

    /// Connect, serve, and reconnect forever. Returns only on an unrecoverable configuration problem —
    /// a network failure is never one of those.
    pub async fn run_forever(self: &std::sync::Arc<Self>) -> Result<()> {
        let mut backoff = RECONNECT_MIN;
        loop {
            match self.serve_once().await {
                Ok(()) => {
                    tracing::info!("supply: link closed by the server, reconnecting");
                    backoff = RECONNECT_MIN;
                }
                Err(err) if is_auth_failure(&err) => {
                    // A revoked, expired or simply wrong node token is not a network hiccup, and retrying
                    // it forever leaves the supplier watching a silent backoff loop while nothing works.
                    // This is exactly the "unrecoverable configuration problem" this function documents.
                    return Err(anyhow!(
                        "this machine is no longer authorised ({err:#}).\n\
                         Run `motyga supply login` to connect it again."));
                }
                Err(err) => {
                    tracing::warn!("supply: link error: {err:#}");
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(RECONNECT_MAX);
        }
    }

    async fn serve_once(self: &std::sync::Arc<Self>) -> Result<()> {
        let ws_url = ws_url(&self.cfg.base());
        let request = tokio_tungstenite::tungstenite::client::IntoClientRequest::into_client_request(
            ws_url.as_str(),
        )?;
        let mut request = request;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", self.token)
                .parse()
                .map_err(|_| anyhow!("the stored node token is not a valid header value"))?,
        );

        let (stream, _resp) = tokio_tungstenite::connect_async(request)
            .await
            .with_context(|| format!("connecting to {ws_url}"))?;
        let (mut tx, mut rx) = stream.split();

        tx.send(Message::Text(self.hello().to_string().into())).await?;
        tracing::info!("supply: connected to {}", self.cfg.base());

        let mut heartbeat_every = Duration::from_secs(15);
        let mut next_beat = Instant::now() + heartbeat_every;
        let mut refresher = TokenRefresher::new(&self.cfg);
        // What this machine is currently serving. Starts at the local config and is narrowed by whatever
        // the server pushes down.
        let mut effective: Vec<Lane> = self.cfg.enabled_lanes().into_iter().cloned().collect();

        // EVERY outgoing frame goes through this channel, so the socket keeps exactly one writer while any
        // number of jobs stream concurrently. Without it a job could not emit a chunk without owning `tx`.
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
        let mut running: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();

        loop {
            running.retain(|_, h| !h.is_finished());
            tokio::select! {
                Some(frame) = out_rx.recv() => {
                    tx.send(Message::Text(frame.to_string().into())).await?;
                }
                _ = tokio::time::sleep_until(next_beat) => {
                    // Spawned, never awaited here: a refresh runs a vendor CLI for up to two minutes, and
                    // blocking this arm would stop the heartbeat and make the server drop a healthy node.
                    refresher.tick();
                    tx.send(Message::Text(json!({"type": "heartbeat"}).to_string().into())).await?;
                    next_beat = Instant::now() + heartbeat_every;
                }
                incoming = rx.next() => {
                    let Some(msg) = incoming else { return Ok(()) };
                    let msg = msg?;
                    let text = match msg {
                        Message::Text(t) => t.to_string(),
                        Message::Ping(_) | Message::Pong(_) => continue,
                        Message::Close(_) => return Ok(()),
                        _ => continue,
                    };
                    let Ok(frame) = serde_json::from_str::<Value>(&text) else { continue };
                    match frame.get("type").and_then(Value::as_str) {
                        Some("hello_ok") => {
                            if let Some(secs) = frame.get("heartbeat_sec").and_then(Value::as_u64) {
                                heartbeat_every = Duration::from_secs(secs.clamp(5, 120));
                                next_beat = Instant::now() + heartbeat_every;
                            }
                            report_hello_ok(&frame);
                        }
                        Some("job") => {
                            // Detached for real this time. A completion can run for minutes; awaiting it in
                            // this arm would starve the heartbeat and look to the server exactly like a
                            // machine that went to sleep — which is what an earlier version of this loop did.
                            // A job is addressed by its id, so an unusable one has to be refused rather than
                            // normalised. An empty id (the old `unwrap_or("")`) made every such job share a
                            // single map slot; a REPEATED id overwrote the previous JoinHandle without
                            // aborting it, so two vendor calls ran on under one id, their chunks interleaved
                            // on the wire, and a later `cancel` reached only the newer one.
                            let job_id = frame.get("job_id").and_then(Value::as_str).unwrap_or("").to_string();
                            if job_id.is_empty() || job_id.len() > 64 {
                                tracing::warn!("supply: dropping a job with an unusable id");
                                continue;
                            }
                            if running.contains_key(&job_id) {
                                tracing::warn!("supply: dropping a duplicate job id {job_id}");
                                let _ = out_tx.send(json!({
                                    "type": "fail", "job_id": job_id,
                                    "code": "duplicate_job_id", "retryable": false,
                                }));
                                continue;
                            }
                            let me = std::sync::Arc::clone(self);
                            let sender = out_tx.clone();
                            let lanes = effective.clone();
                            let id = job_id.clone();
                            running.insert(job_id, tokio::spawn(async move {
                                me.stream_job(frame, lanes, sender, id).await;
                            }));
                        }
                        Some("config") => {
                            // The supplier changed a setting on the website. Applied as a CEILING on what
                            // this machine already agreed to give, never as a floor: the server may narrow
                            // an offer, and may not widen one past the local config. It is their computer.
                            apply_server_config(&mut effective, &self.cfg, frame.get("lanes"));
                        }
                        Some("cancel") => {
                            // The buyer went away. Stop now — that quota is what we pay the supplier for,
                            // and spending it on an answer nobody will read is pure loss to them.
                            if let Some(id) = frame.get("job_id").and_then(Value::as_str)
                                && let Some(handle) = running.remove(id)
                            {
                                handle.abort();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Run one job, forwarding text to the socket as the vendor produces it, then a terminal frame.
    async fn stream_job(&self, frame: Value, effective: Vec<Lane>,
                        out: tokio::sync::mpsc::UnboundedSender<Value>, job_id: String) {
        let (delta_tx, mut delta_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let pump = {
            let out = out.clone();
            let job_id = job_id.clone();
            tokio::spawn(async move {
                let mut seq = 0u64;
                while let Some(text) = delta_rx.recv().await {
                    seq += 1;
                    let _ = out.send(json!({
                        "type": "chunk", "job_id": job_id, "seq": seq, "delta": text,
                    }));
                }
            })
        };

        let vendor = frame.get("vendor").and_then(Value::as_str).unwrap_or("").to_string();
        let result = self.run_job(&frame, &effective, &DeltaSink::new(delta_tx)).await;
        // Drain the pump BEFORE the terminal frame: the server treats `done` as the end of the answer, so a
        // chunk arriving after it would be dropped and the buyer would silently lose the tail.
        let _ = pump.await;

        let _ = out.send(match result {
            Ok(outcome) => {
                // The vendor just told us how much of its window is gone. That reading is what closes the
                // lane once the supplier's share is spent, so it is recorded before the frame goes out.
                self.note_window(&vendor, outcome.window_used_pct);
                done_frame(&job_id, outcome)
            }
            Err(err) => fail_frame(&job_id, &err),
        });
    }

    fn hello(&self) -> Value {
        let lanes: Vec<Value> = self
            .cfg
            .enabled_lanes()
            .iter()
            .map(|l| {
                json!({
                    "vendor": l.vendor,
                    "model": l.model,
                    "share_pct": l.share_pct,
                    "max_concurrency": l.max_concurrency,
                })
            })
            .collect();
        json!({
            "type": "hello",
            "protocol_version": PROTOCOL_VERSION,
            "cli_version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "lanes": lanes,
        })
    }

    async fn run_job(&self, frame: &Value, effective: &[Lane],
                     deltas: &DeltaSink) -> Result<VendorOutcome> {
        let vendor = frame
            .get("vendor")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("job without a vendor"))?
            .to_string();
        let model = frame
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("job without a model"))?
            .to_string();
        let lane_model = frame.get("lane_model").and_then(Value::as_str);
        let payload = frame.get("payload").cloned().unwrap_or(Value::Null);

        // Refuse a vendor this machine did not offer. The server already filters, but a node should not
        // rely on the other end for what it is willing to spend its own subscription on.
        let Some(lane) = effective
            .iter()
            .find(|l| l.enabled && l.vendor == vendor && l.authorises(&model, lane_model))
        else {
            return Err(anyhow!("this node does not offer {vendor}/{model}"));
        };

        // The two limits this CLI promises in its own `--help`, enforced HERE rather than trusted to the
        // other end. The server does apply both, and that is not the point: they are promises made to the
        // person whose subscription and whose machine this is, so the machine has to keep them itself. A
        // backend bug, a stale registry or a replayed frame must not be able to spend past what they agreed
        // to give.
        if self.window_spent(&vendor, lane.share_pct) {
            return Err(anyhow!(
                "the {}% of the {vendor} window this machine offers is already spent", lane.share_pct));
        }
        let _slot = self
            .slot(&vendor, &lane.model, lane.max_concurrency)
            .try_acquire_owned()
            .map_err(|_| anyhow!("this node is already serving {} {vendor} job(s)", lane.max_concurrency))?;

        let job = Job { vendor: vendor.clone(), model, payload };
        match vendor.as_str() {
            "claude" => ClaudeAdapter::load()?.run(&self.http, &job, deltas).await,
            "codex" => CodexAdapter::load()?.run(&self.http, &job, deltas).await,
            other => Err(anyhow!("unsupported vendor {other}")),
        }
    }
}

impl crate::config::Lane {
    /// Does this lane authorise the incoming job?
    ///
    /// `lane_model` is Motyga's own id for the model and is what this machine's config is written in, so
    /// when the server sends it the comparison is EXACT. That is the whole point of the field: the vendor's
    /// id often differs from ours (our `claude-opus-4` is the vendor's `claude-opus-4-8`), and with only the
    /// vendor id to go on this had to fall back to a shared PREFIX — under which a lane for
    /// `claude-opus-4` silently accepted a `claude-opus-4-5` job and spent the supplier's quota on a model
    /// they never enabled.
    ///
    /// The prefix path survives ONLY for a server too old to send `lane_model`, and is deliberately
    /// asymmetric now: the incoming id may be a longer, dated snapshot of what we enabled
    /// (`claude-opus-4` -> `claude-opus-4-8`), never the other way round. `gpt-5` must not match a lane
    /// for `gpt-50`, so the boundary after the prefix has to be a separator rather than any character.
    fn authorises(&self, vendor_model: &str, lane_model: Option<&str>) -> bool {
        if let Some(canonical) = lane_model {
            return self.model == canonical;
        }
        if self.model == vendor_model {
            return true;
        }
        vendor_model
            .strip_prefix(&self.model)
            .is_some_and(|rest| rest.starts_with('-') || rest.starts_with('.'))
    }
}

/// Keeps each offered vendor's credential from lapsing, by letting that vendor's own CLI rotate it.
///
/// Only vendors this machine actually offers are touched — a node selling Codex has no business spawning
/// Claude Code. The cooldown is what stops a broken or signed-out CLI from becoming a spawn loop on
/// somebody's desktop.
struct TokenRefresher {
    vendors: Vec<Vendor>,
    last_attempt: Vec<Option<Instant>>,
}

impl TokenRefresher {
    fn new(cfg: &SupplyConfig) -> Self {
        let mut vendors: Vec<Vendor> = cfg
            .enabled_lanes()
            .iter()
            .filter_map(|l| Vendor::parse(&l.vendor))
            .collect();
        vendors.sort_by_key(|v| v.as_str());
        vendors.dedup();
        let last_attempt = vec![None; vendors.len()];
        Self { vendors, last_attempt }
    }

    /// Decide synchronously, refresh in the background.
    ///
    /// Called from the heartbeat arm of the socket loop, so it must return immediately: a refresh runs a
    /// vendor CLI for up to two minutes, and blocking here would stop the heartbeat and get a perfectly
    /// healthy node dropped by the server.
    fn tick(&mut self) {
        for (i, vendor) in self.vendors.iter().copied().enumerate() {
            if !vendor.needs_refresh() {
                continue;
            }
            if let Some(prev) = self.last_attempt[i]
                && prev.elapsed() < REFRESH_COOLDOWN
            {
                continue;
            }
            self.last_attempt[i] = Some(Instant::now());
            tracing::info!("supply: {} credential is near expiry, asking its CLI to refresh", vendor.as_str());
            tokio::spawn(async move {
                match vendor.refresh().await {
                    Ok(()) => tracing::info!("supply: {} credential refreshed", vendor.as_str()),
                    // Not fatal: the lane keeps serving until the token actually lapses, and the adapter
                    // then fails with a specific "sign in again" message rather than a bare 401.
                    Err(err) => tracing::warn!("supply: could not refresh {}: {err:#}", vendor.as_str()),
                }
            });
        }
    }
}

/// Narrow the machine's offers to what the server just asked for.
///
/// Two rules, and the asymmetry is deliberate. The server may LOWER a share or switch a lane off — that is
/// how a supplier changes their mind from the website, and how we stop selling a model without waiting for
/// anyone to restart anything. The server may NOT raise a share above what the local config says, and may
/// not introduce a lane the machine never offered: this is somebody's home computer, and the last word on
/// how much of their subscription is spent belongs to the file on their disk.
fn apply_server_config(effective: &mut Vec<Lane>, local: &SupplyConfig, pushed: Option<&Value>) {
    let Some(items) = pushed.and_then(Value::as_array) else { return };
    let mut next: Vec<Lane> = Vec::new();
    for item in items {
        let (Some(vendor), Some(model)) = (
            item.get("vendor").and_then(Value::as_str),
            item.get("model").and_then(Value::as_str),
        ) else {
            continue;
        };
        // EXACT, unlike a job frame. A pushed config names models in MOTYGA's ids — the same ones this
        // machine's own config is written in — so there is nothing to translate here, and matching by
        // prefix would let a push for one model quietly re-configure a neighbouring lane.
        let Some(base) = local
            .enabled_lanes()
            .into_iter()
            .find(|l| l.vendor == vendor && l.model == model)
        else {
            continue; // not offered locally -> the server cannot conjure it
        };
        let share = item
            .get("share_pct")
            .and_then(Value::as_u64)
            .map(|v| v.min(100) as u8)
            .unwrap_or(base.share_pct)
            .min(base.share_pct);
        let concurrency = item
            .get("max_concurrency")
            .and_then(Value::as_u64)
            .map(|v| v.clamp(1, 8) as u8)
            .unwrap_or(base.max_concurrency)
            .min(base.max_concurrency);
        next.push(Lane {
            vendor: vendor.to_string(),
            model: base.model.clone(),
            share_pct: share,
            max_concurrency: concurrency,
            enabled: true,
        });
    }
    tracing::info!("supply: server config applied — {} lane(s) active", next.len());
    *effective = next;
}

fn ws_url(base: &str) -> String {
    let scheme = if base.starts_with("http://") { "ws://" } else { "wss://" };
    let host = base
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    format!("{scheme}{host}/v1/fleet/ws")
}

/// The terminal frame. Carries NO text: every character already went out as `chunk` frames, and the
/// server bills on what it relayed rather than on anything asserted here.
fn done_frame(job_id: &str, outcome: VendorOutcome) -> Value {
    json!({
        "type": "done",
        "job_id": job_id,
        "usage": outcome.usage,
        "finish_reason": outcome.finish_reason,
        "tool_calls": outcome.tool_calls,
        // Evidence the answer came from a real vendor, not from something cheap running locally.
        "upstream_id": outcome.upstream_id,
        "window_used_pct": outcome.window_used_pct,
    })
}

/// Did the handshake fail because this token is not accepted, as opposed to the link being down?
///
/// tungstenite reports a rejected upgrade as an Http response rather than a typed error, so the status is
/// read off that. Anything it cannot classify counts as transient — treating an unknown failure as fatal
/// would take a working node offline for a reason nobody could see.
fn is_auth_failure(err: &anyhow::Error) -> bool {
    use tokio_tungstenite::tungstenite::Error as WsError;
    for cause in err.chain() {
        if let Some(WsError::Http(resp)) = cause.downcast_ref::<WsError>() {
            return matches!(resp.status().as_u16(), 401 | 403);
        }
    }
    false
}

fn fail_frame(job_id: &str, err: &anyhow::Error) -> Value {
    json!({
        "type": "fail",
        "job_id": job_id,
        "code": format!("{err:#}").chars().take(200).collect::<String>(),
        "retryable": true,
    })
}

fn report_hello_ok(frame: &Value) {
    let accepted = frame
        .get("lanes")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    let rejected: Vec<&str> = frame
        .get("rejected")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    tracing::info!("supply: {accepted} lane(s) accepted");
    if !rejected.is_empty() {
        // Say it out loud. A silently dropped lane looks to the supplier like a model that simply never
        // sells, and they have no way to find out why.
        tracing::warn!("supply: not accepted (not sold by Motyga): {}", rejected.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Lane;

    #[test]
    fn ws_url_follows_the_base_scheme() {
        assert_eq!(ws_url("https://motyga.com"), "wss://motyga.com/v1/fleet/ws");
        assert_eq!(ws_url("http://localhost:8080"), "ws://localhost:8080/v1/fleet/ws");
    }

    #[test]
    fn lane_accepts_a_dated_vendor_snapshot_id() {
        let lane = Lane {
            vendor: "claude".into(),
            model: "claude-opus-4-7".into(),
            share_pct: 100,
            max_concurrency: 1,
            enabled: true,
        };
        // Old server: no canonical id, so a dated snapshot of the SAME model still resolves.
        assert!(lane.authorises("claude-opus-4-7", None));
        assert!(lane.authorises("claude-opus-4-7-20260115", None));
        assert!(!lane.authorises("claude-sonnet-4-5", None));
    }

    #[test]
    fn a_lane_never_authorises_a_neighbouring_model() {
        // The footgun the canonical id exists to close: `claude-opus-4` used to accept `claude-opus-4-5`
        // through a shared prefix, spending the supplier's quota on a model they never enabled.
        let opus4 = lane("claude", "claude-opus-4", 100, 1);
        assert!(!opus4.authorises("claude-opus-4-5", Some("claude-opus-4-5")));
        assert!(opus4.authorises("claude-opus-4-8", Some("claude-opus-4")), "our id, vendor's snapshot");
        // ...and even without a canonical id, the prefix must end on a separator rather than mid-token.
        let five = lane("codex", "gpt-5", 100, 1);
        assert!(!five.authorises("gpt-50", None));
        assert!(five.authorises("gpt-5.6-sol", None));
    }

    #[test]
    fn the_canonical_id_is_authoritative_when_present() {
        // A server that sends lane_model has already resolved the mapping; the vendor id is then just what
        // we forward upstream, and must not widen what this machine agreed to serve.
        let lane = lane("claude", "claude-opus-4-7", 100, 1);
        assert!(lane.authorises("anything-at-all", Some("claude-opus-4-7")));
        assert!(!lane.authorises("claude-opus-4-7", Some("claude-haiku-4-5")));
    }

    fn lane(vendor: &str, model: &str, share: u8, conc: u8) -> Lane {
        Lane { vendor: vendor.into(), model: model.into(), share_pct: share,
               max_concurrency: conc, enabled: true }
    }

    #[test]
    fn server_may_narrow_an_offer() {
        let local = SupplyConfig { lanes: vec![lane("claude", "claude-opus-4-7", 80, 4)], ..Default::default() };
        let mut effective = local.enabled_lanes().into_iter().cloned().collect::<Vec<_>>();
        let pushed = serde_json::json!([
            {"vendor": "claude", "model": "claude-opus-4-7", "share_pct": 30, "max_concurrency": 1}
        ]);
        apply_server_config(&mut effective, &local, Some(&pushed));
        assert_eq!(effective.len(), 1);
        assert_eq!(effective[0].share_pct, 30);
        assert_eq!(effective[0].max_concurrency, 1);
    }

    #[test]
    fn server_may_not_widen_past_the_local_ceiling() {
        // This is somebody's home machine: the last word on how much of their subscription gets spent is
        // the file on their disk, not a frame from us.
        let local = SupplyConfig { lanes: vec![lane("claude", "claude-opus-4-7", 25, 1)], ..Default::default() };
        let mut effective = local.enabled_lanes().into_iter().cloned().collect::<Vec<_>>();
        let pushed = serde_json::json!([
            {"vendor": "claude", "model": "claude-opus-4-7", "share_pct": 100, "max_concurrency": 8}
        ]);
        apply_server_config(&mut effective, &local, Some(&pushed));
        assert_eq!(effective[0].share_pct, 25);
        assert_eq!(effective[0].max_concurrency, 1);
    }

    #[test]
    fn server_cannot_invent_a_lane_the_machine_never_offered() {
        let local = SupplyConfig { lanes: vec![lane("claude", "claude-opus-4-7", 50, 1)], ..Default::default() };
        let mut effective = local.enabled_lanes().into_iter().cloned().collect::<Vec<_>>();
        let pushed = serde_json::json!([{"vendor": "codex", "model": "gpt-5.6", "share_pct": 100}]);
        apply_server_config(&mut effective, &local, Some(&pushed));
        assert!(effective.is_empty());
    }

    #[test]
    fn a_lane_switched_off_server_side_stops_serving() {
        let local = SupplyConfig { lanes: vec![lane("claude", "claude-opus-4-7", 50, 1)], ..Default::default() };
        let mut effective = local.enabled_lanes().into_iter().cloned().collect::<Vec<_>>();
        apply_server_config(&mut effective, &local, Some(&serde_json::json!([])));
        assert!(effective.is_empty());
    }

    #[test]
    fn fail_frame_is_bounded() {
        let err = anyhow!("x".repeat(1000));
        let frame = fail_frame("j1", &err);
        assert!(frame["code"].as_str().unwrap().len() <= 200);
    }
}
