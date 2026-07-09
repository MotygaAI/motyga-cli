use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
pub struct AppCommand {
    /// Workspace path to open in the Motyga desktop app.
    #[arg(value_name = "PATH", default_value = ".")]
    pub path: PathBuf,

    /// Override the app installer download URL (advanced).
    #[arg(long = "download-url")]
    pub download_url_override: Option<String>,
}

pub async fn run_app(cmd: AppCommand) -> anyhow::Result<()> {
    // Motyga Desktop is not shipping yet. The install/open path lives in
    // `crate::desktop_app` (kept for later) but is intentionally NOT invoked here,
    // so `motyga app` can never download a third-party desktop build. Re-enable
    // this once Motyga Desktop ships with its own installer URL.
    let _ = cmd;
    anyhow::bail!(
        "The Motyga desktop app is not available yet — use the `motyga` CLI in your terminal for now."
    )
}
