//! Keeping the vendor credential alive without ever putting a buyer near a command line.
//!
//! Serving a job is a direct HTTPS call with the token the vendor's own CLI left on disk (see `vendor.rs`).
//! That call never rewrites the credential file, so on a machine where nobody opens Claude Code the token
//! eventually lapses and the node goes quiet. The fix is to let the vendor's CLI do the one thing it is
//! uniquely able to do — rotate its own token — and nothing else.
//!
//! THE SECURITY PROPERTY, and why it survives spawning a process at all: the invariant that matters is not
//! "never spawn a subprocess", it is **no buyer input ever reaches a command line**. These functions take no
//! prompt parameter. The prompt is the compile-time constant below, the tools are switched off, and the
//! child gets a hard timeout. There is no argument a caller — let alone a job — can influence.
//!
//! VERIFIED 2026-07-31 on a live Claude Max install: with the credential 60 seconds from expiry, this exact
//! invocation rotated both the access and refresh tokens and pushed expiry out a full 8 hours. With a token
//! that still had hours left it did nothing, which is why the threshold below exists — the CLI refreshes
//! lazily and pinging it early is wasted quota.

use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;

use crate::vendor::ClaudeAdapter;
use crate::vendor::CodexAdapter;

/// The entire prompt a supply node will ever hand to a vendor agent. A constant, so that "buyer input
/// cannot reach a command" is a fact about the type signature rather than a rule someone must remember.
const REFRESH_PROMPT: &str = "Chupapy-Monyanya";

/// Refresh once the credential is within this window of expiring. Long enough that an in-flight job never
/// straddles the boundary; short enough that we are not pinging a CLI that would decline to refresh anyway.
pub const REFRESH_WHEN_LEFT: Duration = Duration::from_secs(30 * 60);

/// Never retry faster than this after an attempt. A vendor CLI that is broken (signed out, upgrading,
/// missing) must not turn into a spawn loop on someone's home machine.
pub const REFRESH_COOLDOWN: Duration = Duration::from_secs(10 * 60);

/// How long the child gets before we give up on it.
const REFRESH_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Claude,
    Codex,
}

impl Vendor {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
        }
    }

    /// Milliseconds of life left in this vendor's credential, or None when we cannot tell (not signed in,
    /// or the file shape changed). None means "do not refresh" — guessing would spawn a pointless process
    /// on every tick.
    pub fn expires_in_ms(self) -> Option<i64> {
        match self {
            Self::Claude => ClaudeAdapter::load().ok()?.expires_in_ms(),
            Self::Codex => CodexAdapter::load().ok()?.expires_in_ms(),
        }
    }

    pub fn needs_refresh(self) -> bool {
        match self.expires_in_ms() {
            Some(left) => left <= REFRESH_WHEN_LEFT.as_millis() as i64,
            None => false,
        }
    }

    /// The exact argv. Split out from the spawn so a test can assert the shape without running anything.
    fn refresh_argv(self) -> Vec<String> {
        let s = |x: &str| x.to_string();
        match self {
            // --allowed-tools "" is an ALLOW-list of nothing, which fails closed: a tool added in a future
            // Claude Code release is excluded automatically, where a deny-list would silently admit it.
            // --permission-mode plan is the belt to that braces. Haiku because this is a throwaway ping and
            // it should cost the supplier as little of their window as possible.
            Self::Claude => vec![
                s("claude"), s("-p"), s(REFRESH_PROMPT),
                s("--model"), s("claude-haiku-4-5"),
                s("--allowed-tools"), s(""),
                s("--permission-mode"), s("plan"),
                s("--no-session-persistence"),
                s("--output-format"), s("text"),
            ],
            // read-only is the tightest sandbox `codex exec` offers. --skip-git-repo-check because a supply
            // node has no reason to be inside a repository and codex otherwise refuses to start.
            Self::Codex => vec![
                s("codex"), s("exec"),
                s("--sandbox"), s("read-only"),
                s("--skip-git-repo-check"),
                s("-m"), s("gpt-5.6-luna"),
                s(REFRESH_PROMPT),
            ],
        }
    }

    /// Run the vendor CLI purely so it rotates its own credential. Returns Ok only when the credential
    /// actually moved — a CLI that ran happily and refreshed nothing is a failure for our purposes.
    pub async fn refresh(self) -> Result<()> {
        let before = self.expires_in_ms();
        let argv = self.refresh_argv();
        let (program, args) = split_argv(&argv);

        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            // A supplier may well launch `motyga supply run` from a terminal inside Claude Code, and the
            // CLI refuses to start nested. Clearing the marker is what keeps the refresh working there.
            .env_remove("CLAUDECODE");
        #[cfg(windows)]
        {
            // npm installs these as .cmd shims, which CreateProcess will not resolve on its own.
            cmd = rebuild_via_cmd_shell(&argv);
        }

        let child = cmd.spawn().with_context(|| {
            format!("cannot start `{}` — is that vendor's CLI installed and on PATH?", self.as_str())
        })?;
        let status = tokio::time::timeout(REFRESH_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| anyhow!("`{}` did not finish within {:?}", self.as_str(), REFRESH_TIMEOUT))?
            .with_context(|| format!("running `{}`", self.as_str()))?;
        if !status.status.success() {
            return Err(anyhow!("`{}` exited with {}", self.as_str(), status.status));
        }

        let after = self.expires_in_ms();
        match (before, after) {
            (Some(b), Some(a)) if a > b => Ok(()),
            (_, Some(a)) if a > REFRESH_WHEN_LEFT.as_millis() as i64 => Ok(()),
            _ => Err(anyhow!(
                "`{}` ran but did not rotate its credential — try signing in again with that CLI",
                self.as_str()
            )),
        }
    }
}

