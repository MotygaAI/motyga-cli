#![allow(clippy::unwrap_used)]

use anyhow::Context;
use codex_config::types::AuthCredentialsStoreMode;
use codex_login::AuthKeyringBackendKind;
use codex_login::MotygaDeviceLoginOptions;
use codex_login::load_auth_dot_json;
use codex_login::run_motyga_device_login;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tempfile::tempdir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::Request;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

use core_test_support::skip_if_no_network;

async fn mock_device_start(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/auth/device/start"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_code": "device-code-123",
            "user_code": "ABCD-EFGH",
            "verification_uri_complete": "https://motyga.example/cli/authorize?code=ABCD-EFGH",
            "interval": 0,
            "expires_in": 60
        })))
        .expect(1)
        .mount(server)
        .await;
}

fn login_opts(codex_home: &tempfile::TempDir, base_url: String) -> MotygaDeviceLoginOptions {
    MotygaDeviceLoginOptions {
        base_url,
        codex_home: codex_home.path().to_path_buf(),
        cli_auth_credentials_store_mode: AuthCredentialsStoreMode::File,
        auth_keyring_backend_kind: AuthKeyringBackendKind::default(),
        auth_route_config: None,
        open_browser: false,
    }
}

#[tokio::test]
async fn motyga_device_login_persists_approved_api_key() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let codex_home = tempdir().unwrap();
    let mock_server = MockServer::start().await;
    mock_device_start(&mock_server).await;

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_for_mock = counter.clone();
    Mock::given(method("POST"))
        .and(path("/auth/device/token"))
        .respond_with(move |_: &Request| {
            let attempt = counter_for_mock.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                ResponseTemplate::new(400).set_body_json(json!({
                    "error": "authorization_pending"
                }))
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "status": "approved",
                    "api_key": "nb-approved-secret"
                }))
            }
        })
        .expect(2)
        .mount(&mock_server)
        .await;

    run_motyga_device_login(login_opts(&codex_home, mock_server.uri()))
        .await
        .expect("motyga device login should persist approved key");

    let auth = load_auth_dot_json(
        codex_home.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .context("auth.json should load after login succeeds")?
    .context("auth.json written")?;
    assert_eq!(auth.openai_api_key.as_deref(), Some("nb-approved-secret"));
    assert!(auth.tokens.is_none());
    assert_eq!(counter.load(Ordering::SeqCst), 2);
    Ok(())
}

#[tokio::test]
async fn motyga_device_login_does_not_persist_expired_code() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let codex_home = tempdir().unwrap();
    let mock_server = MockServer::start().await;
    mock_device_start(&mock_server).await;

    Mock::given(method("POST"))
        .and(path("/auth/device/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "expired_token"
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    let err = run_motyga_device_login(login_opts(&codex_home, mock_server.uri()))
        .await
        .expect_err("expired device code should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);

    let auth = load_auth_dot_json(
        codex_home.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .context("auth.json should load after login fails")?;
    assert!(auth.is_none());
    Ok(())
}

#[tokio::test]
async fn motyga_device_login_retries_transient_5xx() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));

    let codex_home = tempdir().unwrap();
    let mock_server = MockServer::start().await;
    mock_device_start(&mock_server).await;

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_for_mock = counter.clone();
    Mock::given(method("POST"))
        .and(path("/auth/device/token"))
        .respond_with(move |_: &Request| {
            let attempt = counter_for_mock.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 {
                // a transient gateway error (non-JSON interstitial) must be retried, not fatal
                ResponseTemplate::new(503).set_body_string("<html>503 Service Unavailable</html>")
            } else {
                ResponseTemplate::new(200).set_body_json(json!({
                    "status": "approved",
                    "api_key": "nb-approved-secret"
                }))
            }
        })
        .expect(2)
        .mount(&mock_server)
        .await;

    run_motyga_device_login(login_opts(&codex_home, mock_server.uri()))
        .await
        .expect("a transient 5xx should be retried, then the approved key persists");

    let auth = load_auth_dot_json(
        codex_home.path(),
        AuthCredentialsStoreMode::File,
        AuthKeyringBackendKind::default(),
    )
    .context("auth.json should load after login succeeds")?
    .context("auth.json written")?;
    assert_eq!(auth.openai_api_key.as_deref(), Some("nb-approved-secret"));
    assert_eq!(counter.load(Ordering::SeqCst), 2);
    Ok(())
}
