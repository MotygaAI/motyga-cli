use super::*;
use crate::mcp::MOTYGA_APPS_MCP_SERVER_NAME;
use crate::tools::ToolInfo;
use motyga_protocol::ToolName;
use motyga_protocol::mcp::McpServerInfo;
use pretty_assertions::assert_eq;
use rmcp::model::JsonObject;
use rmcp::model::Tool;
use std::collections::HashSet;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;

fn create_test_tool(server_name: &str, tool_name: &str) -> ToolInfo {
    ToolInfo {
        server_name: server_name.to_string(),
        supports_parallel_tool_calls: false,
        server_origin: None,
        callable_name: tool_name.to_string(),
        callable_namespace: server_name.to_string(),
        namespace_description: None,
        tool: Tool::new(
            tool_name.to_string(),
            format!("Test tool: {tool_name}"),
            Arc::new(JsonObject::default()),
        ),
        connector_id: None,
        connector_name: None,
        plugin_display_names: Vec::new(),
    }
}

fn create_test_tool_with_connector(
    server_name: &str,
    tool_name: &str,
    connector_id: &str,
    connector_name: Option<&str>,
) -> ToolInfo {
    let mut tool = create_test_tool(server_name, tool_name);
    tool.connector_id = Some(connector_id.to_string());
    tool.connector_name = connector_name.map(ToOwned::to_owned);
    tool
}

fn create_motyga_apps_tools_cache_context(
    motyga_home: PathBuf,
    account_id: Option<&str>,
    chatgpt_user_id: Option<&str>,
) -> MotygaAppsToolsCacheContext {
    MotygaAppsToolsCache::default().context(
        motyga_home,
        MotygaAppsToolsCacheKey {
            account_id: account_id.map(ToOwned::to_owned),
            chatgpt_user_id: chatgpt_user_id.map(ToOwned::to_owned),
            is_workspace_account: false,
        },
    )
}

fn create_test_server_info(title: &str) -> McpServerInfo {
    McpServerInfo {
        name: "motyga-apps".to_string(),
        title: Some(title.to_string()),
        version: "1.0.0".to_string(),
        description: None,
        icons: None,
        website_url: None,
    }
}

fn model_tool_names(tools: &[ToolInfo]) -> HashSet<ToolName> {
    tools
        .iter()
        .map(ToolInfo::canonical_tool_name)
        .collect::<HashSet<_>>()
}

#[test]
fn motyga_apps_tools_cache_is_overwritten_by_last_write() {
    let motyga_home = tempdir().expect("tempdir");
    let cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let tools_gateway_1 = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "one")];
    let tools_gateway_2 = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "two")];

    write_cached_motyga_apps_tools(&cache_context, &tools_gateway_1).expect("write first cache");
    let cached_gateway_1 =
        read_cached_motyga_apps_tools(&cache_context).expect("cache entry exists for first write");
    assert_eq!(cached_gateway_1[0].callable_name, "one");

    write_cached_motyga_apps_tools(&cache_context, &tools_gateway_2).expect("write second cache");
    let cached_gateway_2 =
        read_cached_motyga_apps_tools(&cache_context).expect("cache entry exists for second write");
    assert_eq!(cached_gateway_2[0].callable_name, "two");
}

#[test]
fn motyga_apps_tools_cache_is_scoped_per_user() {
    let motyga_home = tempdir().expect("tempdir");
    let cache_context_user_1 = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let cache_context_user_2 = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-two"),
        Some("user-two"),
    );
    let tools_user_1 = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "one")];
    let tools_user_2 = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "two")];

    write_cached_motyga_apps_tools(&cache_context_user_1, &tools_user_1)
        .expect("write user one cache");
    write_cached_motyga_apps_tools(&cache_context_user_2, &tools_user_2)
        .expect("write user two cache");

    let read_user_1 =
        read_cached_motyga_apps_tools(&cache_context_user_1).expect("cache entry for user one");
    let read_user_2 =
        read_cached_motyga_apps_tools(&cache_context_user_2).expect("cache entry for user two");

    assert_eq!(read_user_1[0].callable_name, "one");
    assert_eq!(read_user_2[0].callable_name, "two");
    assert_ne!(
        cache_context_user_1.tools_cache_path(),
        cache_context_user_2.tools_cache_path(),
        "each user should get an isolated cache file"
    );
}

