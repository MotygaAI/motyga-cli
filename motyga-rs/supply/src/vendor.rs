//! Vendor adapters — how a node turns one job into a call on the supplier's own subscription.
//!
//! Each adapter reads the credentials the vendor's OWN CLI already stored on this machine and calls that
//! vendor's API with them. Nothing here ever sends a credential to Motyga: the whole point of running the
//! node locally is that the subscription token stays on the supplier's disk, on their IP, on their device.
//!
//! VERIFIED 2026-07-31 against a live Claude Max 20x and a live ChatGPT Pro install. Everything below —
//! credential paths, endpoints, required parameters, quota headers, model ids — was probed, not inferred.
//! Three of these are counter-intuitive enough to be worth stating up front, because a future edit that
//! "simplifies" any of them will produce a 400 that looks like a bug in our code:
//!   • the Codex surface REQUIRES `stream: true` and `store: false`, and REJECTS `max_output_tokens`;
//!   • it serves the GPT-5.6 family and refuses every `*-codex` model id on a ChatGPT account;
//!   • Anthropic reports quota as a FRACTION (0.63), OpenAI as a PERCENT (31).
//! Vendors change these between releases and document none of them, so re-probe after an upgrade. Parsing
//! stays tolerant and failures stay specific, so a drift reads as "re-check the auth file", not a dead lane.

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde_json::Value;

/// What a job asks for, already normalised by the backend.
#[derive(Debug, Clone)]
pub struct Job {
    pub vendor: String,
    /// The id to send to the VENDOR (the backend resolved our logical id for us).
    pub model: String,
    pub payload: Value,
}

/// What one vendor call produced. `usage` is passed back for telemetry only — the backend counts the
/// tokens it actually relayed and bills on that, so nothing here can move money.
#[derive(Debug, Default)]
pub struct VendorOutcome {
    pub usage: Option<Value>,
    pub finish_reason: Option<String>,
    /// The id the VENDOR minted for this response (Anthropic `msg_…`, OpenAI `resp_…`). Sent back as
    /// evidence that a real vendor produced the answer — see FleetEarning.upstream_id.
    pub upstream_id: Option<String>,
    /// Tool calls the model asked for, always in the OpenAI shape so the backend relays ONE format no
    /// matter which vendor produced them.
    pub tool_calls: Option<Vec<Value>>,
    /// Percentage of the vendor's rate-limit window consumed, if the response told us. Drives the lane's
    /// own stop line so we never spend past what the supplier agreed to sell.
    pub window_used_pct: Option<u8>,
}

/// Where an adapter pushes text as the vendor produces it.
///
/// A plain unbounded sender rather than a trait: the only consumer is the socket writer, and a closed
/// channel simply means the buyer went away — which the adapter should ignore and keep draining, because
/// abandoning the vendor stream mid-answer is what leaves a half-charged request behind.
#[derive(Clone)]
pub struct DeltaSink(tokio::sync::mpsc::UnboundedSender<String>);

impl DeltaSink {
    pub fn new(tx: tokio::sync::mpsc::UnboundedSender<String>) -> Self {
        Self(tx)
    }

    pub fn send(&self, text: &str) {
        let _ = self.0.send(text.to_string());
    }
}

/// Minimal SSE reader over a streaming response body.
///
/// Both vendors speak `data:`-prefixed JSON lines with blank-line separators; neither uses multi-line data
/// payloads, so line-at-a-time is enough and avoids pulling in an SSE crate for twenty lines of parsing.
struct SseReader {
    stream: std::pin::Pin<Box<dyn futures::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>,
    buf: String,
}

impl SseReader {
    fn new(resp: reqwest::Response) -> Self {
        Self { stream: Box::pin(resp.bytes_stream()), buf: String::new() }
    }

