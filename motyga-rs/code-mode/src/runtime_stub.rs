//! Stub `runtime` module, compiled when the `v8_runtime` feature is OFF (the default).
//!
//! "Code mode" (model-authored JavaScript run in an embedded V8 isolate) is experimental
//! (`Stage::UnderDevelopment`) and disabled at runtime, yet the real runtime statically links
//! V8 — roughly 200 MB of the shipped binary. This stub reproduces the small, V8-free surface
//! that `cell_actor` consumes (the command/event enums) plus a `spawn_runtime` that refuses to
//! start, so the crate — and the whole CLI — builds without V8. Enable the `v8_runtime` feature
//! to compile the real V8-backed `runtime/` module instead.
//!
//! The enums below MUST stay structurally identical to their counterparts in `runtime/mod.rs`.

// The stub reproduces the full command/event enum surface so `cell_actor` compiles unchanged, but
// with V8 disabled the runtime never runs, so several variants are matched yet never constructed.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::mpsc as std_mpsc;

use codex_code_mode_protocol::CodeModeToolKind;
use codex_code_mode_protocol::ExecuteRequest;
use codex_code_mode_protocol::FunctionCallOutputContentItem;
use codex_protocol::ToolName;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;

use crate::TaskFailureHandler;

#[derive(Debug)]
pub(crate) enum RuntimeCommand {
    ToolResponse { id: String, result: JsonValue },
    ToolError { id: String, error_text: String },
    TimeoutFired { id: u64 },
    ObservePendingFrontier,
    Terminate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PendingRuntimeMode {
    #[cfg(test)]
    Continue,
    PauseUntilResumed,
}

#[derive(Debug)]
pub(crate) enum RuntimeControlCommand {
    Continue,
    Resume,
    Terminate,
}

#[derive(Debug)]
pub(crate) enum RuntimeEvent {
    Started,
    Pending,
    ContentItem(FunctionCallOutputContentItem),
    YieldRequested,
    ToolCall {
        id: String,
        name: ToolName,
        kind: CodeModeToolKind,
        input: Option<JsonValue>,
    },
    Notify {
        call_id: String,
        text: String,
    },
    Result {
        stored_value_writes: HashMap<String, JsonValue>,
        error_text: Option<String>,
    },
    ThreadPanicked,
}

/// Stand-in for `v8::IsolateHandle` when V8 is not compiled in. Never constructed (the stub
/// `spawn_runtime` always errors before a handle exists), but `cell_actor` names the type.
#[derive(Clone)]
pub(crate) struct RuntimeTerminateHandle;

impl RuntimeTerminateHandle {
    pub(crate) fn terminate_execution(&self) -> bool {
        false
    }
}

/// Refuse to start a code-mode isolate in a build without the `v8_runtime` feature.
pub(crate) fn spawn_runtime(
    _stored_values: HashMap<String, JsonValue>,
    _request: ExecuteRequest,
    _event_tx: mpsc::UnboundedSender<RuntimeEvent>,
    _pending_mode: PendingRuntimeMode,
    _task_failure_handler: Option<TaskFailureHandler>,
) -> Result<
    (
        std_mpsc::Sender<RuntimeCommand>,
        std_mpsc::Sender<RuntimeControlCommand>,
        RuntimeTerminateHandle,
    ),
    String,
> {
    Err(
        "code mode is unavailable: this build was compiled without the `v8_runtime` feature"
            .to_string(),
    )
}
