use std::time::Instant;

use crate::facts::AcceptedLineFingerprint;
use crate::facts::AppInvocation;
use crate::facts::MotygaCompactionEvent;
use crate::facts::MotygaErrKind;
use crate::facts::MotygaGoalEvent;
use crate::facts::CompactionImplementation;
use crate::facts::CompactionPhase;
use crate::facts::CompactionReason;
use crate::facts::CompactionStatus;
use crate::facts::CompactionStrategy;
use crate::facts::CompactionTrigger;
use crate::facts::GoalEventKind;
use crate::facts::HookRunFact;
use crate::facts::InvocationType;
use crate::facts::PluginInstallRequested;
use crate::facts::PluginState;
use crate::facts::SubAgentThreadStartedInput;
use crate::facts::ThreadInitializationMode;
use crate::facts::TrackEventsContext;
use crate::facts::TurnStatus;
use crate::facts::TurnSteerRejectionReason;
use crate::facts::TurnSteerResult;
use crate::facts::TurnSubmissionType;
use crate::now_unix_millis;
use motyga_app_server_protocol::MotygaErrorInfo;
use motyga_app_server_protocol::CommandExecutionSource;
use motyga_login::default_client::originator;
use motyga_plugin::PluginId;
use motyga_plugin::PluginTelemetryMetadata;
use motyga_protocol::approvals::NetworkApprovalProtocol;
use motyga_protocol::models::AdditionalPermissionProfile;
use motyga_protocol::models::SandboxPermissions;
use motyga_protocol::protocol::GuardianAssessmentOutcome;
use motyga_protocol::protocol::GuardianCommandSource;
use motyga_protocol::protocol::GuardianRiskLevel;
use motyga_protocol::protocol::GuardianUserAuthorization;
use motyga_protocol::protocol::HookEventName;
use motyga_protocol::protocol::HookRunStatus;
use motyga_protocol::protocol::HookSource;
use motyga_protocol::protocol::SubAgentSource;
use motyga_protocol::protocol::ThreadSource;
use motyga_protocol::protocol::TokenUsage;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppServerRpcTransport {
    Stdio,
    Websocket,
    InProcess,
}

#[derive(Serialize)]
pub(crate) struct TrackEventsRequest {
    pub(crate) events: Vec<TrackEventRequest>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum TrackEventRequest {
    SkillInvocation(SkillInvocationEventRequest),
    ThreadInitialized(ThreadInitializedEvent),
    GuardianReview(Box<GuardianReviewEventRequest>),
    AppMentioned(MotygaAppMentionedEventRequest),
    AppUsed(MotygaAppUsedEventRequest),
    HookRun(MotygaHookRunEventRequest),
    Compaction(Box<MotygaCompactionEventRequest>),
    Goal(Box<MotygaGoalEventRequest>),
    TurnEvent(Box<MotygaTurnEventRequest>),
    TurnSteer(MotygaTurnSteerEventRequest),
    CommandExecution(MotygaCommandExecutionEventRequest),
    FileChange(MotygaFileChangeEventRequest),
    McpToolCall(MotygaMcpToolCallEventRequest),
    DynamicToolCall(MotygaDynamicToolCallEventRequest),
    CollabAgentToolCall(MotygaCollabAgentToolCallEventRequest),
    WebSearch(MotygaWebSearchEventRequest),
    ImageGeneration(MotygaImageGenerationEventRequest),
    AcceptedLineFingerprints(Box<MotygaAcceptedLineFingerprintsEventRequest>),
    #[allow(dead_code)]
    ReviewEvent(MotygaReviewEventRequest),
    PluginUsed(MotygaPluginUsedEventRequest),
    PluginInstallRequested(MotygaPluginInstallRequestedEventRequest),
    PluginInstalled(MotygaPluginEventRequest),
    PluginUninstalled(MotygaPluginEventRequest),
    PluginEnabled(MotygaPluginEventRequest),
    PluginDisabled(MotygaPluginEventRequest),
    PluginInstallFailed(MotygaPluginInstallFailedEventRequest),
    ExternalAgentConfigImportCompleted(MotygaOnboardingExternalAgentImportCompleteEventRequest),
    ExternalAgentConfigImportFailure(MotygaOnboardingExternalAgentImportFailureEventRequest),
}

impl TrackEventRequest {
    pub(crate) fn should_send_in_isolated_request(&self) -> bool {
        matches!(self, Self::AcceptedLineFingerprints(_))
    }
}

#[derive(Serialize)]
pub(crate) struct MotygaAcceptedLineFingerprintsEventParams {
    pub(crate) event_type: &'static str,
    pub(crate) turn_id: String,
    pub(crate) thread_id: String,
    pub(crate) product_surface: Option<String>,
    pub(crate) model_slug: Option<String>,
    pub(crate) completed_at: u64,
    pub(crate) repo_hash: Option<String>,
    pub(crate) accepted_added_lines: u64,
    pub(crate) accepted_deleted_lines: u64,
    pub(crate) line_fingerprints: Vec<AcceptedLineFingerprint>,
}

#[derive(Serialize)]
pub(crate) struct MotygaAcceptedLineFingerprintsEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaAcceptedLineFingerprintsEventParams,
}

