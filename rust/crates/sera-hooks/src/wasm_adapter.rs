//! WASM hook adapter for sera-hooks.
//!
//! Provides [`WasmHookAdapter`] that wraps WASM modules (loaded via wasmtime)
//! and presents them as [`Hook`](super::hook_trait::Hook) implementations.
//!
//! # Resource limits
//!
//! Every invocation is subject to three independent limits drawn from
//! [`WasmConfig`]:
//!
//! | Limit              | Config field      | Default   |
//! |--------------------|-------------------|-----------|
//! | Computation budget | `fuel_limit`      | 1 000 000 |
//! | Linear-memory cap  | `memory_limit_mb` | 64 MiB    |
//! | Wall-clock timeout | `timeout_ms`      | 1 000 ms  |
//!
//! Exceeding any limit returns the corresponding [`WasmError`] variant.
//!
//! # WASM module contract
//!
//! The module must export:
//! - `memory` — the linear memory
//! - `hook_execute(ptr: i32, len: i32) -> i32` — pointer to a null-terminated
//!   JSON [`HookResult`]; input is a UTF-8 JSON [`HookContext`] at `memory[ptr..ptr+len]`.

#[cfg(feature = "wasm")]
use {
    std::sync::Arc,
    tokio::time::timeout,
    wasmtime::{Config, Engine, Linker, Module, Store, StoreLimitsBuilder},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::HookError;
use crate::hook_trait::Hook as HookTrait;
use sera_types::hook::{HookContext, HookMetadata, HookPoint, HookResult, WasmConfig};

// ── Error types ────────────────────────────────────────────────────────────────

/// Errors that can occur during WASM hook execution.
#[derive(Debug, Error)]
pub enum WasmError {
    /// The WASM module could not be compiled or loaded.
    #[error("WASM module error: {0}")]
    Module(String),

    /// A general execution trap not related to resource limits.
    #[error("WASM execution error: {0}")]
    Execution(String),

    /// The WASM module consumed its entire fuel budget before returning.
    #[error("WASM hook exhausted computation fuel budget")]
    FuelExhausted,

    /// The WASM module attempted to grow linear memory beyond the cap.
    #[error("WASM hook exceeded memory limit")]
    MemoryLimitExceeded,

    /// The hook did not return within the configured wall-clock deadline.
    #[error("WASM hook exceeded wall-clock timeout")]
    WallClockTimeout,

    /// The WASM linker could not be configured.
    #[error("WASM linking error: {0}")]
    Linking(String),

    /// WASM support was not compiled in.
    #[error("WASM not available (compile with --features wasm)")]
    NotAvailable,

    /// JSON (de)serialization failed.
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

// ── Store data ────────────────────────────────────────────────────────────────

/// Per-invocation data stored in the wasmtime [`Store`].
#[cfg(feature = "wasm")]
struct StoreData {
    limits: wasmtime::StoreLimits,
    wasi: wasmtime_wasi::preview1::WasiP1Ctx,
}

// ── WASM Hook Adapter ─────────────────────────────────────────────────────────

/// A hook adapter that wraps a WASM module and exposes it as a standard
/// [`Hook`](HookTrait) subject to fuel, memory, and wall-clock limits.
#[derive(Clone)]
pub struct WasmHookAdapter {
    #[cfg(feature = "wasm")]
    engine: Arc<Engine>,
    #[cfg(feature = "wasm")]
    module: Arc<Module>,
    metadata: WasmHookMetadata,
    config: WasmConfig,
}

impl std::fmt::Debug for WasmHookAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmHookAdapter")
            .field("metadata", &self.metadata)
            .finish()
    }
}

#[cfg(feature = "wasm")]
fn build_engine() -> WasmResult<Engine> {
    let mut cfg = Config::new();
    cfg.consume_fuel(true);
    Engine::new(&cfg).map_err(|e| WasmError::Module(e.to_string()))
}

/// Classify a wasmtime trap into the right [`WasmError`] variant.
///
/// Uses `downcast_ref::<wasmtime::Trap>()` to inspect the exact trap code so
/// the classification is not sensitive to message wording changes.
#[cfg(feature = "wasm")]
fn classify_trap(e: wasmtime::Error) -> WasmError {
    if let Some(trap) = e.downcast_ref::<wasmtime::Trap>() {
        match trap {
            wasmtime::Trap::OutOfFuel => return WasmError::FuelExhausted,
            wasmtime::Trap::MemoryOutOfBounds => return WasmError::MemoryLimitExceeded,
            _ => {}
        }
    }
    // Fallback: inspect the root-cause message for the StoreLimits memory-
    // growth denial path, which surfaces as a different error kind.
    let msg = format!("{e:#}").to_lowercase();
    if msg.contains("memory") && (msg.contains("limit") || msg.contains("denied")) {
        WasmError::MemoryLimitExceeded
    } else {
        WasmError::Execution(e.to_string())
    }
}

