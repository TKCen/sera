//! Integration tests for WasmSandboxProvider.
//!
//! Requires `--features wasm` to compile and run.

#![cfg(feature = "wasm")]

use std::collections::HashMap;

use base64::Engine as B64Engine;
use sera_tools::sandbox::{SandboxConfig, SandboxError, SandboxProvider};
use sera_tools::sandbox::wasm::WasmSandboxProvider;

fn wasm_bytes_from_wat(src: &str) -> String {
    let bytes = wat::parse_str(src).expect("WAT compile failed");
    base64::engine::general_purpose::STANDARD.encode(&bytes)
}

/// A minimal WASIp1 "hello world" module that prints to stdout and exits 0.
const HELLO_WAT: &str = r#"
(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 8) "hello\n")
  (func $main (export "_start")
    ;; iovec at offset 0: ptr=8, len=6
    (i32.store (i32.const 0) (i32.const 8))
    (i32.store (i32.const 4) (i32.const 6))
    ;; fd_write(stdout=1, iovec_ptr=0, iovec_count=1, nwritten_ptr=16)
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 16)))
  )
)
"#;

/// A module that spins forever — used to test timeout.
const SPIN_WAT: &str = r#"
(module
  (func $spin (export "_start")
    (loop $forever
      (br $forever)
    )
  )
)
"#;

#[tokio::test]
async fn test_module_runs_and_exits_zero() {
    let provider = WasmSandboxProvider::new().expect("provider init");
    let image = wasm_bytes_from_wat(HELLO_WAT);

    let config = SandboxConfig { image: Some(image), ..Default::default() };
    let handle = provider.create(&config).await.expect("create");

    let result = provider
        .execute(&handle, "", &HashMap::new())
        .await
        .expect("execute");

    assert_eq!(result.exit_code, 0, "expected exit code 0, got {}", result.exit_code);
    assert_eq!(result.stdout.trim(), "hello", "unexpected stdout: {:?}", result.stdout);

    provider.destroy(&handle).await.expect("destroy");
}

#[tokio::test]
async fn test_timeout_triggers() {
    let provider = WasmSandboxProvider::new().expect("provider init");
    let image = wasm_bytes_from_wat(SPIN_WAT);

    // Set a tiny fuel budget so the spin loop exhausts it quickly instead of
    // waiting the full 10-second epoch timeout.
    let config = SandboxConfig {
        image: Some(image),
        cpu_limit: Some(0.000001), // ~10 fuel units
        ..Default::default()
    };
    let handle = provider.create(&config).await.expect("create");

    let err = provider
        .execute(&handle, "", &HashMap::new())
        .await
        .expect_err("expected ExecFailed due to fuel/timeout");

    assert!(
        matches!(err, SandboxError::ExecFailed { .. }),
        "expected ExecFailed, got {err:?}"
    );

    provider.destroy(&handle).await.expect("destroy");
}

#[tokio::test]
async fn test_destroy_removes_handle() {
    let provider = WasmSandboxProvider::new().expect("provider init");
    let image = wasm_bytes_from_wat(HELLO_WAT);
    let config = SandboxConfig { image: Some(image), ..Default::default() };
    let handle = provider.create(&config).await.expect("create");

    provider.destroy(&handle).await.expect("destroy");

    // Second destroy should fail with NotFound.
    let err = provider.destroy(&handle).await.expect_err("expected NotFound");
    assert!(matches!(err, SandboxError::NotFound));
}

#[tokio::test]
async fn test_create_invalid_image_fails() {
    let provider = WasmSandboxProvider::new().expect("provider init");
    // Valid base64 but not valid wasm.
    let bad_image = base64::engine::general_purpose::STANDARD.encode(b"not-wasm");
    let config = SandboxConfig { image: Some(bad_image), ..Default::default() };
    let err = provider.create(&config).await.expect_err("expected CreateFailed");
    assert!(matches!(err, SandboxError::CreateFailed { .. }));
}
