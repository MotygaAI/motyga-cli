//! Shared raw tool cache for the host-owned Motyga Apps MCP server.
//!
//! Cache entries are process-local live state scoped by the active Motyga auth
//! key. Disk is best-effort cold-start persistence; entries do not reread disk
//! after creation.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::Context;
use arc_swap::ArcSwapOption;
use motyga_login::MotygaAuth;
use motyga_protocol::mcp::McpServerInfo;
use serde::Deserialize;
use serde::Serialize;
use sha1::Digest;
use sha1::Sha1;
use tracing::instrument;

use crate::runtime::emit_duration;
use crate::tools::MCP_TOOLS_CACHE_WRITE_DURATION_METRIC;
use crate::tools::ToolInfo;

const MCP_TOOLS_CACHE_PUBLISH_DURATION_METRIC: &str = "motyga.mcp.tools.cache_publish.duration_ms";

/// The MotygaAuth bits that identify a Motyga Apps catalog.
///
/// Debug bearer-token overrides bypass the shared cache, so shared entries only
/// need the MotygaAuth-backed identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MotygaAppsToolsCacheKey {
    pub(crate) account_id: Option<String>,
    pub(crate) chatgpt_user_id: Option<String>,
    pub(crate) is_workspace_account: bool,
}

/// Builds the MotygaAuth-backed Motyga Apps cache key.
pub fn motyga_apps_tools_cache_key(auth: Option<&MotygaAuth>) -> MotygaAppsToolsCacheKey {
    MotygaAppsToolsCacheKey {
        account_id: auth.and_then(MotygaAuth::get_account_id),
        chatgpt_user_id: auth.and_then(MotygaAuth::get_chatgpt_user_id),
        is_workspace_account: auth.is_some_and(MotygaAuth::is_workspace_account),
    }
}

/// Process-scoped registry for shared Motyga Apps raw tool snapshots.
///
/// Two clients share an entry only when they would read the same Motyga Apps
/// catalog. New entries may seed from disk; live entries read from memory only.
#[derive(Clone, Default)]
pub struct MotygaAppsToolsCache {
    entries: Arc<Mutex<HashMap<MotygaAppsToolsCacheIdentity, Arc<MotygaAppsToolsCacheEntry>>>>,
}

/// Handle to one shared Motyga Apps tools cache entry.
///
/// The connection manager creates this from the auth key, then tool
/// reads and refreshes for that managed client use the same entry.
#[derive(Clone)]
pub(crate) struct MotygaAppsToolsCacheContext {
    entry: Arc<MotygaAppsToolsCacheEntry>,
}

impl MotygaAppsToolsCacheContext {
    pub(crate) fn tools_cache_path(&self) -> PathBuf {
        self.entry
            .identity
            .cache_path_in(MOTYGA_APPS_TOOLS_CACHE_DIR)
    }

    pub(crate) fn server_info_cache_path(&self) -> PathBuf {
        self.entry
            .identity
            .cache_path_in(MOTYGA_APPS_SERVER_INFO_CACHE_DIR)
    }

    pub(crate) fn current_tools(&self) -> Option<Vec<ToolInfo>> {
        self.entry
            .current_tools
            .load_full()
            .map(|tools| tools.as_ref().clone())
    }

    pub(crate) fn has_current_tools(&self) -> bool {
        self.entry.current_tools.load_full().is_some()
    }

    pub(crate) fn begin_fetch(
        &self,
        source: MotygaAppsToolsFetchSource,
    ) -> MotygaAppsToolsFetchTicket {
        MotygaAppsToolsFetchTicket {
            generation: self
                .entry
                .next_fetch_generation
                .fetch_add(1, Ordering::Relaxed)
                + 1,
            source,
        }
    }

    pub(crate) fn publish_if_newest_accepted(
        &self,
        ticket: MotygaAppsToolsFetchTicket,
        server_info: &McpServerInfo,
        tools: Vec<ToolInfo>,
    ) -> Vec<ToolInfo> {
        let publish_start = Instant::now();
        let mut last_accepted_generation = lock_unpoisoned(&self.entry.last_accepted_generation);
        if ticket.generation <= *last_accepted_generation {
            emit_duration(
                MCP_TOOLS_CACHE_PUBLISH_DURATION_METRIC,
                publish_start.elapsed(),
                &[("source", ticket.source.as_str()), ("result", "stale")],
            );
            return self.current_tools().unwrap_or(tools);
        }

        *last_accepted_generation = ticket.generation;
        self.entry
            .current_tools
            .store(Some(Arc::new(tools.clone())));
        persist_motyga_apps_cache(self, server_info, &tools);
        emit_duration(
            MCP_TOOLS_CACHE_PUBLISH_DURATION_METRIC,
            publish_start.elapsed(),
            &[("source", ticket.source.as_str()), ("result", "published")],
        );
        tools
    }

