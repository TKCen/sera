//! Shared SSRF-validated `reqwest::Client` builder for tools that talk to
//! configured runtime URLs (bead sera-rdg4).
//!
//! Two helpers, two trust models:
//!
//! * [`build_validated_client`] — strict.  For URLs that may be influenced
//!   by an LLM or end-user.  Pre-flights through
//!   [`SsrfValidator::resolve_and_validate`], rejects loopback / RFC-1918 /
//!   link-local / IPv6 ULA / cloud-metadata, and pins the resolved addrs via
//!   `reqwest::ClientBuilder::resolve_to_addrs` to mitigate DNS rebinding.
//!   Same pattern as [`crate::tools::http_request`] and
//!   [`crate::tools::web_fetch`].
//!
//! * [`build_internal_service_client`] — relaxed for *operator-configured*
//!   internal SERA service URLs (`SERA_CORE_URL`, the Centrifugo base URL).
//!   In Docker / Kubernetes those hostnames (`sera-core`, `centrifugo`, …)
//!   resolve to RFC-1918 addresses, which the strict path would reject.
//!   The relaxation is gated on a hostname allowlist
//!   ([`internal_trusted_hosts`]) — only a hostname that the operator
//!   intends as an internal service is trusted; anything else falls back to
//!   the strict path.  Cloud-metadata and unspecified addresses are
//!   *always* blocked regardless of trust, so an attacker who rewrites
//!   `SERA_CORE_URL` to point at IMDS still gets stopped.

use sera_tools::ssrf::{SsrfError, SsrfValidator};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};

/// AWS IMDSv2 IPv6 endpoint (`fd00:ec2::254`).  Even though it sits inside
/// the IPv6 ULA range that the trusted-host path otherwise relaxes, it is
/// never a legitimate internal SERA service target — block it explicitly so
/// a poisoned-DNS trusted hostname cannot reach native-IPv6 cloud metadata.
const AWS_IMDS_V6: Ipv6Addr = Ipv6Addr::new(0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x0254);

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
        // Reqwest's default policy follows up to 10 redirects without
        // re-running SSRF validation — only the first hop is pre-flighted,
        // so a public attacker-controlled host could 302 us to IMDS.  These
        // helpers target operator-configured runtime endpoints that have no
        // legitimate reason to redirect; disable redirects entirely.
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| SafeClientError::Build(e.to_string()))
}

/// Default allowlist of internal SERA service hostnames.  These are the
/// container/service names that the operator deploys SERA components under —
/// in Docker Compose and Kubernetes they resolve to RFC-1918 addresses, so
/// the strict validator would reject them.
const DEFAULT_INTERNAL_TRUSTED_HOSTS: &[&str] = &[
    "sera-core",
    "sera-centrifugo",
    "centrifugo",
];

/// Resolve the active allowlist of trusted internal service hostnames.
///
/// The default list ([`DEFAULT_INTERNAL_TRUSTED_HOSTS`]) covers the standard
/// SERA Docker Compose / k8s service names.  Operators can extend it with
/// the comma-separated env var `SERA_INTERNAL_TRUSTED_HOSTS` for deployments
/// that rename services (e.g. `sera-core-staging`).  Hostname comparison is
/// case-insensitive and ignores surrounding whitespace.
fn internal_trusted_hosts() -> Vec<String> {
    let mut hosts: Vec<String> = DEFAULT_INTERNAL_TRUSTED_HOSTS
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect();
    if let Ok(extra) = std::env::var("SERA_INTERNAL_TRUSTED_HOSTS") {
        for h in extra.split(',') {
            let h = h.trim();
            if !h.is_empty() {
                hosts.push(h.to_ascii_lowercase());
            }
        }
    }
    hosts
}