    /// The next parsed `data:` payload, or None at end of stream. `[DONE]` and unparseable lines are
    /// skipped rather than raised: a stray keep-alive must not fail an answer that is otherwise fine.
    async fn next_event(&mut self) -> Result<Option<Value>> {
        use futures::StreamExt;
        loop {
            while let Some(idx) = self.buf.find('\n') {
                let line = self.buf[..idx].trim().to_string();
                self.buf.drain(..=idx);
                let Some(raw) = line.strip_prefix("data:") else { continue };
                let raw = raw.trim();
                if raw.is_empty() || raw == "[DONE]" {
                    continue;
                }
                if let Ok(v) = serde_json::from_str::<Value>(raw) {
                    return Ok(Some(v));
                }
            }
            match self.stream.next().await {
                Some(chunk) => {
                    let bytes = chunk.context("reading the vendor stream")?;
                    self.buf.push_str(&String::from_utf8_lossy(&bytes));
                }
                None => return Ok(None),
            }
        }
    }
}

/// (name, description, json-schema) out of a tool declaration in ANY of the three shapes a buyer can send.
///
/// Buyers reach Motyga through three APIs and each spells a tool differently — OpenAI chat nests it under
/// `function`, the Responses API flattens it, Anthropic calls the schema `input_schema`. The backend passes
/// the caller's tools through untouched, so normalising is the node's job. Reading all three and emitting
/// the vendor's own is what makes tool use work regardless of which door the buyer came in.
fn tool_parts(t: &Value) -> Option<(String, Option<String>, Value)> {
    let f = t.get("function").unwrap_or(t);
    let name = f.get("name").and_then(Value::as_str)?.to_string();
    let desc = f.get("description").and_then(Value::as_str).map(str::to_string);
    let schema = f
        .get("parameters")
        .or_else(|| f.get("input_schema"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"type": "object", "properties": {}}));
    Some((name, desc, schema))
}

fn anthropic_tools(tools: &Value) -> Vec<Value> {
    tools
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(tool_parts)
                .map(|(name, desc, schema)| {
                    let mut t = serde_json::json!({"name": name, "input_schema": schema});
                    if let Some(d) = desc {
                        t["description"] = Value::String(d);
                    }
                    t
                })
                .collect()
        })
        .unwrap_or_default()
}

