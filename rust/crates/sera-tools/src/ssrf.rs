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
    #[error("address is not allowed")]
    NotAllowed,
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

        let ip: IpAddr = host.parse().map_err(|e: std::net::AddrParseError| {
            SsrfError::ParseError {
                reason: e.to_string(),
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
