//! OCI manifest puller — phase S scope.
//!
//! [`OciPuller`] wraps `oci_distribution::Client` and resolves an
//! [`OciReference`] to the *YAML plugin manifest* layer. Binary-layer pulls
//! and push operations land in phase M.

use oci_distribution::{
    Client, Reference,
    client::ClientConfig,
    manifest::{OciDescriptor, OciManifest},
    secrets::RegistryAuth,
};
use tracing::debug;

use crate::auth::RegistryCredential;
use crate::error::OciError;
use crate::media_types::PLUGIN_MANIFEST_V1_YAML;
use crate::reference::OciReference;

/// Raw bytes of a plugin manifest YAML layer pulled from an OCI registry.
///
/// Callers are expected to hand this to a YAML parser (e.g.
/// `sera_plugins::PluginManifestV1::from_yaml`). Keeping the crate a
/// zero-parse transport avoids coupling `sera-oci` to the plugin type
/// surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifestBytes(pub Vec<u8>);

impl PluginManifestBytes {
    /// Borrow as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Consume into the raw `Vec<u8>`.
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// Client for pulling SERA plugin artifacts from an OCI registry.
pub struct OciPuller {
    client: Client,
    creds: Option<RegistryCredential>,
}

impl OciPuller {
    /// Create a puller with the default `oci_distribution` client config.
    ///
    /// TLS settings follow the workspace convention (`rustls-tls`, no
    /// system OpenSSL dependency).
    pub fn new() -> Self {
        Self {
            client: Client::new(ClientConfig::default()),
            creds: None,
        }
    }

    /// Builder: attach credentials for authenticated pulls.
    pub fn with_credentials(mut self, creds: RegistryCredential) -> Self {
        self.creds = Some(creds);
        self
    }

    /// Pull the plugin manifest YAML layer for `reference`.
    ///
    /// Flow:
    ///   1. Fetch the OCI image manifest at `reference`.
    ///   2. Find the layer with media type
    ///      `application/vnd.sera.plugin.manifest.v1+yaml`.
    ///   3. Pull that layer's blob.
    ///   4. Return the raw bytes.
    ///
    /// # Errors
    ///
    /// - [`OciError::InvalidReference`] — `reference` does not convert to a
    ///   valid `oci_distribution::Reference`.
    /// - [`OciError::NotFound`] — the image or layer does not exist.
    /// - [`OciError::Unauthorized`] — the registry rejected authentication.
    /// - [`OciError::Transport`] — lower-level network failure.
    /// - [`OciError::MissingManifestLayer`] — the image manifest exists but
    ///   contains no layer with the SERA plugin manifest media type.
    pub async fn pull_manifest(
        &self,
        reference: &OciReference,
    ) -> Result<PluginManifestBytes, OciError> {
        let oci_ref = to_oci_reference(reference);
        let auth = self.registry_auth();

        debug!(
            registry = %reference.registry,
            repository = %reference.repository,
            "pulling OCI manifest"
        );

        let (manifest, _digest) = self
            .client
            .pull_manifest(&oci_ref, &auth)
            .await
            .map_err(classify_oci_error)?;

        let layer = find_plugin_manifest_layer(&manifest)?;

        let mut blob: Vec<u8> = Vec::new();
        self.client
            .pull_blob(&oci_ref, layer, &mut blob)
            .await
            .map_err(classify_oci_error)?;

        Ok(PluginManifestBytes(blob))
    }

