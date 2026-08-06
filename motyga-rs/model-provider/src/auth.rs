use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use motyga_agent_identity::AgentIdentityKey;
use motyga_agent_identity::authorization_header_for_agent_task;
use motyga_api::AgentIdentityTelemetry;
use motyga_api::AuthProvider;
use motyga_api::SharedAuthProvider;
use motyga_login::AuthManager;
use motyga_login::MotygaAuth;
use motyga_login::auth::AgentIdentityAuth;
use motyga_login::auth::AgentIdentityAuthError;
use motyga_login::auth::AgentIdentityAuthPolicy;
use motyga_model_provider_info::ModelProviderInfo;
use motyga_protocol::error::MotygaErr;
use motyga_protocol::protocol::SessionSource;
use http::HeaderMap;
use http::HeaderValue;

use crate::bearer_auth_provider::BearerAuthProvider;

const BEDROCK_API_KEY_UNSUPPORTED_MESSAGE: &str =
    "Bedrock API key auth is only supported by the Amazon Bedrock model provider";

#[derive(Clone, Debug)]
pub struct ProviderAuthScope {
    pub agent_identity_policy: AgentIdentityAuthPolicy,
    pub session_source: SessionSource,
    pub agent_identity_session_fallback: AgentIdentitySessionFallback,
}

#[derive(Clone, Debug, Default)]
pub struct AgentIdentitySessionFallback {
    engaged: Arc<AtomicBool>,
}

impl AgentIdentitySessionFallback {
    pub fn is_engaged(&self) -> bool {
        self.engaged.load(Ordering::Relaxed)
    }

    fn engage(&self) -> bool {
        !self.engaged.swap(true, Ordering::Relaxed)
    }
}

/// Provider auth resolved for a request, plus metadata describing the effective auth.
#[derive(Clone)]
pub struct ResolvedProviderAuth {
    pub auth: SharedAuthProvider,
    pub agent_identity_telemetry: Option<AgentIdentityTelemetry>,
}

impl ResolvedProviderAuth {
    pub(crate) fn new(auth: SharedAuthProvider) -> Self {
        Self {
            auth,
            agent_identity_telemetry: None,
        }
    }

    fn for_agent_identity(auth: AgentIdentityAuth) -> Self {
        let agent_identity_telemetry = agent_identity_telemetry(&auth);
        Self {
            auth: Arc::new(AgentIdentityAuthProvider { auth }),
            agent_identity_telemetry: Some(agent_identity_telemetry),
        }
    }
}

pub(crate) fn agent_identity_telemetry(auth: &AgentIdentityAuth) -> AgentIdentityTelemetry {
    AgentIdentityTelemetry {
        agent_id: auth.record().agent_runtime_id.clone(),
        task_id: auth.run_task_id().to_string(),
    }
}

#[derive(Clone, Debug)]
struct AgentIdentityAuthProvider {
    auth: AgentIdentityAuth,
}

impl AuthProvider for AgentIdentityAuthProvider {
    fn add_auth_headers(&self, headers: &mut HeaderMap) {
        let record = self.auth.record();
        let header_value = authorization_header_for_agent_task(
            AgentIdentityKey {
                agent_runtime_id: &record.agent_runtime_id,
                private_key_pkcs8_base64: &record.agent_private_key,
            },
            self.auth.run_task_id(),
        )
        .map_err(std::io::Error::other);

        if let Ok(header_value) = header_value
            && let Ok(header) = HeaderValue::from_str(&header_value)
        {
            let _ = headers.insert(http::header::AUTHORIZATION, header);
        }

        if let Ok(header) = HeaderValue::from_str(self.auth.account_id()) {
            let _ = headers.insert("ChatGPT-Account-ID", header);
        }

        if self.auth.is_fedramp_account() {
            let _ = headers.insert("X-OpenAI-Fedramp", HeaderValue::from_static("true"));
        }
    }
}