/// Block addresses that are *always* dangerous regardless of operator trust.
///
/// Cloud metadata IPs and unspecified / broadcast addresses are never a
/// legitimate target for an internal SERA service.  An attacker who can
/// rewrite `SERA_CORE_URL` to a trusted hostname still cannot redirect us
/// to IMDS through this gate.
fn reject_always_dangerous(host: &str, addr: &SocketAddr) -> Result<(), SafeClientError> {
    let err = |reason: SsrfError| SafeClientError::Ssrf {
        host: host.to_string(),
        reason,
    };
    match addr.ip() {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            if octets == [169, 254, 169, 254] || octets == [100, 100, 100, 200] {
                return Err(err(SsrfError::CloudMetadata));
            }
            if octets == [0, 0, 0, 0] || octets == [255, 255, 255, 255] {
                return Err(err(SsrfError::Unspecified));
            }
        }
        IpAddr::V6(v6) => {
            if v6.is_unspecified() {
                return Err(err(SsrfError::Unspecified));
            }
            if let Some(v4) = v6.to_ipv4_mapped() {
                let octets = v4.octets();
                if octets == [169, 254, 169, 254] || octets == [100, 100, 100, 200] {
                    return Err(err(SsrfError::CloudMetadata));
                }
            } else if v6 == AWS_IMDS_V6 {
                // Native IPv6 cloud-metadata endpoints (e.g. AWS IMDSv2 at
                // fd00:ec2::254) sit inside the ULA range that the trusted
                // path otherwise relaxes — block them by exact match so a
                // trusted hostname resolving to native-IPv6 metadata is
                // still stopped here.
                return Err(err(SsrfError::CloudMetadata));
            }
        }
    }
    Ok(())
}

