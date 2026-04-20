//! Wasm sandbox provider — boots WASM modules under WASIp1 via wasmtime.
//!
//! # Trait mapping notes
//!
//! The [`SandboxProvider`] trait was designed around a Docker/container mental
//! model: `create` starts a long-lived sandbox, `execute` runs commands inside
//! it, and `destroy` tears it down.  WASM modules are not long-lived processes;
//! each `execute` call is a single instantiation-and-run.  The mapping used
//! here is:
//!
//! | Trait method  | WASM semantics                                           |
//! |---------------|----------------------------------------------------------|
//! | `create`      | Validate + store module bytes, return a handle           |
//! | `execute`     | Instantiate under WASIp1, run `_start`, capture stdio   |
//! | `read_file`   | `NotImplemented` (no persistent guest FS)                |
//! | `write_file`  | `NotImplemented` (no persistent guest FS)                |
//! | `destroy`     | Drop stored module bytes                                 |
//! | `status`      | `"ready"` if handle exists, `NotFound` otherwise         |
//!
//! `SandboxConfig.image` carries the WASM bytes as standard base64.
//! `cpu_limit` from `SandboxConfig` is translated to wasmtime fuel units.
//! Timeout is enforced via epoch-interruption with a background ticker thread.

#[cfg(feature = "wasm")]
mod inner {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use wasmtime::{Config, Engine, Linker, Module, Store};
    use wasmtime_wasi::WasiCtxBuilder;
    use wasmtime_wasi::p1::{self, WasiP1Ctx};
    use wasmtime_wasi::p2::pipe::MemoryOutputPipe;

    use crate::sandbox::{
        ExecResult, SandboxConfig, SandboxError, SandboxHandle, SandboxProvider,
    };

    /// Default fuel per `execute` call (~10 million Wasm instructions).
    const DEFAULT_FUEL: u64 = 10_000_000;
    /// Default timeout per `execute` call in seconds.
    const DEFAULT_TIMEOUT_SECS: u64 = 10;
    /// Capture pipe capacity (1 MiB).
    const PIPE_CAPACITY: usize = 1024 * 1024;

    struct WasmEntry {
        module_bytes: Vec<u8>,
        config: SandboxConfig,
    }

    /// Wasmtime-backed sandbox provider.
    ///
    /// A single [`Engine`] is shared across all sandboxes. Each `execute` call
    /// gets its own [`Store`] so there is no state leakage between invocations.
    pub struct WasmSandboxProvider {
        engine: Arc<Engine>,
        sandboxes: Arc<Mutex<HashMap<String, WasmEntry>>>,
    }

    impl WasmSandboxProvider {
        pub fn new() -> Result<Self, SandboxError> {
            let mut cfg = Config::new();
            cfg.epoch_interruption(true);
            cfg.consume_fuel(true);

            let engine = Engine::new(&cfg).map_err(|e| SandboxError::CreateFailed {
                reason: format!("engine init: {e}"),
            })?;

            Ok(Self {
                engine: Arc::new(engine),
                sandboxes: Arc::new(Mutex::new(HashMap::new())),
            })
        }
    }

    impl Default for WasmSandboxProvider {
        fn default() -> Self {
            Self::new().expect("WasmSandboxProvider engine init failed")
        }
    }

    #[async_trait]
    impl SandboxProvider for WasmSandboxProvider {
        fn name(&self) -> &str {
            "wasm"
        }

        async fn create(
            &self,
            config: &SandboxConfig,
        ) -> Result<SandboxHandle, SandboxError> {
            let image = config.image.as_deref().unwrap_or_default();
            if image.is_empty() {
                return Err(SandboxError::CreateFailed {
                    reason: "SandboxConfig.image must contain base64-encoded WASM bytes"
                        .to_string(),
                });
            }

            let module_bytes = decode_base64(image)?;

            // Pre-validate before storing.
            Module::validate(&self.engine, &module_bytes).map_err(|e| {
                SandboxError::CreateFailed {
                    reason: format!("module validation: {e}"),
                }
            })?;

            let id = uuid::Uuid::new_v4().to_string();
            self.sandboxes.lock().unwrap().insert(
                id.clone(),
                WasmEntry { module_bytes, config: config.clone() },
            );

            Ok(SandboxHandle(id))
        }