#[test]
fn motyga_apps_tools_cache_preserves_formerly_disallowed_connectors() {
    let motyga_home = tempdir().expect("tempdir");
    let cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let tools = vec![
        create_test_tool_with_connector(
            MOTYGA_APPS_MCP_SERVER_NAME,
            "formerly_blocked_tool",
            "connector_2b0a9009c9c64bf9933a3dae3f2b1254",
            Some("Formerly Blocked"),
        ),
        create_test_tool_with_connector(
            MOTYGA_APPS_MCP_SERVER_NAME,
            "calendar_tool",
            "calendar",
            Some("Calendar"),
        ),
    ];

    write_cached_motyga_apps_tools(&cache_context, &tools).expect("write cache");
    let cached = read_cached_motyga_apps_tools(&cache_context).expect("cache entry exists for user");

    assert_eq!(
        cached
            .iter()
            .map(|tool| (tool.callable_name.as_str(), tool.connector_id.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (
                "formerly_blocked_tool",
                Some("connector_2b0a9009c9c64bf9933a3dae3f2b1254")
            ),
            ("calendar_tool", Some("calendar")),
        ]
    );
}

#[test]
fn motyga_apps_tools_cache_is_ignored_when_schema_version_mismatches() {
    let motyga_home = tempdir().expect("tempdir");
    let cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let cache_path = cache_context.tools_cache_path();
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": MOTYGA_APPS_TOOLS_CACHE_SCHEMA_VERSION + 1,
        "tools": [create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "one")],
    }))
    .expect("serialize");
    std::fs::write(cache_path, bytes).expect("write");

    assert!(read_cached_motyga_apps_tools(&cache_context).is_none());
}

#[test]
fn motyga_apps_tools_cache_is_ignored_when_json_is_invalid() {
    let motyga_home = tempdir().expect("tempdir");
    let cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let cache_path = cache_context.tools_cache_path();
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    std::fs::write(cache_path, b"{not json").expect("write");

    assert!(read_cached_motyga_apps_tools(&cache_context).is_none());
}

#[test]
fn startup_cached_motyga_apps_tools_loads_from_disk_cache() {
    let motyga_home = tempdir().expect("tempdir");
    let writer_cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let cached_tools = vec![create_test_tool(
        MOTYGA_APPS_MCP_SERVER_NAME,
        "calendar_search",
    )];
    let server_info = create_test_server_info("Motyga Apps");
    write_cached_motyga_apps_tools_for_test(&writer_cache_context, &server_info, &cached_tools);
    let cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );

    let startup_tools = cache_context
        .current_tools()
        .expect("expected startup snapshot to load from cache");
    let cached_server_info = load_startup_cached_motyga_apps_server_info(&cache_context);

    assert_eq!(startup_tools.len(), 1);
    assert_eq!(startup_tools[0].server_name, MOTYGA_APPS_MCP_SERVER_NAME);
    assert_eq!(startup_tools[0].callable_name, "calendar_search");
    assert_eq!(cached_server_info, Some(server_info));
}

#[test]
fn startup_cached_motyga_apps_tools_loads_without_server_info_cache() {
    let motyga_home = tempdir().expect("tempdir");
    let writer_cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let cache_path = writer_cache_context.tools_cache_path();
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": MOTYGA_APPS_TOOLS_CACHE_SCHEMA_VERSION,
        "tools": [create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "calendar_search")],
    }))
    .expect("serialize");
    std::fs::write(cache_path, bytes).expect("write");
    let cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );

    let startup_tools = cache_context
        .current_tools()
        .expect("legacy startup snapshot should remain available");
    let cached_server_info = load_startup_cached_motyga_apps_server_info(&cache_context);

    assert_eq!(startup_tools.len(), 1);
    assert_eq!(startup_tools[0].callable_name, "calendar_search");
    assert_eq!(cached_server_info, None);
}

#[test]
fn motyga_apps_server_info_cache_survives_legacy_tools_cache_write() {
    let motyga_home = tempdir().expect("tempdir");
    let cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let server_info = create_test_server_info("Motyga Apps");
    write_cached_motyga_apps_tools_for_test(
        &cache_context,
        &server_info,
        &[create_test_tool(
            MOTYGA_APPS_MCP_SERVER_NAME,
            "calendar_search",
        )],
    );

    let cache_path = cache_context.tools_cache_path();
    if let Some(parent) = cache_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent");
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({
        "schema_version": MOTYGA_APPS_TOOLS_CACHE_SCHEMA_VERSION - 1,
        "tools": [create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "calendar_search")],
    }))
    .expect("serialize");
    std::fs::write(cache_path, bytes).expect("write legacy tools cache");
    let startup_cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );

    assert_eq!(
        load_startup_cached_motyga_apps_server_info(&startup_cache_context),
        Some(server_info)
    );
    assert!(startup_cache_context.current_tools().is_none());
}