// Some providers are meant to send no auth headers. Examples include local OSS
// providers and custom test providers with `requires_openai_auth = false`.
#[derive(Clone, Debug)]
struct UnauthenticatedAuthProvider;

impl AuthProvider for UnauthenticatedAuthProvider {
    fn add_auth_headers(&self, _headers: &mut HeaderMap) {}
}

pub fn unauthenticated_auth_provider() -> SharedAuthProvider {
    Arc::new(UnauthenticatedAuthProvider)
}

/// Returns the provider-scoped auth manager when this provider uses command-backed auth.
///
/// Providers without custom auth continue using the caller-supplied base manager, when present.
pub(crate) fn auth_manager_for_provider(
    auth_manager: Option<Arc<AuthManager>>,
    provider: &ModelProviderInfo,
) -> Option<Arc<AuthManager>> {
    match provider.auth.clone() {
        Some(config) => Some(AuthManager::external_bearer_only(config)),
        None => auth_manager,
    }
}

pub(crate) fn resolve_provider_auth(
    auth: Option<&MotygaAuth>,
    provider: &ModelProviderInfo,
) -> motyga_protocol::error::Result<SharedAuthProvider> {
    if matches!(auth, Some(MotygaAuth::BedrockApiKey(_))) {
        return Err(MotygaErr::UnsupportedOperation(
            BEDROCK_API_KEY_UNSUPPORTED_MESSAGE.to_string(),
        ));
    }

    match bearer_auth_for_provider(provider) {
        Ok(Some(bearer)) => return Ok(Arc::new(bearer)),
        Ok(None) => {}
        Err(err) => {
            // A declared `env_key` is a hard requirement only when there is nothing else to
            // authenticate with. A stored device login is a first-class credential source, so
            // hard-failing here made `motyga login` a no-op for the default provider: it wrote a
            // live key to auth.json that nothing ever read, and a stale env var silently outranked
            // the fresh login. Kept deliberately narrow — first-party provider and an API-key login
            // only, so a third-party provider can never be handed a credential minted for us.
            let stored_login_usable =
                provider.is_motyga() && matches!(auth, Some(MotygaAuth::ApiKey(_)));
            if !(stored_login_usable && matches!(err, MotygaErr::EnvVar(_))) {
                return Err(err);
            }
        }
    }

    Ok(match auth {
        Some(auth) => auth_provider_from_auth(auth),
        None => unauthenticated_auth_provider(),
    })
}

pub(crate) async fn resolve_provider_auth_for_scope(
    auth_manager: Option<Arc<AuthManager>>,
    auth: Option<&MotygaAuth>,
    provider: &ModelProviderInfo,
    scope: ProviderAuthScope,
) -> motyga_protocol::error::Result<ResolvedProviderAuth> {
    let ProviderAuthScope {
        agent_identity_policy,
        session_source,
        agent_identity_session_fallback,
    } = scope;
    if let Some(MotygaAuth::AgentIdentity(agent_identity_auth)) = auth {
        return Ok(ResolvedProviderAuth::for_agent_identity(
            agent_identity_auth.clone(),
        ));
    }

    if !should_bootstrap_chatgpt_agent_identity(agent_identity_policy, auth)
        || agent_identity_session_fallback.is_engaged()
    {
        return resolve_provider_auth(auth, provider).map(ResolvedProviderAuth::new);
    }

    let Some(auth_manager) = auth_manager else {
        return resolve_provider_auth(auth, provider).map(ResolvedProviderAuth::new);
    };

    match auth_manager
        .agent_identity_auth(agent_identity_policy, session_source)
        .await
    {
        Ok(Some(agent_identity_auth)) => Ok(ResolvedProviderAuth::for_agent_identity(
            agent_identity_auth,
        )),
        Ok(None) => resolve_provider_auth(auth, provider).map(ResolvedProviderAuth::new),
        Err(err) => {
            if let Some(AgentIdentityAuthError::BootstrapUnavailable {
                operation,
                attempts,
                message,
            }) = err
                .get_ref()
                .and_then(|source| source.downcast_ref::<AgentIdentityAuthError>())
            {
                let newly_engaged = agent_identity_session_fallback.engage();
                tracing::warn!(
                    operation,
                    attempts = *attempts,
                    error = %message,
                    newly_engaged,
                    "agent identity bootstrap unavailable; using ChatGPT bearer auth for this session"
                );
                resolve_provider_auth(auth, provider).map(ResolvedProviderAuth::new)
            } else {
                Err(err.into())
            }
        }
    }
}

