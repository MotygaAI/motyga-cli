mod cell_actor;
mod remote_session;
// The real V8-backed runtime is compiled only with the `v8_runtime` feature (off by default).
// Without it, a lightweight stub stands in so the crate builds without linking ~200 MB of V8.
#[cfg(feature = "v8_runtime")]
mod runtime;
#[cfg(not(feature = "v8_runtime"))]
#[path = "runtime_stub.rs"]
mod runtime;
mod service;
mod session_runtime;

pub(crate) type TaskFailureHandler = std::sync::Arc<dyn Fn(String) + Send + Sync>;

pub use codex_code_mode_protocol::*;
pub use remote_session::ProcessOwnedCodeModeSession;
pub use remote_session::ProcessOwnedCodeModeSessionProvider;
pub use service::InProcessCodeModeSession;
pub use service::InProcessCodeModeSessionProvider;
pub use service::NoopCodeModeSessionDelegate;
