//! `motyga supply` — run your own Codex / Claude subscription through Motyga.
//!
//! What this is: a small daemon that holds one outbound WebSocket to Motyga, receives model calls, runs
//! them against the subscription YOU are already signed into on this machine, and sends the answer back.
//!
//! WHO RECEIVES THOSE CALLS IS DECIDED ON THE WEBSITE, PER MODEL, AND DEFAULTS TO NOBODY BUT YOU:
//!   * only me    — your own other devices reach this subscription through Motyga. Open to everyone.
//!   * my team    — plus the active members of ONE organization you belong to.
//!   * for sale   — the marketplace, by invitation only. Only here are you paid a share of the sale.
//!
//! That setting deliberately does NOT live in this file or in `~/.motyga/supply.json`. It is a statement
//! about other PEOPLE, not about this machine, so the server owns it — which is also what lets you change
//! your mind from a phone without restarting anything.
//!
//! What this is NOT, and cannot be made into without changing the frame vocabulary in `node.rs`: a remote
//! shell. There is no frame that runs a command, reads a file or opens a port. A supply node executes model
//! calls and nothing else.
//!
//! Your vendor credentials never leave this machine. The node calls Anthropic/OpenAI directly, from your IP,
//! with the token their own CLI stored — Motyga only ever sees the prompt it sent you and the answer you
//! returned.
//!
//! ⚠️ Your vendor's terms generally licence a subscription to ONE person. Reaching your own subscription
//! from your own other devices is the mild reading of that; letting colleagues use it, and selling access
//! to strangers, are not. You accept the risk that applies to you explicitly in the browser, before a node
//! token is ever issued.

pub mod config;
pub mod enroll;
pub mod node;
pub mod refresh;
pub mod vendor;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use clap::Parser;
use clap::Subcommand;

use crate::config::Lane;
use crate::config::SupplyConfig;

#[derive(Debug, Parser)]
pub struct SupplyCli {
    #[command(subcommand)]
    pub command: SupplyCommand,
}

#[derive(Debug, Subcommand)]
pub enum SupplyCommand {
    /// Connect this machine to your Motyga account (opens a browser approval).
    Login(LoginArgs),
    /// Show what this machine offers and whether it is connected.
    Status,
    /// List the models this machine can serve (and, if you sell, what each one pays).
    Models,
    /// Offer a model from one of your subscriptions.
    Enable(EnableArgs),
    /// Stop offering a vendor's models (or everything).
    Disable(DisableArgs),
    /// Run the supply daemon.
    Run,
    /// Forget the node token stored on this machine.
    Logout,
}

#[derive(Debug, Parser)]
pub struct LoginArgs {
    /// Backend base URL (for staging).
    #[arg(long)]
    pub base_url: Option<String>,
    /// A name for this machine, shown in your Motyga console.
    #[arg(long, default_value = "node")]
    pub name: String,
}

#[derive(Debug, Parser)]
pub struct EnableArgs {
    /// Which subscription this comes from.
    #[arg(value_parser = ["claude", "codex"])]
    pub vendor: String,
    /// Model id — run `motyga supply models` to see what is being bought.
    #[arg(long)]
    pub model: String,
    /// Percentage of that subscription's WEEKLY window this machine may spend (1-100). The node stops
    /// offering the model once the vendor reports this much of the window is gone, so the rest stays yours.
    #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u8).range(1..=100))]
    pub share: u8,
    /// How many requests this machine will serve at once.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u8).range(1..=8))]
    pub max_concurrency: u8,
}

#[derive(Debug, Parser)]
pub struct DisableArgs {
    /// Vendor to stop offering. Omit with --all to stop everything.
    pub vendor: Option<String>,
    #[arg(long)]
    pub all: bool,
}

