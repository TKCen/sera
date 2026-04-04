//! Heartbeat — periodic pings to sera-core to announce agent liveness.

use reqwest::Client;
use tracing::{debug, warn};

pub async fn run(
    core_url: String,
    instance_id: String,
    identity_token: String,
    interval_ms: u64,
) {
    let client = Client::new();
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(interval_ms));

    loop {
        interval.tick().await;

        let url = format!("{}/api/agents/{}/heartbeat", core_url, instance_id);

        match client
            .post(&url)
            .header("Authorization", format!("Bearer {}", identity_token))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                debug!("Heartbeat sent successfully");
            }
            Ok(resp) => {
                warn!("Heartbeat failed with status: {}", resp.status());
            }
            Err(e) => {
                warn!("Heartbeat error: {}", e);
            }
        }
    }
}
