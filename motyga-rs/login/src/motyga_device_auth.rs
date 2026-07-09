use serde::Deserialize;
use serde::Serialize;
use std::io;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use crate::AuthCredentialsStoreMode;
use crate::auth::AuthKeyringBackendKind;
use crate::auth::login_with_api_key;
use crate::default_client::build_default_auth_reqwest_client;
use crate::outbound_proxy::AuthRouteConfig;

const DEFAULT_INTERVAL_SECS: u64 = 3;
const DEFAULT_EXPIRES_IN_SECS: u64 = 600;

#[derive(Debug, Clone)]
pub struct MotygaDeviceLoginOptions {
    pub base_url: String,
    pub codex_home: PathBuf,
    pub cli_auth_credentials_store_mode: AuthCredentialsStoreMode,
    pub auth_keyring_backend_kind: AuthKeyringBackendKind,
    pub auth_route_config: Option<AuthRouteConfig>,
    pub open_browser: bool,
}

#[derive(Debug, Deserialize)]
struct DeviceStartResponse {
    device_code: String,
    user_code: String,
    #[serde(default)]
    verification_uri: Option<String>,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    interval: Option<u64>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Serialize)]
struct DeviceTokenRequest<'a> {
    device_code: &'a str,
}

#[derive(Debug, Deserialize)]
struct DeviceTokenResponse {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DeviceErrorResponse {
    #[serde(default)]
    error: Option<String>,
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

async fn request_device_start(
    client: &reqwest::Client,
    base_url: &str,
) -> io::Result<DeviceStartResponse> {
    let response = client
        .post(endpoint(base_url, "auth/device/start"))
        .send()
        .await
        .map_err(io::Error::other)?;
    let status = response.status();
    let text = response.text().await.map_err(io::Error::other)?;
    if !status.is_success() {
        return Err(io::Error::other(format!(
            "device login start failed with HTTP {status}: {text}"
        )));
    }
    serde_json::from_str(&text).map_err(io::Error::other)
}

async fn request_device_token(
    client: &reqwest::Client,
    base_url: &str,
    device_code: &str,
) -> io::Result<Result<String, PollPending>> {
    // A transient failure (dropped connection, proxy 5xx/interstitial, unparseable body) must NOT abort
    // the whole login: the browser-approval window is up to ~10 minutes (~200 polls) and one bad poll is
    // expected. Only expired_token / access_denied are terminal; everything transient becomes Retry so the
    // caller keeps polling until the deadline.
    let response = match client
        .post(endpoint(base_url, "auth/device/token"))
        .json(&DeviceTokenRequest { device_code })
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) => return Ok(Err(PollPending::Retry(format!("network error: {err}")))),
    };
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    let text = match response.text().await {
        Ok(text) => text,
        Err(err) => {
            return Ok(Err(PollPending::Retry(format!(
                "failed to read token response: {err}"
            ))));
        }
    };

    if status.is_success() {
        match serde_json::from_str::<DeviceTokenResponse>(&text) {
            Ok(body) => {
                if body.status.as_deref() == Some("approved") {
                    if let Some(api_key) = body.api_key.filter(|api_key| !api_key.trim().is_empty())
                    {
                        return Ok(Ok(api_key));
                    }
                }
                // A 2xx that is not an approved key is unexpected; retry rather than abort — the code is
                // still valid and a later poll delivers the key (or the deadline/expired_token ends it).
                return Ok(Err(PollPending::Retry(
                    "token endpoint returned a success response without an API key".to_string(),
                )));
            }
            Err(err) => {
                return Ok(Err(PollPending::Retry(format!(
                    "unparseable token response: {err}"
                ))));
            }
        }
    }

    let error = serde_json::from_str::<DeviceErrorResponse>(&text)
        .ok()
        .and_then(|body| body.error)
        .unwrap_or_else(|| text.clone());
    match error.as_str() {
        "authorization_pending" => Ok(Err(PollPending::Pending)),
        "slow_down" => Ok(Err(PollPending::SlowDown(retry_after))),
        "expired_token" => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "device login code expired",
        )),
        "access_denied" | "denied" => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "device login was denied",
        )),
        // Unknown error code or a raw 5xx/interstitial body — transient, keep polling until the deadline.
        _ => Ok(Err(PollPending::Retry(format!(
            "transient token error (HTTP {status}): {error}"
        )))),
    }
}

#[derive(Debug)]
enum PollPending {
    Pending,
    SlowDown(Option<Duration>),
    /// A transient failure (network blip, proxy 5xx/interstitial, unparseable body). Keep polling until
    /// the deadline instead of aborting sign-in — one bad poll out of ~200 must not fail the whole login.
    Retry(String),
}

async fn poll_for_api_key(
    client: &reqwest::Client,
    base_url: &str,
    start: &DeviceStartResponse,
) -> io::Result<String> {
    let deadline =
        Instant::now() + Duration::from_secs(start.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECS));
    let mut interval = Duration::from_secs(start.interval.unwrap_or(DEFAULT_INTERVAL_SECS));
    loop {
        match request_device_token(client, base_url, &start.device_code).await? {
            Ok(api_key) => return Ok(api_key),
            Err(PollPending::Pending) => {}
            Err(PollPending::SlowDown(retry_after)) => {
                interval = retry_after.unwrap_or(interval + Duration::from_secs(5));
            }
            Err(PollPending::Retry(reason)) => {
                eprintln!("Still waiting for approval (transient issue: {reason}); retrying...");
            }
        }

        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "device login code expired",
            ));
        }
        tokio::time::sleep(interval).await;
    }
}

fn print_device_prompt(start: &DeviceStartResponse, url: &str) {
    eprintln!(
        "\
Sign in to Motyga from your browser to authorize this device.

Code: {}
URL:  {}

Waiting for approval. On a remote or headless machine, open the URL on a device where you are signed in.
",
        start.user_code, url
    );
}

pub async fn run_motyga_device_login(opts: MotygaDeviceLoginOptions) -> io::Result<()> {
    let base_url = opts.base_url.trim_end_matches('/').to_string();
    let client = build_default_auth_reqwest_client(&base_url, opts.auth_route_config.as_ref())
        .map_err(io::Error::other)?;
    let start = request_device_start(&client, &base_url).await?;
    let url = start
        .verification_uri_complete
        .as_deref()
        .or(start.verification_uri.as_deref())
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| {
            io::Error::other("device login start response did not include an authorization URL")
        })?;

    print_device_prompt(&start, url);
    if opts.open_browser
        && let Err(err) = webbrowser::open(url)
    {
        eprintln!("Could not open the browser automatically: {err}");
    }

    let api_key = poll_for_api_key(&client, &base_url, &start).await?;
    login_with_api_key(
        &opts.codex_home,
        &api_key,
        opts.cli_auth_credentials_store_mode,
        opts.auth_keyring_backend_kind,
    )
}