pub async fn run_main(cli: SupplyCli) -> Result<()> {
    match cli.command {
        SupplyCommand::Login(args) => cmd_login(args).await,
        SupplyCommand::Status => cmd_status().await,
        SupplyCommand::Models => cmd_models().await,
        SupplyCommand::Enable(args) => cmd_enable(args).await,
        SupplyCommand::Disable(args) => cmd_disable(args),
        SupplyCommand::Run => cmd_run().await,
        SupplyCommand::Logout => cmd_logout(),
    }
}

async fn cmd_login(args: LoginArgs) -> Result<()> {
    let mut cfg = config::load()?;
    if let Some(base) = args.base_url {
        cfg.base_url = Some(base);
    }
    cfg.node_name = Some(args.name.clone());
    let token = enroll::enroll(&cfg.base()).await?;
    cfg.node_token = Some(token);
    config::save(&cfg)?;
    println!("This machine is connected.");
    println!("Next: motyga supply models        # which models this machine can serve");
    // No model id here on purpose: this text ships inside the npm binary, so a retired model would keep
    // being suggested until every supplier upgrades.
    println!("      motyga supply enable claude --model <id from the list above> --share 50");
    println!("Then: motyga supply run");
    // Said at the moment the machine becomes reachable, because "who can reach it" is the one thing this
    // command does NOT decide — and the answer it starts with is "nobody but you".
    println!();
    println!("By default only YOU reach this subscription, from your own other devices.");
    println!("Sharing it with your team, or selling it, is chosen per model on the website.");
    Ok(())
}

/// What the SERVER says about this machine. Everything `status` could report on its own was local intent —
/// a token file exists, a config lists some lanes — and none of that is the question anyone actually has,
/// which is "is this thing working, what did the server accept, and what is relay costing me".
#[derive(serde::Deserialize)]
struct NodeStatus {
    node: NodeInfo,
    #[serde(default)]
    may_sell: bool,
    #[serde(default)]
    relay: RelayInfo,
    #[serde(default)]
    lanes: Vec<ServerLane>,
}
#[derive(serde::Deserialize)]
struct NodeInfo {
    #[serde(default)]
    name: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    online: bool,
    #[serde(default)]
    last_seen_at: Option<String>,
}
#[derive(serde::Deserialize, Default)]
struct RelayInfo {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    org_sharing_enabled: bool,
    #[serde(default)]
    fee_microusd: i64,
    #[serde(default)]
    requests_total: i64,
    #[serde(default)]
    spent_microusd: i64,
    #[serde(default)]
    owed_microusd: i64,
}
#[derive(serde::Deserialize)]
struct ServerLane {
    vendor: String,
    model: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    audience: String,
    #[serde(default)]
    share_pct: u8,
    #[serde(default)]
    max_concurrency: u8,
    #[serde(default)]
    window_used_pct: Option<u8>,
    #[serde(default)]
    live: bool,
}

/// "only me" / "my team" / "for sale" — the server's own word, translated once, here.
fn audience_label(a: &str) -> &'static str {
    match a {
        "private" => "only me",
        "org" => "my team",
        "public" => "for sale",
        _ => "unknown",
    }
}

async fn fetch_status(cfg: &SupplyConfig) -> Result<NodeStatus> {
    let token = cfg
        .node_token
        .as_deref()
        .context("this machine is not connected — run `motyga supply login` first")?;
    let resp = reqwest::Client::builder()
        // A status command must never be the thing that hangs a terminal.
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(format!("{}/v1/fleet/node/status", cfg.base()))
        .bearer_auth(token)
        .send()
        .await
        .with_context(|| format!("cannot reach {}", cfg.base()))?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow!("this machine is no longer authorised — run `motyga supply login` again"));
    }
    Ok(resp.error_for_status()?.json().await?)
}

