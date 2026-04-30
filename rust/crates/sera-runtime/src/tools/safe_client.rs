//! Shared SSRF-validated `reqwest::Client` builder for tools that talk to
//! configured runtime URLs (bead sera-rdg4).
//!
//! The four runtime tool sites that previously called `reqwest::Client::new()`
//! against a host taken from configuration (`SERA_CORE_URL`, the Centrifugo
//! base URL) now route through [`build_validated_client`].  The helper
//! pre-flights the URL through [`SsrfValidator::resolve_and_validate`] and
//! pins the resolved addrs via `reqwest::ClientBuilder::resolve_to_addrs` to
//! mitigate DNS rebinding — the same pattern already used by
//! [`crate::tools::http_request`] and [`crate::tools::web_fetch`].
//!
//! Loopback (127.0.0.0/8, ::1), RFC-1918 private ranges, link-local,
//! IPv6 ULA, and cloud metadata endpoints (169.254.169.254, 100.100.100.200)
//! are rejected before any connection attempt.

use sera_tools::ssrf::{SsrfError, SsrfValidator};

/// Errors from base-URL validation + client construction.
#[derive(Debug, thiserror::Error)]
pub enum SafeClientError {
    #[error("invalid base URL '{url}': {reason}")]
    InvalidUrl { url: String, reason: String },
    #[error("ssrf: refusing to connect to {host}: {reason}")]
    Ssrf { host: String, reason: SsrfError },
    #[error("client build failed: {0}")]
    Build(String),
}

/// Validate `base_url` against the SSRF blocklist and return a
/// [`reqwest::Client`] whose DNS answer for `base_url`'s host is pinned to
/// the validated `SocketAddr`s.
///
/// Hostnames go through `tokio::net::lookup_host`; every resolved address is
/// checked against the blocklist.  A hostname that mixes public and private
/// IPs is rejected outright — partial trust is not allowed.
pub async fn build_validated_client(
    base_url: &str,
) -> Result<reqwest::Client, SafeClientError> {
    let parsed = reqwest::Url::parse(base_url).map_err(|e| SafeClientError::InvalidUrl {
        url: base_url.to_string(),
        reason: e.to_string(),
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| SafeClientError::InvalidUrl {
            url: base_url.to_string(),
            reason: "URL has no host".to_string(),
        })?
        .to_owned();
    let port = parsed.port_or_known_default().unwrap_or(80);

    let addrs = SsrfValidator::resolve_and_validate(&host, port)
        .await
        .map_err(|e| SafeClientError::Ssrf {
            host: host.clone(),
            reason: e,
        })?;

    reqwest::Client::builder()
        .resolve_to_addrs(&host, &addrs)
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| SafeClientError::Build(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Negative: configured-URL host blocks (loopback, private, link-local,
    //    ipv6 loopback, cloud metadata).  Each maps to the matching
    //    `SsrfError` variant so tools surface a precise reason.

    #[tokio::test]
    async fn rejects_loopback_v4() {
        let err = build_validated_client("http://127.0.0.1:8080")
            .await
            .expect_err("loopback must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::Loopback),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_loopback_v6() {
        let err = build_validated_client("http://[::1]:8080")
            .await
            .expect_err("ipv6 loopback must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::Loopback),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_rfc1918_10() {
        let err = build_validated_client("http://10.0.0.10:8080")
            .await
            .expect_err("10.0.0.0/8 must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::PrivateRange),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_rfc1918_192_168() {
        let err = build_validated_client("http://192.168.1.1:8080")
            .await
            .expect_err("192.168.0.0/16 must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::PrivateRange),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_rfc1918_172_16() {
        let err = build_validated_client("http://172.16.0.1:8080")
            .await
            .expect_err("172.16.0.0/12 must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::PrivateRange),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_link_local_metadata() {
        // 169.254.169.254 is the AWS/GCP/Azure IMDS — the v4 validator
        // reports it as `CloudMetadata`, not generic `LinkLocal`.
        let err = build_validated_client("http://169.254.169.254/")
            .await
            .expect_err("cloud metadata must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::CloudMetadata),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_link_local_general() {
        let err = build_validated_client("http://169.254.0.5/")
            .await
            .expect_err("169.254.0.0/16 must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::LinkLocal),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_localhost_hostname() {
        // `localhost` resolves to 127.0.0.1 / ::1 — both loopback.  The
        // post-resolve validator must reject the resolved IPs, not let the
        // hostname through because the literal isn't in the blocklist.
        let err = build_validated_client("http://localhost:8080/")
            .await
            .expect_err("localhost-resolving-to-loopback must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { .. }),
            "got {err:?}"
        );
    }

    #[tokio::test]
    async fn rejects_invalid_url() {
        let err = build_validated_client("not a url")
            .await
            .expect_err("malformed URL must be rejected");
        assert!(matches!(err, SafeClientError::InvalidUrl { .. }), "got {err:?}");
    }

    // ── Positive: a public IP literal passes validation and yields a usable
    //    client.  We do NOT issue a real request — the helper's contract is
    //    "validate + build", so checking that we got a `Client` back exercises
    //    the same code path the tools use.
    #[tokio::test]
    async fn accepts_public_ip_literal() {
        // 1.1.1.1 is a public IP — `resolve_and_validate` returns it
        // verbatim from `lookup_host`, blocklist lets it through, builder
        // produces a `Client`.
        let client = build_validated_client("https://1.1.1.1/")
            .await
            .expect("public IP must build a client");
        // Sanity: the value we got back is a real reqwest::Client, not a
        // panicking placeholder.  `Debug` is implemented on `Client`.
        let _ = format!("{client:?}");
    }

    /// Allowed-host happy path running through the same `build_validated_client`
    /// code path as the production tools, end-to-end against a `wiremock`
    /// mock server.  We bind wiremock to its default loopback socket, then
    /// pre-resolve a public-style hostname (`safe-test.example`) to that
    /// socket via `resolve_to_addrs` on a *separate* override client — this
    /// proves the validator + pinned-addrs pattern works without exposing a
    /// real loopback address through the validator.  The validator path is
    /// covered by `accepts_public_ip_literal` above; this test covers the
    /// reqwest pinning behaviour the helper depends on.
    #[tokio::test]
    async fn pinned_addrs_route_to_mock_server() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/health"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        // Parse the wiremock URI to grab the bound socket addr.
        let uri = server.uri();
        let parsed = reqwest::Url::parse(&uri).unwrap();
        let port = parsed.port().unwrap();
        let mock_addr: std::net::SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        // Build a client the same way `build_validated_client` does — pin
        // the addrs and set the timeout — but skip the SSRF call so we can
        // exercise the *positive* request path under the same builder.
        let host = "safe-test.example";
        let client = reqwest::Client::builder()
            .resolve_to_addrs(host, &[mock_addr])
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        let resp = client
            .get(format!("http://{host}:{port}/health"))
            .send()
            .await
            .expect("request must succeed against the mock");
        assert_eq!(resp.status().as_u16(), 200);
    }
}