#[derive(Serialize)]
pub(crate) struct SkillInvocationEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) skill_id: String,
    pub(crate) skill_name: String,
    pub(crate) event_params: SkillInvocationEventParams,
}

#[derive(Serialize)]
pub(crate) struct SkillInvocationEventParams {
    pub(crate) product_client_id: Option<String>,
    pub(crate) skill_scope: Option<String>,
    pub(crate) plugin_id: Option<String>,
    pub(crate) repo_url: Option<String>,
    pub(crate) thread_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) invoke_type: Option<InvocationType>,
    pub(crate) model_slug: Option<String>,
}

#[derive(Clone, Serialize)]
pub(crate) struct MotygaAppServerClientMetadata {
    pub(crate) product_client_id: String,
    pub(crate) client_name: Option<String>,
    pub(crate) client_version: Option<String>,
    pub(crate) rpc_transport: AppServerRpcTransport,
    pub(crate) experimental_api_enabled: Option<bool>,
}

#[derive(Clone, Serialize)]
pub(crate) struct MotygaRuntimeMetadata {
    pub(crate) motyga_rs_version: String,
    pub(crate) runtime_os: String,
    pub(crate) runtime_os_version: String,
    pub(crate) runtime_arch: String,
}

#[derive(Serialize)]
pub(crate) struct ThreadInitializedEventParams {
    pub(crate) thread_id: String,
    pub(crate) session_id: String,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    pub(crate) model: String,
    pub(crate) ephemeral: bool,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) initialization_mode: ThreadInitializationMode,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) forked_from_thread_id: Option<String>,
    pub(crate) created_at: u64,
}

#[derive(Serialize)]
pub(crate) struct ThreadInitializedEvent {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: ThreadInitializedEventParams,
}