async fn cmd_status() -> Result<()> {
    let cfg = config::load()?;
    println!("Backend:   {}", cfg.base());

    // Ask the server first. It is the only party that knows whether this machine is actually on the socket,
    // which of its lanes were accepted, and who may reach them — none of which is knowable from disk.
    let remote = match fetch_status(&cfg).await {
        Ok(s) => Some(s),
        Err(err) => {
            // Not fatal: an old backend, a dead link or no token at all still leaves the local view worth
            // printing. Say WHY the live half is missing rather than quietly showing less.
            println!("Server:    unavailable — {err}");
            None
        }
    };

    match &remote {
        Some(s) => {
            println!(
                "Node:      {} — {}{}",
                if s.node.name.is_empty() { "this machine" } else { &s.node.name },
                if s.node.online { "ONLINE, taking work" } else { "offline (run `motyga supply run`)" },
                if s.node.status == "active" { String::new() } else { format!(" [{}]", s.node.status) },
            );
            if !s.node.online && let Some(seen) = &s.node.last_seen_at {
                println!("           last seen {}", seen.replace('T', " ").chars().take(16).collect::<String>());
            }
            if s.relay.enabled {
                println!(
                    "Relay:     ${} per request · {} request(s) so far · ${} spent{}",
                    trim_zeros(s.relay.fee_microusd as f64 / 1_000_000.0),
                    s.relay.requests_total,
                    trim_zeros(s.relay.spent_microusd as f64 / 1_000_000.0),
                    // The carried remainder explains the thing that otherwise looks like a bug: most
                    // requests take nothing, and then one takes a whole credit.
                    if s.relay.owed_microusd > 0 {
                        format!(" (${} accrued toward the next credit)",
                                trim_zeros(s.relay.owed_microusd as f64 / 1_000_000.0))
                    } else { String::new() },
                );
                if !s.relay.org_sharing_enabled {
                    println!("           sharing with a team is not switched on for this account");
                }
            }
            if s.may_sell {
                println!("Selling:   enabled — models marked 'for sale' below are on the marketplace");
            }
        }
        None => println!(
            "Connected: {}",
            if cfg.node_token.is_some() { "a token is stored (server state unknown)" }
            else { "no — run `motyga supply login`" }
        ),
    }

    match &remote {
        // The server's view wins: it knows which lanes it ACCEPTED and who may reach them, and a lane
        // sitting in the local file that the server never took is exactly what a supplier needs told.
        Some(s) if !s.lanes.is_empty() => {
            println!("Offering:");
            for lane in &s.lanes {
                let window = lane
                    .window_used_pct
                    .map(|p| format!("window {p}% used"))
                    .unwrap_or_else(|| "window unknown".to_string());
                println!(
                    "  {:<8} {:<24} {:<9} share {:>3}%  at once {}  {}  {}",
                    lane.vendor,
                    lane.model,
                    audience_label(&lane.audience),
                    lane.share_pct,
                    lane.max_concurrency,
                    window,
                    if !lane.enabled { "OFF" } else if lane.live { "live" } else { "not on the link" },
                );
            }
        }
        _ if cfg.lanes.is_empty() => {
            println!("Offering:  nothing yet — see `motyga supply enable --help`");
        }
        _ => {
            println!("Offering (local config; who may reach each is on the website):");
            for lane in &cfg.lanes {
                println!(
                    "  {:<8} {:<24} share {:>3}%  concurrency {}  {}",
                    lane.vendor,
                    lane.model,
                    lane.share_pct,
                    lane.max_concurrency,
                    if lane.enabled { "on" } else { "off" }
                );
            }
        }
    }
    // Say which vendor logins are actually usable from here, so a supplier is not left guessing why a lane
    // never serves anything.
    println!("Vendor sign-ins on this machine:");
    for v in [refresh::Vendor::Claude, refresh::Vendor::Codex] {
        println!("  {:<7} {}", format!("{}:", v.as_str()), describe(v));
    }
    Ok(())
}