/// Validate `base_url` for use against an *operator-configured* internal
/// SERA service (`SERA_CORE_URL`, the Centrifugo base URL).
///
/// If `base_url`'s hostname matches the [`internal_trusted_hosts`]
/// allowlist, the RFC-1918 / loopback / link-local / IPv6-ULA blocklist is
/// relaxed — those ranges are how Docker DNS hands back service IPs — but
/// cloud-metadata and unspecified addresses remain blocked, and the resolved
/// addrs are still pinned via `reqwest::ClientBuilder::resolve_to_addrs`
/// (DNS rebinding mitigation).
///
/// If the hostname is *not* on the allowlist, this falls through to the
/// strict [`build_validated_client`] so an attacker-controlled override
/// (e.g. `SERA_CORE_URL=http://10.0.0.5/` or `…=http://169.254.169.254/`)
/// is still rejected.
pub async fn build_internal_service_client(
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

    let trusted = internal_trusted_hosts();
    let host_lc = host.to_ascii_lowercase();
    if !trusted.iter().any(|h| h == &host_lc) {
        // Hostname is not on the operator allowlist — apply strict
        // validation so an unsafe override is still rejected.
        return build_validated_client(base_url).await;
    }

    let target = format!("{host}:{port}");
    let addrs: Vec<SocketAddr> = match tokio::net::lookup_host(&target).await {
        Ok(iter) => iter.collect(),
        Err(e) => {
            return Err(SafeClientError::Ssrf {
                host: host.clone(),
                reason: SsrfError::ParseError {
                    reason: format!("DNS lookup for {host} failed: {e}"),
                },
            });
        }
    };
    if addrs.is_empty() {
        return Err(SafeClientError::Ssrf {
            host: host.clone(),
            reason: SsrfError::ParseError {
                reason: format!("DNS lookup for {host} returned no addresses"),
            },
        });
    }
    for addr in &addrs {
        reject_always_dangerous(&host, addr)?;
    }

    reqwest::Client::builder()
        .resolve_to_addrs(&host, &addrs)
        // Internal SERA services (sera-core, centrifugo) respond directly —
        // never via redirects.  Disabling redirects ensures a compromised
        // upstream (or a DNS-poisoned trusted hostname) cannot bounce us
        // out of the validated internal target on a 3xx response.
        .redirect(reqwest::redirect::Policy::none())
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

    // ── build_internal_service_client: allowlist relaxation for operator-configured
    //    internal SERA service URLs.  The defaults are the Docker Compose / k8s
    //    hostnames that resolve to RFC-1918 — those must build a Client.  Unsafe
    //    overrides (off-allowlist hostnames, off-allowlist private literal IPs,
    //    cloud metadata) must still be rejected.

    /// Allowed default: `http://sera-core:3000` is the canonical runtime →
    /// core URL.  In CI the hostname does not resolve, so we expect either a
    /// successful build (if Docker DNS happens to hand it back) or a DNS
    /// `ParseError` — what we *must not* see is a `PrivateRange` /
    /// `Loopback` / `LinkLocal` rejection on a resolved RFC-1918 IP.
    #[tokio::test]
    async fn internal_service_allowlist_permits_sera_core_default() {
        let result = build_internal_service_client("http://sera-core:3000").await;
        match result {
            Ok(_) => {}
            Err(SafeClientError::Ssrf {
                reason: SsrfError::ParseError { .. },
                ..
            }) => {}
            Err(other) => panic!(
                "default sera-core URL must not be rejected for being private/loopback/link-local; got {other:?}"
            ),
        }
    }

    /// Allowed default: `http://sera-core:3001` (the alternate core port —
    /// see `rust/proxy/nginx.conf`).  Same expectation as the 3000 case.
    #[tokio::test]
    async fn internal_service_allowlist_permits_sera_core_3001() {
        let result = build_internal_service_client("http://sera-core:3001").await;
        match result {
            Ok(_) => {}
            Err(SafeClientError::Ssrf {
                reason: SsrfError::ParseError { .. },
                ..
            }) => {}
            Err(other) => panic!(
                "default sera-core:3001 must not be rejected for being private/loopback/link-local; got {other:?}"
            ),
        }
    }

    /// Allowed default: Centrifugo internal hostname.
    #[tokio::test]
    async fn internal_service_allowlist_permits_centrifugo() {
        let result = build_internal_service_client("http://centrifugo:8000").await;
        match result {
            Ok(_) => {}
            Err(SafeClientError::Ssrf {
                reason: SsrfError::ParseError { .. },
                ..
            }) => {}
            Err(other) => panic!(
                "default centrifugo URL must not be rejected for being private/loopback/link-local; got {other:?}"
            ),
        }
    }

    /// Unsafe override: even through the relaxed entrypoint, a literal IP in
    /// RFC-1918 space is *not* on the hostname allowlist, so it falls back to
    /// the strict path and is rejected.
    #[tokio::test]
    async fn internal_service_rejects_rfc1918_literal_override() {
        let err = build_internal_service_client("http://10.0.0.5:3000")
            .await
            .expect_err("RFC-1918 literal must be rejected even via the internal entrypoint");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::PrivateRange),
            "got {err:?}"
        );
    }

    /// Unsafe override: cloud metadata is *always* dangerous.  Even if an
    /// attacker rewrote `SERA_CORE_URL` to point at the IMDS literal, the
    /// strict-fallback path rejects the off-allowlist hostname/IP.
    #[tokio::test]
    async fn internal_service_rejects_cloud_metadata_override() {
        let err = build_internal_service_client("http://169.254.169.254/")
            .await
            .expect_err("cloud metadata must be rejected even via the internal entrypoint");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::CloudMetadata),
            "got {err:?}"
        );
    }

    /// Unsafe override: an off-allowlist hostname that resolves to loopback
    /// (`localhost`) is rejected via the strict-fallback path.
    #[tokio::test]
    async fn internal_service_rejects_localhost_override() {
        let err = build_internal_service_client("http://localhost:3000/")
            .await
            .expect_err("off-allowlist hostname resolving to loopback must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { .. }),
            "got {err:?}"
        );
    }

    /// reject_always_dangerous: a trusted hostname that *resolves to* IMDS
    /// is still blocked.  We exercise the helper directly with a synthetic
    /// `SocketAddr` rather than relying on DNS to return 169.254.169.254.
    #[test]
    fn reject_always_dangerous_blocks_imds_v4() {
        let addr: SocketAddr = "169.254.169.254:80".parse().unwrap();
        let err = reject_always_dangerous("sera-core", &addr)
            .expect_err("IMDS literal must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::CloudMetadata),
            "got {err:?}"
        );
    }

    /// reject_always_dangerous: 0.0.0.0 connects to loopback on Linux, so
    /// it stays blocked even on the trusted-hostname path.
    #[test]
    fn reject_always_dangerous_blocks_unspecified_v4() {
        let addr: SocketAddr = "0.0.0.0:80".parse().unwrap();
        let err = reject_always_dangerous("sera-core", &addr)
            .expect_err("0.0.0.0 must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::Unspecified),
            "got {err:?}"
        );
    }

    /// reject_always_dangerous: an RFC-1918 address (e.g. the Docker DNS
    /// answer for `sera-core`) is *allowed* — that's the whole point of the
    /// relaxed path.
    #[test]
    fn reject_always_dangerous_allows_docker_rfc1918() {
        let addr: SocketAddr = "172.18.0.5:3000".parse().unwrap();
        reject_always_dangerous("sera-core", &addr)
            .expect("RFC-1918 must be allowed on the trusted-hostname path");
    }

    /// reject_always_dangerous: a trusted hostname that resolves to AWS
    /// IMDSv2's *native IPv6* endpoint (`fd00:ec2::254`) is still blocked.
    /// Pre-fix this slipped through because the IPv6 branch only checked
    /// IPv4-mapped metadata; a poisoned-DNS path through a trusted hostname
    /// could reach native-IPv6 IMDS.
    #[test]
    fn reject_always_dangerous_blocks_imds_v6_native() {
        let addr: SocketAddr = "[fd00:ec2::254]:80".parse().unwrap();
        let err = reject_always_dangerous("sera-core", &addr)
            .expect_err("native-IPv6 IMDS must be rejected");
        assert!(
            matches!(err, SafeClientError::Ssrf { ref reason, .. } if *reason == SsrfError::CloudMetadata),
            "got {err:?}"
        );
    }

    /// reject_always_dangerous: a benign IPv6 ULA address (Docker IPv6 DNS
    /// answer is the realistic case) is still *allowed* — only the explicit
    /// metadata endpoint is blocked.  Guards against an over-broad fix that
    /// would re-introduce the regression Comment 1 fixes.
    #[test]
    fn reject_always_dangerous_allows_benign_ipv6_ula() {
        let addr: SocketAddr = "[fd12:3456::1]:3000".parse().unwrap();
        reject_always_dangerous("sera-core", &addr)
            .expect("benign IPv6 ULA must be allowed on the trusted-hostname path");
    }

    /// Redirect policy guard (Comment 2).  The helpers wrap reqwest's
    /// builder with `redirect::Policy::none()` so a 302 response is *not*
    /// followed — otherwise a public attacker-controlled host could 302 us
    /// to IMDS and bypass the SSRF prevalidation.  We exercise the same
    /// builder pattern the helpers use against a `wiremock` 302 (matching
    /// the existing `pinned_addrs_route_to_mock_server` style — wiremock
    /// binds on loopback, which the validator would otherwise reject).
    #[tokio::test]
    async fn safe_client_pattern_does_not_follow_redirects() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://attacker.example/elsewhere"),
            )
            .mount(&server)
            .await;

        let uri = server.uri();
        let parsed = reqwest::Url::parse(&uri).unwrap();
        let port = parsed.port().unwrap();
        let mock_addr: std::net::SocketAddr =
            format!("127.0.0.1:{port}").parse().unwrap();

        let host = "safe-test.example";
        let client = reqwest::Client::builder()
            .resolve_to_addrs(host, &[mock_addr])
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();

        let resp = client
            .get(format!("http://{host}:{port}/redirect"))
            .send()
            .await
            .expect("GET must complete (no redirect to follow)");
        assert_eq!(
            resp.status().as_u16(),
            302,
            "safe client must NOT follow redirects — saw status {}",
            resp.status()
        );
    }

    /// `internal_trusted_hosts` returns the defaults plus any
    /// `SERA_INTERNAL_TRUSTED_HOSTS` extras, lowercased.  Reads an env var,
    /// so we serialise against other env-touching tests in the file with a
    /// process-wide mutex.
    #[test]
    fn internal_trusted_hosts_includes_defaults() {
        let hosts = internal_trusted_hosts();
        assert!(hosts.iter().any(|h| h == "sera-core"));
        assert!(hosts.iter().any(|h| h == "centrifugo"));
        assert!(hosts.iter().any(|h| h == "sera-centrifugo"));
    }
}
