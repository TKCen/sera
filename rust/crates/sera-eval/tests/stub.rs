//! Smoke tests for the sera-eval stub. Verify:
//! - The bundled sample task YAMLs parse cleanly.
//! - The SQLite store accepts the schema and round-trips a task result.
//! - The harness-config labels match what the design doc pins as stable.

use std::path::PathBuf;

use sera_eval::{
    AssertionKind, BenchmarkSuite, EvalStore, HarnessConfig, MetricSet, SuiteId, TaskDef,
    TaskResult, Verdict,
    task_def::{MemoryPrecision, parse_task_file},
};

fn tasks_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tasks")
}

/// Test-only adapter that loads the bundled `tasks/` directory as the
/// `sera-internal` suite. The production adapter will live alongside the
/// runner in a later PR — this one exists only so we can exercise the
/// `BenchmarkSuite` trait in the stub.
struct BundledInternalSuite {
    dir: PathBuf,
}

impl BenchmarkSuite for BundledInternalSuite {
    fn id(&self) -> SuiteId {
        SuiteId::SeraInternal
    }

    fn tasks(&self) -> Result<Vec<TaskDef>, sera_eval::EvalError> {
        let mut out = Vec::new();
        let mut entries: Vec<_> = std::fs::read_dir(&self.dir)?
            .filter_map(Result::ok)
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext == "yaml" || ext == "yml")
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let contents = std::fs::read_to_string(entry.path())?;
            out.push(parse_task_file(&contents)?);
        }
        Ok(out)
    }
}

#[test]
fn sample_tasks_parse_and_validate() {
    let suite = BundledInternalSuite { dir: tasks_dir() };
    let tasks = suite.tasks().expect("tasks should load");
    assert!(
        tasks.len() >= 5 && tasks.len() <= 20,
        "expected 5–20 bundled tasks, got {}",
        tasks.len()
    );

    for task in &tasks {
        assert_eq!(task.suite, "sera-internal", "task {} in wrong suite", task.id);
        assert!(!task.input.prompt.is_empty(), "task {} has empty prompt", task.id);
        assert!(
            !task.expected.assertions.is_empty(),
            "task {} has no assertions",
            task.id
        );
    }

    let ids: Vec<_> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        ids.len(),
        "duplicate task ids in bundled corpus: {ids:?}"
    );
}

#[test]
fn filter_applies_glob() {
    let suite = BundledInternalSuite { dir: tasks_dir() };
    let all = suite.tasks().unwrap();
    let filtered = suite.filter(all.clone(), Some("sera-internal-000?"));
    // 9 matches single-digit suffixes (0001..0009); 0010 is excluded.
    assert!(
        filtered.len() < all.len(),
        "glob should drop the two-digit suffix task"
    );
    assert!(filtered.iter().all(|t| t.id.len() == "sera-internal-0001".len()));
}

#[test]
fn assertion_kinds_cover_design_doc() {
    // Sanity: the bundled corpus should exercise a reasonable spread of
    // assertion kinds, not just `contains_any`. If the corpus shrinks to one
    // kind this test will warn us.
    let suite = BundledInternalSuite { dir: tasks_dir() };
    let tasks = suite.tasks().unwrap();
    let mut kinds = std::collections::HashSet::new();
    for t in &tasks {
        for a in &t.expected.assertions {
            kinds.insert(a.kind);
        }
    }
    assert!(kinds.contains(&AssertionKind::ContainsAny));
    assert!(kinds.contains(&AssertionKind::NotContains));
    assert!(kinds.contains(&AssertionKind::ToolCalled));
}

#[test]
fn store_round_trips_a_task_result() {
    let store = EvalStore::open(std::path::Path::new(":memory:")).unwrap();
    store
        .insert_run(
            "run_test_0001",
            "sera-internal",
            "qwen/qwen3.6-35b-a3b",
            HarnessConfig::Full,
            "{}",
            "2026-04-19T00:00:00Z",
            "abc1234",
            "test-host",
            Some("stub smoke test"),
        )
        .unwrap();

    let result = TaskResult {
        task_id: "sera-internal-0001".into(),
        verdict: Verdict::Pass,
        metrics: MetricSet {
            turns: 2,
            prompt_tokens: 1200,
            completion_tokens: 150,
            latency_ms: 3500,
            tool_calls_total: 1,
            tool_calls_valid: 1,
            memory_precision: Some(MemoryPrecision {
                k: 3,
                gold_count: 1,
                hit_count: 1,
            }),
            cost_usd: 0.0,
        },
        transcript: serde_json::json!([{"role": "user", "content": "..."}]),
        error_message: None,
    };
    store
        .insert_task_result("result_test_0001", "run_test_0001", &result, "2026-04-19T00:00:05Z")
        .unwrap();

    assert_eq!(store.count_passes("run_test_0001").unwrap(), 1);

    let row = store.get_run("run_test_0001").unwrap().expect("run exists");
    assert_eq!(row.suite, "sera-internal");
    assert_eq!(row.harness, "+full");
    assert!(row.finished_at.is_none());

    store.finish_run("run_test_0001", "2026-04-19T00:00:10Z").unwrap();
    let row = store.get_run("run_test_0001").unwrap().unwrap();
    assert!(row.finished_at.is_some());
}

#[test]
fn parse_rejects_empty_prompt() {
    let bad = r#"---
id: bad-001
title: bad
suite: sera-internal
input:
  prompt: ""
expected:
  assertions: []
---
"#;
    let err = parse_task_file(bad).unwrap_err();
    assert!(format!("{err}").contains("prompt must not be empty"));
}
