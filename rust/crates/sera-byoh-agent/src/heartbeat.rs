use tracing::{debug, warn};

/// Build the heartbeat URL from core_url and instance_id.
pub fn build_heartbeat_url(core_url: &str, instance_id: &str) -> String {
    format!("{core_url}/api/agents/{instance_id}/heartbeat")
}

/// Build the Authorization header value.
pub fn build_auth_header(token: &str) -> String {
    format!("Bearer {token}")
}

pub async fn run(
    core_url: String,
    instance_id: String,
    token: String,
    interval_ms: u64,
) {
    let client = reqwest::Client::new();
    let url = build_heartbeat_url(&core_url, &instance_id);
    let interval = tokio::time::Duration::from_millis(interval_ms);

    loop {
        tokio::time::sleep(interval).await;

        match client
            .post(&url)
            .header("Authorization", build_auth_header(&token))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_heartbeat_url_standard() {
        let url = build_heartbeat_url("https://sera.internal", "agent-001");
        assert_eq!(url, "https://sera.internal/api/agents/agent-001/heartbeat");
    }

    #[test]
    fn build_heartbeat_url_with_port() {
        let url = build_heartbeat_url("http://localhost:8080", "worker-abc");
        assert_eq!(url, "http://localhost:8080/api/agents/worker-abc/heartbeat");
    }

    #[test]
    fn build_heartbeat_url_preserves_slashes() {
        let url = build_heartbeat_url("https://example.com/", "test");
        // Note: trailing slash on core_url will result in double slash
        assert_eq!(url, "https://example.com//api/agents/test/heartbeat");
    }

    #[test]
    fn build_heartbeat_url_with_special_chars_in_id() {
        let url = build_heartbeat_url("http://localhost", "agent-with-dashes");
        assert_eq!(url, "http://localhost/api/agents/agent-with-dashes/heartbeat");
    }

    #[test]
    fn build_auth_header_standard() {
        let header = build_auth_header("sera_token_xyz");
        assert_eq!(header, "Bearer sera_token_xyz");
    }

    #[test]
    fn build_auth_header_with_long_token() {
        let long_token = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0";
        let header = build_auth_header(long_token);
        assert!(header.starts_with("Bearer "));
        assert!(header.contains("eyJhbGciOiJIUzI1NiIs"));
    }

    #[test]
    fn build_auth_header_empty_token() {
        let header = build_auth_header("");
        assert_eq!(header, "Bearer ");
    }

    #[test]
    fn heartbeat_interval_from_millis() {
        let interval = tokio::time::Duration::from_millis(5000);
        assert_eq!(interval.as_millis(), 5000);
    }

    #[test]
    fn heartbeat_interval_various_rates() {
        let intervals = vec![1000, 5000, 30000, 60000];
        for ms in intervals {
            let duration = tokio::time::Duration::from_millis(ms);
            assert_eq!(duration.as_millis() as u64, ms);
        }
    }
}