fn should_bootstrap_chatgpt_agent_identity(
    agent_identity_policy: AgentIdentityAuthPolicy,
    auth: Option<&MotygaAuth>,
) -> bool {
    agent_identity_policy == AgentIdentityAuthPolicy::ChatGptAuth
        && matches!(auth, Some(MotygaAuth::Chatgpt(_)))
}

fn bearer_auth_for_provider(
    provider: &ModelProviderInfo,
) -> motyga_protocol::error::Result<Option<BearerAuthProvider>> {
    if let Some(api_key) = provider.api_key()? {
        return Ok(Some(BearerAuthProvider::new(api_key)));
    }

    if let Some(token) = provider.experimental_bearer_token.clone() {
        return Ok(Some(BearerAuthProvider::new(token)));
    }

    Ok(None)
}

/// Builds request-header auth for a first-party Motyga auth snapshot.
pub fn auth_provider_from_auth(auth: &MotygaAuth) -> SharedAuthProvider {
    match auth {
        MotygaAuth::AgentIdentity(auth) => {
            Arc::new(AgentIdentityAuthProvider { auth: auth.clone() })
        }
        MotygaAuth::BedrockApiKey(_) => unreachable!("{BEDROCK_API_KEY_UNSUPPORTED_MESSAGE}"),
        MotygaAuth::ApiKey(_)
        | MotygaAuth::Chatgpt(_)
        | MotygaAuth::ChatgptAuthTokens(_)
        | MotygaAuth::PersonalAccessToken(_) => Arc::new(BearerAuthProvider {
            token: auth.get_token().ok(),
            account_id: auth.get_account_id(),
            is_fedramp_account: auth.is_fedramp_account(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use motyga_agent_identity::generate_agent_key_material;
    use motyga_login::AuthCredentialsStoreMode;
    use motyga_login::AuthKeyringBackendKind;
    use motyga_login::auth::AgentIdentityAuthRecord;
    use motyga_login::auth::BedrockApiKeyAuth;
    use motyga_model_provider_info::WireApi;
    use motyga_model_provider_info::create_oss_provider_with_base_url;
    use motyga_protocol::account::PlanType;
    use pretty_assertions::assert_eq;
    use serde_json::json;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    use super::*;

    static NEXT_MOTYGA_HOME_ID: AtomicUsize = AtomicUsize::new(0);
    const TEST_CHATGPT_ID_TOKEN: &str = "eyJhbGciOiJub25lIiwidHlwIjoiSldUIn0.eyJlbWFpbCI6InVzZXJAZXhhbXBsZS5jb20iLCJlbWFpbF92ZXJpZmllZCI6dHJ1ZSwiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfdXNlcl9pZCI6InVzZXItMTIzNDUiLCJ1c2VyX2lkIjoidXNlci0xMjM0NSIsImNoYXRncHRfcGxhbl90eXBlIjoicHJvIiwiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjb3VudC0xMjMifX0.c2ln";

    async fn agent_identity_auth(chatgpt_account_is_fedramp: bool) -> AgentIdentityAuth {
        let key_material = generate_agent_key_material().expect("generate key material");
        AgentIdentityAuth::from_record(
            AgentIdentityAuthRecord {
                agent_runtime_id: "agent-runtime-1".to_string(),
                agent_private_key: key_material.private_key_pkcs8_base64,
                account_id: "account-1".to_string(),
                chatgpt_user_id: "user-1".to_string(),
                email: Some("agent@example.com".to_string()),
                plan_type: PlanType::Plus,
                chatgpt_account_is_fedramp,
                task_id: Some("task-run-1".to_string()),
            },
            "https://auth.openai.com/api/accounts",
            /*auth_route_config*/ None,
        )
        .await
        .expect("agent identity auth record should include task id")
    }

    fn provider_auth_scope(
        policy: AgentIdentityAuthPolicy,
        fallback: AgentIdentitySessionFallback,
    ) -> ProviderAuthScope {
        ProviderAuthScope {
            agent_identity_policy: policy,
            session_source: SessionSource::Cli,
            agent_identity_session_fallback: fallback,
        }
    }

    fn test_motyga_home() -> PathBuf {
        let id = NEXT_MOTYGA_HOME_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "motyga-model-provider-agent-identity-{pid}-{id}",
            pid = std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp motyga home");
        path
    }

    fn write_chatgpt_auth_json(motyga_home: &Path) {
        let auth_json = json!({
            "tokens": {
                "id_token": TEST_CHATGPT_ID_TOKEN,
                "access_token": "test-access-token",
                "refresh_token": "test-refresh-token",
                "account_id": "account-123"
            },
            "last_refresh": "2099-01-01T00:00:00Z"
        });
        std::fs::write(
            motyga_home.join("auth.json"),
            serde_json::to_string_pretty(&auth_json).expect("serialize auth.json"),
        )
        .expect("write auth.json");
    }

    async fn chatgpt_auth_manager(
        agent_identity_authapi_base_url: String,
    ) -> (PathBuf, Arc<AuthManager>, MotygaAuth) {
        let motyga_home = test_motyga_home();
        write_chatgpt_auth_json(&motyga_home);
        let auth_manager = AuthManager::shared(
            motyga_home.clone(),
            /*enable_motyga_api_key_env*/ false,
            AuthCredentialsStoreMode::File,
            /*forced_chatgpt_workspace_id*/ None,
            /*chatgpt_base_url*/ None,
            AuthKeyringBackendKind::default(),
            /*auth_route_config*/ None,
        )
        .await;
        let auth = auth_manager.auth().await.expect("auth should load");
        let auth_manager = AuthManager::from_auth_for_testing_with_agent_identity_authapi_base_url(
            auth.clone(),
            agent_identity_authapi_base_url,
        );
        (motyga_home, auth_manager, auth)
    }

    async fn mount_transient_agent_registration(
        server: &MockServer,
        status: u16,
        registration_count: Arc<AtomicUsize>,
    ) {
        Mock::given(method("POST"))
            .and(path("/v1/agent/register"))
            .respond_with(move |_request: &wiremock::Request| {
                registration_count.fetch_add(1, Ordering::SeqCst);
                ResponseTemplate::new(status)
            })
            .mount(server)
            .await;
    }

    #[test]
    fn unauthenticated_auth_provider_adds_no_headers() {
        let provider =
            create_oss_provider_with_base_url("http://localhost:11434/v1", WireApi::Responses);
        let auth = resolve_provider_auth(/*auth*/ None, &provider).expect("auth should resolve");

        assert!(auth.to_auth_headers().is_empty());
    }

    #[test]
    fn openai_provider_rejects_bedrock_api_key_auth() {
        let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
        let auth = MotygaAuth::BedrockApiKey(BedrockApiKeyAuth {
            api_key: "bedrock-api-key-test".to_string(),
            region: "us-east-1".to_string(),
        });

        match resolve_provider_auth(Some(&auth), &provider) {
            Err(MotygaErr::UnsupportedOperation(message)) => {
                assert_eq!(message, BEDROCK_API_KEY_UNSUPPORTED_MESSAGE);
            }
            Err(err) => panic!("unexpected auth error: {err:?}"),
            Ok(_) => panic!("Bedrock API key auth should be rejected"),
        }
    }

    #[tokio::test]
    async fn first_party_run_scope_uses_agent_assertion_and_exposes_telemetry() {
        let auth = MotygaAuth::AgentIdentity(
            agent_identity_auth(/*chatgpt_account_is_fedramp*/ false).await,
        );
        let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);

        let auth = resolve_provider_auth_for_scope(
            /*auth_manager*/ None,
            Some(&auth),
            &provider,
            provider_auth_scope(
                AgentIdentityAuthPolicy::JwtOnly,
                AgentIdentitySessionFallback::default(),
            ),
        )
        .await
        .expect("auth should resolve");

        assert_eq!(
            auth.agent_identity_telemetry,
            Some(AgentIdentityTelemetry {
                agent_id: "agent-runtime-1".to_string(),
                task_id: "task-run-1".to_string(),
            })
        );
        let headers = auth.auth.to_auth_headers();
        assert!(
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("AgentAssertion "))
        );
    }

    #[tokio::test]
    async fn agent_identity_auth_provider_preserves_account_routing_headers() {
        let auth = agent_identity_auth(/*chatgpt_account_is_fedramp*/ true).await;
        let provider = auth_provider_from_auth(&MotygaAuth::AgentIdentity(auth));

        let headers = provider.to_auth_headers();

        assert!(
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("AgentAssertion "))
        );
        assert_eq!(
            headers
                .get("ChatGPT-Account-ID")
                .and_then(|value| value.to_str().ok()),
            Some("account-1")
        );
        assert_eq!(
            headers
                .get("X-OpenAI-Fedramp")
                .and_then(|value| value.to_str().ok()),
            Some("true")
        );
    }

    #[tokio::test]
    async fn chatgpt_bootstrap_unavailable_uses_session_bearer_fallback() {
        let server = MockServer::start().await;
        let registration_count = Arc::new(AtomicUsize::new(0));
        mount_transient_agent_registration(
            &server,
            /*status*/ 503,
            Arc::clone(&registration_count),
        )
        .await;
        let (_motyga_home, auth_manager, auth) = chatgpt_auth_manager(server.uri()).await;
        let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
        let fallback = AgentIdentitySessionFallback::default();

        let provider_auth = resolve_provider_auth_for_scope(
            Some(auth_manager),
            Some(&auth),
            &provider,
            provider_auth_scope(AgentIdentityAuthPolicy::ChatGptAuth, fallback.clone()),
        )
        .await
        .expect("fallback should resolve bearer auth");

        let headers = provider_auth.auth.to_auth_headers();
        assert_eq!(
            headers
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-access-token")
        );
        assert_eq!(
            headers
                .get("ChatGPT-Account-ID")
                .and_then(|value| value.to_str().ok()),
            Some("account-123")
        );
        assert!(fallback.is_engaged());
        assert_eq!(registration_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn chatgpt_session_fallback_skips_later_agent_identity_bootstrap() {
        let server = MockServer::start().await;
        let registration_count = Arc::new(AtomicUsize::new(0));
        mount_transient_agent_registration(
            &server,
            /*status*/ 503,
            Arc::clone(&registration_count),
        )
        .await;
        let (_motyga_home, auth_manager, auth) = chatgpt_auth_manager(server.uri()).await;
        let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
        let fallback = AgentIdentitySessionFallback::default();

        resolve_provider_auth_for_scope(
            Some(Arc::clone(&auth_manager)),
            Some(&auth),
            &provider,
            provider_auth_scope(AgentIdentityAuthPolicy::ChatGptAuth, fallback.clone()),
        )
        .await
        .expect("first fallback should resolve bearer auth");
        resolve_provider_auth_for_scope(
            Some(auth_manager),
            Some(&auth),
            &provider,
            provider_auth_scope(AgentIdentityAuthPolicy::ChatGptAuth, fallback),
        )
        .await
        .expect("second fallback should resolve bearer auth");

        assert_eq!(registration_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn chatgpt_sessions_share_bootstrap_failure_cooldown() {
        let server = MockServer::start().await;
        let registration_count = Arc::new(AtomicUsize::new(0));
        mount_transient_agent_registration(
            &server,
            /*status*/ 503,
            Arc::clone(&registration_count),
        )
        .await;
        let (_motyga_home, auth_manager, auth) = chatgpt_auth_manager(server.uri()).await;
        let provider = ModelProviderInfo::create_openai_provider(/*base_url*/ None);
        let first_fallback = AgentIdentitySessionFallback::default();
        let second_fallback = AgentIdentitySessionFallback::default();

        resolve_provider_auth_for_scope(
            Some(Arc::clone(&auth_manager)),
            Some(&auth),
            &provider,
            provider_auth_scope(AgentIdentityAuthPolicy::ChatGptAuth, first_fallback.clone()),
        )
        .await
        .expect("first session fallback should resolve bearer auth");
        resolve_provider_auth_for_scope(
            Some(auth_manager),
            Some(&auth),
            &provider,
            provider_auth_scope(
                AgentIdentityAuthPolicy::ChatGptAuth,
                second_fallback.clone(),
            ),
        )
        .await
        .expect("second session fallback should resolve bearer auth");

        assert!(first_fallback.is_engaged());
        assert!(second_fallback.is_engaged());
        assert_eq!(registration_count.load(Ordering::SeqCst), 3);
    }

    /// An env var no test may set, so these cases never depend on the ambient environment.
    const ABSENT_ENV_KEY: &str = "MOTYGA_API_KEY_DELIBERATELY_UNSET_FOR_TESTS";

    fn motyga_provider_without_env_key() -> ModelProviderInfo {
        let mut provider = ModelProviderInfo::create_motyga_provider();
        provider.env_key = Some(ABSENT_ENV_KEY.to_string());
        provider
    }

    #[test]
    fn motyga_provider_uses_stored_login_when_env_key_is_absent() {
        let provider = motyga_provider_without_env_key();
        let auth = MotygaAuth::from_api_key("nb-stored-login-secret");

        let resolved = resolve_provider_auth(Some(&auth), &provider)
            .expect("a stored device login should authenticate without the env var");

        assert_eq!(
            resolved
                .to_auth_headers()
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer nb-stored-login-secret"),
        );
    }

    #[test]
    fn motyga_provider_prefers_env_key_over_stored_login() {
        // The env var stays an override; only its absence hands over to the stored login.
        let provider = ModelProviderInfo {
            env_key: None,
            experimental_bearer_token: Some("nb-env-override".to_string()),
            ..ModelProviderInfo::create_motyga_provider()
        };
        let auth = MotygaAuth::from_api_key("nb-stored-login-secret");

        let resolved =
            resolve_provider_auth(Some(&auth), &provider).expect("override should resolve");

        assert_eq!(
            resolved
                .to_auth_headers()
                .get(http::header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer nb-env-override"),
        );
    }

    #[test]
    fn motyga_provider_still_reports_missing_env_key_without_a_stored_login() {
        // Without any credential the actionable "set MOTYGA_API_KEY" error must survive; the
        // fallback must not degrade this into an unauthenticated request that 401s later.
        let provider = motyga_provider_without_env_key();

        let Err(err) = resolve_provider_auth(/*auth*/ None, &provider) else {
            panic!("no env var and no login is still a hard error");
        };

        assert!(matches!(err, MotygaErr::EnvVar(_)), "unexpected error: {err}");
    }

    #[test]
    fn third_party_provider_never_borrows_the_stored_login() {
        // The stored key is minted for Motyga. A provider that merely declares an `env_key` must
        // never be handed it just because the env var happens to be missing.
        let provider = ModelProviderInfo {
            name: "Some Other Provider".to_string(),
            ..motyga_provider_without_env_key()
        };
        let auth = MotygaAuth::from_api_key("nb-stored-login-secret");

        let Err(err) = resolve_provider_auth(Some(&auth), &provider) else {
            panic!("a third-party provider must not fall back to the Motyga login");
        };

        assert!(matches!(err, MotygaErr::EnvVar(_)), "unexpected error: {err}");
    }
}