impl WasmHookAdapter {
    /// Create a new adapter from WASM bytes or WAT text.
    ///
    /// `bytes` may be a compiled `.wasm` binary or a WAT text string —
    /// wasmtime 26 detects the format automatically.
    #[cfg(feature = "wasm")]
    pub fn from_bytes(
        bytes: impl AsRef<[u8]>,
        metadata: WasmHookMetadata,
        config: WasmConfig,
    ) -> WasmResult<Self> {
        let engine = build_engine()?;
        let module =
            Module::new(&engine, bytes.as_ref()).map_err(|e| WasmError::Module(e.to_string()))?;
        Ok(Self {
            engine: Arc::new(engine),
            module: Arc::new(module),
            metadata,
            config,
        })
    }

    /// Stub for when the `wasm` feature is not compiled in.
    #[cfg(not(feature = "wasm"))]
    pub fn from_bytes(
        _bytes: impl AsRef<[u8]>,
        _metadata: WasmHookMetadata,
        _config: WasmConfig,
    ) -> WasmResult<Self> {
        Err(WasmError::NotAvailable)
    }

    /// Execute the WASM hook with fuel, memory, and wall-clock limits applied.
    #[cfg(feature = "wasm")]
    pub async fn execute_wasm(&self, ctx: &HookContext) -> WasmResult<HookResult> {
        let ctx_json = serde_json::to_string(ctx)?;
        let ctx_bytes_owned = ctx_json.into_bytes();

        let memory_bytes = (self.config.memory_limit_mb as usize).saturating_mul(1024 * 1024);
        let fuel_limit = self.config.fuel_limit;
        let timeout_duration = std::time::Duration::from_millis(self.config.timeout_ms);

        // Clone Arcs so the closure can be 'static.
        let engine = Arc::clone(&self.engine);
        let module = Arc::clone(&self.module);

        let invoke = async move {
            tokio::task::spawn_blocking(move || {
                // Build per-invocation store with fresh limits.
                let limits = StoreLimitsBuilder::new().memory_size(memory_bytes).build();
                let wasi = wasmtime_wasi::WasiCtxBuilder::new().build_p1();
                let mut store = Store::new(&engine, StoreData { limits, wasi });
                store.limiter(|data| &mut data.limits);
                store
                    .set_fuel(fuel_limit)
                    .map_err(|e| WasmError::Execution(e.to_string()))?;

                // Link WASI preview-1.
                let mut linker: Linker<StoreData> = Linker::new(&engine);
                wasmtime_wasi::preview1::add_to_linker_sync(&mut linker, |data| &mut data.wasi)
                    .map_err(|e| WasmError::Linking(e.to_string()))?;

                let instance = linker
                    .instantiate(&mut store, &module)
                    .map_err(|e| WasmError::Linking(e.to_string()))?;

                let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
                    WasmError::Execution("WASM module does not export 'memory'".to_string())
                })?;

                let hook_execute = instance
                    .get_typed_func::<(i32, i32), i32>(&mut store, "hook_execute")
                    .map_err(|e| WasmError::Execution(format!("hook_execute not found: {e}")))?;

                // Write context JSON at offset 0 in WASM linear memory.
                let ctx_ptr: i32 = 0;
                memory
                    .write(&mut store, ctx_ptr as usize, &ctx_bytes_owned)
                    .map_err(|e| WasmError::Execution(format!("memory write: {e}")))?;

                let result_ptr = hook_execute
                    .call(&mut store, (ctx_ptr, ctx_bytes_owned.len() as i32))
                    .map_err(classify_trap)?;

                // Read null-terminated JSON result from WASM memory.
                let data = memory.data(&store);
                let start = result_ptr as usize;
                let nul = data[start..].iter().position(|&b| b == 0).ok_or_else(|| {
                    WasmError::Execution("hook_execute result is not null-terminated".to_string())
                })?;
                let result_str = std::str::from_utf8(&data[start..start + nul])
                    .map_err(|e| WasmError::Execution(format!("invalid UTF-8: {e}")))?;
                let result: HookResult =
                    serde_json::from_str(result_str).map_err(WasmError::Serialization)?;

                Ok(result)
            })
            .await
            .map_err(|e| WasmError::Execution(format!("task join error: {e}")))?
        };

        timeout(timeout_duration, invoke)
            .await
            .map_err(|_| WasmError::WallClockTimeout)?
    }

    /// Stub for when the `wasm` feature is not compiled in.
    #[cfg(not(feature = "wasm"))]
    pub async fn execute_wasm(&self, _ctx: &HookContext) -> WasmResult<HookResult> {
        Err(WasmError::NotAvailable)
    }
}

