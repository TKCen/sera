//! InferenceLocal URL rewriter — rewrites `inference.local` to the configured gateway endpoint.

/// Rewrites `inference.local` URLs to the configured gateway endpoint.
pub struct InferenceLocalResolver {
    pub gateway_endpoint: String,
}

impl InferenceLocalResolver {
    pub fn new(gateway_endpoint: impl Into<String>) -> Self {
        Self {
            gateway_endpoint: gateway_endpoint.into(),
        }
    }

    /// Rewrite a URL, replacing `inference.local` with the gateway endpoint.
    ///
    /// Preserves the path and query string. The gateway endpoint should not
    /// have a trailing slash.
    pub fn rewrite(&self, url: &str) -> String {
        // Match http:// or https:// followed by inference.local
        for scheme in &["https://inference.local", "http://inference.local"] {
            if let Some(rest) = url.strip_prefix(scheme) {
                return format!("{}{}", self.gateway_endpoint.trim_end_matches('/'), rest);
            }
        }
        // No scheme — bare inference.local
        if let Some(rest) = url.strip_prefix("inference.local") {
            return format!("{}{}", self.gateway_endpoint.trim_end_matches('/'), rest);
        }
        url.to_string()
    }
}