#[test]
fn motyga_apps_tools_cache_context_does_not_reread_disk_after_creation() {
    let motyga_home = tempdir().expect("tempdir");
    let writer_cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let cached_tools = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "cached")];
    write_cached_motyga_apps_tools(&writer_cache_context, &cached_tools).expect("write cache");
    let reader_cache_context = create_motyga_apps_tools_cache_context(
        motyga_home.path().to_path_buf(),
        Some("account-one"),
        Some("user-one"),
    );
    let updated_tools = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "updated")];
    write_cached_motyga_apps_tools(&writer_cache_context, &updated_tools).expect("rewrite cache");

    assert_eq!(
        reader_cache_context
            .current_tools()
            .expect("in-memory tools")[0]
            .callable_name,
        "cached"
    );
    assert_eq!(
        read_cached_motyga_apps_tools(&writer_cache_context).expect("disk tools")[0].callable_name,
        "updated"
    );
}

#[test]
fn motyga_apps_tools_cache_publishes_newest_shared_snapshot() {
    let motyga_home = tempdir().expect("tempdir");
    let cache = MotygaAppsToolsCache::default();
    let cache_context_1 = cache.context(
        motyga_home.path().to_path_buf(),
        MotygaAppsToolsCacheKey {
            account_id: Some("account-one".to_string()),
            chatgpt_user_id: Some("user-one".to_string()),
            is_workspace_account: false,
        },
    );
    let cache_context_2 = cache.context(
        motyga_home.path().to_path_buf(),
        MotygaAppsToolsCacheKey {
            account_id: Some("account-one".to_string()),
            chatgpt_user_id: Some("user-one".to_string()),
            is_workspace_account: false,
        },
    );
    let older_ticket = cache_context_1.begin_fetch(MotygaAppsToolsFetchSource::Startup);
    let newer_ticket = cache_context_2.begin_fetch(MotygaAppsToolsFetchSource::HardRefresh);
    let server_info = create_test_server_info("Motyga Apps");
    let newer_tools = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "newer")];
    let older_tools = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "older")];

    let published_tools =
        cache_context_2.publish_if_newest_accepted(newer_ticket, &server_info, newer_tools);
    assert_eq!(
        model_tool_names(&published_tools),
        model_tool_names(
            &cache_context_1
                .current_tools()
                .expect("new snapshot should publish")
        )
    );
    let current_tools =
        cache_context_1.publish_if_newest_accepted(older_ticket, &server_info, older_tools);

    assert_eq!(current_tools[0].callable_name, "newer");
    assert_eq!(
        cache_context_2.current_tools().expect("shared snapshot")[0].callable_name,
        "newer"
    );
    assert_eq!(
        read_cached_motyga_apps_tools(&cache_context_1).expect("persisted snapshot")[0]
            .callable_name,
        "newer"
    );
}

#[test]
fn motyga_apps_tools_cache_keeps_live_publish_when_disk_persistence_fails() {
    let motyga_home = tempdir().expect("tempdir");
    let motyga_home_file = motyga_home.path().join("not-a-directory");
    std::fs::write(&motyga_home_file, b"occupied").expect("create motyga home file");
    let cache_context = MotygaAppsToolsCache::default().context(
        motyga_home_file,
        MotygaAppsToolsCacheKey {
            account_id: Some("account-one".to_string()),
            chatgpt_user_id: Some("user-one".to_string()),
            is_workspace_account: false,
        },
    );
    let tools = vec![create_test_tool(MOTYGA_APPS_MCP_SERVER_NAME, "live")];
    let published_tools = cache_context.publish_if_newest_accepted(
        cache_context.begin_fetch(MotygaAppsToolsFetchSource::HardRefresh),
        &create_test_server_info("Motyga Apps"),
        tools.clone(),
    );

    assert_eq!(model_tool_names(&published_tools), model_tool_names(&tools));
    assert_eq!(
        model_tool_names(&cache_context.current_tools().expect("live snapshot")),
        model_tool_names(&tools)
    );
}

#[cfg(unix)]
#[test]
fn motyga_apps_tools_cache_scopes_non_utf8_home_disk_paths() {
    let motyga_home = PathBuf::from(std::ffi::OsString::from_vec(
        b"/tmp/motyga-home-\xff".to_vec(),
    ));
    let cache = MotygaAppsToolsCache::default();
    let user_one_context = cache.context(
        motyga_home.clone(),
        MotygaAppsToolsCacheKey {
            account_id: Some("account-one".to_string()),
            chatgpt_user_id: Some("user-one".to_string()),
            is_workspace_account: false,
        },
    );
    let user_two_context = cache.context(
        motyga_home,
        MotygaAppsToolsCacheKey {
            account_id: Some("account-two".to_string()),
            chatgpt_user_id: Some("user-two".to_string()),
            is_workspace_account: false,
        },
    );
    let cache_paths = [
        user_one_context.tools_cache_path(),
        user_two_context.tools_cache_path(),
    ];

    assert_eq!(
        cache_paths.iter().collect::<HashSet<_>>().len(),
        cache_paths.len()
    );
}