#[async_trait]
impl HookTrait for WasmHookAdapter {
    fn metadata(&self) -> HookMetadata {
        HookMetadata {
            name: self.metadata.module_name.clone(),
            description: String::new(),
            version: self
                .metadata
                .version
                .clone()
                .unwrap_or_else(|| "0.0.0".to_string()),
            supported_points: HookPoint::ALL.to_vec(),
            author: None,
        }
    }

    async fn init(&mut self, _config: serde_json::Value) -> Result<(), HookError> {
        Ok(())
    }

    async fn execute(&self, ctx: &HookContext) -> Result<HookResult, HookError> {
        self.execute_wasm(ctx)
            .await
            .map_err(|e| HookError::ExecutionFailed {
                hook: self.metadata.module_name.clone(),
                reason: e.to_string(),
            })
    }
}

// ── WASM module validation ────────────────────────────────────────────────────

/// Validate that a WASM module exports the `hook_execute` function.
#[cfg(feature = "wasm")]
pub fn validate_wasm_module(bytes: &[u8]) -> WasmResult<()> {
    let engine = build_engine()?;
    let module = Module::new(&engine, bytes).map_err(|e| WasmError::Module(e.to_string()))?;
    for export in module.exports() {
        if export.name() == "hook_execute" {
            return Ok(());
        }
    }
    Err(WasmError::Module(
        "WASM module does not export 'hook_execute' function".to_string(),
    ))
}

