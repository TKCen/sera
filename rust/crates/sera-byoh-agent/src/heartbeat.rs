use tracing::{debug, warn};

pub async fn run(
    core_url: String,
    instance_id: String,
    token: String,
    interval_ms: u64,
) {
    let client = reqwest::Client::new();
    let url = format!("{core_url}/api/agents/{instance_id}/heartbeat");
    let interval = tokio::time::Duration::from_millis(interval_ms);

    loop {
        tokio::time::sleep(interval).await;

        match client
            .post(&url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => {
                debug!("Heartbeat sent");
            }
            Ok(res) => {
                warn!("Heartbeat returned {}", res.status());
            }
            Err(e) => {
                warn!("Heartbeat failed: {e}");
            }
        }
    }
}