        async fn execute(
            &self,
            handle: &SandboxHandle,
            command: &str,
            env: &HashMap<String, String>,
        ) -> Result<ExecResult, SandboxError> {
            let (module_bytes, sandbox_config) = {
                let guard = self.sandboxes.lock().unwrap();
                let entry = guard.get(&handle.0).ok_or(SandboxError::NotFound)?;
                (entry.module_bytes.clone(), entry.config.clone())
            };

            // Clone what we need for the blocking thread.
            let engine = Arc::clone(&self.engine);
            let command = command.to_owned();
            let env = env.clone();

            let result = tokio::task::spawn_blocking(move || {
                run_module(&engine, &module_bytes, &command, &env, &sandbox_config)
            })
            .await
            .map_err(|e| SandboxError::ExecFailed {
                reason: format!("spawn_blocking panicked: {e}"),
            })??;

            Ok(result)
        }

        async fn read_file(
            &self,
            _handle: &SandboxHandle,
            _path: &str,
        ) -> Result<Vec<u8>, SandboxError> {
            Err(SandboxError::NotImplemented)
        }

        async fn write_file(
            &self,
            _handle: &SandboxHandle,
            _path: &str,
            _content: &[u8],
        ) -> Result<(), SandboxError> {
            Err(SandboxError::NotImplemented)
        }

        async fn destroy(&self, handle: &SandboxHandle) -> Result<(), SandboxError> {
            self.sandboxes
                .lock()
                .unwrap()
                .remove(&handle.0)
                .ok_or(SandboxError::NotFound)?;
            Ok(())
        }

        async fn status(&self, handle: &SandboxHandle) -> Result<String, SandboxError> {
            if self.sandboxes.lock().unwrap().contains_key(&handle.0) {
                Ok("ready".to_string())
            } else {
                Err(SandboxError::NotFound)
            }
        }
    }

