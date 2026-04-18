//! LayeredManifestSet — ordered layer stack with last-wins shallow merge.

use serde_json::{Map, Value};

/// A named layer of key/value manifest entries.
#[derive(Debug, Clone)]
pub struct ManifestLayer {
    pub name: String,
    pub values: Map<String, Value>,
}

impl ManifestLayer {
    /// Create a new empty layer with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: Map::new(),
        }
    }

    /// Create a layer from an existing map.
    pub fn from_map(name: impl Into<String>, values: Map<String, Value>) -> Self {
        Self {
            name: name.into(),
            values,
        }
    }
}

/// An ordered stack of `ManifestLayer`s.
///
/// Later (higher-index) layers win on key conflicts during `merge()`.
#[derive(Debug, Clone, Default)]
pub struct LayeredManifestSet {
    layers: Vec<ManifestLayer>,
}

impl LayeredManifestSet {
    /// Create an empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a layer onto the top of the stack (highest precedence so far).
    pub fn push(&mut self, layer: ManifestLayer) {
        self.layers.push(layer);
    }

    /// Produce a flat merged map: later layers overwrite earlier ones (shallow).
    pub fn merge(&self) -> Map<String, Value> {
        let mut result = Map::new();
        for layer in &self.layers {
            for (k, v) in &layer.values {
                result.insert(k.clone(), v.clone());
            }
        }
        result
    }

    /// Names of all layers in push order (lowest to highest precedence).
    pub fn layer_names(&self) -> Vec<&str> {
        self.layers.iter().map(|l| l.name.as_str()).collect()
    }

    /// Number of layers in the set.
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// True if no layers have been pushed.
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}
