//! On-disk state for supply mode: which lanes this machine offers, and the node token.
//!
//! Lives at `~/.motyga/supply.json`, separate from the agent's own config. Supply mode is a different
//! product with a different trust story — someone who runs `motyga supply` is selling capacity, not coding —
//! and mixing the two files would make it far too easy to leak one into the other.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

/// A model this machine is willing to serve, and how much of the vendor's window the supplier will give up.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lane {
    pub vendor: String,
    pub model: String,
    /// Percentage of the vendor rate-limit window we may spend before the lane stops offering. 100 means
    /// "sell it all"; the node stops on its own so the supplier is never locked out of what they kept.
    #[serde(default = "default_share")]
    pub share_pct: u8,
    #[serde(default = "default_concurrency")]
    pub max_concurrency: u8,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_share() -> u8 {
    100
}
fn default_concurrency() -> u8 {
    1
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SupplyConfig {
    /// Backend base URL, e.g. `https://motyga.com`. Overridable for staging.
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub node_name: Option<String>,
    #[serde(default)]
    pub lanes: Vec<Lane>,
    /// Fallback storage for the node token when the OS keyring is unavailable — a headless Linux box with
    /// no secret service is exactly where a supplier is likely to run this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_token: Option<String>,
}

pub const DEFAULT_BASE_URL: &str = "https://motyga.com";

impl SupplyConfig {
    pub fn base(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string())
            .trim_end_matches('/')
            .to_string()
    }

    pub fn enabled_lanes(&self) -> Vec<&Lane> {
        self.lanes.iter().filter(|l| l.enabled).collect()
    }

    pub fn upsert_lane(&mut self, lane: Lane) {
        if let Some(existing) = self
            .lanes
            .iter_mut()
            .find(|l| l.vendor == lane.vendor && l.model == lane.model)
        {
            *existing = lane;
        } else {
            self.lanes.push(lane);
        }
    }

    pub fn remove_lanes(&mut self, vendor: Option<&str>) {
        match vendor {
            Some(v) => self.lanes.retain(|l| l.vendor != v),
            None => self.lanes.clear(),
        }
    }
}

pub fn config_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("cannot determine the home directory")?;
    Ok(home.join(".motyga"))
}

pub fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("supply.json"))
}

pub fn load() -> Result<SupplyConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(SupplyConfig::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))
}

pub fn save(cfg: &SupplyConfig) -> Result<()> {
    let dir = config_dir()?;
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let path = config_path()?;
    let body = serde_json::to_string_pretty(cfg)?;

    // Write-then-rename so an interrupted save cannot leave a half-written config that locks the supplier
    // out of their own node.
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
    }
    restrict_permissions(&tmp)?;
    fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Owner-only on unix. The file may hold the node token when no keyring is available, and that token lets
/// its holder sell on the supplier's behalf.
fn restrict_permissions(path: &PathBuf) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_upsert_replaces_rather_than_duplicates() {
        let mut cfg = SupplyConfig::default();
        cfg.upsert_lane(Lane {
            vendor: "claude".into(),
            model: "claude-opus-4-7".into(),
            share_pct: 50,
            max_concurrency: 1,
            enabled: true,
        });
        cfg.upsert_lane(Lane {
            vendor: "claude".into(),
            model: "claude-opus-4-7".into(),
            share_pct: 100,
            max_concurrency: 2,
            enabled: true,
        });
        assert_eq!(cfg.lanes.len(), 1);
        assert_eq!(cfg.lanes[0].share_pct, 100);
        assert_eq!(cfg.lanes[0].max_concurrency, 2);
    }

    #[test]
    fn disabled_lanes_are_not_offered() {
        let mut cfg = SupplyConfig::default();
        cfg.upsert_lane(Lane {
            vendor: "codex".into(),
            model: "gpt-5.3-codex".into(),
            share_pct: 100,
            max_concurrency: 1,
            enabled: false,
        });
        assert!(cfg.enabled_lanes().is_empty());
    }

    #[test]
    fn base_url_is_normalised() {
        let cfg = SupplyConfig {
            base_url: Some("https://example.test/".into()),
            ..Default::default()
        };
        assert_eq!(cfg.base(), "https://example.test");
    }
}
