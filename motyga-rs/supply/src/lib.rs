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
        SupplyCommand::Enable(args) => cmd_enable(args),
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

async fn cmd_status() -> Result<()> {
    let cfg = config::load()?;
    println!("Backend:   {}", cfg.base());
    println!(
        "Connected: {}",
        if cfg.node_token.is_some() { "yes" } else { "no — run `motyga supply login`" }
    );
    if cfg.lanes.is_empty() {
        println!("Offering:  nothing yet — see `motyga supply enable --help`");
    } else {
        // Deliberately NOT claiming who reaches each lane: that lives on the server and this file would be
        // guessing. Saying nothing is better than printing a stale "for sale" next to a private model.
        println!("Offering (who may reach each — see the website):");
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
async fn cmd_models() -> Result<()> {
    #[derive(serde::Deserialize)]
    struct Row {
        vendor: String,
        model: String,
        label: String,
        pay_usd_per_1m_output: f64,
        buyer_usd_per_1m_output: f64,
    }
    #[derive(serde::Deserialize)]
    struct Resp {
        models: Vec<Row>,
        /// Is this account actually in the SALE programme? The rates below are what a seller is paid, and
        /// printing them to someone who is only relaying their own subscription tells them something false
        /// about their own account. There is no browser session in a terminal, so the answer travels with
        /// the catalog. Defaults to false — the honest reading of an older backend that does not send it.
        #[serde(default)]
        may_sell: bool,
        #[serde(default)]
        relay_fee_microusd: i64,
    }

    let cfg = config::load()?;
    // The node token is the only credential a terminal has — there is no browser session here. Without it
    // this request was answered 401 for everyone, so the command documented as "see the live rates" had
    // never once worked.
    let mut req = reqwest::Client::new().get(format!("{}/api/web/fleet/catalog", cfg.base()));
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
    let body: Resp = resp
        .error_for_status()
        .context("the catalog is unavailable")?
        .json()
        .await
        .context("unexpected response from the catalog")?;

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

fn cmd_enable(args: EnableArgs) -> Result<()> {
    let mut cfg = config::load()?;
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
    println!("Serving {} lane(s). Ctrl-C to stop.", cfg.enabled_lanes().len());
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
