use crate::bespoke_event_handling::apply_bespoke_event_handling;
use crate::bespoke_event_handling::maybe_emit_hook_prompt_item_completed;
use crate::command_exec::CommandExecManager;
use crate::command_exec::StartCommandExecParams;
use crate::config_manager::ConfigManager;
use crate::error_code::INPUT_TOO_LARGE_ERROR_CODE;
use crate::error_code::invalid_params;
use crate::models::supported_models;
use crate::outgoing_message::ConnectionId;
use crate::outgoing_message::ConnectionRequestId;
use crate::outgoing_message::OutgoingMessageSender;
use crate::outgoing_message::RequestContext;
use crate::outgoing_message::ThreadScopedOutgoingMessageSender;
use crate::skills_watcher::SkillsWatcher;
use crate::thread_status::ThreadWatchManager;
use crate::thread_status::resolve_thread_status;
use chrono::Duration as ChronoDuration;
use chrono::SecondsFormat;
use motyga_analytics::AnalyticsEventsClient;
use motyga_analytics::AnalyticsJsonRpcError;
use motyga_analytics::InputError;
use motyga_analytics::TurnSteerRequestError;
use motyga_app_server_protocol::Account;
use motyga_app_server_protocol::AccountLoginCompletedNotification;
use motyga_app_server_protocol::AccountTokenUsageDailyBucket;
use motyga_app_server_protocol::AccountTokenUsageSummary;
use motyga_app_server_protocol::AccountUpdatedNotification;
use motyga_app_server_protocol::AddCreditsNudgeCreditType;
use motyga_app_server_protocol::AddCreditsNudgeEmailStatus;
use motyga_app_server_protocol::AdditionalContextEntry;
use motyga_app_server_protocol::AdditionalContextKind;
use motyga_app_server_protocol::AppListUpdatedNotification;
use motyga_app_server_protocol::AppSummary;
use motyga_app_server_protocol::AppTemplateSummary;
use motyga_app_server_protocol::AppTemplateUnavailableReason;
use motyga_app_server_protocol::AppsListParams;
use motyga_app_server_protocol::AppsListResponse;
use motyga_app_server_protocol::AskForApproval;
use motyga_app_server_protocol::AuthMode;
use motyga_app_server_protocol::CancelLoginAccountParams;
use motyga_app_server_protocol::CancelLoginAccountResponse;
use motyga_app_server_protocol::CancelLoginAccountStatus;
use motyga_app_server_protocol::ClientInfo;
use motyga_app_server_protocol::ClientRequest;
use motyga_app_server_protocol::ClientResponsePayload;
use motyga_app_server_protocol::MotygaErrorInfo;
use motyga_app_server_protocol::CollaborationModeListParams;
use motyga_app_server_protocol::CollaborationModeListResponse;
use motyga_app_server_protocol::CommandExecParams;
use motyga_app_server_protocol::CommandExecResizeParams;
use motyga_app_server_protocol::CommandExecTerminateParams;
use motyga_app_server_protocol::CommandExecWriteParams;
use motyga_app_server_protocol::ConfigWarningNotification;
use motyga_app_server_protocol::ConsumeAccountRateLimitResetCreditOutcome;
use motyga_app_server_protocol::ConsumeAccountRateLimitResetCreditParams;
use motyga_app_server_protocol::ConsumeAccountRateLimitResetCreditResponse;
use motyga_app_server_protocol::ConversationGitInfo;
use motyga_app_server_protocol::ConversationSummary;
use motyga_app_server_protocol::DeprecationNoticeNotification;
use motyga_app_server_protocol::DynamicToolFunctionSpec;
use motyga_app_server_protocol::DynamicToolNamespaceTool;
use motyga_app_server_protocol::DynamicToolSpec;
use motyga_app_server_protocol::EnvironmentAddParams;
use motyga_app_server_protocol::EnvironmentAddResponse;
use motyga_app_server_protocol::EnvironmentInfoParams;
use motyga_app_server_protocol::EnvironmentInfoResponse;
use motyga_app_server_protocol::EnvironmentShellInfo;
use motyga_app_server_protocol::ExperimentalFeature as ApiExperimentalFeature;
use motyga_app_server_protocol::ExperimentalFeatureListParams;
use motyga_app_server_protocol::ExperimentalFeatureListResponse;
use motyga_app_server_protocol::ExperimentalFeatureStage as ApiExperimentalFeatureStage;
use motyga_app_server_protocol::FeedbackUploadParams;
use motyga_app_server_protocol::FeedbackUploadResponse;
use motyga_app_server_protocol::GetAccountParams;
use motyga_app_server_protocol::GetAccountRateLimitsResponse;
use motyga_app_server_protocol::GetAccountResponse;
use motyga_app_server_protocol::GetAccountTokenUsageResponse;
use motyga_app_server_protocol::GetAuthStatusParams;
use motyga_app_server_protocol::GetAuthStatusResponse;
use motyga_app_server_protocol::GetConversationSummaryParams;
use motyga_app_server_protocol::GetConversationSummaryResponse;
use motyga_app_server_protocol::GetWorkspaceMessagesResponse;
use motyga_app_server_protocol::GitDiffToRemoteParams;
use motyga_app_server_protocol::GitDiffToRemoteResponse;
use motyga_app_server_protocol::GitInfo as ApiGitInfo;
use motyga_app_server_protocol::HookMetadata;
use motyga_app_server_protocol::HooksListParams;
use motyga_app_server_protocol::HooksListResponse;
use motyga_app_server_protocol::InitializeParams;
use motyga_app_server_protocol::InitializeResponse;
use motyga_app_server_protocol::JSONRPCErrorError;
use motyga_app_server_protocol::ListMcpServerStatusParams;
use motyga_app_server_protocol::ListMcpServerStatusResponse;
use motyga_app_server_protocol::LoginAccountParams;
use motyga_app_server_protocol::LoginAccountResponse;
use motyga_app_server_protocol::LoginApiKeyParams;
use motyga_app_server_protocol::LogoutAccountResponse;
use motyga_app_server_protocol::MarketplaceAddParams;
use motyga_app_server_protocol::MarketplaceAddResponse;
use motyga_app_server_protocol::MarketplaceInterface;
use motyga_app_server_protocol::MarketplaceRemoveParams;
use motyga_app_server_protocol::MarketplaceRemoveResponse;
use motyga_app_server_protocol::MarketplaceUpgradeErrorInfo;
use motyga_app_server_protocol::MarketplaceUpgradeParams;
use motyga_app_server_protocol::MarketplaceUpgradeResponse;
use motyga_app_server_protocol::McpResourceReadParams;
use motyga_app_server_protocol::McpResourceReadResponse;
use motyga_app_server_protocol::McpServerOauthLoginCompletedNotification;
use motyga_app_server_protocol::McpServerOauthLoginParams;
use motyga_app_server_protocol::McpServerOauthLoginResponse;
use motyga_app_server_protocol::McpServerRefreshResponse;
use motyga_app_server_protocol::McpServerStatus;
use motyga_app_server_protocol::McpServerStatusDetail;
use motyga_app_server_protocol::McpServerToolCallParams;
use motyga_app_server_protocol::McpServerToolCallResponse;
use motyga_app_server_protocol::MemoryResetResponse;
use motyga_app_server_protocol::MockExperimentalMethodParams;
use motyga_app_server_protocol::MockExperimentalMethodResponse;
use motyga_app_server_protocol::ModelListParams;
use motyga_app_server_protocol::ModelListResponse;
use motyga_app_server_protocol::PermissionProfileListParams;
use motyga_app_server_protocol::PermissionProfileListResponse;
use motyga_app_server_protocol::PermissionProfileSummary;
use motyga_app_server_protocol::PluginDetail;
use motyga_app_server_protocol::PluginInstallParams;
use motyga_app_server_protocol::PluginInstallResponse;
use motyga_app_server_protocol::PluginInstalledParams;
use motyga_app_server_protocol::PluginInstalledResponse;
use motyga_app_server_protocol::PluginInterface;
use motyga_app_server_protocol::PluginListMarketplaceKind;
use motyga_app_server_protocol::PluginListParams;
use motyga_app_server_protocol::PluginListResponse;
use motyga_app_server_protocol::PluginMarketplaceEntry;
use motyga_app_server_protocol::PluginReadParams;
use motyga_app_server_protocol::PluginReadResponse;
use motyga_app_server_protocol::PluginShareCheckoutParams;
use motyga_app_server_protocol::PluginShareCheckoutResponse;
use motyga_app_server_protocol::PluginShareContext;
use motyga_app_server_protocol::PluginShareDeleteParams;
use motyga_app_server_protocol::PluginShareDeleteResponse;
use motyga_app_server_protocol::PluginShareDiscoverability;
use motyga_app_server_protocol::PluginShareListItem;
use motyga_app_server_protocol::PluginShareListParams;
use motyga_app_server_protocol::PluginShareListResponse;
use motyga_app_server_protocol::PluginSharePrincipal;
use motyga_app_server_protocol::PluginSharePrincipalType;
use motyga_app_server_protocol::PluginShareSaveParams;
use motyga_app_server_protocol::PluginShareSaveResponse;
use motyga_app_server_protocol::PluginShareTarget;
use motyga_app_server_protocol::PluginShareUpdateDiscoverability;
use motyga_app_server_protocol::PluginShareUpdateTargetsParams;
use motyga_app_server_protocol::PluginShareUpdateTargetsResponse;
use motyga_app_server_protocol::PluginSkillReadParams;
use motyga_app_server_protocol::PluginSkillReadResponse;
use motyga_app_server_protocol::PluginSource;
use motyga_app_server_protocol::PluginSummary;
use motyga_app_server_protocol::PluginUninstallParams;
use motyga_app_server_protocol::PluginUninstallResponse;
use motyga_app_server_protocol::RateLimitResetCreditsSummary;
use motyga_app_server_protocol::RequestId;
use motyga_app_server_protocol::ReviewDelivery as ApiReviewDelivery;
use motyga_app_server_protocol::ReviewStartParams;
use motyga_app_server_protocol::ReviewStartResponse;
use motyga_app_server_protocol::ReviewTarget as ApiReviewTarget;
use motyga_app_server_protocol::SandboxMode;
use motyga_app_server_protocol::SendAddCreditsNudgeEmailParams;
use motyga_app_server_protocol::SendAddCreditsNudgeEmailResponse;
use motyga_app_server_protocol::ServerNotification;
use motyga_app_server_protocol::ServerRequestResolvedNotification;
use motyga_app_server_protocol::SkillSummary;
use motyga_app_server_protocol::SkillsConfigWriteParams;
use motyga_app_server_protocol::SkillsConfigWriteResponse;
use motyga_app_server_protocol::SkillsExtraRootsSetParams;
use motyga_app_server_protocol::SkillsExtraRootsSetResponse;
use motyga_app_server_protocol::SkillsListParams;
use motyga_app_server_protocol::SkillsListResponse;
use motyga_app_server_protocol::SortDirection;
use motyga_app_server_protocol::Thread;
use motyga_app_server_protocol::ThreadApproveGuardianDeniedActionParams;
use motyga_app_server_protocol::ThreadApproveGuardianDeniedActionResponse;
use motyga_app_server_protocol::ThreadArchiveParams;
use motyga_app_server_protocol::ThreadArchiveResponse;
use motyga_app_server_protocol::ThreadArchivedNotification;
use motyga_app_server_protocol::ThreadBackgroundTerminal;
use motyga_app_server_protocol::ThreadBackgroundTerminalsCleanParams;
use motyga_app_server_protocol::ThreadBackgroundTerminalsCleanResponse;
use motyga_app_server_protocol::ThreadBackgroundTerminalsListParams;
use motyga_app_server_protocol::ThreadBackgroundTerminalsListResponse;
use motyga_app_server_protocol::ThreadBackgroundTerminalsTerminateParams;
use motyga_app_server_protocol::ThreadBackgroundTerminalsTerminateResponse;
use motyga_app_server_protocol::ThreadClosedNotification;
use motyga_app_server_protocol::ThreadCompactStartParams;
use motyga_app_server_protocol::ThreadCompactStartResponse;
use motyga_app_server_protocol::ThreadDecrementElicitationParams;
use motyga_app_server_protocol::ThreadDecrementElicitationResponse;
use motyga_app_server_protocol::ThreadDeleteParams;
use motyga_app_server_protocol::ThreadDeleteResponse;
use motyga_app_server_protocol::ThreadDeletedNotification;
use motyga_app_server_protocol::ThreadForkParams;
use motyga_app_server_protocol::ThreadForkResponse;
use motyga_app_server_protocol::ThreadGoal;
use motyga_app_server_protocol::ThreadGoalClearParams;
use motyga_app_server_protocol::ThreadGoalClearResponse;
use motyga_app_server_protocol::ThreadGoalClearedNotification;
use motyga_app_server_protocol::ThreadGoalGetParams;
use motyga_app_server_protocol::ThreadGoalGetResponse;
use motyga_app_server_protocol::ThreadGoalSetParams;
use motyga_app_server_protocol::ThreadGoalSetResponse;
use motyga_app_server_protocol::ThreadGoalStatus;
use motyga_app_server_protocol::ThreadGoalUpdatedNotification;
use motyga_app_server_protocol::ThreadHistoryBuilder;
#[cfg(test)]
use motyga_app_server_protocol::ThreadHistoryMode;
use motyga_app_server_protocol::ThreadIncrementElicitationParams;
use motyga_app_server_protocol::ThreadIncrementElicitationResponse;
use motyga_app_server_protocol::ThreadInjectItemsParams;
use motyga_app_server_protocol::ThreadInjectItemsResponse;
use motyga_app_server_protocol::ThreadItem;
use motyga_app_server_protocol::ThreadItemsListParams;
use motyga_app_server_protocol::ThreadItemsListResponse;
use motyga_app_server_protocol::ThreadListCwdFilter;
use motyga_app_server_protocol::ThreadListParams;
use motyga_app_server_protocol::ThreadListResponse;
use motyga_app_server_protocol::ThreadLoadedListParams;
use motyga_app_server_protocol::ThreadLoadedListResponse;
use motyga_app_server_protocol::ThreadMemoryModeSetParams;
use motyga_app_server_protocol::ThreadMemoryModeSetResponse;
use motyga_app_server_protocol::ThreadMetadataGitInfoUpdateParams;
use motyga_app_server_protocol::ThreadMetadataUpdateParams;
use motyga_app_server_protocol::ThreadMetadataUpdateResponse;
use motyga_app_server_protocol::ThreadNameUpdatedNotification;
use motyga_app_server_protocol::ThreadReadParams;
use motyga_app_server_protocol::ThreadReadResponse;
use motyga_app_server_protocol::ThreadRealtimeAppendAudioParams;
use motyga_app_server_protocol::ThreadRealtimeAppendAudioResponse;
use motyga_app_server_protocol::ThreadRealtimeAppendSpeechParams;
use motyga_app_server_protocol::ThreadRealtimeAppendSpeechResponse;
use motyga_app_server_protocol::ThreadRealtimeAppendTextParams;
use motyga_app_server_protocol::ThreadRealtimeAppendTextResponse;
use motyga_app_server_protocol::ThreadRealtimeListVoicesResponse;
use motyga_app_server_protocol::ThreadRealtimeStartParams;
use motyga_app_server_protocol::ThreadRealtimeStartResponse;
use motyga_app_server_protocol::ThreadRealtimeStartTransport;
use motyga_app_server_protocol::ThreadRealtimeStopParams;
use motyga_app_server_protocol::ThreadRealtimeStopResponse;
use motyga_app_server_protocol::ThreadResumeInitialTurnsPageParams;
use motyga_app_server_protocol::ThreadResumeParams;
use motyga_app_server_protocol::ThreadResumeResponse;
use motyga_app_server_protocol::ThreadRollbackParams;
use motyga_app_server_protocol::ThreadSearchParams;
use motyga_app_server_protocol::ThreadSearchResponse;
use motyga_app_server_protocol::ThreadSearchResult;
use motyga_app_server_protocol::ThreadSetNameParams;
use motyga_app_server_protocol::ThreadSetNameResponse;
use motyga_app_server_protocol::ThreadSettings;
use motyga_app_server_protocol::ThreadSettingsUpdateParams;
use motyga_app_server_protocol::ThreadSettingsUpdateResponse;
use motyga_app_server_protocol::ThreadShellCommandParams;
use motyga_app_server_protocol::ThreadShellCommandResponse;
use motyga_app_server_protocol::ThreadSortKey;
use motyga_app_server_protocol::ThreadSourceKind;
use motyga_app_server_protocol::ThreadStartParams;
use motyga_app_server_protocol::ThreadStartResponse;
use motyga_app_server_protocol::ThreadStartedNotification;
use motyga_app_server_protocol::ThreadStatus;
use motyga_app_server_protocol::ThreadTurnsListParams;
use motyga_app_server_protocol::ThreadTurnsListResponse;
use motyga_app_server_protocol::ThreadUnarchiveParams;
use motyga_app_server_protocol::ThreadUnarchiveResponse;
use motyga_app_server_protocol::ThreadUnarchivedNotification;
use motyga_app_server_protocol::ThreadUnsubscribeParams;
use motyga_app_server_protocol::ThreadUnsubscribeResponse;
use motyga_app_server_protocol::ThreadUnsubscribeStatus;
use motyga_app_server_protocol::Turn;
use motyga_app_server_protocol::TurnEnvironmentParams;
use motyga_app_server_protocol::TurnError;
use motyga_app_server_protocol::TurnInterruptParams;
use motyga_app_server_protocol::TurnInterruptResponse;
use motyga_app_server_protocol::TurnItemsView;
use motyga_app_server_protocol::TurnStartParams;
use motyga_app_server_protocol::TurnStartResponse;
use motyga_app_server_protocol::TurnStatus;
use motyga_app_server_protocol::TurnSteerParams;
use motyga_app_server_protocol::TurnSteerResponse;
use motyga_app_server_protocol::UserInput as V2UserInput;
use motyga_app_server_protocol::WindowsSandboxReadiness;
use motyga_app_server_protocol::WindowsSandboxReadinessResponse;
use motyga_app_server_protocol::WindowsSandboxSetupCompletedNotification;
use motyga_app_server_protocol::WindowsSandboxSetupMode;
use motyga_app_server_protocol::WindowsSandboxSetupStartParams;
use motyga_app_server_protocol::WindowsSandboxSetupStartResponse;
use motyga_app_server_protocol::WorkspaceMessage;
use motyga_app_server_protocol::WorkspaceMessageType;
use motyga_arg0::Arg0DispatchPaths;
use motyga_backend_client::AddCreditsNudgeCreditType as BackendAddCreditsNudgeCreditType;
use motyga_backend_client::Client as BackendClient;
use motyga_backend_client::MotygaWorkspaceMessage as BackendWorkspaceMessage;
use motyga_backend_client::MotygaWorkspaceMessageType as BackendWorkspaceMessageType;
use motyga_backend_client::MotygaWorkspaceMessagesResponse as BackendWorkspaceMessagesResponse;
use motyga_backend_client::ConsumeRateLimitResetCreditCode as BackendConsumeRateLimitResetCreditCode;
use motyga_backend_client::RequestError as BackendRequestError;
use motyga_backend_client::TokenUsageProfile;
use motyga_chatgpt::connectors;
use motyga_chatgpt::workspace_settings;
use motyga_config::CloudConfigBundleLoadError;
use motyga_config::CloudConfigBundleLoadErrorCode;
use motyga_config::ConfigLayerStack;
use motyga_config::loader::project_trust_key;
use motyga_config::types::McpServerTransportConfig;
use motyga_connectors::AppInfo;
use motyga_core::MotygaThread;
use motyga_core::MotygaThreadSettingsOverrides;
use motyga_core::ForkSnapshot;
use motyga_core::McpManager;
use motyga_core::NewThread;
#[cfg(test)]
use motyga_core::SessionMeta;
use motyga_core::StartThreadOptions;
use motyga_core::SteerInputError;
use motyga_core::ThreadConfigSnapshot;
use motyga_core::ThreadManager;
use motyga_core::config::Config;
use motyga_core::config::ConfigOverrides;
use motyga_core::config::NetworkProxyAuditMetadata;
use motyga_core::config::edit::ConfigEdit;
use motyga_core::config::edit::ConfigEditsBuilder;
use motyga_core::connectors::AccessibleConnectorsStatus;
use motyga_core::exec::ExecCapturePolicy;
use motyga_core::exec::ExecExpiration;
use motyga_core::exec::ExecParams;
use motyga_core::exec_env::create_env;
use motyga_core::path_utils;
#[cfg(test)]
use motyga_core::read_head_for_summary;
use motyga_core::sandboxing::SandboxPermissions;
use motyga_core::truncate_rollout_after_turn_id;
use motyga_core::windows_sandbox::WindowsSandboxLevelExt;
use motyga_core::windows_sandbox::WindowsSandboxSetupMode as CoreWindowsSandboxSetupMode;
use motyga_core::windows_sandbox::WindowsSandboxSetupRequest;
use motyga_core::windows_sandbox::sandbox_setup_is_complete;
use motyga_core_plugins::PluginInstallError as CorePluginInstallError;
use motyga_core_plugins::PluginInstallRequest;
use motyga_core_plugins::PluginReadRequest;
use motyga_core_plugins::PluginUninstallError as CorePluginUninstallError;
use motyga_core_plugins::PluginsManager;
use motyga_core_plugins::loader::load_plugin_apps;
use motyga_core_plugins::loader::load_plugin_mcp_servers;
use motyga_core_plugins::manifest::PluginManifestInterface;
use motyga_core_plugins::marketplace::MarketplaceError;
use motyga_core_plugins::marketplace::MarketplacePluginSource;
use motyga_core_plugins::marketplace_add::MarketplaceAddError;
use motyga_core_plugins::marketplace_add::MarketplaceAddRequest;
use motyga_core_plugins::marketplace_add::add_marketplace as add_marketplace_to_motyga_home;
use motyga_core_plugins::marketplace_remove::MarketplaceRemoveError;
use motyga_core_plugins::marketplace_remove::MarketplaceRemoveRequest as CoreMarketplaceRemoveRequest;
use motyga_core_plugins::marketplace_remove::remove_marketplace;
use motyga_core_plugins::remote::RemoteMarketplace;
use motyga_core_plugins::remote::RemoteMarketplaceSource;
use motyga_core_plugins::remote::RemotePluginCatalogError;
use motyga_core_plugins::remote::RemotePluginDetail as RemoteCatalogPluginDetail;
use motyga_core_plugins::remote::RemotePluginServiceConfig;
use motyga_core_plugins::remote::RemotePluginShareContext as RemoteCatalogPluginShareContext;
use motyga_core_plugins::remote::RemotePluginShareSummary as RemoteCatalogPluginShareSummary;
use motyga_core_plugins::remote::RemotePluginSummary as RemoteCatalogPluginSummary;
use motyga_exec_server::EnvironmentManager;
use motyga_exec_server::LOCAL_ENVIRONMENT_ID;
use motyga_exec_server::LOCAL_FS;
use motyga_features::FEATURES;
use motyga_features::Feature;
use motyga_features::Stage;
use motyga_feedback::MotygaFeedback;
use motyga_feedback::FeedbackAttachmentPath;
use motyga_feedback::FeedbackUploadOptions;
use motyga_git_utils::git_diff_to_remote;
use motyga_git_utils::resolve_root_git_project_for_trust;
use motyga_login::AuthManager;
use motyga_login::MotygaAuth;
use motyga_login::ServerOptions as LoginServerOptions;
use motyga_login::ShutdownHandle;
use motyga_login::auth::login_with_chatgpt_auth_tokens;
use motyga_login::complete_device_code_login;
use motyga_login::login_with_api_key;
use motyga_login::oauth_client_id;
use motyga_login::request_device_code;
use motyga_login::run_login_server;
use motyga_mcp::McpRuntimeContext;
use motyga_mcp::McpServerStatusSnapshot;
use motyga_mcp::McpSnapshotDetail;
use motyga_mcp::collect_mcp_server_status_snapshot_with_detail;
use motyga_mcp::discover_supported_scopes_with_http_client;
use motyga_mcp::read_mcp_resource as read_mcp_resource_without_thread;
use motyga_mcp::resolve_oauth_scopes;
use motyga_memories_write::clear_memory_roots_contents;
use motyga_model_provider::create_model_provider;
use motyga_models_manager::collaboration_mode_presets::builtin_collaboration_mode_presets;
use motyga_protocol::ThreadId;
use motyga_protocol::config_types::CollaborationMode;
use motyga_protocol::config_types::ForcedLoginMethod;
use motyga_protocol::config_types::Personality;
use motyga_protocol::config_types::ReasoningSummary;
use motyga_protocol::config_types::TrustLevel;
use motyga_protocol::config_types::WindowsSandboxLevel;
use motyga_protocol::error::MotygaErr;
use motyga_protocol::error::Result as MotygaResult;
#[cfg(test)]
use motyga_protocol::items::TurnItem;
use motyga_protocol::models::ResponseItem;
use motyga_protocol::openai_models::ReasoningEffort;
#[cfg(test)]
use motyga_protocol::permissions::FileSystemSandboxPolicy;
use motyga_protocol::protocol::AgentStatus;
use motyga_protocol::protocol::ConversationAudioParams;
use motyga_protocol::protocol::ConversationSpeechParams;
use motyga_protocol::protocol::ConversationStartParams;
use motyga_protocol::protocol::ConversationStartTransport;
use motyga_protocol::protocol::ConversationTextParams;
use motyga_protocol::protocol::EventMsg;
#[cfg(test)]
use motyga_protocol::protocol::GitInfo as CoreGitInfo;
use motyga_protocol::protocol::InitialHistory;
use motyga_protocol::protocol::McpAuthStatus as CoreMcpAuthStatus;
use motyga_protocol::protocol::Op;
use motyga_protocol::protocol::RealtimeVoicesList;
use motyga_protocol::protocol::ResumedHistory;
use motyga_protocol::protocol::ReviewDelivery as CoreReviewDelivery;
use motyga_protocol::protocol::ReviewRequest;
use motyga_protocol::protocol::ReviewTarget as CoreReviewTarget;
use motyga_protocol::protocol::RolloutItem;
use motyga_protocol::protocol::SessionConfiguredEvent;
#[cfg(test)]
use motyga_protocol::protocol::SessionMetaLine;
use motyga_protocol::protocol::TurnEnvironmentSelection;
use motyga_protocol::protocol::TurnEnvironmentSelections;
use motyga_protocol::protocol::USER_MESSAGE_BEGIN;
use motyga_protocol::protocol::W3cTraceContext;
use motyga_protocol::user_input::MAX_USER_INPUT_TEXT_CHARS;
use motyga_protocol::user_input::UserInput as CoreInputItem;
use motyga_rmcp_client::perform_oauth_login_return_url_with_http_client;
use motyga_rollout::is_persisted_rollout_item;
use motyga_rollout::state_db::StateDbHandle;
use motyga_rollout::state_db::reconcile_rollout;
use motyga_state::ThreadMetadata;
use motyga_state::log_db::LogDbLayer;
use motyga_thread_store::ArchiveThreadParams as StoreArchiveThreadParams;
use motyga_thread_store::DeleteThreadParams as StoreDeleteThreadParams;
use motyga_thread_store::GitInfoPatch as StoreGitInfoPatch;
use motyga_thread_store::ListItemsParams as StoreListItemsParams;
use motyga_thread_store::ListThreadsParams as StoreListThreadsParams;
use motyga_thread_store::LocalThreadStore;
use motyga_thread_store::ReadThreadByRolloutPathParams as StoreReadThreadByRolloutPathParams;
use motyga_thread_store::ReadThreadParams as StoreReadThreadParams;
use motyga_thread_store::SearchThreadsParams as StoreSearchThreadsParams;
use motyga_thread_store::SortDirection as StoreSortDirection;
use motyga_thread_store::StoredThread;
use motyga_thread_store::ThreadMetadataPatch as StoreThreadMetadataPatch;
use motyga_thread_store::ThreadRelationFilter as StoreThreadRelationFilter;
use motyga_thread_store::ThreadSortKey as StoreThreadSortKey;
use motyga_thread_store::ThreadStore;
use motyga_thread_store::ThreadStoreError;
use motyga_utils_absolute_path::AbsolutePathBuf;
use motyga_utils_pty::DEFAULT_OUTPUT_BYTES_CAP;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Error as IoError;
use std::path::Path;
use std::path::PathBuf;
use std::result::Result;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;
use tokio::sync::broadcast;
use tokio::sync::oneshot;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use tokio_util::sync::DropGuard;
use tokio_util::task::TaskTracker;
use toml::Value as TomlValue;
use tracing::Instrument;
use tracing::error;
use tracing::info;
use tracing::warn;
use uuid::Uuid;

