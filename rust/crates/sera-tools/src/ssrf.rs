//! SSRF protection — blocks requests to loopback, link-local, private, and cloud metadata endpoints.

use std::net::IpAddr;

/// Errors from SSRF validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SsrfError {
    #[error("address is loopback")]
    Loopback,
    #[error("address is link-local")]
    LinkLocal,
    #[error("address is a cloud metadata endpoint")]
    CloudMetadata,
    #[error("address is in a private range")]
    PrivateRange,
    /// `0.0.0.0` / `::` — unspecified addresses.  Linux `connect(2)` rewrites
    /// these to loopback, so they are as dangerous as `127.0.0.1` in
    /// practice.  Also covers `255.255.255.255` (limited broadcast).
    #[error("address is unspecified or broadcast")]
    Unspecified,
    #[error("address is not allowed: {reason}")]
    NotAllowed { reason: String },
    #[error("parse error: {reason}")]
    ParseError { reason: String },
}

/// Validates that an address is not an SSRF risk.
pub struct SsrfValidator;

impl SsrfValidator {
    /// Validate that `addr` is safe to connect to.
    ///
    /// Blocks loopback (127.0.0.0/8, ::1), link-local (169.254.0.0/16, fe80::/10),
    /// cloud metadata endpoints (169.254.169.254, 100.100.100.200), RFC-1918 private
    /// ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16), and IPv6 ULA (fc00::/7).
    pub fn validate(addr: &str) -> Result<(), SsrfError> {
        // Strip port if present (e.g. "127.0.0.1:8080")
        let host = if let Some(stripped) = addr.strip_prefix('[') {
            // IPv6 with brackets: [::1]:8080
            stripped
                .split(']')
                .next()
                .unwrap_or(addr)
        } else if addr.matches(':').count() > 1 {
            // Bare IPv6 without brackets.  We use colon-count instead of
            // "contains ':' and no '.'" because IPv4-mapped syntax
            // (`::ffff:10.0.0.1`) contains both — the old heuristic
            // classified it as hostname-with-port and masked the bypass.
            addr
        } else {
            // IPv4 or hostname — strip port
            addr.split(':').next().unwrap_or(addr)
        };

        // Distinguish hostname inputs from malformed-IP inputs.
        // An IP address contains only: digits, hex letters a-f/A-F, colons,
        // dots, and (for bracketed IPv6) brackets. Anything else is a hostname.
        // Heuristic: an input "looks like an IP" if every character is one that
        // could appear in an IP address or a common IP-bypass encoding attempt
        // (digits, hex letters a-f, dots, colons, percent sign).
        // Inputs with letters outside a-f (e.g. "localhost", "example.com")
        // are classified as hostnames and get NotAllowed.
        // Percent-encoded bypass attempts (e.g. "169.254.169%2E254") still
        // get ParseError so callers know the input was IP-shaped but malformed.
        let looks_like_ip = !host.is_empty()
            && host.bytes().all(|b| {
                b.is_ascii_digit()
                    || b == b'.'
                    || b == b':'
                    || b == b'%'
                    || matches!(b, b'a'..=b'f' | b'A'..=b'F')
            });

        let ip: IpAddr = host.parse().map_err(|e: std::net::AddrParseError| {
            if looks_like_ip {
                SsrfError::ParseError {
                    reason: e.to_string(),
                }
            } else {
                SsrfError::NotAllowed {
                    reason: "hostname inputs are not supported — resolve to IP and re-validate"
                        .to_string(),
                }
            }
        })?;

        match ip {
            IpAddr::V4(v4) => Self::validate_v4(v4),
            IpAddr::V6(v6) => {
                // IPv4-mapped IPv6 (`::ffff:a.b.c.d`) connects on the v4
                // stack on Linux — run it through the v4 rules so
                // `[::ffff:10.0.0.1]` is blocked just like `10.0.0.1`.
                if let Some(v4) = v6.to_ipv4_mapped() {
                    return Self::validate_v4(v4);
                }
                // `::` — unspecified; kernel `connect(2)` rewrites to loopback.
                if v6.is_unspecified() {
                    return Err(SsrfError::Unspecified);
                }
                // ::1 — loopback
                if v6.is_loopback() {
                    return Err(SsrfError::Loopback);
                }
                let segments = v6.segments();
                // fe80::/10 — link-local
                if (segments[0] & 0xffc0) == 0xfe80 {
                    return Err(SsrfError::LinkLocal);
                }
                // fc00::/7 — IPv6 ULA (fc00:: through fdff::)
                if (segments[0] & 0xfe00) == 0xfc00 {
                    return Err(SsrfError::PrivateRange);
                }
                Ok(())
            }
        }
    }

