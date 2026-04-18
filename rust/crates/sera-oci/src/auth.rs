//! Docker `config.json` passthrough — registry credentials for OCI pulls.
//!
//! We don't implement a fully-featured Docker credential resolver (credential
//! helpers, `credsStore`, `credHelpers`) in phase S — that lands in phase M
//! when push support requires writing credentials. Phase S just parses the
//! `auths` map and returns the embedded base64 `auth` string (or explicit
//! `username` / `password` / `identitytoken` fields).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::OciError;

/// Parsed `~/.docker/config.json`. Only the fields we need are modelled —
/// unknown keys are preserved by serde's default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DockerConfig {
    /// Map from registry hostname → auth entry. Phase S honours this.
    #[serde(default)]
    pub auths: HashMap<String, DockerAuthEntry>,
}

/// A single entry in the `auths` map.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DockerAuthEntry {
    /// Base64-encoded `user:password`, the classic Docker format.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<String>,
    /// Explicit username (overrides `auth` decoding).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Explicit password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// OAuth identity token (used by `docker login` against ACR / GCR / GHCR).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identitytoken: Option<String>,
}

/// Credentials for a single registry resolved from `config.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryCredential {
    /// Username for HTTP Basic auth. May be empty when `identitytoken` is set
    /// — the Docker convention is `<token>` as the username in that case.
    pub username: String,
    /// Password or secret.
    pub password: String,
    /// Optional OAuth identity token. When present, prefer this over
    /// username/password.
    pub identitytoken: Option<String>,
}

/// Load the user's Docker config from the conventional locations.
///
/// Resolution order:
///   1. `$DOCKER_CONFIG/config.json` if the env var is set
///   2. `$HOME/.docker/config.json`
///
/// Returns a default (empty) config when neither file exists — a missing
/// file is not an error, it just means the user hasn't logged in.
pub fn load_docker_config() -> Result<DockerConfig, OciError> {
    let path = docker_config_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => load_docker_config_from_str(&s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DockerConfig::default()),
        Err(e) => Err(OciError::Io(e)),
    }
}

/// Test-friendly variant: parse a config.json body directly.
pub fn load_docker_config_from_str(body: &str) -> Result<DockerConfig, OciError> {
    serde_json::from_str(body).map_err(|e| OciError::Auth(format!("parse config.json: {e}")))
}

/// Look up credentials for `registry` inside `cfg`.
///
/// Matching uses the registry key verbatim and also tries the common
/// Docker-Hub aliases (`https://index.docker.io/v1/` ↔ `docker.io`). Phase S
/// ignores wildcard subdomains and credential helpers — fully covered by
/// phase M.
pub fn credential_for(registry: &str, cfg: &DockerConfig) -> Option<RegistryCredential> {
    let candidates = [
        registry.to_string(),
        format!("https://{registry}"),
        format!("https://{registry}/"),
        format!("https://{registry}/v1/"),
        format!("https://{registry}/v2/"),
        // docker.io special-case
        if registry == "docker.io" || registry == "index.docker.io" {
            "https://index.docker.io/v1/".to_string()
        } else {
            String::new()
        },
    ];

    for key in candidates.iter().filter(|k| !k.is_empty()) {
        if let Some(entry) = cfg.auths.get(key)
            && let Some(cred) = entry_to_credential(entry)
        {
            return Some(cred);
        }
    }
    None
}

fn entry_to_credential(entry: &DockerAuthEntry) -> Option<RegistryCredential> {
    // Explicit username/password wins over the base64 `auth` blob.
    if let (Some(u), Some(p)) = (&entry.username, &entry.password) {
        return Some(RegistryCredential {
            username: u.clone(),
            password: p.clone(),
            identitytoken: entry.identitytoken.clone(),
        });
    }

    // Decode the `auth` blob: base64 of `user:password`.
    if let Some(blob) = &entry.auth
        && let Some((u, p)) = decode_auth_blob(blob)
    {
        return Some(RegistryCredential {
            username: u,
            password: p,
            identitytoken: entry.identitytoken.clone(),
        });
    }

    // Identity-token-only entry — some registries issue tokens without a
    // paired user/pass. Use the Docker convention: username `<token>`,
    // password = the token itself.
    if let Some(token) = &entry.identitytoken {
        return Some(RegistryCredential {
            username: "<token>".to_string(),
            password: token.clone(),
            identitytoken: Some(token.clone()),
        });
    }

    None
}