    fn registry_auth(&self) -> RegistryAuth {
        match &self.creds {
            Some(cred) => {
                // When an identity token is present, many registries accept
                // basic auth with username `<token>` and the token as the
                // password — honour the docker convention.
                if let Some(token) = &cred.identitytoken {
                    RegistryAuth::Basic("<token>".into(), token.clone())
                } else {
                    RegistryAuth::Basic(cred.username.clone(), cred.password.clone())
                }
            }
            None => RegistryAuth::Anonymous,
        }
    }
}

impl Default for OciPuller {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert our `OciReference` into `oci_distribution::Reference`.
fn to_oci_reference(r: &OciReference) -> Reference {
    match (&r.tag, &r.digest) {
        (_, Some(digest)) => {
            Reference::with_digest(r.registry.clone(), r.repository.clone(), digest.clone())
        }
        (Some(tag), None) => {
            Reference::with_tag(r.registry.clone(), r.repository.clone(), tag.clone())
        }
        (None, None) => {
            // Bare references resolve to the registry's default tag ("latest"
            // per OCI convention).
            Reference::with_tag(r.registry.clone(), r.repository.clone(), "latest".into())
        }
    }
}

/// Locate the plugin manifest layer inside an OCI image manifest.
fn find_plugin_manifest_layer(manifest: &OciManifest) -> Result<&OciDescriptor, OciError> {
    let layers: &[OciDescriptor] = match manifest {
        OciManifest::Image(img) => &img.layers,
        // An index manifest means the registry returned a multi-platform
        // index. Phase S doesn't fan out across platforms — we refuse and
        // let the caller request a platform-specific digest.
        OciManifest::ImageIndex(_) => {
            return Err(OciError::MissingManifestLayer);
        }
    };

    layers
        .iter()
        .find(|l| l.media_type == PLUGIN_MANIFEST_V1_YAML)
        .ok_or(OciError::MissingManifestLayer)
}

/// Map an `oci_distribution` error to our `OciError`, preserving enough
/// signal for callers to decide on retry / reauth / surface-to-user.
fn classify_oci_error(err: oci_distribution::errors::OciDistributionError) -> OciError {
    use oci_distribution::errors::OciDistributionError as E;
    match err {
        E::AuthenticationFailure(msg) => OciError::Unauthorized(msg),
        E::ImageManifestNotFoundError(msg) => OciError::NotFound(msg),
        E::IoError(e) => OciError::Io(e),
        other => {
            let msg = other.to_string();
            // Heuristic fallback — the crate's HTTP layer surfaces some
            // failures only via RegistryError / ServerError with an
            // embedded status code, so we look at the rendered message.
            let lower = msg.to_lowercase();
            if lower.contains("not found") || lower.contains("404") {
                OciError::NotFound(msg)
            } else if lower.contains("unauthor")
                || lower.contains("not authorized")
                || lower.contains("denied")
                || lower.contains("401")
                || lower.contains("403")
            {
                OciError::Unauthorized(msg)
            } else {
                OciError::Transport(msg)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oci_distribution::manifest::OciImageManifest;

    fn descriptor(media_type: &str) -> OciDescriptor {
        OciDescriptor {
            media_type: media_type.to_string(),
            digest: "sha256:cafebabe".to_string(),
            size: 42,
            urls: None,
            annotations: None,
        }
    }

    fn image_manifest_with_layer(media_type: &str) -> OciManifest {
        OciManifest::Image(OciImageManifest {
            schema_version: 2,
            media_type: Some("application/vnd.oci.image.manifest.v1+json".into()),
            config: descriptor("application/vnd.oci.image.config.v1+json"),
            layers: vec![descriptor(media_type)],
            artifact_type: None,
            annotations: None,
        })
    }

    #[test]
    fn finds_plugin_manifest_layer() {
        let m = image_manifest_with_layer(PLUGIN_MANIFEST_V1_YAML);
        let layer = find_plugin_manifest_layer(&m).unwrap();
        assert_eq!(layer.media_type, PLUGIN_MANIFEST_V1_YAML);
    }

    #[test]
    fn missing_manifest_layer_errors() {
        let m = image_manifest_with_layer("application/vnd.oci.image.layer.v1.tar");
        let err = find_plugin_manifest_layer(&m).unwrap_err();
        assert!(matches!(err, OciError::MissingManifestLayer));
    }

    #[test]
    fn image_index_is_missing_manifest_layer() {
        let m = OciManifest::ImageIndex(oci_distribution::manifest::OciImageIndex {
            schema_version: 2,
            media_type: Some("application/vnd.oci.image.index.v1+json".into()),
            manifests: vec![],
            annotations: None,
        });
        let err = find_plugin_manifest_layer(&m).unwrap_err();
        assert!(matches!(err, OciError::MissingManifestLayer));
    }

    #[test]
    fn reference_conversion_tag() {
        let r = OciReference::parse("ghcr.io/org/app:1.0").unwrap();
        let oci = to_oci_reference(&r);
        assert_eq!(oci.registry(), "ghcr.io");
        assert_eq!(oci.repository(), "org/app");
        assert_eq!(oci.tag(), Some("1.0"));
        assert_eq!(oci.digest(), None);
    }

    #[test]
    fn reference_conversion_digest() {
        let r = OciReference::parse("ghcr.io/org/app@sha256:deadbeef").unwrap();
        let oci = to_oci_reference(&r);
        assert_eq!(oci.digest(), Some("sha256:deadbeef"));
    }

    #[test]
    fn reference_conversion_bare_uses_latest() {
        let r = OciReference::parse("ghcr.io/org/app").unwrap();
        let oci = to_oci_reference(&r);
        assert_eq!(oci.tag(), Some("latest"));
    }

    #[test]
    fn default_puller_is_anonymous() {
        let p = OciPuller::new();
        assert!(matches!(p.registry_auth(), RegistryAuth::Anonymous));
    }

    #[test]
    fn puller_with_basic_credentials() {
        let p = OciPuller::new().with_credentials(RegistryCredential {
            username: "alice".into(),
            password: "hunter2".into(),
            identitytoken: None,
        });
        match p.registry_auth() {
            RegistryAuth::Basic(u, pw) => {
                assert_eq!(u, "alice");
                assert_eq!(pw, "hunter2");
            }
            _ => panic!("expected Basic auth"),
        }
    }

    #[test]
    fn puller_prefers_identity_token() {
        let p = OciPuller::new().with_credentials(RegistryCredential {
            username: "alice".into(),
            password: "ignored".into(),
            identitytoken: Some("tok".into()),
        });
        match p.registry_auth() {
            RegistryAuth::Basic(u, pw) => {
                assert_eq!(u, "<token>");
                assert_eq!(pw, "tok");
            }
            _ => panic!("expected Basic auth with token"),
        }
    }

    #[test]
    fn plugin_manifest_bytes_accessors() {
        let b = PluginManifestBytes(vec![1, 2, 3]);
        assert_eq!(b.as_bytes(), &[1, 2, 3]);
        assert_eq!(b.clone().into_vec(), vec![1, 2, 3]);
    }

    // --- classify_oci_error tests ---

    #[test]
    fn classify_authentication_failure() {
        use oci_distribution::errors::OciDistributionError;
        let err = classify_oci_error(OciDistributionError::AuthenticationFailure(
            "token expired".into(),
        ));
        assert!(matches!(err, OciError::Unauthorized(_)));
    }

    #[test]
    fn classify_image_manifest_not_found() {
        use oci_distribution::errors::OciDistributionError;
        let err = classify_oci_error(OciDistributionError::ImageManifestNotFoundError(
            "ghcr.io/x/y:1".into(),
        ));
        assert!(matches!(err, OciError::NotFound(_)));
    }

    #[test]
    fn classify_io_error() {
        use oci_distribution::errors::OciDistributionError;
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe");
        let err = classify_oci_error(OciDistributionError::IoError(io_err));
        assert!(matches!(err, OciError::Io(_)));
    }

    #[test]
    fn classify_heuristic_not_found_message() {
        use oci_distribution::errors::OciDistributionError;
        // A generic error whose message contains "not found" → NotFound.
        let err = classify_oci_error(OciDistributionError::GenericError(Some(
            "repository not found".into(),
        )));
        assert!(matches!(err, OciError::NotFound(_)));
    }

    #[test]
    fn classify_heuristic_404_message() {
        use oci_distribution::errors::OciDistributionError;
        let err = classify_oci_error(OciDistributionError::GenericError(Some(
            "HTTP 404 from registry".into(),
        )));
        assert!(matches!(err, OciError::NotFound(_)));
    }

    #[test]
    fn classify_heuristic_unauthorized_message() {
        use oci_distribution::errors::OciDistributionError;
        let err = classify_oci_error(OciDistributionError::GenericError(Some(
            "unauthorized: access denied".into(),
        )));
        assert!(matches!(err, OciError::Unauthorized(_)));
    }

    #[test]
    fn classify_heuristic_denied_message() {
        use oci_distribution::errors::OciDistributionError;
        let err = classify_oci_error(OciDistributionError::GenericError(Some(
            "denied: requested access to resource is denied".into(),
        )));
        assert!(matches!(err, OciError::Unauthorized(_)));
    }

    #[test]
    fn classify_generic_transport_fallback() {
        use oci_distribution::errors::OciDistributionError;
        let err = classify_oci_error(OciDistributionError::GenericError(Some(
            "connection reset by peer".into(),
        )));
        assert!(matches!(err, OciError::Transport(_)));
    }

    // --- multi-layer manifest: correct layer is selected ---

    #[test]
    fn finds_correct_layer_among_multiple() {
        use oci_distribution::manifest::OciImageManifest;
        let manifest = OciManifest::Image(OciImageManifest {
            schema_version: 2,
            media_type: Some("application/vnd.oci.image.manifest.v1+json".into()),
            config: descriptor("application/vnd.oci.image.config.v1+json"),
            layers: vec![
                descriptor("application/vnd.oci.image.layer.v1.tar"),
                descriptor(PLUGIN_MANIFEST_V1_YAML),
                descriptor("application/vnd.sera.plugin.binary"),
            ],
            artifact_type: None,
            annotations: None,
        });
        let layer = find_plugin_manifest_layer(&manifest).unwrap();
        assert_eq!(layer.media_type, PLUGIN_MANIFEST_V1_YAML);
    }

    // --- OciPuller::default() ---

    #[test]
    fn default_puller_equals_new() {
        // Default::default() should behave the same as new().
        let p = OciPuller::default();
        assert!(matches!(p.registry_auth(), RegistryAuth::Anonymous));
    }

    // --- PluginManifestBytes equality ---

    #[test]
    fn plugin_manifest_bytes_eq() {
        let a = PluginManifestBytes(vec![10, 20]);
        let b = PluginManifestBytes(vec![10, 20]);
        assert_eq!(a, b);
    }
}