    /// IPv4-specific rules.  Split out so [`Ipv6Addr::to_ipv4_mapped`] can
    /// rerun the v4 blocklist on `::ffff:a.b.c.d` addresses (which Linux
    /// routes to the v4 stack on connect).
    fn validate_v4(v4: std::net::Ipv4Addr) -> Result<(), SsrfError> {
        let octets = v4.octets();
        // Cloud metadata endpoints (check before link-local since they overlap)
        // 169.254.169.254 — AWS/GCP/Azure metadata
        if octets == [169, 254, 169, 254] {
            return Err(SsrfError::CloudMetadata);
        }
        // 100.100.100.200 — Alibaba Cloud metadata
        if octets == [100, 100, 100, 200] {
            return Err(SsrfError::CloudMetadata);
        }
        // 0.0.0.0 — unspecified (kernel rewrites to loopback on connect)
        if octets == [0, 0, 0, 0] {
            return Err(SsrfError::Unspecified);
        }
        // 255.255.255.255 — limited broadcast
        if octets == [255, 255, 255, 255] {
            return Err(SsrfError::Unspecified);
        }
        // 127.0.0.0/8 — loopback
        if octets[0] == 127 {
            return Err(SsrfError::Loopback);
        }
        // 169.254.0.0/16 — link-local
        if octets[0] == 169 && octets[1] == 254 {
            return Err(SsrfError::LinkLocal);
        }
        // 10.0.0.0/8 — RFC-1918 private
        if octets[0] == 10 {
            return Err(SsrfError::PrivateRange);
        }
        // 172.16.0.0/12 — RFC-1918 private (172.16.0.0 – 172.31.255.255)
        if octets[0] == 172 && (16..=31).contains(&octets[1]) {
            return Err(SsrfError::PrivateRange);
        }
        // 192.168.0.0/16 — RFC-1918 private
        if octets[0] == 192 && octets[1] == 168 {
            return Err(SsrfError::PrivateRange);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Loopback ---

    #[test]
    fn blocks_loopback_127_0_0_1() {
        assert_eq!(SsrfValidator::validate("127.0.0.1"), Err(SsrfError::Loopback));
    }

    #[test]
    fn blocks_loopback_127_0_0_1_with_port() {
        assert_eq!(SsrfValidator::validate("127.0.0.1:8080"), Err(SsrfError::Loopback));
    }

    #[test]
    fn blocks_loopback_127_255_255_255() {
        // Entire 127.0.0.0/8 must be blocked, not just .1
        assert_eq!(SsrfValidator::validate("127.255.255.255"), Err(SsrfError::Loopback));
    }

    #[test]
    fn blocks_loopback_127_0_0_2() {
        assert_eq!(SsrfValidator::validate("127.0.0.2"), Err(SsrfError::Loopback));
    }

    #[test]
    fn blocks_ipv6_loopback() {
        assert_eq!(SsrfValidator::validate("::1"), Err(SsrfError::Loopback));
    }

    #[test]
    fn blocks_ipv6_loopback_bracketed_with_port() {
        assert_eq!(SsrfValidator::validate("[::1]:443"), Err(SsrfError::Loopback));
    }

    // --- Link-local ---

    #[test]
    fn blocks_link_local_169_254_0_1() {
        assert_eq!(SsrfValidator::validate("169.254.0.1"), Err(SsrfError::LinkLocal));
    }

    #[test]
    fn blocks_link_local_169_254_255_255() {
        assert_eq!(SsrfValidator::validate("169.254.255.255"), Err(SsrfError::LinkLocal));
    }

    #[test]
    fn blocks_link_local_169_254_with_port() {
        assert_eq!(SsrfValidator::validate("169.254.1.1:80"), Err(SsrfError::LinkLocal));
    }

    #[test]
    fn blocks_ipv6_link_local_fe80() {
        // fe80::1 — classic link-local
        assert_eq!(
            SsrfValidator::validate("fe80::1"),
            Err(SsrfError::LinkLocal)
        );
    }

    #[test]
    fn blocks_ipv6_link_local_fe80_bracketed() {
        assert_eq!(
            SsrfValidator::validate("[fe80::1]:80"),
            Err(SsrfError::LinkLocal)
        );
    }

    #[test]
    fn blocks_ipv6_link_local_fe8f_boundary() {
        // fe8f:: is still in fe80::/10 (fe80–febf)
        assert_eq!(
            SsrfValidator::validate("fe8f::1"),
            Err(SsrfError::LinkLocal)
        );
    }

    #[test]
    fn blocks_ipv6_link_local_febf_boundary() {
        // febf:: is the top of fe80::/10
        assert_eq!(
            SsrfValidator::validate("febf::1"),
            Err(SsrfError::LinkLocal)
        );
    }

    #[test]
    fn allows_ipv6_fec0_not_link_local() {
        // fec0:: is above fe80::/10 — not link-local, not loopback → should pass
        assert!(SsrfValidator::validate("fec0::1").is_ok());
    }

    // --- Cloud metadata ---

    #[test]
    fn blocks_aws_metadata_169_254_169_254() {
        assert_eq!(
            SsrfValidator::validate("169.254.169.254"),
            Err(SsrfError::CloudMetadata)
        );
    }

    #[test]
    fn blocks_aws_metadata_with_port() {
        assert_eq!(
            SsrfValidator::validate("169.254.169.254:80"),
            Err(SsrfError::CloudMetadata)
        );
    }

    #[test]
    fn blocks_alibaba_metadata_100_100_100_200() {
        assert_eq!(
            SsrfValidator::validate("100.100.100.200"),
            Err(SsrfError::CloudMetadata)
        );
    }

    // --- Allowed public IPs ---

    #[test]
    fn allows_public_ip_1_1_1_1() {
        assert!(SsrfValidator::validate("1.1.1.1").is_ok());
    }

    #[test]
    fn allows_public_ip_with_port() {
        assert!(SsrfValidator::validate("8.8.8.8:443").is_ok());
    }

    #[test]
    fn allows_public_ipv6_2001() {
        assert!(SsrfValidator::validate("2001:db8::1").is_ok());
    }

    #[test]
    fn allows_public_ipv6_bracketed() {
        assert!(SsrfValidator::validate("[2001:db8::1]:443").is_ok());
    }

    // --- Percent-encoded / parse bypass attempts ---

    #[test]
    fn rejects_percent_encoded_ip_as_parse_error() {
        // Percent-encoding is not valid in a bare IP address; the parser
        // must return ParseError rather than accidentally resolving it.
        let result = SsrfValidator::validate("169.254.169%2E254");
        assert!(
            matches!(result, Err(SsrfError::ParseError { .. })),
            "expected ParseError for percent-encoded input, got {result:?}"
        );
    }

    #[test]
    fn rejects_decimal_encoded_ip_as_parse_error() {
        // Integer-form IP (e.g. 2130706433 = 127.0.0.1) is not parsed by
        // std::net::IpAddr — must surface as ParseError, not pass through.
        let result = SsrfValidator::validate("2130706433");
        assert!(
            matches!(result, Err(SsrfError::ParseError { .. })),
            "expected ParseError for decimal-form IP, got {result:?}"
        );
    }

    #[test]
    fn rejects_octal_encoded_ip_as_parse_error() {
        // Octal notation (0177.0.0.1) is not valid in std::net::IpAddr.
        let result = SsrfValidator::validate("0177.0.0.1");
        assert!(
            matches!(result, Err(SsrfError::ParseError { .. })),
            "expected ParseError for octal IP, got {result:?}"
        );
    }

    #[test]
    fn rejects_hostname_as_not_allowed() {
        // Hostnames are not IPs — validator returns NotAllowed so callers can
        // distinguish "bad input type" from "malformed IP".
        let result = SsrfValidator::validate("localhost");
        assert!(
            matches!(result, Err(SsrfError::NotAllowed { .. })),
            "expected NotAllowed for hostname, got {result:?}"
        );
    }

    #[test]
    fn rejects_empty_string_as_not_allowed() {
        // Empty string is not an IP address — treated as non-IP input.
        let result = SsrfValidator::validate("");
        assert!(
            matches!(result, Err(SsrfError::NotAllowed { .. })),
            "expected NotAllowed for empty string, got {result:?}"
        );
    }

    // --- Hostname / input-type classification (G2) ---

    #[test]
    fn rejects_hostname_example_com_as_not_allowed() {
        let result = SsrfValidator::validate("example.com");
        assert!(
            matches!(result, Err(SsrfError::NotAllowed { .. })),
            "expected NotAllowed for hostname, got {result:?}"
        );
    }

    #[test]
    fn not_allowed_reason_is_populated() {
        let err = SsrfValidator::validate("evil.internal").unwrap_err();
        if let SsrfError::NotAllowed { reason } = err {
            assert!(!reason.is_empty(), "reason must not be empty");
        } else {
            panic!("expected NotAllowed, got something else");
        }
    }

    #[test]
    fn rejects_malformed_ip_like_input_as_parse_error() {
        // 192.168.1.999 looks like an IP (only digits and dots) but is invalid.
        let result = SsrfValidator::validate("192.168.1.999");
        assert!(
            matches!(result, Err(SsrfError::ParseError { .. })),
            "expected ParseError for malformed IP-like input, got {result:?}"
        );
    }

    #[test]
    fn rejects_localhost_hostname_as_not_allowed() {
        // "localhost" is a hostname, not an IP — must be NotAllowed.
        let result = SsrfValidator::validate("localhost");
        assert!(
            matches!(result, Err(SsrfError::NotAllowed { .. })),
            "expected NotAllowed for localhost, got {result:?}"
        );
    }

    #[test]
    fn rejects_dotted_hostname_as_not_allowed() {
        let result = SsrfValidator::validate("metadata.internal");
        assert!(
            matches!(result, Err(SsrfError::NotAllowed { .. })),
            "expected NotAllowed for dotted hostname, got {result:?}"
        );
    }

    // --- RFC-1918 private ranges ---

    // 10.0.0.0/8
    #[test]
    fn blocks_rfc1918_10_0_0_0() {
        assert_eq!(SsrfValidator::validate("10.0.0.0"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn blocks_rfc1918_10_255_255_255() {
        assert_eq!(SsrfValidator::validate("10.255.255.255"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn allows_9_255_255_255_not_private() {
        // Just below 10.0.0.0/8
        assert!(SsrfValidator::validate("9.255.255.255").is_ok());
    }

    #[test]
    fn allows_11_0_0_0_not_private() {
        // Just above 10.0.0.0/8
        assert!(SsrfValidator::validate("11.0.0.0").is_ok());
    }

    // 172.16.0.0/12
    #[test]
    fn allows_172_15_255_255_not_private() {
        // Just below 172.16.0.0/12
        assert!(SsrfValidator::validate("172.15.255.255").is_ok());
    }

    #[test]
    fn blocks_rfc1918_172_16_0_0() {
        assert_eq!(SsrfValidator::validate("172.16.0.0"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn blocks_rfc1918_172_31_255_255() {
        assert_eq!(SsrfValidator::validate("172.31.255.255"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn allows_172_32_0_0_not_private() {
        // Just above 172.16.0.0/12
        assert!(SsrfValidator::validate("172.32.0.0").is_ok());
    }

    // 192.168.0.0/16
    #[test]
    fn allows_192_167_255_255_not_private() {
        // Just below 192.168.0.0/16
        assert!(SsrfValidator::validate("192.167.255.255").is_ok());
    }

    #[test]
    fn blocks_rfc1918_192_168_0_0() {
        assert_eq!(SsrfValidator::validate("192.168.0.0"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn blocks_rfc1918_192_168_255_255() {
        assert_eq!(SsrfValidator::validate("192.168.255.255"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn allows_192_169_0_0_not_private() {
        // Just above 192.168.0.0/16
        assert!(SsrfValidator::validate("192.169.0.0").is_ok());
    }

    // --- IPv6 ULA (fc00::/7) ---

    #[test]
    fn blocks_ipv6_ula_fc00() {
        assert_eq!(SsrfValidator::validate("fc00::1"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn blocks_ipv6_ula_fd00() {
        assert_eq!(SsrfValidator::validate("fd00::1"), Err(SsrfError::PrivateRange));
    }

    #[test]
    fn allows_ipv6_fbff_not_ula() {
        // fbff:ffff:...:ffff is just below fc00::/7
        assert!(SsrfValidator::validate("fbff:ffff:ffff:ffff:ffff:ffff:ffff:ffff").is_ok());
    }

    #[test]
    fn allows_ipv6_fe00_not_ula() {
        // fe00:: is above fdff::/7 — not ULA (fe80:: is link-local, but fe00:: is neither)
        assert!(SsrfValidator::validate("fe00::1").is_ok());
    }

    // --- Error trait / display ---

    #[test]
    fn ssrf_error_display_strings() {
        assert_eq!(SsrfError::Loopback.to_string(), "address is loopback");
        assert_eq!(SsrfError::LinkLocal.to_string(), "address is link-local");
        assert_eq!(
            SsrfError::CloudMetadata.to_string(),
            "address is a cloud metadata endpoint"
        );
    }

    #[test]
    fn ssrf_error_parse_error_contains_reason() {
        let err = SsrfError::ParseError {
            reason: "invalid octets".to_string(),
        };
        assert!(err.to_string().contains("invalid octets"));
    }

    // --- Unspecified / broadcast ---

    /// `0.0.0.0` must be blocked — Linux `connect(2)` rewrites it to
    /// loopback, so passing it through the validator would let an attacker
    /// reach the local host anyway.
    #[test]
    fn blocks_ipv4_unspecified() {
        assert_eq!(SsrfValidator::validate("0.0.0.0"), Err(SsrfError::Unspecified));
    }

    /// `255.255.255.255` limited broadcast — also rejected to prevent
    /// directed-broadcast amplification / local-network reach.
    #[test]
    fn blocks_ipv4_limited_broadcast() {
        assert_eq!(
            SsrfValidator::validate("255.255.255.255"),
            Err(SsrfError::Unspecified)
        );
    }

    /// `::` must be blocked for the same reason as `0.0.0.0` — kernel maps
    /// the unspecified IPv6 address to loopback on connect.
    #[test]
    fn blocks_ipv6_unspecified() {
        assert_eq!(SsrfValidator::validate("::"), Err(SsrfError::Unspecified));
    }

    // --- IPv4-mapped IPv6 (the Linux dual-stack bypass) ---

    /// `[::ffff:10.0.0.1]` → kernel connects via the v4 stack to 10.0.0.1.
    /// Pre-fix, the IPv6 branch saw no matching rule and the address passed
    /// through.  Post-fix, `Ipv6Addr::to_ipv4_mapped()` projects it back to
    /// v4 and reruns the v4 rules, which reject RFC-1918.
    #[test]
    fn blocks_ipv4_mapped_private_range() {
        assert_eq!(
            SsrfValidator::validate("::ffff:10.0.0.1"),
            Err(SsrfError::PrivateRange)
        );
        assert_eq!(
            SsrfValidator::validate("[::ffff:192.168.1.1]:8080"),
            Err(SsrfError::PrivateRange)
        );
    }

    /// IPv4-mapped loopback (`::ffff:127.0.0.1`) must also be rejected.
    #[test]
    fn blocks_ipv4_mapped_loopback() {
        assert_eq!(
            SsrfValidator::validate("::ffff:127.0.0.1"),
            Err(SsrfError::Loopback)
        );
    }

    /// IPv4-mapped cloud metadata (`::ffff:169.254.169.254`) must be
    /// rejected — this is the concrete EC2 IMDS exfil bypass the security
    /// review flagged.
    #[test]
    fn blocks_ipv4_mapped_cloud_metadata() {
        assert_eq!(
            SsrfValidator::validate("::ffff:169.254.169.254"),
            Err(SsrfError::CloudMetadata)
        );
    }
}