#[derive(Serialize)]
pub(crate) struct GuardianReviewEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: GuardianReviewEventPayload,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianReviewDecision {
    Approved,
    Denied,
    Aborted,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianReviewTerminalStatus {
    Approved,
    Denied,
    Aborted,
    TimedOut,
    FailedClosed,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianReviewFailureReason {
    Timeout,
    Cancelled,
    PromptBuildError,
    SessionError,
    ParseError,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianReviewSessionKind {
    TrunkNew,
    TrunkReused,
    EphemeralForked,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GuardianApprovalRequestSource {
    /// Approval requested directly by the main Motyga turn.
    MainTurn,
    /// Approval requested by a delegated subagent and routed through the parent
    /// session for guardian review.
    DelegatedSubagent,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuardianReviewedAction {
    Shell {
        sandbox_permissions: SandboxPermissions,
        additional_permissions: Option<AdditionalPermissionProfile>,
    },
    UnifiedExec {
        sandbox_permissions: SandboxPermissions,
        additional_permissions: Option<AdditionalPermissionProfile>,
        tty: bool,
    },
    Execve {
        source: GuardianCommandSource,
        program: String,
        additional_permissions: Option<AdditionalPermissionProfile>,
    },
    ApplyPatch {},
    NetworkAccess {
        protocol: NetworkApprovalProtocol,
        port: u16,
    },
    McpToolCall {
        server: String,
        tool_name: String,
        connector_id: Option<String>,
        connector_name: Option<String>,
        tool_title: Option<String>,
    },
    RequestPermissions {},
}

#[derive(Clone, Serialize)]
pub struct GuardianReviewEventParams {
    pub thread_id: String,
    pub turn_id: String,
    pub review_id: String,
    pub target_item_id: Option<String>,
    pub approval_request_source: GuardianApprovalRequestSource,
    pub reviewed_action: GuardianReviewedAction,
    pub reviewed_action_truncated: bool,
    pub decision: GuardianReviewDecision,
    pub terminal_status: GuardianReviewTerminalStatus,
    pub failure_reason: Option<GuardianReviewFailureReason>,
    pub attempt_count: i64,
    pub risk_level: Option<GuardianRiskLevel>,
    pub user_authorization: Option<GuardianUserAuthorization>,
    pub outcome: Option<GuardianAssessmentOutcome>,
    pub guardian_thread_id: Option<String>,
    pub guardian_session_kind: Option<GuardianReviewSessionKind>,
    pub guardian_model: Option<String>,
    pub guardian_reasoning_effort: Option<String>,
    pub guardian_default_review_model_id: Option<String>,
    pub guardian_catalog_contains_auto_review: Option<bool>,
    pub guardian_review_model_overridden: Option<bool>,
    pub guardian_review_model_override: Option<String>,
    pub guardian_model_provider_id: Option<String>,
    pub had_prior_review_context: Option<bool>,
    pub review_timeout_ms: u64,
    pub tool_call_count: Option<u64>,
    pub time_to_first_token_ms: Option<u64>,
    pub completion_latency_ms: Option<u64>,
    pub started_at: u64,
    pub completed_at: Option<u64>,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub reasoning_output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}

pub struct GuardianReviewTrackContext {
    thread_id: String,
    turn_id: String,
    review_id: String,
    target_item_id: Option<String>,
    approval_request_source: GuardianApprovalRequestSource,
    reviewed_action: GuardianReviewedAction,
    review_timeout_ms: u64,
    pub started_at_ms: u64,
    started_instant: Instant,
}

impl GuardianReviewTrackContext {
    pub fn new(
        thread_id: String,
        turn_id: String,
        review_id: String,
        target_item_id: Option<String>,
        approval_request_source: GuardianApprovalRequestSource,
        reviewed_action: GuardianReviewedAction,
        review_timeout_ms: u64,
    ) -> Self {
        Self {
            thread_id,
            turn_id,
            review_id,
            target_item_id,
            approval_request_source,
            reviewed_action,
            review_timeout_ms,
            started_at_ms: now_unix_millis(),
            started_instant: Instant::now(),
        }
    }

    pub(crate) fn event_params(
        &self,
        result: GuardianReviewAnalyticsResult,
        completed_at_ms: u64,
    ) -> GuardianReviewEventParams {
        GuardianReviewEventParams {
            thread_id: self.thread_id.clone(),
            turn_id: self.turn_id.clone(),
            review_id: self.review_id.clone(),
            target_item_id: self.target_item_id.clone(),
            approval_request_source: self.approval_request_source,
            reviewed_action: self.reviewed_action.clone(),
            reviewed_action_truncated: result.reviewed_action_truncated,
            decision: result.decision,
            terminal_status: result.terminal_status,
            failure_reason: result.failure_reason,
            attempt_count: result.attempt_count,
            risk_level: result.risk_level,
            user_authorization: result.user_authorization,
            outcome: result.outcome,
            guardian_thread_id: result.guardian_thread_id,
            guardian_session_kind: result.guardian_session_kind,
            guardian_model: result.guardian_model,
            guardian_reasoning_effort: result.guardian_reasoning_effort,
            guardian_default_review_model_id: result.guardian_default_review_model_id,
            guardian_catalog_contains_auto_review: result.guardian_catalog_contains_auto_review,
            guardian_review_model_overridden: result.guardian_review_model_overridden,
            guardian_review_model_override: result.guardian_review_model_override,
            guardian_model_provider_id: result.guardian_model_provider_id,
            had_prior_review_context: result.had_prior_review_context,
            review_timeout_ms: self.review_timeout_ms,
            // TODO(rhan-oai): plumb nested Guardian review session tool-call counts.
            tool_call_count: None,
            time_to_first_token_ms: result.time_to_first_token_ms,
            completion_latency_ms: Some(self.started_instant.elapsed().as_millis() as u64),
            started_at: self.started_at_ms / 1_000,
            completed_at: Some(completed_at_ms / 1_000),
            input_tokens: result.token_usage.as_ref().map(|usage| usage.input_tokens),
            cached_input_tokens: result
                .token_usage
                .as_ref()
                .map(|usage| usage.cached_input_tokens),
            output_tokens: result.token_usage.as_ref().map(|usage| usage.output_tokens),
            reasoning_output_tokens: result
                .token_usage
                .as_ref()
                .map(|usage| usage.reasoning_output_tokens),
            total_tokens: result.token_usage.as_ref().map(|usage| usage.total_tokens),
        }
    }
}

#[derive(Debug)]
pub struct GuardianReviewAnalyticsResult {
    pub decision: GuardianReviewDecision,
    pub terminal_status: GuardianReviewTerminalStatus,
    pub failure_reason: Option<GuardianReviewFailureReason>,
    pub attempt_count: i64,
    pub risk_level: Option<GuardianRiskLevel>,
    pub user_authorization: Option<GuardianUserAuthorization>,
    pub outcome: Option<GuardianAssessmentOutcome>,
    pub guardian_thread_id: Option<String>,
    pub guardian_session_kind: Option<GuardianReviewSessionKind>,
    pub guardian_model: Option<String>,
    pub guardian_reasoning_effort: Option<String>,
    pub guardian_default_review_model_id: Option<String>,
    pub guardian_catalog_contains_auto_review: Option<bool>,
    pub guardian_review_model_overridden: Option<bool>,
    pub guardian_review_model_override: Option<String>,
    pub guardian_model_provider_id: Option<String>,
    pub had_prior_review_context: Option<bool>,
    pub reviewed_action_truncated: bool,
    pub token_usage: Option<TokenUsage>,
    pub time_to_first_token_ms: Option<u64>,
}

impl GuardianReviewAnalyticsResult {
    pub fn without_session() -> Self {
        Self {
            decision: GuardianReviewDecision::Denied,
            terminal_status: GuardianReviewTerminalStatus::FailedClosed,
            failure_reason: None,
            attempt_count: 1,
            risk_level: None,
            user_authorization: None,
            outcome: None,
            guardian_thread_id: None,
            guardian_session_kind: None,
            guardian_model: None,
            guardian_reasoning_effort: None,
            guardian_default_review_model_id: None,
            guardian_catalog_contains_auto_review: None,
            guardian_review_model_overridden: None,
            guardian_review_model_override: None,
            guardian_model_provider_id: None,
            had_prior_review_context: None,
            reviewed_action_truncated: false,
            token_usage: None,
            time_to_first_token_ms: None,
        }
    }

    pub fn from_session(params: GuardianReviewSessionAnalyticsParams) -> Self {
        Self {
            guardian_thread_id: Some(params.guardian_thread_id),
            guardian_session_kind: Some(params.guardian_session_kind),
            guardian_model: Some(params.guardian_model),
            guardian_reasoning_effort: params.guardian_reasoning_effort,
            guardian_default_review_model_id: Some(params.guardian_default_review_model_id),
            guardian_catalog_contains_auto_review: Some(
                params.guardian_catalog_contains_auto_review,
            ),
            guardian_review_model_overridden: Some(params.guardian_review_model_overridden),
            guardian_review_model_override: params.guardian_review_model_override,
            guardian_model_provider_id: Some(params.guardian_model_provider_id),
            had_prior_review_context: Some(params.had_prior_review_context),
            ..Self::without_session()
        }
    }
}

pub struct GuardianReviewSessionAnalyticsParams {
    pub guardian_thread_id: String,
    pub guardian_session_kind: GuardianReviewSessionKind,
    pub guardian_model: String,
    pub guardian_reasoning_effort: Option<String>,
    pub guardian_default_review_model_id: String,
    pub guardian_catalog_contains_auto_review: bool,
    pub guardian_review_model_overridden: bool,
    pub guardian_review_model_override: Option<String>,
    pub guardian_model_provider_id: String,
    pub had_prior_review_context: bool,
}

#[derive(Serialize)]
pub(crate) struct GuardianReviewEventPayload {
    pub(crate) session_id: String,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    #[serde(flatten)]
    pub(crate) guardian_review: GuardianReviewEventParams,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FinalApprovalOutcome {
    Unknown,
    NotNeeded,
    ConfigAllowed,
    PolicyForbidden,
    GuardianApproved,
    GuardianDenied,
    GuardianAborted,
    UserApproved,
    UserApprovedForSession,
    UserDenied,
    UserAborted,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolItemTerminalStatus {
    Completed,
    Failed,
    Rejected,
    Interrupted,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolItemFailureKind {
    ToolError,
    ApprovalDenied,
    ApprovalAborted,
    SandboxDenied,
    PolicyForbidden,
}

#[derive(Serialize)]
pub(crate) struct MotygaToolItemEventBase {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    /// App-server ThreadItem.id. For tool-originated items this generally
    /// corresponds to the originating core call_id.
    pub(crate) item_id: String,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) tool_name: String,
    pub(crate) started_at_ms: u64,
    pub(crate) completed_at_ms: u64,
    // Observed item lifecycle duration. This may undercount end-to-end execution
    // for tools where app-server only sees part of the upstream flow.
    pub(crate) duration_ms: Option<u64>,
    pub(crate) execution_duration_ms: Option<u64>,
    pub(crate) review_count: u64,
    pub(crate) guardian_review_count: u64,
    pub(crate) user_review_count: u64,
    pub(crate) final_approval_outcome: FinalApprovalOutcome,
    pub(crate) terminal_status: ToolItemTerminalStatus,
    pub(crate) failure_kind: Option<ToolItemFailureKind>,
    pub(crate) requested_additional_permissions: bool,
    pub(crate) requested_network_access: bool,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewSubjectKind {
    CommandExecution,
    FileChange,
    McpToolCall,
    Permissions,
    NetworkAccess,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reviewer {
    Guardian,
    User,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewTrigger {
    Initial,
    SandboxDenial,
    NetworkPolicyDenial,
    ExecveIntercept,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewStatus {
    Approved,
    Denied,
    Aborted,
    TimedOut,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReviewResolution {
    None,
    SessionApproval,
    ExecPolicyAmendment,
    NetworkPolicyAmendment,
}

#[derive(Serialize)]
pub(crate) struct MotygaReviewEventParams {
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) item_id: Option<String>,
    pub(crate) review_id: String,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) subject_kind: ReviewSubjectKind,
    pub(crate) subject_name: String,
    pub(crate) reviewer: Reviewer,
    pub(crate) trigger: ReviewTrigger,
    pub(crate) status: ReviewStatus,
    pub(crate) resolution: ReviewResolution,
    pub(crate) started_at_ms: u64,
    pub(crate) completed_at_ms: u64,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct MotygaReviewEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaReviewEventParams,
}
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WebSearchActionKind {
    Search,
    OpenPage,
    FindInPage,
    Other,
}

#[derive(Serialize)]
pub(crate) struct MotygaCommandExecutionEventParams {
    #[serde(flatten)]
    pub(crate) base: MotygaToolItemEventBase,
    pub(crate) command_execution_source: CommandExecutionSource,
    pub(crate) exit_code: Option<i32>,
    pub(crate) command_total_action_count: u64,
    pub(crate) command_read_action_count: u64,
    pub(crate) command_list_files_action_count: u64,
    pub(crate) command_search_action_count: u64,
    pub(crate) command_unknown_action_count: u64,
}

#[derive(Serialize)]
pub(crate) struct MotygaCommandExecutionEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaCommandExecutionEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaFileChangeEventParams {
    #[serde(flatten)]
    pub(crate) base: MotygaToolItemEventBase,
    pub(crate) file_change_count: u64,
    pub(crate) file_add_count: u64,
    pub(crate) file_update_count: u64,
    pub(crate) file_delete_count: u64,
    pub(crate) file_move_count: u64,
}

#[derive(Serialize)]
pub(crate) struct MotygaFileChangeEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaFileChangeEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaMcpToolCallEventParams {
    #[serde(flatten)]
    pub(crate) base: MotygaToolItemEventBase,
    pub(crate) mcp_server_name: String,
    pub(crate) mcp_tool_name: String,
    pub(crate) mcp_error_present: bool,
    pub(crate) plugin_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaMcpToolCallEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaMcpToolCallEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaDynamicToolCallEventParams {
    #[serde(flatten)]
    pub(crate) base: MotygaToolItemEventBase,
    pub(crate) dynamic_tool_name: String,
    pub(crate) success: Option<bool>,
    pub(crate) output_content_item_count: Option<u64>,
    pub(crate) output_text_item_count: Option<u64>,
    pub(crate) output_image_item_count: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct MotygaDynamicToolCallEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaDynamicToolCallEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaCollabAgentToolCallEventParams {
    #[serde(flatten)]
    pub(crate) base: MotygaToolItemEventBase,
    pub(crate) sender_thread_id: String,
    pub(crate) receiver_thread_count: u64,
    pub(crate) receiver_thread_ids: Option<Vec<String>>,
    pub(crate) requested_model: Option<String>,
    pub(crate) requested_reasoning_effort: Option<String>,
    pub(crate) agent_state_count: Option<u64>,
    pub(crate) completed_agent_count: Option<u64>,
    pub(crate) failed_agent_count: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct MotygaCollabAgentToolCallEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaCollabAgentToolCallEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaWebSearchEventParams {
    #[serde(flatten)]
    pub(crate) base: MotygaToolItemEventBase,
    pub(crate) web_search_action: Option<WebSearchActionKind>,
    pub(crate) query_present: bool,
    pub(crate) query_count: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct MotygaWebSearchEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaWebSearchEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaImageGenerationEventParams {
    #[serde(flatten)]
    pub(crate) base: MotygaToolItemEventBase,
    pub(crate) revised_prompt_present: bool,
    pub(crate) saved_path_present: bool,
}

#[derive(Serialize)]
pub(crate) struct MotygaImageGenerationEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaImageGenerationEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaAppMetadata {
    pub(crate) connector_id: Option<String>,
    pub(crate) thread_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) app_name: Option<String>,
    pub(crate) product_client_id: Option<String>,
    pub(crate) invoke_type: Option<InvocationType>,
    pub(crate) model_slug: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaAppMentionedEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaAppMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaAppUsedEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaAppMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaHookRunMetadata {
    pub(crate) thread_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) product_client_id: Option<String>,
    pub(crate) model_slug: Option<String>,
    pub(crate) hook_name: Option<String>,
    pub(crate) hook_source: Option<&'static str>,
    pub(crate) status: Option<HookRunStatus>,
}

#[derive(Serialize)]
pub(crate) struct MotygaHookRunEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaHookRunMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaCompactionEventParams {
    pub(crate) thread_id: String,
    pub(crate) session_id: String,
    pub(crate) turn_id: String,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) trigger: CompactionTrigger,
    pub(crate) reason: CompactionReason,
    pub(crate) implementation: CompactionImplementation,
    pub(crate) phase: CompactionPhase,
    pub(crate) strategy: CompactionStrategy,
    pub(crate) status: CompactionStatus,
    pub(crate) motyga_error_kind: Option<MotygaErrKind>,
    pub(crate) motyga_error_http_status_code: Option<u16>,
    pub(crate) active_context_tokens_before: i64,
    pub(crate) active_context_tokens_after: i64,
    pub(crate) retained_image_count: Option<usize>,
    pub(crate) compaction_summary_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
    pub(crate) started_at: u64,
    pub(crate) completed_at: u64,
    pub(crate) duration_ms: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct MotygaCompactionEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaCompactionEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaGoalEventParams {
    pub(crate) thread_id: String,
    pub(crate) session_id: String,
    pub(crate) turn_id: Option<String>,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) goal_id: String,
    pub(crate) event_kind: GoalEventKind,
    pub(crate) goal_status: motyga_state::ThreadGoalStatus,
    pub(crate) has_token_budget: bool,
    pub(crate) cumulative_tokens_accounted: Option<i64>,
    pub(crate) cumulative_time_accounted_seconds: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct MotygaGoalEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaGoalEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaTurnEventParams {
    pub(crate) thread_id: String,
    pub(crate) session_id: String,
    pub(crate) turn_id: String,
    // TODO(rhan-oai): Populate once queued/default submission type is plumbed from
    // the turn/start callsites instead of always being reported as None.
    pub(crate) submission_type: Option<TurnSubmissionType>,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    pub(crate) ephemeral: bool,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) initialization_mode: ThreadInitializationMode,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) model_provider: String,
    pub(crate) sandbox_policy: Option<&'static str>,
    pub(crate) reasoning_effort: Option<String>,
    pub(crate) reasoning_summary: Option<String>,
    pub(crate) service_tier: String,
    pub(crate) approval_policy: String,
    pub(crate) approvals_reviewer: String,
    pub(crate) sandbox_network_access: bool,
    pub(crate) collaboration_mode: Option<&'static str>,
    pub(crate) personality: Option<String>,
    pub(crate) workspace_kind: Option<String>,
    pub(crate) num_input_images: usize,
    pub(crate) is_first_turn: bool,
    pub(crate) status: Option<TurnStatus>,
    pub(crate) turn_error: Option<MotygaErrorInfo>,
    pub(crate) motyga_error_kind: Option<MotygaErrKind>,
    pub(crate) motyga_error_http_status_code: Option<u16>,
    pub(crate) steer_count: Option<usize>,
    pub(crate) total_tool_call_count: Option<usize>,
    pub(crate) shell_command_count: Option<usize>,
    pub(crate) file_change_count: Option<usize>,
    pub(crate) mcp_tool_call_count: Option<usize>,
    pub(crate) dynamic_tool_call_count: Option<usize>,
    pub(crate) subagent_tool_call_count: Option<usize>,
    pub(crate) web_search_count: Option<usize>,
    pub(crate) image_generation_count: Option<usize>,
    pub(crate) input_tokens: Option<i64>,
    pub(crate) cached_input_tokens: Option<i64>,
    pub(crate) output_tokens: Option<i64>,
    pub(crate) reasoning_output_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
    pub(crate) before_first_sampling_ms: u64,
    pub(crate) sampling_ms: u64,
    pub(crate) between_sampling_overhead_ms: u64,
    pub(crate) tool_blocking_ms: u64,
    pub(crate) after_last_sampling_ms: u64,
    pub(crate) sampling_request_count: u32,
    pub(crate) sampling_retry_count: u32,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) started_at: Option<u64>,
    pub(crate) completed_at: Option<u64>,
}

#[derive(Serialize)]
pub(crate) struct MotygaTurnEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaTurnEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaTurnSteerEventParams {
    pub(crate) thread_id: String,
    pub(crate) session_id: String,
    pub(crate) expected_turn_id: Option<String>,
    pub(crate) accepted_turn_id: Option<String>,
    pub(crate) app_server_client: MotygaAppServerClientMetadata,
    pub(crate) runtime: MotygaRuntimeMetadata,
    pub(crate) thread_source: Option<ThreadSource>,
    pub(crate) subagent_source: Option<String>,
    pub(crate) parent_thread_id: Option<String>,
    pub(crate) num_input_images: usize,
    pub(crate) result: TurnSteerResult,
    pub(crate) rejection_reason: Option<TurnSteerRejectionReason>,
    pub(crate) created_at: u64,
}

#[derive(Serialize)]
pub(crate) struct MotygaTurnSteerEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaTurnSteerEventParams,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginMetadata {
    pub(crate) plugin_id: Option<String>,
    pub(crate) remote_plugin_id: Option<String>,
    pub(crate) plugin_name: Option<String>,
    pub(crate) marketplace_name: Option<String>,
    pub(crate) has_skills: Option<bool>,
    pub(crate) mcp_server_count: Option<usize>,
    pub(crate) connector_ids: Option<Vec<String>>,
    pub(crate) product_client_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginUsedMetadata {
    #[serde(flatten)]
    pub(crate) plugin: MotygaPluginMetadata,
    pub(crate) mcp_server_names: Option<Vec<String>>,
    pub(crate) thread_id: Option<String>,
    pub(crate) turn_id: Option<String>,
    pub(crate) model_slug: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginInstallRequestedPluginMetadata {
    pub(crate) plugin_id: String,
    pub(crate) remote_plugin_id: Option<String>,
    pub(crate) plugin_name: String,
    pub(crate) connector_ids: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginInstallRequestedMetadata {
    pub(crate) suggestion_id: String,
    pub(crate) plugins: Vec<MotygaPluginInstallRequestedPluginMetadata>,
    pub(crate) source: crate::facts::PluginInstallRequestSource,
    pub(crate) thread_id: String,
    pub(crate) turn_id: String,
    pub(crate) model_slug: String,
    pub(crate) product_client_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginInstallRequestedEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaPluginInstallRequestedMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaPluginMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginInstallFailedMetadata {
    #[serde(flatten)]
    pub(crate) plugin: MotygaPluginMetadata,
    pub(crate) error_type: String,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginInstallFailedEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaPluginInstallFailedMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaOnboardingExternalAgentImportCompleteMetadata {
    pub(crate) import_id: String,
    pub(crate) source: String,
    #[serde(rename = "type")]
    pub(crate) item_type: String,
    pub(crate) success_count: usize,
    pub(crate) failed_count: usize,
    pub(crate) product_client_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaOnboardingExternalAgentImportCompleteEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaOnboardingExternalAgentImportCompleteMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaOnboardingExternalAgentImportFailureMetadata {
    pub(crate) import_id: String,
    pub(crate) source: String,
    #[serde(rename = "type")]
    pub(crate) item_type: String,
    pub(crate) failure_stage: String,
    pub(crate) error_type: String,
    pub(crate) product_client_id: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct MotygaOnboardingExternalAgentImportFailureEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaOnboardingExternalAgentImportFailureMetadata,
}

#[derive(Serialize)]
pub(crate) struct MotygaPluginUsedEventRequest {
    pub(crate) event_type: &'static str,
    pub(crate) event_params: MotygaPluginUsedMetadata,
}

pub(crate) fn plugin_state_event_type(state: PluginState) -> &'static str {
    match state {
        PluginState::Installed => "motyga_plugin_installed",
        PluginState::Uninstalled => "motyga_plugin_uninstalled",
        PluginState::Enabled => "motyga_plugin_enabled",
        PluginState::Disabled => "motyga_plugin_disabled",
    }
}

pub(crate) fn motyga_app_metadata(
    tracking: &TrackEventsContext,
    app: AppInvocation,
) -> MotygaAppMetadata {
    MotygaAppMetadata {
        connector_id: app.connector_id,
        thread_id: Some(tracking.thread_id.clone()),
        turn_id: Some(tracking.turn_id.clone()),
        app_name: app.app_name,
        product_client_id: Some(tracking.product_client_id.clone()),
        invoke_type: app.invocation_type,
        model_slug: Some(tracking.model_slug.clone()),
    }
}

pub(crate) fn motyga_plugin_metadata(plugin: PluginTelemetryMetadata) -> MotygaPluginMetadata {
    motyga_plugin_metadata_with_product_client_id(plugin, originator().value)
}

fn motyga_plugin_metadata_with_product_client_id(
    plugin: PluginTelemetryMetadata,
    product_client_id: String,
) -> MotygaPluginMetadata {
    let PluginTelemetryMetadata {
        plugin_id,
        remote_plugin_id,
        capability_summary,
    } = plugin;
    MotygaPluginMetadata {
        plugin_id: plugin_id.as_ref().map(PluginId::as_key),
        remote_plugin_id,
        plugin_name: plugin_id
            .as_ref()
            .map(|plugin_id| plugin_id.plugin_name.clone()),
        marketplace_name: plugin_id.map(|plugin_id| plugin_id.marketplace_name),
        has_skills: capability_summary
            .as_ref()
            .map(|summary| summary.has_skills),
        mcp_server_count: capability_summary
            .as_ref()
            .map(|summary| summary.mcp_server_names.len()),
        connector_ids: capability_summary.map(|summary| {
            summary
                .app_connector_ids
                .into_iter()
                .map(|connector_id| connector_id.0)
                .collect()
        }),
        product_client_id: Some(product_client_id),
    }
}

pub(crate) fn motyga_plugin_install_requested_metadata(
    tracking: &TrackEventsContext,
    request: PluginInstallRequested,
) -> MotygaPluginInstallRequestedMetadata {
    MotygaPluginInstallRequestedMetadata {
        suggestion_id: request.suggestion_id,
        plugins: request
            .plugins
            .into_iter()
            .map(|plugin| MotygaPluginInstallRequestedPluginMetadata {
                plugin_id: plugin.plugin_id,
                remote_plugin_id: plugin.remote_plugin_id,
                plugin_name: plugin.plugin_name,
                connector_ids: plugin.connector_ids,
            })
            .collect(),
        source: request.source,
        thread_id: tracking.thread_id.clone(),
        turn_id: tracking.turn_id.clone(),
        model_slug: tracking.model_slug.clone(),
        product_client_id: Some(originator().value),
    }
}

pub(crate) fn motyga_compaction_event_params(
    input: MotygaCompactionEvent,
    session_id: String,
    app_server_client: MotygaAppServerClientMetadata,
    runtime: MotygaRuntimeMetadata,
    thread_source: Option<ThreadSource>,
    subagent_source: Option<String>,
    parent_thread_id: Option<String>,
) -> MotygaCompactionEventParams {
    MotygaCompactionEventParams {
        thread_id: input.thread_id,
        session_id,
        turn_id: input.turn_id,
        app_server_client,
        runtime,
        thread_source,
        subagent_source,
        parent_thread_id,
        trigger: input.trigger,
        reason: input.reason,
        implementation: input.implementation,
        phase: input.phase,
        strategy: input.strategy,
        status: input.status,
        motyga_error_kind: input.motyga_error_kind,
        motyga_error_http_status_code: input.motyga_error_http_status_code,
        active_context_tokens_before: input.active_context_tokens_before,
        active_context_tokens_after: input.active_context_tokens_after,
        retained_image_count: input.retained_image_count,
        compaction_summary_tokens: input.compaction_summary_tokens,
        cached_input_tokens: input.cached_input_tokens,
        started_at: input.started_at,
        completed_at: input.completed_at,
        duration_ms: input.duration_ms,
    }
}

pub(crate) fn motyga_goal_event_params(
    input: MotygaGoalEvent,
    session_id: String,
    app_server_client: MotygaAppServerClientMetadata,
    runtime: MotygaRuntimeMetadata,
    thread_source: Option<ThreadSource>,
    subagent_source: Option<String>,
    parent_thread_id: Option<String>,
) -> MotygaGoalEventParams {
    MotygaGoalEventParams {
        thread_id: input.thread_id,
        session_id,
        turn_id: input.turn_id,
        app_server_client,
        runtime,
        thread_source,
        subagent_source,
        parent_thread_id,
        goal_id: input.goal_id,
        event_kind: input.event_kind,
        goal_status: input.goal_status,
        has_token_budget: input.has_token_budget,
        cumulative_tokens_accounted: input.cumulative_tokens_accounted,
        cumulative_time_accounted_seconds: input.cumulative_time_accounted_seconds,
    }
}

pub(crate) fn motyga_plugin_used_metadata(
    tracking: &TrackEventsContext,
    plugin: PluginTelemetryMetadata,
) -> MotygaPluginUsedMetadata {
    let mcp_server_names = plugin
        .capability_summary
        .as_ref()
        .map(|summary| summary.mcp_server_names.clone());
    MotygaPluginUsedMetadata {
        plugin: motyga_plugin_metadata_with_product_client_id(
            plugin,
            tracking.product_client_id.clone(),
        ),
        mcp_server_names,
        thread_id: Some(tracking.thread_id.clone()),
        turn_id: Some(tracking.turn_id.clone()),
        model_slug: Some(tracking.model_slug.clone()),
    }
}

pub(crate) fn motyga_hook_run_metadata(
    tracking: &TrackEventsContext,
    hook: HookRunFact,
) -> MotygaHookRunMetadata {
    MotygaHookRunMetadata {
        thread_id: Some(tracking.thread_id.clone()),
        turn_id: Some(tracking.turn_id.clone()),
        product_client_id: Some(tracking.product_client_id.clone()),
        model_slug: Some(tracking.model_slug.clone()),
        hook_name: Some(analytics_hook_event_name(hook.event_name).to_owned()),
        hook_source: Some(analytics_hook_source(hook.hook_source)),
        status: Some(analytics_hook_status(hook.status)),
    }
}

fn analytics_hook_event_name(event_name: HookEventName) -> &'static str {
    match event_name {
        HookEventName::PreToolUse => "PreToolUse",
        HookEventName::PermissionRequest => "PermissionRequest",
        HookEventName::PostToolUse => "PostToolUse",
        HookEventName::PreCompact => "PreCompact",
        HookEventName::PostCompact => "PostCompact",
        HookEventName::SessionStart => "SessionStart",
        HookEventName::UserPromptSubmit => "UserPromptSubmit",
        HookEventName::SubagentStart => "SubagentStart",
        HookEventName::SubagentStop => "SubagentStop",
        HookEventName::Stop => "Stop",
    }
}

fn analytics_hook_source(source: HookSource) -> &'static str {
    match source {
        HookSource::System => "system",
        HookSource::User => "user",
        HookSource::Project => "project",
        HookSource::Mdm => "mdm",
        HookSource::SessionFlags => "session_flags",
        HookSource::Plugin => "plugin",
        HookSource::CloudRequirements => "cloud_requirements",
        HookSource::CloudManagedConfig => "cloud_managed_config",
        HookSource::LegacyManagedConfigFile => "legacy_managed_config_file",
        HookSource::LegacyManagedConfigMdm => "legacy_managed_config_mdm",
        HookSource::Unknown => "unknown",
    }
}

pub(crate) fn current_runtime_metadata() -> MotygaRuntimeMetadata {
    let os_info = os_info::get();
    MotygaRuntimeMetadata {
        motyga_rs_version: env!("CARGO_PKG_VERSION").to_string(),
        runtime_os: std::env::consts::OS.to_string(),
        runtime_os_version: os_info.version().to_string(),
        runtime_arch: std::env::consts::ARCH.to_string(),
    }
}

pub(crate) fn subagent_thread_started_event_request(
    input: SubAgentThreadStartedInput,
) -> ThreadInitializedEvent {
    let event_params = ThreadInitializedEventParams {
        thread_id: input.thread_id,
        session_id: input.session_id,
        app_server_client: MotygaAppServerClientMetadata {
            product_client_id: input.product_client_id,
            client_name: Some(input.client_name),
            client_version: Some(input.client_version),
            rpc_transport: AppServerRpcTransport::InProcess,
            experimental_api_enabled: None,
        },
        runtime: current_runtime_metadata(),
        model: input.model,
        ephemeral: input.ephemeral,
        thread_source: Some(ThreadSource::Subagent),
        initialization_mode: ThreadInitializationMode::New,
        subagent_source: Some(subagent_source_name(&input.subagent_source)),
        parent_thread_id: input.parent_thread_id,
        forked_from_thread_id: input.forked_from_thread_id,
        created_at: input.created_at,
    };
    ThreadInitializedEvent {
        event_type: "motyga_thread_initialized",
        event_params,
    }
}

pub(crate) fn subagent_source_name(subagent_source: &SubAgentSource) -> String {
    subagent_source.kind().to_string()
}

fn analytics_hook_status(status: HookRunStatus) -> HookRunStatus {
    match status {
        // Running is unexpected here and normalized defensively.
        HookRunStatus::Running => HookRunStatus::Failed,
        other => other,
    }
}