#[cfg(test)]
use motyga_app_server_protocol::ServerRequest;

mod account_processor;
mod apps_processor;
mod catalog_processor;
mod command_exec_processor;
mod config_processor;
mod environment_processor;
mod external_agent_config_processor;
mod external_agent_session_import;
mod feedback_doctor_report;
mod feedback_processor;
mod fs_processor;
mod git_processor;
mod initialize_processor;
mod marketplace_processor;
mod mcp_processor;
mod plugins;
mod process_exec_processor;
mod remote_control_processor;
mod search;
mod thread_processor;
mod token_usage_replay;
mod turn_processor;
mod windows_sandbox_processor;

pub(crate) use account_processor::AccountRequestProcessor;
pub(crate) use apps_processor::AppsRequestProcessor;
pub(crate) use catalog_processor::CatalogRequestProcessor;
pub(crate) use command_exec_processor::CommandExecRequestProcessor;
pub(crate) use config_processor::ConfigRequestProcessor;
pub(crate) use environment_processor::EnvironmentRequestProcessor;
pub(crate) use external_agent_config_processor::ExternalAgentConfigRequestProcessor;
pub(crate) use external_agent_config_processor::ExternalAgentConfigRequestProcessorArgs;
pub(crate) use feedback_processor::FeedbackRequestProcessor;
pub(crate) use fs_processor::FsRequestProcessor;
pub(crate) use git_processor::GitRequestProcessor;
pub(crate) use initialize_processor::InitializeRequestProcessor;
pub(crate) use marketplace_processor::MarketplaceRequestProcessor;
pub(crate) use mcp_processor::McpRequestProcessor;
pub(crate) use plugins::PluginRequestProcessor;
pub(crate) use process_exec_processor::ProcessExecRequestProcessor;
pub(crate) use remote_control_processor::RemoteControlRequestProcessor;
pub(crate) use search::SearchRequestProcessor;
pub(crate) use thread_goal_processor::ThreadGoalRequestProcessor;
pub(crate) use thread_processor::ThreadRequestProcessor;
pub(crate) use turn_processor::TurnRequestProcessor;
pub(crate) use windows_sandbox_processor::WindowsSandboxRequestProcessor;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;
use crate::filters::compute_source_filters;
use crate::filters::source_kind_matches;
use crate::thread_state::ConnectionCapabilities;
use crate::thread_state::ThreadListenerCommand;
use crate::thread_state::ThreadState;
use crate::thread_state::ThreadStateManager;
use token_usage_replay::latest_token_usage_turn_id_from_rollout_items;
use token_usage_replay::send_thread_token_usage_update_to_connection;