/// Minimal dependency-free base64 decode for `user:password`.
///
/// We intentionally avoid pulling `base64` for a one-shot use. The input
/// must be standard (RFC 4648) base64 and must contain a literal `:` in the
/// decoded UTF-8 payload.
fn decode_auth_blob(blob: &str) -> Option<(String, String)> {
    let bytes = base64_decode_standard(blob.trim())?;
    let decoded = String::from_utf8(bytes).ok()?;
    let (u, p) = decoded.split_once(':')?;
    Some((u.to_string(), p.to_string()))
}

fn base64_decode_standard(input: &str) -> Option<Vec<u8>> {
    // RFC 4648 alphabet: A-Z a-z 0-9 + / with `=` padding.
    const PAD: u8 = 0xFF;
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => PAD,
            _ => 0xFE,
        }
    }

    let raw: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if !raw.len().is_multiple_of(4) {
        return None;
    }

    let mut out = Vec::with_capacity(raw.len() / 4 * 3);
    for chunk in raw.chunks(4) {
        let mut v = [0u8; 4];
        for (i, b) in chunk.iter().enumerate() {
            let x = val(*b);
            if x == 0xFE {
                return None;
            }
            v[i] = x;
        }
        let pad_count = chunk.iter().rev().take_while(|b| **b == b'=').count();
        let n0 = (v[0] << 2) | ((v[1] & 0x30) >> 4);
        out.push(n0);
        if pad_count < 2 {
            let n1 = ((v[1] & 0x0F) << 4) | ((v[2] & 0x3C) >> 2);
            out.push(n1);
        }
        if pad_count < 1 {
            let n2 = ((v[2] & 0x03) << 6) | v[3];
            out.push(n2);
        }
    }
    Some(out)
}