fn split_argv(argv: &[String]) -> (&str, &[String]) {
    (argv[0].as_str(), &argv[1..])
}

#[cfg(windows)]
fn rebuild_via_cmd_shell(argv: &[String]) -> tokio::process::Command {
    // `cmd /C` with each argument passed separately: the arguments are our own constants, never anything a
    // caller supplied, so there is no quoting hazard to reason about here.
    let mut cmd = tokio::process::Command::new("cmd");
    cmd.arg("/C");
    for a in argv {
        cmd.arg(a);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env_remove("CLAUDECODE");
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_is_the_only_thing_we_ever_send() {
        // The point of this test is not the string, it is that the argv is fully determined by the vendor:
        // nothing here can be steered by a job, because nothing here is a parameter.
        for v in [Vendor::Claude, Vendor::Codex] {
            let argv = v.refresh_argv();
            assert_eq!(argv.iter().filter(|a| *a == REFRESH_PROMPT).count(), 1);
        }
    }

    #[test]
    fn claude_refresh_runs_with_no_tools() {
        let argv = Vendor::Claude.refresh_argv();
        let joined = argv.join(" ");
        // An allow-list of nothing, not a deny-list: a tool introduced by a future release stays excluded.
        let i = argv.iter().position(|a| a == "--allowed-tools").expect("allow-list flag");
        assert_eq!(argv[i + 1], "");
        assert!(joined.contains("--permission-mode plan"));
        assert!(joined.contains("--no-session-persistence"));
    }

    #[test]
    fn codex_refresh_runs_sandboxed() {
        let argv = Vendor::Codex.refresh_argv();
        let i = argv.iter().position(|a| a == "--sandbox").expect("sandbox flag");
        assert_eq!(argv[i + 1], "read-only");
        assert!(!argv.iter().any(|a| a.contains("dangerously")));
    }

    #[test]
    fn vendor_names_round_trip() {
        assert_eq!(Vendor::parse("claude"), Some(Vendor::Claude));
        assert_eq!(Vendor::parse("codex"), Some(Vendor::Codex));
        assert_eq!(Vendor::parse("gemini"), None);
    }
}