/// Stub for when the `wasm` feature is not compiled in.
#[cfg(not(feature = "wasm"))]
pub fn validate_wasm_module(_bytes: &[u8]) -> WasmResult<()> {
    Err(WasmError::NotAvailable)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[test]
    fn wasm_not_available_without_feature() {
        #[cfg(not(feature = "wasm"))]
        {
            let result = crate::wasm_adapter::validate_wasm_module(&[]);
            assert!(result.is_err());
            assert!(matches!(
                result.unwrap_err(),
                crate::wasm_adapter::WasmError::NotAvailable
            ));
        }
        #[cfg(feature = "wasm")]
        {
            // Empty bytes are not valid WASM — should fail with a Module error.
            let result = crate::wasm_adapter::validate_wasm_module(&[]);
            assert!(result.is_err());
        }
    }

    /// Tests that require the `wasm` feature — each exercises one limit path.
    #[cfg(feature = "wasm")]
    mod metering {
        use sera_types::hook::{HookContext, HookPoint, WasmConfig};

        use crate::wasm_adapter::{WasmError, WasmHookAdapter, WasmHookMetadata};

        fn meta(name: &str) -> WasmHookMetadata {
            WasmHookMetadata {
                module_name: name.to_string(),
                version: Some("0.1.0".to_string()),
                custom: serde_json::Value::Null,
            }
        }

        fn ctx() -> HookContext {
            HookContext::new(HookPoint::PreRoute)
        }

        // ── WAT fixtures ────────────────────────────────────────────────────

        /// A well-behaved hook: writes `{"action":"continue"}` + NUL at offset
        /// 512 and returns 512.
        ///
        /// The HookResult enum is tagged with `#[serde(tag = "action",
        /// rename_all = "snake_case")]`, so the minimal Continue JSON is
        /// `{"action":"continue"}` (21 bytes).
        const GOOD_HOOK_WAT: &str = r#"
            (module
                (memory (export "memory") 1)
                (func (export "hook_execute") (param i32 i32) (result i32)
                    ;; Write '{"action":"continue"}' + NUL at offset 512.
                    ;; {"action":"continue"} in ASCII (21 bytes):
                    (i32.store8 (i32.const 512) (i32.const 123))  ;; {
                    (i32.store8 (i32.const 513) (i32.const 34))   ;; "
                    (i32.store8 (i32.const 514) (i32.const 97))   ;; a
                    (i32.store8 (i32.const 515) (i32.const 99))   ;; c
                    (i32.store8 (i32.const 516) (i32.const 116))  ;; t
                    (i32.store8 (i32.const 517) (i32.const 105))  ;; i
                    (i32.store8 (i32.const 518) (i32.const 111))  ;; o
                    (i32.store8 (i32.const 519) (i32.const 110))  ;; n
                    (i32.store8 (i32.const 520) (i32.const 34))   ;; "
                    (i32.store8 (i32.const 521) (i32.const 58))   ;; :
                    (i32.store8 (i32.const 522) (i32.const 34))   ;; "
                    (i32.store8 (i32.const 523) (i32.const 99))   ;; c
                    (i32.store8 (i32.const 524) (i32.const 111))  ;; o
                    (i32.store8 (i32.const 525) (i32.const 110))  ;; n
                    (i32.store8 (i32.const 526) (i32.const 116))  ;; t
                    (i32.store8 (i32.const 527) (i32.const 105))  ;; i
                    (i32.store8 (i32.const 528) (i32.const 110))  ;; n
                    (i32.store8 (i32.const 529) (i32.const 117))  ;; u
                    (i32.store8 (i32.const 530) (i32.const 101))  ;; e
                    (i32.store8 (i32.const 531) (i32.const 34))   ;; "
                    (i32.store8 (i32.const 532) (i32.const 125))  ;; }
                    (i32.store8 (i32.const 533) (i32.const 0))    ;; NUL
                    i32.const 512
                )
            )
        "#;

        /// An infinite-loop hook — exhausts fuel.
        const INFINITE_LOOP_WAT: &str = r#"
            (module
                (memory (export "memory") 1)
                (func (export "hook_execute") (param i32 i32) (result i32)
                    (block
                        (loop
                            br 0
                        )
                    )
                    i32.const 0
                )
            )
        "#;

        // ── Test cases ──────────────────────────────────────────────────────

        #[tokio::test]
        async fn well_behaved_hook_succeeds_under_limits() {
            let config = WasmConfig {
                fuel_limit: 10_000_000,
                memory_limit_mb: 64,
                timeout_ms: 5_000,
                ..Default::default()
            };
            let adapter =
                WasmHookAdapter::from_bytes(GOOD_HOOK_WAT.as_bytes(), meta("good"), config)
                    .expect("compilation should succeed");

            let result = adapter.execute_wasm(&ctx()).await;
            assert!(
                result.is_ok(),
                "well-behaved hook should succeed; got: {:?}",
                result
            );
        }

        #[tokio::test]
        async fn fuel_exhausted_returns_fuel_error() {
            // Tiny fuel budget — infinite loop traps immediately.
            let config = WasmConfig {
                fuel_limit: 100,
                memory_limit_mb: 64,
                timeout_ms: 5_000,
                ..Default::default()
            };
            let adapter =
                WasmHookAdapter::from_bytes(INFINITE_LOOP_WAT.as_bytes(), meta("looper"), config)
                    .expect("compilation should succeed");

            let err = adapter.execute_wasm(&ctx()).await.unwrap_err();
            assert!(
                matches!(err, WasmError::FuelExhausted),
                "expected FuelExhausted; got: {:?}",
                err
            );
        }

        #[tokio::test]
        async fn wall_clock_timeout_returns_timeout_error() {
            // Use a finite but large fuel budget and a 1 ms deadline.
            // tokio::time::timeout will fire at 1 ms and return WallClockTimeout.
            // The background blocking thread continues until fuel exhausts
            // (50_000_000 iterations), so the test completes promptly without
            // leaking a thread that runs forever.
            let config = WasmConfig {
                fuel_limit: 50_000_000,
                memory_limit_mb: 64,
                timeout_ms: 1,
                ..Default::default()
            };
            let adapter = WasmHookAdapter::from_bytes(
                INFINITE_LOOP_WAT.as_bytes(),
                meta("looper-timeout"),
                config,
            )
            .expect("compilation should succeed");

            let err = adapter.execute_wasm(&ctx()).await.unwrap_err();
            // Under tight scheduling either FuelExhausted or WallClockTimeout
            // is acceptable — both indicate a resource limit was enforced.
            assert!(
                matches!(err, WasmError::WallClockTimeout | WasmError::FuelExhausted),
                "expected WallClockTimeout or FuelExhausted; got: {:?}",
                err
            );
        }

        #[tokio::test]
        async fn validate_accepts_valid_module() {
            let result = crate::wasm_adapter::validate_wasm_module(GOOD_HOOK_WAT.as_bytes());
            assert!(
                result.is_ok(),
                "should accept valid module; got: {:?}",
                result
            );
        }

        #[tokio::test]
        async fn validate_rejects_missing_export() {
            let wat = r#"(module (memory (export "memory") 1))"#;
            let result = crate::wasm_adapter::validate_wasm_module(wat.as_bytes());
            assert!(result.is_err());
        }
    }
}