fn responses_tools(tools: &Value) -> Vec<Value> {
    tools
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(tool_parts)
                .map(|(name, desc, schema)| {
                    let mut t = serde_json::json!({"type": "function", "name": name, "parameters": schema});
                    if let Some(d) = desc {
                        t["description"] = Value::String(d);
                    }
                    t
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Anthropic's tool_choice vocabulary. Anything we cannot map confidently is DROPPED rather than guessed:
/// a wrong choice silently changes whether the model may call a tool at all, which is worse than the
/// vendor's default.
fn anthropic_tool_choice(v: &Value) -> Option<Value> {
    match v.as_str() {
        Some("auto") => Some(serde_json::json!({"type": "auto"})),
        Some("required") | Some("any") => Some(serde_json::json!({"type": "any"})),
        Some("none") => Some(serde_json::json!({"type": "none"})),
        _ => {
            let name = v
                .get("function")
                .and_then(|f| f.get("name"))
                .or_else(|| v.get("name"))
                .and_then(Value::as_str)?;
            Some(serde_json::json!({"type": "tool", "name": name}))
        }
    }
}

/// One tool call, in the OpenAI shape the backend expects on the wire regardless of vendor.
fn openai_tool_call(id: &str, name: &str, arguments: &str) -> Value {
    serde_json::json!({
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": arguments },
    })
}

fn home() -> Result<PathBuf> {
    dirs::home_dir().context("cannot determine the home directory")
}

fn read_json(path: &PathBuf) -> Result<Value> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {} — is that vendor's CLI signed in?", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("{} is not valid JSON", path.display()))
}

/// Pull the first present string at any of `paths` (dotted lookups). Vendors rename these fields between
/// releases, so accept the known spellings rather than pinning one and dying on the next update.
fn first_str(root: &Value, paths: &[&str]) -> Option<String> {
    for path in paths {
        let mut cur = root;
        let mut ok = true;
        for seg in path.split('.') {
            match cur.get(seg) {
                Some(next) => cur = next,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok && let Some(s) = cur.as_str() && !s.is_empty() {
            return Some(s.to_string());
        }
    }
    None
}

// --------------------------------------------------------------------------- Claude

pub struct ClaudeAdapter {
    token: String,
    /// Unix millis at which the access token dies. Verified TTL is ~1 HOUR, which is short enough that a
    /// long-running node must re-read the file per job rather than cache a token at startup.
    expires_at_ms: Option<i64>,
    pub plan: Option<String>,
}

impl ClaudeAdapter {
    /// Reads the OAuth credential Claude Code stores after `claude login`.
    ///
    /// Deliberately called fresh for EVERY job, never cached: the access token lasts about an hour, and
    /// Claude Code rewrites this file when it refreshes. Re-reading is what lets a node keep serving across
    /// a refresh without a restart — and what makes the expiry check below meaningful.
    pub fn load() -> Result<Self> {
        let path = home()?.join(".claude").join(".credentials.json");
        let root = read_json(&path)?;
        let token = first_str(
            &root,
            &[
                "claudeAiOauth.accessToken",
                "claudeAiOauth.access_token",
                "accessToken",
                "access_token",
            ],
        )
        .ok_or_else(|| {
            anyhow!(
                "no access token found in {} — run `claude login`, and if you just updated Claude Code, \
                 re-check the credential layout (see supply/src/vendor.rs)",
                path.display()
            )
        })?;
        let expires_at_ms = root
            .get("claudeAiOauth")
            .and_then(|o| o.get("expiresAt"))
            .and_then(Value::as_i64);
        let plan = first_str(&root, &["claudeAiOauth.rateLimitTier", "claudeAiOauth.subscriptionType"]);
        Ok(Self { token, expires_at_ms, plan })
    }

    /// Milliseconds until the access token expires, if we can tell.
    pub fn expires_in_ms(&self) -> Option<i64> {
        let exp = self.expires_at_ms?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_millis() as i64;
        Some(exp - now)
    }

    fn ensure_fresh(&self) -> Result<()> {
        // Fail with the fix rather than letting the vendor answer 401 and reporting that as a generic
        // error. On a machine where nobody runs Claude Code interactively this is THE failure mode, and
        // "run `claude` once to refresh" is the whole remedy.
        if let Some(left) = self.expires_in_ms()
            && left <= 0
        {
            return Err(anyhow!(
                "the Claude access token on this machine has expired — run `claude` once to refresh it \
                 (Claude Code rewrites ~/.claude/.credentials.json; the token lasts about an hour)"
            ));
        }
        Ok(())
    }

    /// Stream one job, emitting each text delta through `deltas` as it arrives.
    ///
    /// VERIFIED event shape (live Max 20x, 2026-07-31): `message_start` carries the `msg_…` id and the
    /// input usage, `content_block_delta` carries `delta.text`, `message_delta` carries the final output
    /// usage, and `ping` events are interleaved and must be ignored rather than treated as junk.
    pub async fn run(&self, client: &reqwest::Client, job: &Job,
                     deltas: &DeltaSink) -> Result<VendorOutcome> {
        self.ensure_fresh()?;
        let payload = &job.payload;
        let mut body = serde_json::json!({
            "model": job.model,
            "max_tokens": payload.get("max_tokens").and_then(Value::as_u64).unwrap_or(2048),
            "messages": payload.get("messages").cloned().unwrap_or(Value::Array(vec![])),
            "stream": true,
        });
        for key in ["system", "temperature", "top_p"] {
            if let Some(v) = payload.get(key) {
                body[key] = v.clone();
            }
        }
        // Anthropic calls it `stop_sequences`, not `stop`. Forwarding the OpenAI name would be silently
        // ignored and the buyer's stop condition would simply never fire.
        if let Some(v) = payload.get("stop") {
            body["stop_sequences"] = match v {
                Value::String(s) => Value::Array(vec![Value::String(s.clone())]),
                other => other.clone(),
            };
        }
        if let Some(tools) = payload.get("tools") {
            let mapped = anthropic_tools(tools);
            if !mapped.is_empty() {
                body["tools"] = Value::Array(mapped);
                if let Some(choice) = payload.get("tool_choice").and_then(anthropic_tool_choice) {
                    body["tool_choice"] = choice;
                }
            }
        }
        // `reasoning_effort` is deliberately NOT forwarded: Anthropic has no such parameter (its knob is
        // `thinking`), and sending it would 400 an otherwise valid request.

        let resp = client
            .post("https://api.anthropic.com/v1/messages")
            .bearer_auth(&self.token)
            .header("anthropic-version", "2023-06-01")
            // The subscription (OAuth) surface is gated behind this beta header; an API-key call does not
            // need it. If Anthropic renames it, the symptom is a 401 on an otherwise valid token.
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("Accept", "text/event-stream")
            .json(&body)
            .send()
            .await
            .context("calling the Anthropic API")?;

        // Anthropic reports utilization as a FRACTION of each window. Take the worst of the two — the 5-hour
        // burst window and the 7-day one — because whichever fills first is what actually stops this lane.
        let window = fraction_headers_to_pct(
            resp.headers(),
            &[
                "anthropic-ratelimit-unified-7d-utilization",
                "anthropic-ratelimit-unified-5h-utilization",
            ],
        );
        let status = resp.status();
        // A 429 here is "this model's window is spent", not "the supplier is broken". Saying so lets the
        // hub stop offering the lane until it resets, instead of the node failing every job it is handed
        // for the next several hours.
        if status.as_u16() == 429 {
            return Err(anyhow!("rate_limited: this subscription's window for {} is spent", job.model));
        }
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "anthropic {}: {}",
                status.as_u16(),
                detail.chars().take(200).collect::<String>()
            ));
        }

        let mut out = VendorOutcome { window_used_pct: window, ..Default::default() };
        let mut usage = serde_json::Map::new();
        // Tool calls arrive as a `tool_use` block whose arguments are streamed as JSON fragments across
        // several deltas, addressed by content-block index. Accumulate per index, assemble at the end.
        let mut tools_building: std::collections::BTreeMap<u64, (String, String, String)> =
            std::collections::BTreeMap::new();
        let mut sse = SseReader::new(resp);
        while let Some(ev) = sse.next_event().await? {
            let idx = ev.get("index").and_then(Value::as_u64).unwrap_or(0);
            match ev.get("type").and_then(Value::as_str) {
                Some("content_block_start") => {
                    let cb = ev.get("content_block");
                    if cb.and_then(|c| c.get("type")).and_then(Value::as_str) == Some("tool_use") {
                        let id = cb.and_then(|c| c.get("id")).and_then(Value::as_str).unwrap_or("");
                        let name = cb.and_then(|c| c.get("name")).and_then(Value::as_str).unwrap_or("");
                        tools_building.insert(idx, (id.to_string(), name.to_string(), String::new()));
                    }
                }
                Some("message_start") => {
                    let msg = ev.get("message");
                    out.upstream_id = msg
                        .and_then(|m| m.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    if let Some(Value::Object(u)) = msg.and_then(|m| m.get("usage")) {
                        usage.extend(u.clone());
                    }
                }
                Some("content_block_delta") => {
                    let d = ev.get("delta");
                    match d.and_then(|d| d.get("type")).and_then(Value::as_str) {
                        Some("text_delta") => {
                            if let Some(text) = d.and_then(|d| d.get("text")).and_then(Value::as_str)
                                && !text.is_empty()
                            {
                                deltas.send(text);
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(frag) = d.and_then(|d| d.get("partial_json")).and_then(Value::as_str)
                                && let Some(slot) = tools_building.get_mut(&idx)
                            {
                                slot.2.push_str(frag);
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_delta") => {
                    if let Some(Value::Object(u)) = ev.get("usage") {
                        usage.extend(u.clone());
                    }
                    if let Some(reason) = ev
                        .get("delta")
                        .and_then(|d| d.get("stop_reason"))
                        .and_then(Value::as_str)
                    {
                        out.finish_reason = Some(reason.to_string());
                    }
                }
                // `ping`, `content_block_start/stop`, `message_stop` carry nothing we bill or relay.
                _ => {}
            }
        }
        if !usage.is_empty() {
            out.usage = Some(Value::Object(usage));
        }
        if !tools_building.is_empty() {
            out.tool_calls = Some(
                tools_building
                    .values()
                    .map(|(id, name, args)| {
                        // An empty accumulator means a no-argument tool; "{}" is what every caller expects
                        // to parse, and "" would blow up the buyer's json.loads.
                        openai_tool_call(id, name, if args.is_empty() { "{}" } else { args })
                    })
                    .collect(),
            );
        }
        Ok(out)
    }
}

// --------------------------------------------------------------------------- Codex / ChatGPT

pub struct CodexAdapter {
    token: String,
    account_id: Option<String>,
}

impl CodexAdapter {
    /// Reads the ChatGPT OAuth credential the Codex CLI stores after `codex login`.
    pub fn load() -> Result<Self> {
        let path = home()?.join(".codex").join("auth.json");
        let root = read_json(&path)?;
        let token = first_str(
            &root,
            &["tokens.access_token", "access_token", "OPENAI_API_KEY"],
        )
        .ok_or_else(|| {
            anyhow!(
                "no access token found in {} — run `codex login` (a ChatGPT subscription login, not an \
                 API key)",
                path.display()
            )
        })?;
        let account_id = first_str(
            &root,
            &["tokens.account_id", "account_id", "tokens.chatgpt_account_id"],
        );
        Ok(Self { token, account_id })
    }

    /// Milliseconds until the access token expires, read from the JWT's own `exp` claim.
    ///
    /// Unlike Claude, the Codex credential file carries no expiry field — but the access token is a JWT, so
    /// it states its own deadline. Measured TTL is 240 hours, two orders of magnitude longer than Claude's,
    /// which is why a Codex node survives unattended for over a week. Returns None when the token is not a
    /// JWT (an API-key login), because then there is nothing to expire and nothing to refresh.
    pub fn expires_in_ms(&self) -> Option<i64> {
        let exp = jwt_exp_seconds(&self.token)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs() as i64;
        Some((exp - now) * 1000)
    }

    pub async fn run(&self, client: &reqwest::Client, job: &Job,
                     deltas: &DeltaSink) -> Result<VendorOutcome> {
        let payload = &job.payload;
        // The Responses shape: one `input` list rather than `messages`.
        let input = payload
            .get("messages")
            .and_then(Value::as_array)
            .map(|msgs| {
                msgs.iter()
                    .map(|m| {
                        serde_json::json!({
                            "role": m.get("role").and_then(Value::as_str).unwrap_or("user"),
                            "content": [{
                                "type": "input_text",
                                "text": message_text(m),
                            }],
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Three non-negotiables on this surface, each verified by a 400 when omitted or set otherwise:
        //   stream MUST be true, store MUST be false, and max_output_tokens is rejected outright
        //   ("Unsupported parameter"). The buyer's max_tokens therefore cannot be pushed down to the
        //   vendor here; it is enforced upstream in the request the backend accepted.
        let mut body = serde_json::json!({
            "model": job.model,
            "input": input,
            "stream": true,
            "store": false,
        });
        if let Some(v) = payload.get("reasoning_effort").and_then(Value::as_str) {
            body["reasoning"] = serde_json::json!({ "effort": v });
        }
        if let Some(v) = payload.get("system").and_then(Value::as_str) {
            body["instructions"] = Value::String(v.to_string());
        }
        if let Some(tools) = payload.get("tools") {
            let mapped = responses_tools(tools);
            if !mapped.is_empty() {
                body["tools"] = Value::Array(mapped);
                // The Responses vocabulary is the plain OpenAI one, so a string choice passes straight
                // through; a named-function choice is flattened out of the chat shape.
                if let Some(choice) = payload.get("tool_choice") {
                    body["tool_choice"] = match choice.as_str() {
                        Some(_) => choice.clone(),
                        None => choice
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(Value::as_str)
                            .map(|n| serde_json::json!({"type": "function", "name": n}))
                            .unwrap_or_else(|| choice.clone()),
                    };
                }
            }
        }
        // temperature / top_p / stop are NOT forwarded: this surface serves the GPT-5.x reasoning family,
        // which rejects sampling parameters outright. The catalog already reports them as un-honoured for
        // fleet routes (pricing.sampling_param_honored), so nothing upstream promises the buyer otherwise.

        let mut req = client
            .post("https://chatgpt.com/backend-api/codex/responses")
            .bearer_auth(&self.token)
            .header("originator", "codex_cli_rs")
            .header("Accept", "text/event-stream");
        if let Some(acc) = &self.account_id {
            req = req.header("chatgpt-account-id", acc.as_str());
        }

        let resp = req
            .json(&body)
            .send()
            .await
            .context("calling the ChatGPT Codex backend")?;
        // OpenAI reports quota as a whole PERCENT, and `primary` is the weekly window
        // (x-codex-primary-window-minutes = 10080).
        let window = percent_headers_to_pct(
            resp.headers(),
            &["x-codex-primary-used-percent", "x-codex-secondary-used-percent"],
        );
        let status = resp.status();
        if !status.is_success() {
            let detail = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "codex {}: {}",
                status.as_u16(),
                detail.chars().take(200).collect::<String>()
            ));
        }
        let mut out = VendorOutcome { window_used_pct: window, ..Default::default() };
        let mut sse = SseReader::new(resp);
        while let Some(ev) = sse.next_event().await? {
            match ev.get("type").and_then(Value::as_str) {
                Some("response.output_text.delta") => {
                    if let Some(d) = ev.get("delta").and_then(Value::as_str)
                        && !d.is_empty()
                    {
                        deltas.send(d);
                    }
                }
                Some("response.output_item.done") => {
                    // A completed function call arrives as its own output item, arguments already whole —
                    // no fragment accumulation needed on this surface, unlike Anthropic's.
                    let item = ev.get("item");
                    if item.and_then(|i| i.get("type")).and_then(Value::as_str) == Some("function_call") {
                        let id = item
                            .and_then(|i| i.get("call_id"))
                            .or_else(|| item.and_then(|i| i.get("id")))
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        let name = item.and_then(|i| i.get("name")).and_then(Value::as_str).unwrap_or("");
                        let args = item
                            .and_then(|i| i.get("arguments"))
                            .and_then(Value::as_str)
                            .unwrap_or("{}");
                        out.tool_calls
                            .get_or_insert_with(Vec::new)
                            .push(openai_tool_call(id, name, args));
                    }
                }
                Some("response.completed" | "response.incomplete") => {
                    let response = ev.get("response");
                    out.usage = response.and_then(|r| r.get("usage")).cloned();
                    out.upstream_id = response
                        .and_then(|r| r.get("id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    out.finish_reason = response
                        .and_then(|r| r.get("status"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
                _ => {}
            }
        }
        Ok(out)
    }
}


// --------------------------------------------------------------------------- shared helpers

fn message_text(message: &Value) -> String {
    match message.get("content") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| {
                p.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| p.as_str())
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}


/// How much of the vendor's window is gone, as a percentage — the WORST of the named headers, because
/// whichever window fills first is what actually stops this lane.
///
/// Absent or unparseable means "unknown", which leaves the lane's share gate untouched rather than guessing
/// a number that would either stop selling early or overrun the slice the supplier agreed to sell. The two
/// vendors report on different scales, hence two readers rather than one clever one: mixing a fraction into
/// a percent reader silently turns "63% spent" into "0% spent", and the supplier's cap stops working.
fn max_header_value(headers: &reqwest::header::HeaderMap, names: &[&str], scale: f64) -> Option<u8> {
    let mut worst: Option<f64> = None;
    for name in names {
        if let Some(v) = headers.get(*name).and_then(|h| h.to_str().ok())
            && let Ok(raw) = v.trim().parse::<f64>()
        {
            let pct = raw * scale;
            if (0.0..=100.0).contains(&pct) {
                worst = Some(worst.map_or(pct, |w: f64| w.max(pct)));
            }
        }
    }
    worst.map(|p| p.round() as u8)
}

/// Anthropic: `anthropic-ratelimit-unified-{5h,7d}-utilization`, a fraction in 0..1 (0.63 = 63% spent).
fn fraction_headers_to_pct(headers: &reqwest::header::HeaderMap, names: &[&str]) -> Option<u8> {
    max_header_value(headers, names, 100.0)
}

/// OpenAI: `x-codex-{primary,secondary}-used-percent`, already a whole percent (31 = 31% spent).
fn percent_headers_to_pct(headers: &reqwest::header::HeaderMap, names: &[&str]) -> Option<u8> {
    max_header_value(headers, names, 1.0)
}

/// The `exp` claim (seconds since epoch) of a JWT, without verifying the signature.
///
/// We are reading OUR OWN credential to find out when it dies, not authenticating anything, so there is no
/// signature to check — the issuer already vouched for it and the server will reject it if we are wrong.
/// Returns None for anything that is not a three-part JWT with a numeric `exp`.
fn jwt_exp_seconds(token: &str) -> Option<i64> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims.get("exp").and_then(Value::as_i64)
}

/// A short, non-leaky rendering of a vendor error body for the fail frame.
fn compact_error(value: &Value) -> String {
    let msg = first_str(value, &["error.message", "message", "detail"])
        .unwrap_or_else(|| value.to_string());
    msg.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_either_credential_spelling() {
        let nested = serde_json::json!({"claudeAiOauth": {"accessToken": "tok-a"}});
        assert_eq!(first_str(&nested, &["claudeAiOauth.accessToken"]).as_deref(), Some("tok-a"));
        let flat = serde_json::json!({"access_token": "tok-b"});
        assert_eq!(
            first_str(&flat, &["claudeAiOauth.accessToken", "access_token"]).as_deref(),
            Some("tok-b")
        );
    }

    #[test]
    fn empty_credential_is_not_a_credential() {
        let blank = serde_json::json!({"access_token": ""});
        assert!(first_str(&blank, &["access_token"]).is_none());
    }


    #[test]
    fn message_text_handles_string_and_part_list() {
        assert_eq!(message_text(&serde_json::json!({"content": "hi"})), "hi");
        assert_eq!(
            message_text(&serde_json::json!({"content": [{"type": "text", "text": "hi"}]})),
            "hi"
        );
    }

    #[test]
    fn openai_percent_headers_read_as_percent() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-codex-primary-used-percent", "31".parse().unwrap());
        headers.insert("x-codex-secondary-used-percent", "0".parse().unwrap());
        // Worst of the two windows: whichever fills first is what stops the lane.
        assert_eq!(
            percent_headers_to_pct(
                &headers,
                &["x-codex-primary-used-percent", "x-codex-secondary-used-percent"]
            ),
            Some(31)
        );
        assert_eq!(percent_headers_to_pct(&headers, &["missing"]), None);
    }

    #[test]
    fn anthropic_fraction_headers_read_as_fraction() {
        // The bug this pins: reading Anthropic's 0.63 with a percent reader yields 1%, and the supplier's
        // "sell half my window" cap silently never engages.
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("anthropic-ratelimit-unified-7d-utilization", "0.63".parse().unwrap());
        headers.insert("anthropic-ratelimit-unified-5h-utilization", "0.04".parse().unwrap());
        let names = [
            "anthropic-ratelimit-unified-7d-utilization",
            "anthropic-ratelimit-unified-5h-utilization",
        ];
        assert_eq!(fraction_headers_to_pct(&headers, &names), Some(63));
        assert_eq!(percent_headers_to_pct(&headers, &names), Some(1));
    }

    #[test]
    fn unparseable_quota_is_unknown_not_zero() {
        let mut bad = reqwest::header::HeaderMap::new();
        bad.insert("x-codex-primary-used-percent", "nonsense".parse().unwrap());
        assert_eq!(percent_headers_to_pct(&bad, &["x-codex-primary-used-percent"]), None);
    }

    #[tokio::test]
    async fn sse_reader_splits_events_across_chunk_boundaries() {
        // The real failure this guards: a vendor chunk that ends mid-line. Buffering by line rather than by
        // network chunk is the whole reason this reader exists.
        let body = "data: {\"a\":1}
data: {\"b\"";
        let rest = ":2}
data: [DONE]
";
        let mut r = SseReader { stream: Box::pin(futures::stream::empty()), buf: String::new() };
        r.buf.push_str(body);
        assert_eq!(r.next_event().await.unwrap().unwrap()["a"], 1);
        r.buf.push_str(rest);
        assert_eq!(r.next_event().await.unwrap().unwrap()["b"], 2);
        assert!(r.next_event().await.unwrap().is_none());
    }

    #[tokio::test]
    async fn sse_reader_skips_keepalives_and_junk() {
        let mut r = SseReader { stream: Box::pin(futures::stream::empty()), buf: String::new() };
        r.buf.push_str("garbage
: comment
data: not-json
data: [DONE]
data: {\"ok\":true}
");
        assert_eq!(r.next_event().await.unwrap().unwrap()["ok"], true);
    }

    #[test]
    fn tools_are_read_in_all_three_buyer_shapes() {
        // A buyer reaches us through /v1/chat/completions, /v1/responses or /v1/messages, and each spells a
        // tool differently. All three must produce the same vendor request, or tool use works on one door
        // and silently not on the others.
        let openai_chat = serde_json::json!({"type": "function", "function": {
            "name": "get_weather", "description": "w", "parameters": {"type": "object"}}});
        let responses = serde_json::json!({"type": "function",
            "name": "get_weather", "description": "w", "parameters": {"type": "object"}});
        let anthropic = serde_json::json!({
            "name": "get_weather", "description": "w", "input_schema": {"type": "object"}});
        for t in [&openai_chat, &responses, &anthropic] {
            let (name, desc, schema) = tool_parts(t).expect("parsed");
            assert_eq!(name, "get_weather");
            assert_eq!(desc.as_deref(), Some("w"));
            assert_eq!(schema["type"], "object");
        }
    }

    #[test]
    fn tools_are_emitted_in_each_vendors_own_shape() {
        let tools = serde_json::json!([{"type": "function", "function": {
            "name": "f", "description": "d", "parameters": {"type": "object"}}}]);
        let a = anthropic_tools(&tools);
        assert_eq!(a[0]["name"], "f");
        assert_eq!(a[0]["input_schema"]["type"], "object");   // Anthropic: input_schema
        assert!(a[0].get("parameters").is_none());
        let r = responses_tools(&tools);
        assert_eq!(r[0]["type"], "function");                  // Responses: flat + parameters
        assert_eq!(r[0]["name"], "f");
        assert_eq!(r[0]["parameters"]["type"], "object");
    }

    #[test]
    fn a_nameless_tool_is_dropped_not_half_sent() {
        let tools = serde_json::json!([{"type": "function", "function": {"description": "no name"}}]);
        assert!(anthropic_tools(&tools).is_empty());
        assert!(responses_tools(&tools).is_empty());
    }

    #[test]
    fn tool_choice_maps_or_is_dropped() {
        assert_eq!(anthropic_tool_choice(&serde_json::json!("auto")).unwrap()["type"], "auto");
        assert_eq!(anthropic_tool_choice(&serde_json::json!("required")).unwrap()["type"], "any");
        let named = serde_json::json!({"type": "function", "function": {"name": "f"}});
        let m = anthropic_tool_choice(&named).unwrap();
        assert_eq!(m["type"], "tool");
        assert_eq!(m["name"], "f");
        // Unmappable -> None. Guessing here silently changes whether the model may call a tool at all.
        assert!(anthropic_tool_choice(&serde_json::json!(42)).is_none());
    }

    #[test]
    fn tool_calls_leave_in_one_shape_regardless_of_vendor() {
        let c = openai_tool_call("toolu_1", "get_weather", "{\"city\":\"Paris\"}");
        assert_eq!(c["type"], "function");
        assert_eq!(c["function"]["name"], "get_weather");
        // `arguments` stays a STRING: that is what every OpenAI-shaped client parses.
        assert!(c["function"]["arguments"].is_string());
    }

    #[test]
    fn delta_sink_survives_a_closed_receiver() {
        // The buyer hanging up must not abort the vendor stream: an adapter that stops draining leaves a
        // half-served request behind.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = DeltaSink::new(tx);
        drop(rx);
        sink.send("still fine");
    }
}
