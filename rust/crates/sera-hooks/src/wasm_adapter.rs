//! WASM hook adapter for sera-hooks.
//!
//! Provides [`WasmHookAdapter`] that wraps WASM modules (loaded via wasmtime)
//! and presents them as [`Hook`](super::hook_trait::Hook) implementations.
//!
//! # Overview
//!
//! The adapter:
//! - Loads WASM modules from bytes or WASM files
//! - Exposes a hook interface compatible with the native hook system
//! - Handles serialization of hook context to/from WASM memory
//!
//! # Example
//!
//! ```rust,ignore
//! use sera_hooks::wasm_adapter::WasmHookAdapter;
//!
//! let adapter = WasmHookAdapter::from_bytes(wasm_bytes).await?;
//! let result = adapter.execute(&ctx).await?;
//! ```

#[cfg(feature = "wasm")]
use {
    std::sync::Arc,
    wasmtime::{Engine, Instance, Linker, Module, Store},
    wasmtime_wasi::WasiCtxBuilder,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::HookError;
use crate::hook_trait::{Hook, Hook as HookTrait};
use sera_types::hook::{HookContext, HookMetadata, HookResult};

// ── Error types ────────────────────────────────────────────────────────────────

/// Errors that can occur during WASM hook execution.
#[derive(Debug, Error)]
pub enum WasmError {
    #[error("WASM module error: {0}")]
    Module(String),

    #[error("WASM execution error: {0}")]
    Execution(String),

    #[error("WASM linking error: {0}")]
    Linking(String),

    #[error("WASM not available (compile with --features wasm)")]
    NotAvailable,

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Result type for WASM operations.
pub type WasmResult<T> = Result<T, WasmError>;

// ── Hook metadata ─────────────────────────────────────────────────────────────

/// Metadata for a WASM hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmHookMetadata {
    /// Name of the WASM module.
    pub module_name: String,
    /// Version string from the module.
    pub version: Option<String>,
    /// Custom metadata from the module.
    pub custom: serde_json::Value,
}

impl From<HookMetadata> for WasmHookMetadata {
    fn from(m: HookMetadata) -> Self {
        Self {
            module_name: m.name,
            version: m.version,
            custom: m.metadata,
        }
    }
}

// ── WASM Hook Adapter ─────────────────────────────────────────────────────────

/// A hook adapter that wraps a WASM module.
///
/// This adapter loads a WASM module and exposes it as a standard [`Hook`]
/// that can be registered in the hook registry.
///
/// The WASM module must export a function named `hook_execute` that takes
/// a JSON string of [`HookContext`] and returns a JSON string of [`HookResult`].
#[derive(Clone)]
pub struct WasmHookAdapter {
    #[cfg(feature = "wasm")]
    engine: Arc<Engine>,
    #[cfg(feature = "wasm")]
    module: Arc<Module>,
    #[cfg(feature = "wasm")]
    instance: Arc<Instance>,
    #[cfg(feature = "wasm")]
    wasi_ctx: wasmtime_wasi::WasiCtx,
    metadata: WasmHookMetadata,
}

impl std::fmt::Debug for WasmHookAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmHookAdapter")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl WasmHookAdapter {
    /// Create a new WASM hook adapter from WASM bytes.
    #[cfg(feature = "wasm")]
    pub async fn from_bytes(
        bytes: impl AsRef<[u8]>,
        metadata: WasmHookMetadata,
    ) -> WasmResult<Self> {
        let engine = Engine::default();
        let module = Module::from_binary(&engine, bytes.as_ref())
            .map_err(|e| WasmError::Module(e.to_string()))?;

        // Set up WASI context
        let wasi_ctx = WasiCtxBuilder::new().build();

        // Create a store with the WASI context
        let mut store = Store::new(&engine, wasi_ctx.clone());

        // Create linker and instantiate
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::add_to_linker(&mut linker, |s| s)
            .map_err(|e| WasmError::Linking(e.to_string()))?;

        // Instantiate the module
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| WasmError::Linking(e.to_string()))?;

        Ok(Self {
            engine: Arc::new(engine),
            module: Arc::new(module),
            instance: Arc::new(instance),
            wasi_ctx,
            metadata,
        })
    }

    /// Create a new WASM hook adapter from WASM bytes (sync version).
    ///
    /// This is a convenience method that wraps the async version.
    #[cfg(feature = "wasm")]
    pub fn from_bytes_sync(
        bytes: impl AsRef<[u8]>,
        metadata: WasmHookMetadata,
    ) -> WasmResult<Self> {
        // For sync creation, we use the same async function but block on it
        // In practice, wasmtime's from_binary is sync anyway
        let engine = Engine::default();
        let module = Module::from_binary(&engine, bytes.as_ref())
            .map_err(|e| WasmError::Module(e.to_string()))?;

        let wasi_ctx = WasiCtxBuilder::new().build();

        let mut store = Store::new(&engine, wasi_ctx.clone());
        let mut linker = Linker::new(&engine);
        wasmtime_wasi::add_to_linker(&mut linker, |s| s)
            .map_err(|e| WasmError::Linking(e.to_string()))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| WasmError::Linking(e.to_string()))?;

        Ok(Self {
            engine: Arc::new(engine),
            module: Arc::new(module),
            instance: Arc::new(instance),
            wasi_ctx,
            metadata,
        })
    }

    /// Returns an error indicating WASM support is not compiled in.
    #[cfg(not(feature = "wasm"))]
    pub async fn from_bytes(
        _bytes: impl AsRef<[u8]>,
        _metadata: WasmHookMetadata,
    ) -> WasmResult<Self> {
        Err(WasmError::NotAvailable)
    }

    /// Execute the WASM hook with the given context.
    #[cfg(feature = "wasm")]
    pub async fn execute_wasm(&self, ctx: &HookContext) -> WasmResult<HookResult> {
        use wasmtime::Val;

        let ctx_json = serde_json::to_string(ctx)?;
        let ctx_bytes = ctx_json.as_bytes();

        // Allocate memory in WASM module and copy context
        let memory = self
            .instance
            .get_global(&mut wasmtime::Scope::new(), "memory")
            .and_then(|g| g.as_extern().memory().cloned());

        let memory = memory.ok_or_else(|| {
            WasmError::Execution("WASM module does not export memory".to_string())
        })?;

        // Allocate space for the context
        let alloc = self
            .instance
            .get_typed_func::<i32, i32>(&mut wasmtime::Scope::new(), "alloc")
            .ok();

        let ctx_ptr = if let Some(alloc_fn) = alloc {
            // Call alloc to get memory
            let ptr = alloc_fn.call(&mut wasmtime::Scope::new(), ctx_bytes.len() as i32)?;
            memory
                .write(&mut wasmtime::Scope::new(), ptr as usize, ctx_bytes)
                .map_err(|e| WasmError::Execution(e.to_string()))?;
            ptr as usize
        } else {
            // Fallback: assume first linear memory at offset 0
            0
        };

        // Find the hook_execute function
        let hook_execute = self
            .instance
            .get_typed_func::<(i32, i32), i32>(&mut wasmtime::Scope::new(), "hook_execute")
            .map_err(|e| WasmError::Execution(format!("hook_execute not found: {}", e)))?;

        // Call the hook
        let result_ptr = hook_execute
            .call(
                &mut wasmtime::Scope::new(),
                (ctx_ptr as i32, ctx_bytes.len() as i32),
            )
            .map_err(|e| WasmError::Execution(e.to_string()))?;

        // Read the result from WASM memory
        // Assume result is a null-terminated string at result_ptr
        let result_bytes = memory
            .read(&wasmtime::Scope::new(), result_ptr as usize, usize::MAX - result_ptr as usize)
            .map_err(|e| WasmError::Execution(e.to_string()))?;

        let result_str = std::str::from_utf8(&result_bytes)
            .map_err(|e| WasmError::Execution(e.to_string()))?
            .trim_end_matches('\0');

        let result: HookResult = serde_json::from_str(result_str)?;

        Ok(result)
    }

    /// Execute the WASM hook (stub when WASM is not available).
    #[cfg(not(feature = "wasm"))]
    pub async fn execute_wasm(&self, _ctx: &HookContext) -> WasmResult<HookResult> {
        Err(WasmError::NotAvailable.into())
    }
}