/// One line per vendor: signed in or not, and how long the credential has left. The remaining time is the
/// number that actually predicts whether a lane will keep serving overnight, so it is worth showing even
/// though the node refreshes on its own.
fn describe(v: refresh::Vendor) -> String {
    match v.expires_in_ms() {
        Some(ms) if ms <= 0 => "signed in, but the token has EXPIRED (the node will refresh it)".to_string(),
        Some(ms) => {
            let hours = ms as f64 / 3_600_000.0;
            format!("signed in, {hours:.1}h left")
        }
        None => "not found (sign in with that vendor's own CLI first)".to_string(),
    }
}

/// The catalog comes from the SERVER, not a table baked into the CLI: prices move, models are added and
/// retired, and a supplier deciding what to switch on needs today's rate rather than whatever was true when
/// they last updated.
#[derive(serde::Deserialize)]
struct CatalogRow {
    vendor: String,
    model: String,
    label: String,
    pay_usd_per_1m_output: f64,
    buyer_usd_per_1m_output: f64,
}

#[derive(serde::Deserialize)]
struct Catalog {
    models: Vec<CatalogRow>,
    /// Is this account actually in the SALE programme? The rates are what a seller is paid, and printing
    /// them to someone who is only relaying their own subscription tells them something false about their
    /// own account. There is no browser session in a terminal, so the answer travels with the catalog.
    /// Defaults to false — the honest reading of an older backend that does not send it.
    #[serde(default)]
    may_sell: bool,
    #[serde(default)]
    relay_fee_microusd: i64,
}

async fn fetch_catalog(cfg: &SupplyConfig) -> Result<Catalog> {
    // The node token is the only credential a terminal has — there is no browser session here. Without it
    // this request was answered 401 for everyone, so the command documented as "see the live rates" had
    // never once worked.
    let mut req = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()?
        .get(format!("{}/api/web/fleet/catalog", cfg.base()));
    if let Some(token) = cfg.node_token.as_deref() {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("cannot reach {}", cfg.base()))?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err(anyhow!("run `motyga supply login` first, then run this again"));
    }
    resp.error_for_status()
        .context("the catalog is unavailable")?
        .json()
        .await
        .context("unexpected response from the catalog")
}

async fn cmd_models() -> Result<()> {
    let cfg = config::load()?;
    let body = fetch_catalog(&cfg).await?;

    if body.models.is_empty() {
        println!("No models are available right now.");
        return Ok(());
    }
    if body.may_sell {
        println!("{:<8} {:<20} {:<22} {:>14} {:>14}", "VENDOR", "MODEL", "NAME", "YOU EARN", "BUYER PAYS");
        println!("{:<8} {:<20} {:<22} {:>14} {:>14}", "", "", "", "$ / 1M out", "$ / 1M out");
        for m in &body.models {
            println!(
                "{:<8} {:<20} {:<22} {:>14.2} {:>14.2}",
                m.vendor, m.model, m.label, m.pay_usd_per_1m_output, m.buyer_usd_per_1m_output
            );
        }
    } else {
        // No earnings column: this account is not in the sale programme, so those numbers describe money
        // that will never arrive. What it CAN do is reach its own subscription — so price that instead.
        println!("{:<8} {:<20} {:<22}", "VENDOR", "MODEL", "NAME");
        for m in &body.models {
            println!("{:<8} {:<20} {:<22}", m.vendor, m.model, m.label);
        }
        if body.relay_fee_microusd > 0 {
            println!();
            println!(
                "Reaching your own subscription through Motyga costs ${} per request — any model, any length.",
                trim_zeros(body.relay_fee_microusd as f64 / 1_000_000.0)
            );
        }
    }
    println!();
    println!("Offer one with:  motyga supply enable <vendor> --model <model> --share <1-100>");
    println!("--share is the percentage of that subscription's weekly window this machine may spend.");
    println!("Who may reach it — only you, your team, or the marketplace — is chosen per model on the website.");
    Ok(())
}

/// "$0.001", not "$0.0010000". A fee below a cent needs more places than money usually gets, and the
/// trailing zeros then read as false precision.
fn trim_zeros(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() { "0".to_string() } else { s.to_string() }
}