    #[cfg(test)]
    pub(crate) fn store_current_tools_for_test(&self, tools: Vec<ToolInfo>) {
        self.entry.current_tools.store(Some(Arc::new(tools)));
    }
}

impl MotygaAppsToolsCache {
    pub(crate) fn context(
        &self,
        motyga_home: PathBuf,
        auth_key: MotygaAppsToolsCacheKey,
    ) -> MotygaAppsToolsCacheContext {
        let identity = MotygaAppsToolsCacheIdentity {
            motyga_home,
            auth_key,
        };
        let mut entries = lock_unpoisoned(&self.entries);
        let entry = entries
            .entry(identity.clone())
            .or_insert_with(|| Arc::new(MotygaAppsToolsCacheEntry::new(identity)))
            .clone();
        MotygaAppsToolsCacheContext { entry }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum MotygaAppsToolsFetchSource {
    Startup,
    HardRefresh,
}

impl MotygaAppsToolsFetchSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::HardRefresh => "hard_refresh",
        }
    }
}

pub(crate) struct MotygaAppsToolsFetchTicket {
    generation: u64,
    source: MotygaAppsToolsFetchSource,
}

struct MotygaAppsToolsCacheEntry {
    identity: MotygaAppsToolsCacheIdentity,
    current_tools: ArcSwapOption<Vec<ToolInfo>>,
    next_fetch_generation: AtomicU64,
    last_accepted_generation: Mutex<u64>,
}

impl MotygaAppsToolsCacheEntry {
    fn new(identity: MotygaAppsToolsCacheIdentity) -> Self {
        let current_tools = load_cached_motyga_apps_tools_for_identity(&identity).map(Arc::new);
        Self {
            identity,
            current_tools: ArcSwapOption::from(current_tools),
            next_fetch_generation: AtomicU64::new(0),
            last_accepted_generation: Mutex::new(0),
        }
    }
}

/// Everything that decides whether two Motyga Apps clients can share tools.
///
/// The auth key says whose catalog we are reading. `motyga_home` keeps the
/// persisted cache under the right home directory.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MotygaAppsToolsCacheIdentity {
    motyga_home: PathBuf,
    auth_key: MotygaAppsToolsCacheKey,
}

impl MotygaAppsToolsCacheIdentity {
    fn cache_path_in(&self, cache_dir: &str) -> PathBuf {
        // `motyga_home` is already the parent directory. Keep it out of the
        // filename hash so non-UTF-8 Unix paths cannot collapse distinct auth
        // keys onto the same disk cache file.
        let identity_json = serde_json::to_string(&self.auth_key).unwrap_or_default();
        let identity_hash = sha1_hex(&identity_json);
        self.motyga_home
            .join(cache_dir)
            .join(format!("{identity_hash}.json"))
    }
}

#[cfg(test)]
fn write_cached_motyga_apps_tools_for_test(
    cache_context: &MotygaAppsToolsCacheContext,
    server_info: &McpServerInfo,
    tools: &[ToolInfo],
) {
    cache_context
        .entry
        .current_tools
        .store(Some(Arc::new(tools.to_vec())));
    persist_motyga_apps_cache(cache_context, server_info, tools);
}

pub(crate) fn load_startup_cached_motyga_apps_server_info(
    cache_context: &MotygaAppsToolsCacheContext,
) -> Option<McpServerInfo> {
    load_cached_motyga_apps_server_info(cache_context)
}

#[cfg(test)]
fn read_cached_motyga_apps_tools(
    cache_context: &MotygaAppsToolsCacheContext,
) -> Option<Vec<ToolInfo>> {
    load_cached_motyga_apps_tools_for_identity(&cache_context.entry.identity)
}

#[instrument(level = "trace", skip_all)]
fn load_cached_motyga_apps_tools_for_identity(
    identity: &MotygaAppsToolsCacheIdentity,
) -> Option<Vec<ToolInfo>> {
    let cache_path = identity.cache_path_in(MOTYGA_APPS_TOOLS_CACHE_DIR);
    let bytes = std::fs::read(cache_path).ok()?;
    let cache: MotygaAppsToolsDiskCache = serde_json::from_slice(&bytes).ok()?;
    (cache.schema_version == MOTYGA_APPS_TOOLS_CACHE_SCHEMA_VERSION).then_some(cache.tools)
}

