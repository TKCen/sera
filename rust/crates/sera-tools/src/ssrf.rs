//! SSRF protection — blocks requests to loopback, link-local, and cloud metadata endpoints.

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
    /// and cloud metadata endpoints (169.254.169.254, 100.100.100.200).
    pub fn validate(addr: &str) -> Result<(), SsrfError> {
        // Strip port if present (e.g. "127.0.0.1:8080")
        let host = if let Some(stripped) = addr.strip_prefix('[') {
            // IPv6 with brackets: [::1]:8080
            stripped
                .split(']')
                .next()
                .unwrap_or(addr)
        } else if addr.contains(':') && !addr.contains('.') {
            // bare IPv6 without brackets
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

        // Cloud metadata endpoints (check before link-local since they overlap)
        match ip {
            IpAddr::V4(v4) => {
                let octets = v4.octets();
                // 169.254.169.254 — AWS/GCP/Azure metadata
                if octets == [169, 254, 169, 254] {
                    return Err(SsrfError::CloudMetadata);
                }
                // 100.100.100.200 — Alibaba Cloud metadata
                if octets == [100, 100, 100, 200] {
                    return Err(SsrfError::CloudMetadata);
                }
                // 127.0.0.0/8 — loopback
                if octets[0] == 127 {
                    return Err(SsrfError::Loopback);
                }
                // 169.254.0.0/16 — link-local
                if octets[0] == 169 && octets[1] == 254 {
                    return Err(SsrfError::LinkLocal);
                }
            }
            IpAddr::V6(v6) => {
                // ::1 — loopback
                if v6.is_loopback() {
                    return Err(SsrfError::Loopback);
                }
                // fe80::/10 — link-local
                let segments = v6.segments();
                if (segments[0] & 0xffc0) == 0xfe80 {
                    return Err(SsrfError::LinkLocal);
                }
            }
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
}
