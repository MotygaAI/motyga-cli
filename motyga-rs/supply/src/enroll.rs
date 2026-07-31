//! Device-code enrollment: bind this machine to a Motyga account without ever typing a password here.
//!
//! The CLI asks for a code, the supplier approves it in a browser they are already signed into, and only
//! then is a node token minted and handed back on the next poll. A bare visit to the link approves nothing —
//! the same anti-phishing shape as the agent's own login, for the same reason: the link travels, and
//! whoever receives it must not be able to enroll a machine by opening it.

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct StartResponse {
    device_code: String,
    user_code: String,
    #[serde(default = "default_interval")]
    interval: u64,
    #[serde(default = "default_expires")]
    expires_in: u64,
    #[serde(default)]
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: String,
}

fn default_interval() -> u64 {
    3
}
fn default_expires() -> u64 {
    600
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    node_token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

pub async fn enroll(base: &str) -> Result<String> {
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;

    let start: StartResponse = http
        .post(format!("{base}/v1/fleet/enroll/start"))
        .send()
        .await
        .with_context(|| format!("cannot reach {base}"))?
        .error_for_status()
        .context("the backend refused to start an enrollment (is supply mode enabled for you?)")?
        .json()
        .await
        .context("unexpected response from the enrollment endpoint")?;

    let link = if start.verification_uri_complete.is_empty() {
        start.verification_uri.clone()
    } else {
        start.verification_uri_complete.clone()
    };
    println!("Open this link and approve the connection:");
    println!("  {link}");
    println!("Code: {}", start.user_code);
    println!();
    println!("You will be asked to accept the supplier agreement first — reselling subscription");
    println!("access very likely breaches your vendor's terms, and that risk is yours.");
    println!();
    println!("Waiting…");

    // Poll at the server's suggested interval until it says approved or the grant expires. `slow_down`
    // (429) is honoured rather than ignored: the endpoint is deliberately rate-limited.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(start.expires_in.clamp(60, 1800));
    let mut interval = Duration::from_secs(start.interval.clamp(1, 30));
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("the enrollment code expired — run `motyga supply login` again"));
        }
        tokio::time::sleep(interval).await;

        let resp = http
            .post(format!("{base}/v1/fleet/enroll/token"))
            .json(&serde_json::json!({ "device_code": start.device_code }))
            .send()
            .await
            .context("polling the enrollment endpoint")?;

        if resp.status().as_u16() == 429 {
            interval = (interval * 2).min(Duration::from_secs(30));
            continue;
        }
        let body: TokenResponse = resp
            .json()
            .await
            .context("unexpected response while polling for approval")?;
        if body.status.as_deref() == Some("approved")
            && let Some(token) = body.node_token
        {
            return Ok(token);
        }
        match body.error.as_deref() {
            Some("authorization_pending") | None => continue,
            Some("slow_down") => {
                interval = (interval * 2).min(Duration::from_secs(30));
            }
            Some(other) => {
                return Err(anyhow!(
                    "enrollment failed: {other} — start again with `motyga supply login`"
                ));
            }
        }
    }
}