fn resolve_request_cwd(cwd: Option<PathBuf>) -> Result<Option<AbsolutePathBuf>, JSONRPCErrorError> {
    cwd.map(|cwd| {
        AbsolutePathBuf::relative_to_current_dir(path_utils::normalize_for_native_workdir(cwd))
            .map_err(|err| invalid_request(format!("invalid cwd: {err}")))
    })
    .transpose()
}

fn resolve_turn_environment_selections(
    thread_manager: &ThreadManager,
    environments: Option<Vec<TurnEnvironmentParams>>,
) -> Result<Option<Vec<TurnEnvironmentSelection>>, JSONRPCErrorError> {
    let Some(environments) = environments else {
        return Ok(None);
    };
    let mut selections = Vec::with_capacity(environments.len());
    for environment in environments {
        let environment_id = environment.environment_id;
        let cwd = environment
            .cwd
            .to_inferred_path_uri()
            .ok_or_else(|| {
                invalid_request(format!(
                    "invalid cwd for environment `{environment_id}`: path `{}` does not use absolute POSIX or Windows path syntax",
                    environment.cwd
                ))
            })?;
        selections.push(TurnEnvironmentSelection {
            environment_id,
            cwd,
        });
    }
    thread_manager
        .validate_environment_selections(&selections)
        .map_err(environment_selection_error)?;
    Ok(Some(selections))
}