fn write_cached_motyga_apps_tools(
    cache_context: &MotygaAppsToolsCacheContext,
    tools: &[ToolInfo],
) -> anyhow::Result<()> {
    let cache_path = cache_context.tools_cache_path();
    let bytes = serde_json::to_vec_pretty(&MotygaAppsToolsDiskCache {
        schema_version: MOTYGA_APPS_TOOLS_CACHE_SCHEMA_VERSION,
        tools: tools.to_vec(),
    })
    .context("failed to serialize Motyga Apps tools cache")?;
    write_motyga_apps_cache_file(&cache_path, "tools", bytes)
}

#[instrument(level = "trace", skip_all)]
fn load_cached_motyga_apps_server_info(
    cache_context: &MotygaAppsToolsCacheContext,
) -> Option<McpServerInfo> {
    let bytes = std::fs::read(cache_context.server_info_cache_path()).ok()?;
    let cache: MotygaAppsServerInfoDiskCache = serde_json::from_slice(&bytes).ok()?;
    (cache.schema_version == MOTYGA_APPS_SERVER_INFO_CACHE_SCHEMA_VERSION)
        .then_some(cache.server_info)
}

fn write_cached_motyga_apps_server_info(
    cache_context: &MotygaAppsToolsCacheContext,
    server_info: &McpServerInfo,
) -> anyhow::Result<()> {
    let cache_path = cache_context.server_info_cache_path();
    let bytes = serde_json::to_vec_pretty(&MotygaAppsServerInfoDiskCache {
        schema_version: MOTYGA_APPS_SERVER_INFO_CACHE_SCHEMA_VERSION,
        server_info: server_info.clone(),
    })
    .context("failed to serialize Motyga Apps server info cache")?;
    write_motyga_apps_cache_file(&cache_path, "server info", bytes)
}

fn write_motyga_apps_cache_file(
    cache_path: &Path,
    cache_name: &str,
    bytes: Vec<u8>,
) -> anyhow::Result<()> {
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create Motyga Apps {cache_name} cache directory `{}`",
                parent.display()
            )
        })?;
    }
    std::fs::write(cache_path, bytes).with_context(|| {
        format!(
            "failed to write Motyga Apps {cache_name} cache `{}`",
            cache_path.display()
        )
    })?;
    Ok(())
}

fn persist_motyga_apps_cache(
    cache_context: &MotygaAppsToolsCacheContext,
    server_info: &McpServerInfo,
    tools: &[ToolInfo],
) {
    let cache_write_start = Instant::now();
    let tools_result = write_cached_motyga_apps_tools(cache_context, tools);
    if let Err(err) = &tools_result {
        tracing::warn!("failed to write Motyga Apps tools cache: {err:#}");
    }
    let server_info_result = write_cached_motyga_apps_server_info(cache_context, server_info);
    if let Err(err) = &server_info_result {
        tracing::warn!("failed to write Motyga Apps server info cache: {err:#}");
    }
    let status = if tools_result.is_ok() && server_info_result.is_ok() {
        "success"
    } else {
        "failure"
    };
    emit_duration(
        MCP_TOOLS_CACHE_WRITE_DURATION_METRIC,
        cache_write_start.elapsed(),
        &[("status", status)],
    );
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MotygaAppsToolsDiskCache {
    schema_version: u8,
    tools: Vec<ToolInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MotygaAppsServerInfoDiskCache {
    schema_version: u8,
    server_info: McpServerInfo,
}

const MOTYGA_APPS_TOOLS_CACHE_DIR: &str = "cache/motyga_apps_tools";
const MOTYGA_APPS_TOOLS_CACHE_SCHEMA_VERSION: u8 = 4;

const MOTYGA_APPS_SERVER_INFO_CACHE_DIR: &str = "cache/motyga_apps_server_info";
const MOTYGA_APPS_SERVER_INFO_CACHE_SCHEMA_VERSION: u8 = 1;

fn sha1_hex(s: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(s.as_bytes());
    let sha1 = hasher.finalize();
    format!("{sha1:x}")
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
#[path = "motyga_apps_cache_tests.rs"]
mod tests;