    fn decode_base64(s: &str) -> Result<Vec<u8>, SandboxError> {
        use base64::Engine as B64Engine;
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|e| SandboxError::CreateFailed {
                reason: format!("base64 decode: {e}"),
            })
    }

    /// Compile, instantiate, and run a WASIp1 module synchronously.
    fn run_module(
        engine: &Engine,
        module_bytes: &[u8],
        command: &str,
        env: &HashMap<String, String>,
        config: &SandboxConfig,
    ) -> Result<ExecResult, SandboxError> {
        let module = Module::new(engine, module_bytes).map_err(|e| SandboxError::ExecFailed {
            reason: format!("module compile: {e}"),
        })?;

        // Capture pipes.
        let stdout_pipe = MemoryOutputPipe::new(PIPE_CAPACITY);
        let stderr_pipe = MemoryOutputPipe::new(PIPE_CAPACITY);

        // Build WASIp1 context.
        let mut builder = WasiCtxBuilder::new();
        builder.stdout(stdout_pipe.clone());
        builder.stderr(stderr_pipe.clone());

        // argv: split command string, default to ["main"].
        let args: Vec<String> = if command.is_empty() {
            vec!["main".to_string()]
        } else {
            command.split_whitespace().map(str::to_owned).collect()
        };
        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        builder.args(&args_refs);

        for (k, v) in env {
            builder.env(k, v);
        }

        let wasi_ctx: WasiP1Ctx = builder.build_p1();

        let mut store = Store::new(engine, wasi_ctx);

        // Fuel limit (CPU).
        let fuel = config
            .cpu_limit
            .map(|f| (f * DEFAULT_FUEL as f64) as u64)
            .unwrap_or(DEFAULT_FUEL);
        store.set_fuel(fuel).map_err(|e| SandboxError::ExecFailed {
            reason: format!("set_fuel: {e}"),
        })?;

        // Epoch deadline + background ticker for timeout.
        store.set_epoch_deadline(1);
        let engine_clone = engine.clone();
        let _ticker = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(DEFAULT_TIMEOUT_SECS));
            engine_clone.increment_epoch();
        });

        // Link WASIp1.
        let mut linker: Linker<WasiP1Ctx> = Linker::new(engine);
        p1::add_to_linker_sync(&mut linker, |t| t).map_err(|e| SandboxError::ExecFailed {
            reason: format!("link wasi: {e}"),
        })?;

        let instance =
            linker
                .instantiate(&mut store, &module)
                .map_err(|e| SandboxError::ExecFailed {
                    reason: format!("instantiate: {e}"),
                })?;

        let exit_code = match instance.get_typed_func::<(), ()>(&mut store, "_start") {
            Ok(start_fn) => match start_fn.call(&mut store, ()) {
                Ok(()) => 0,
                Err(e) => {
                    if let Some(exit) = e.downcast_ref::<wasmtime_wasi::I32Exit>() {
                        exit.0
                    } else {
                        // Walk the full anyhow cause chain for fuel/interrupt signals.
                        let full = format!("{e:#}");
                        if full.contains("all fuel consumed by WebAssembly") {
                            return Err(SandboxError::ExecFailed {
                                reason: "execution ran out of fuel (CPU limit)".to_string(),
                            });
                        } else if full.contains("interrupt") {
                            return Err(SandboxError::ExecFailed {
                                reason: "execution timed out (epoch interrupt)".to_string(),
                            });
                        } else {
                            1
                        }
                    }
                }
            },
            // No _start export — module is valid but has no entry point.
            Err(_) => 0,
        };

        let stdout = String::from_utf8_lossy(&stdout_pipe.contents()).into_owned();
        let stderr = String::from_utf8_lossy(&stderr_pipe.contents()).into_owned();

        Ok(ExecResult { exit_code, stdout, stderr })
    }
}

#[cfg(feature = "wasm")]
pub use inner::WasmSandboxProvider;

// Stub when feature is disabled — keeps the module visible without wasmtime.
#[cfg(not(feature = "wasm"))]
pub use stub::WasmSandboxProvider;

#[cfg(not(feature = "wasm"))]
mod stub {
    use std::collections::HashMap;

    use async_trait::async_trait;

    use crate::sandbox::{
        ExecResult, SandboxConfig, SandboxError, SandboxHandle, SandboxProvider,
    };

    /// Stub WASM sandbox provider.  Enable the `wasm` feature for the real
    /// wasmtime-backed implementation.
    pub struct WasmSandboxProvider;

    #[async_trait]
    impl SandboxProvider for WasmSandboxProvider {
        fn name(&self) -> &str {
            "wasm"
        }
        async fn create(&self, _: &SandboxConfig) -> Result<SandboxHandle, SandboxError> {
            Err(SandboxError::NotImplemented)
        }
        async fn execute(
            &self,
            _: &SandboxHandle,
            _: &str,
            _: &HashMap<String, String>,
        ) -> Result<ExecResult, SandboxError> {
            Err(SandboxError::NotImplemented)
        }
        async fn read_file(&self, _: &SandboxHandle, _: &str) -> Result<Vec<u8>, SandboxError> {
            Err(SandboxError::NotImplemented)
        }
        async fn write_file(
            &self,
            _: &SandboxHandle,
            _: &str,
            _: &[u8],
        ) -> Result<(), SandboxError> {
            Err(SandboxError::NotImplemented)
        }
        async fn destroy(&self, _: &SandboxHandle) -> Result<(), SandboxError> {
            Err(SandboxError::NotImplemented)
        }
        async fn status(&self, _: &SandboxHandle) -> Result<String, SandboxError> {
            Err(SandboxError::NotImplemented)
        }
    }
}