async fn cmd_enable(args: EnableArgs) -> Result<()> {
    let mut cfg = config::load()?;

    // Check the model against the live catalog BEFORE writing it down. A typo used to be accepted in
    // silence: the lane was saved, `status` listed it, and the server then dropped it from every hello as
    // a model it does not carry — so the supplier believed they were offering something they were not, and
    // the only clue was a line in `run`'s output they had probably already scrolled past.
    match fetch_catalog(&cfg).await {
        Ok(cat) => {
            if !cat.models.iter().any(|m| m.model == args.model && m.vendor == args.vendor) {
                let near: Vec<&str> = cat
                    .models
                    .iter()
                    .filter(|m| m.vendor == args.vendor)
                    .map(|m| m.model.as_str())
                    .collect();
                return Err(anyhow!(
                    "{} does not carry {} — run `motyga supply models` to see what it does.{}",
                    args.vendor,
                    args.model,
                    if near.is_empty() { String::new() } else { format!("\nAvailable: {}", near.join(", ")) },
                ));
            }
        }
        // Unreachable backend must not stop someone configuring a machine offline; the server drops an
        // unknown lane anyway. Say what was skipped rather than pretending it was checked.
        Err(err) => println!("(could not verify against the catalog: {err})"),
    }

    cfg.upsert_lane(Lane {
        vendor: args.vendor.clone(),
        model: args.model.clone(),
        share_pct: args.share,
        max_concurrency: args.max_concurrency,
        enabled: true,
    });
    config::save(&cfg)?;
    println!(
        "Offering {} {} — up to {}% of the window, {} at a time.",
        args.vendor, args.model, args.share, args.max_concurrency
    );
    println!("Reachable by you only, until you say otherwise on the website.");
    println!("Restart `motyga supply run` for this to take effect.");
    Ok(())
}

fn cmd_disable(args: DisableArgs) -> Result<()> {
    let mut cfg = config::load()?;
    if args.all {
        cfg.remove_lanes(None);
        println!("Stopped offering everything.");
    } else {
        let vendor = args
            .vendor
            .as_deref()
            .ok_or_else(|| anyhow!("name a vendor, or pass --all"))?;
        cfg.remove_lanes(Some(vendor));
        println!("Stopped offering {vendor}.");
    }
    config::save(&cfg)?;
    Ok(())
}

async fn cmd_run() -> Result<()> {
    let cfg = config::load()?;
    let token = cfg
        .node_token
        .clone()
        .context("this machine is not connected — run `motyga supply login` first")?;
    if cfg.enabled_lanes().is_empty() {
        return Err(anyhow!(
            "nothing is being offered — run `motyga supply enable <vendor> --model <model>` first"
        ));
    }
    // NOT "serving": nothing is being served until the socket is up and the server has said which lanes it
    // accepted, which report_hello_ok prints a moment later. Claiming it here made a node that was rejected
    // outright look identical to one that was working.
    println!("Connecting to {} with {} lane(s). Ctrl-C to stop.",
             cfg.base(), cfg.enabled_lanes().len());
    let runtime = std::sync::Arc::new(node::NodeRuntime::new(cfg, token));
    tokio::select! {
        result = runtime.run_forever() => result,
        _ = tokio::signal::ctrl_c() => {
            println!("Stopped.");
            Ok(())
        }
    }
}

fn cmd_logout() -> Result<()> {
    let mut cfg = config::load()?;
    cfg.node_token = None;
    config::save(&cfg)?;
    println!("Node token removed from this machine.");
    println!("Revoke it server-side too if the machine may be compromised.");
    Ok(())
}

/// Re-exported so the binary can name the type without depending on clap's derive here.
pub type Cli = SupplyCli;

#[allow(dead_code)]
fn _assert_config_is_send(cfg: SupplyConfig) -> impl Send {
    cfg
}