fn resolve_runtime_workspace_roots(workspace_roots: Vec<AbsolutePathBuf>) -> Vec<AbsolutePathBuf> {
    let mut resolved_roots = Vec::new();
    for root in workspace_roots {
        if !resolved_roots.iter().any(|existing| existing == &root) {
            resolved_roots.push(root);
        }
    }
    resolved_roots
}

mod config_errors;
mod request_errors;
mod thread_delete;
mod thread_goal_processor;
mod thread_lifecycle;
mod thread_resume_redaction;
mod thread_summary;

use self::config_errors::*;
use self::request_errors::*;
use self::thread_goal_processor::api_thread_goal_from_state;
use self::thread_lifecycle::*;
use self::thread_resume_redaction::*;
use self::thread_summary::*;

pub(crate) use self::thread_lifecycle::populate_thread_turns_from_history;
pub(crate) use self::thread_processor::thread_from_stored_thread;
#[cfg(test)]
pub(crate) use self::thread_summary::read_summary_from_rollout;
#[cfg(test)]
pub(crate) use self::thread_summary::summary_to_thread;
pub(crate) use self::thread_summary::thread_settings_from_config_snapshot;
pub(crate) use self::thread_summary::thread_settings_from_core_snapshot;

pub(crate) fn build_api_turns_from_rollout_items(items: &[RolloutItem]) -> Vec<Turn> {
    let mut builder = ThreadHistoryBuilder::new();
    for item in items {
        if is_persisted_rollout_item(item) {
            builder.handle_rollout_item(item);
        }
    }
    builder.finish()
}