#[async_trait]
impl HookTrait for WasmHookAdapter {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: self.metadata.module_name.clone(),
            version: self.metadata.version.clone(),
            metadata: self.metadata.custom.clone(),
        }
    }

    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        // WASM hooks don't need initialization beyond loading
        Ok(())
    }

    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError> {
        self.execute_wasm(ctx)
            .await
            .map_err(|e| HookError::Execution {
                hook: self.metadata.module_name.clone(),
                reason: e.to_string(),
            })
    }
}

// ── WASM module validation ────────────────────────────────────────────────────

/// Validate that a WASM module contains the required exports for a hook.
#[cfg(feature = "wasm")]
pub fn validate_wasm_module(bytes: &[u8]) -> WasmResult<()> {
    let engine = Engine::default();
    let module = Module::from_binary(&engine, bytes)
        .map_err(|e| WasmError::Module(e.to_string()))?;

    // Check for required exports
    // - memory (optional but recommended)
    // - hook_execute function

    for export in module.exports() {
        let name = export.name();
        if name == "hook_execute" {
            return Ok(()); // Found the required export
        }
    }

    Err(WasmError::Module(
        "WASM module does not export 'hook_execute' function".to_string(),
    ))
}

/// Validate a WASM module (stub when WASM is not available).
#[cfg(not(feature = "wasm"))]
pub fn validate_wasm_module(_bytes: &[u8]) -> WasmResult<()> {
    Err(WasmError::NotAvailable)
}

#[cfg(test)]
mod tests {
    #[test]
    fn wasm_not_available_without_feature() {
        // When compiled without wasm feature, operations should fail gracefully
        // This test validates the feature flag behavior
        #[cfg(not(feature = "wasm"))]
        {
            let result = crate::wasm_adapter::validate_wasm_module(&[]);
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                crate::wasm_adapter::WasmError::NotAvailable
            ));
        }
    }
}