fn docker_config_path() -> PathBuf {
    if let Some(base) = std::env::var_os("DOCKER_CONFIG") {
        return PathBuf::from(base).join("config.json");
    }
    // Fall back to HOME.
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".docker").join("config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Realistic-shaped config.json. The `auth` value is base64("alice:hunter2").
    const DOCKER_CONFIG_FIXTURE: &str = r#"{
  "auths": {
    "ghcr.io": {
      "auth": "YWxpY2U6aHVudGVyMg=="
    },
    "https://index.docker.io/v1/": {
      "username": "bob",
      "password": "s3cret"
    },
    "quay.io": {
      "identitytoken": "oauth-token-xyz"
    }
  },
  "credsStore": "desktop"
}"#;

    #[test]
    fn parses_config_fixture() {
        let cfg = load_docker_config_from_str(DOCKER_CONFIG_FIXTURE).unwrap();
        assert_eq!(cfg.auths.len(), 3);
        assert!(cfg.auths.contains_key("ghcr.io"));
    }

    #[test]
    fn resolves_ghcr_credentials_via_auth_blob() {
        let cfg = load_docker_config_from_str(DOCKER_CONFIG_FIXTURE).unwrap();
        let cred = credential_for("ghcr.io", &cfg).expect("ghcr cred");
        assert_eq!(cred.username, "alice");
        assert_eq!(cred.password, "hunter2");
        assert!(cred.identitytoken.is_none());
    }

    #[test]
    fn resolves_docker_hub_alias() {
        let cfg = load_docker_config_from_str(DOCKER_CONFIG_FIXTURE).unwrap();
        let cred = credential_for("docker.io", &cfg).expect("docker.io cred");
        assert_eq!(cred.username, "bob");
        assert_eq!(cred.password, "s3cret");
    }

    #[test]
    fn resolves_identity_token_only_entry() {
        let cfg = load_docker_config_from_str(DOCKER_CONFIG_FIXTURE).unwrap();
        let cred = credential_for("quay.io", &cfg).expect("quay cred");
        assert_eq!(cred.identitytoken.as_deref(), Some("oauth-token-xyz"));
        assert_eq!(cred.password, "oauth-token-xyz");
    }

    #[test]
    fn unknown_registry_returns_none() {
        let cfg = load_docker_config_from_str(DOCKER_CONFIG_FIXTURE).unwrap();
        assert!(credential_for("private.example.com", &cfg).is_none());
    }

    #[test]
    fn explicit_username_password_beats_auth_blob() {
        let body = r#"{
          "auths": {
            "ghcr.io": {
              "auth": "YWxpY2U6aHVudGVyMg==",
              "username": "carol",
              "password": "override"
            }
          }
        }"#;
        let cfg = load_docker_config_from_str(body).unwrap();
        let cred = credential_for("ghcr.io", &cfg).unwrap();
        assert_eq!(cred.username, "carol");
        assert_eq!(cred.password, "override");
    }

    #[test]
    fn malformed_json_is_error() {
        let err = load_docker_config_from_str("{ not json").unwrap_err();
        assert!(matches!(err, OciError::Auth(_)));
    }

    #[test]
    fn empty_auths_is_fine() {
        let cfg = load_docker_config_from_str("{}").unwrap();
        assert!(cfg.auths.is_empty());
        assert!(credential_for("ghcr.io", &cfg).is_none());
    }

    #[test]
    fn base64_decode_roundtrips() {
        // "hello:world" → "aGVsbG86d29ybGQ="
        let out = base64_decode_standard("aGVsbG86d29ybGQ=").unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "hello:world");
    }

    // --- additional auth tests ---

    #[test]
    fn resolves_credential_via_https_prefixed_key() {
        // Some docker configs store keys as "https://ghcr.io".
        let body = r#"{
          "auths": {
            "https://ghcr.io": {
              "username": "dave",
              "password": "pass123"
            }
          }
        }"#;
        let cfg = load_docker_config_from_str(body).unwrap();
        let cred = credential_for("ghcr.io", &cfg).expect("should resolve via https prefix");
        assert_eq!(cred.username, "dave");
    }

    #[test]
    fn resolves_credential_via_https_slash_prefixed_key() {
        // Keys stored as "https://ghcr.io/".
        let body = r#"{
          "auths": {
            "https://ghcr.io/": {
              "username": "eve",
              "password": "secret"
            }
          }
        }"#;
        let cfg = load_docker_config_from_str(body).unwrap();
        let cred = credential_for("ghcr.io", &cfg).expect("should resolve via https/ prefix");
        assert_eq!(cred.username, "eve");
    }

    #[test]
    fn resolves_index_docker_io_alias_for_index_docker_io_key() {
        // When the key is already "index.docker.io" it should be found via verbatim match.
        let body = r#"{
          "auths": {
            "index.docker.io": {
              "username": "frank",
              "password": "pw"
            }
          }
        }"#;
        let cfg = load_docker_config_from_str(body).unwrap();
        let cred = credential_for("index.docker.io", &cfg).expect("verbatim match");
        assert_eq!(cred.username, "frank");
    }

    #[test]
    fn entry_with_only_username_no_password_returns_none() {
        // An entry with username but no password cannot produce a credential.
        let body = r#"{
          "auths": {
            "example.com": {
              "username": "alice"
            }
          }
        }"#;
        let cfg = load_docker_config_from_str(body).unwrap();
        assert!(credential_for("example.com", &cfg).is_none());
    }

    #[test]
    fn entry_with_only_password_no_username_returns_none() {
        // An entry with password but no username cannot produce a credential.
        let body = r#"{
          "auths": {
            "example.com": {
              "password": "hunter2"
            }
          }
        }"#;
        let cfg = load_docker_config_from_str(body).unwrap();
        assert!(credential_for("example.com", &cfg).is_none());
    }

    #[test]
    fn base64_decode_rejects_invalid_character() {
        // `!` is not in the RFC 4648 alphabet.
        assert!(base64_decode_standard("YWxp!2U=").is_none());
    }

    #[test]
    fn base64_decode_rejects_wrong_padding_length() {
        // Input not a multiple of 4 characters is invalid.
        assert!(base64_decode_standard("abc").is_none());
    }

    #[test]
    fn auth_blob_without_colon_returns_none_credential() {
        // A base64 blob that decodes to a string with no `:` cannot be split
        // into user:password, so no credential should be returned.
        // base64("nocolon") = "bm9jb2xvbg=="
        let body = r#"{
          "auths": {
            "example.com": {
              "auth": "bm9jb2xvbg=="
            }
          }
        }"#;
        let cfg = load_docker_config_from_str(body).unwrap();
        assert!(credential_for("example.com", &cfg).is_none());
    }

    #[test]
    fn registry_credential_eq() {
        let a = RegistryCredential {
            username: "u".into(),
            password: "p".into(),
            identitytoken: None,
        };
        let b = a.clone();
        assert_eq!(a, b);
    }
}
