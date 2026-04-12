//! Environment variable overrides for SERA config.
//!
//! Any environment variable prefixed with `SERA_` is treated as a config
//! override. The key is derived by stripping the prefix and lower-casing,
//! e.g. `SERA_LOG_LEVEL=debug` → key `log_level`, value `"debug"`.

use std::env;
use serde_json::{Map, Value};

use crate::layer_merge::{LayeredManifestSet, ManifestLayer};

/// Scan the process environment for `SERA_*` variables and return them as
/// a `ManifestLayer` named `"env"`.
///
/// Variable names are stripped of the `SERA_` prefix and lower-cased to
/// produce config keys. Values are always JSON strings.
pub fn scan_env_overrides() -> ManifestLayer {
    let mut map = Map::new();
    for (key, val) in env::vars() {
        if let Some(suffix) = key.strip_prefix("SERA_") {
            let config_key = suffix.to_lowercase();
            map.insert(config_key, Value::String(val));
        }
    }
    ManifestLayer::from_map("env", map)
}

/// Append environment overrides as the top (highest-precedence) layer of `base`.
pub fn apply_env_overrides(base: &mut LayeredManifestSet) {
    base.push(scan_env_overrides());
}
