//! OCI media types used by SERA plugin artifacts.
//!
//! See `docs/plan/PLUGIN-MCP-ECOSYSTEM.md` §3.2 for the artifact layout. Each
//! layer in a SERA plugin OCI artifact carries one of these media types.

/// YAML plugin manifest layer — the `plugin.yaml` (`api_version: sera/v1`)
/// bundled into the OCI artifact. This is the only layer phase S pulls.
pub const PLUGIN_MANIFEST_V1_YAML: &str = "application/vnd.sera.plugin.manifest.v1+yaml";

/// Plugin binary layer — compiled plugin executable or container image
/// reference. Pulled in phase M.
pub const PLUGIN_BINARY: &str = "application/vnd.sera.plugin.binary";

/// Plugin proto schema bundle (optional) — proto files describing the
/// plugin's gRPC contract. Pulled alongside the binary in phase M.
pub const PLUGIN_PROTO: &str = "application/vnd.sera.plugin.proto";
