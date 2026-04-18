//! Integration tests for the ShadowSessionExecutor scaffold.
//!
//! Covers:
//! - Single-turn round-trip (run + identical-output diff matches)
//! - Budget exceeded (4 turns against max_turns=3)
//! - Invalid ruleset (empty rules_json)
//! - Diff surfacing TextDiff on mismatched assistant text
//! - Diff surfacing ToolCallMismatch on differing tool-call vectors
//! - Serde round-trip for ShadowTurnOutput

use std::time::Duration;

use sera_runtime::shadow::{
    InMemoryShadowExecutor, ShadowBudget, ShadowDelta, ShadowError,
    ShadowRuleset, ShadowSessionExecutor, ShadowTurnInput, ShadowTurnOutput, diff,
};

fn rs(desc: &str) -> ShadowRuleset {
    ShadowRuleset {
        rules_json: serde_json::json!({ "r1": "allow" }),
        description: desc.to_string(),
    }
}

fn inp(msg: &str) -> ShadowTurnInput {
    ShadowTurnInput {
        user_message: msg.to_string(),
        context: serde_json::json!({}),
    }
}

#[tokio::test]
async fn single_turn_round_trip_matches() {
    let exec = InMemoryShadowExecutor::new();
    let ruleset = rs("hello");
    let out = exec
        .run(&ruleset, vec![inp("ping")], ShadowBudget::default())
        .await
        .expect("run should succeed");

    assert_eq!(out.len(), 1);
    // Diff against itself must match.
    let d = diff(&out[0], &out[0].clone());
    assert!(d.matched, "identical output should match");
    assert!(d.deltas.is_empty());
}

#[tokio::test]
async fn budget_exceeded_when_turn_count_exceeds_max() {
    let exec = InMemoryShadowExecutor::new();
    let budget = ShadowBudget {
        max_turns: 3,
        max_wall_time: Duration::from_secs(30),
        max_tool_calls: 20,
    };
    let turns = vec![inp("a"), inp("b"), inp("c"), inp("d")];
    let err = exec
        .run(&rs("x"), turns, budget)
        .await
        .expect_err("4 turns against max_turns=3 must fail");

    match err {
        ShadowError::BudgetExceeded(msg) => {
            assert!(msg.contains("max_turns=3"), "message: {msg}");
        }
        other => panic!("expected BudgetExceeded, got {other:?}"),
    }
}

#[tokio::test]
async fn invalid_ruleset_rejected_when_rules_json_empty() {
    let exec = InMemoryShadowExecutor::new();
    let ruleset = ShadowRuleset {
        rules_json: serde_json::json!({}),
        description: "empty".into(),
    };
    let err = exec
        .run(&ruleset, vec![inp("x")], ShadowBudget::default())
        .await
        .expect_err("empty rules_json must be rejected");

    match err {
        ShadowError::RulesetInvalid(_) => {}
        other => panic!("expected RulesetInvalid, got {other:?}"),
    }
}

#[tokio::test]
async fn invalid_ruleset_rejected_when_rules_json_null() {
    let exec = InMemoryShadowExecutor::new();
    let ruleset = ShadowRuleset {
        rules_json: serde_json::Value::Null,
        description: "null".into(),
    };
    let err = exec
        .run(&ruleset, vec![inp("x")], ShadowBudget::default())
        .await
        .expect_err("null rules_json must be rejected");

    assert!(matches!(err, ShadowError::RulesetInvalid(_)));
}

#[test]
fn diff_text_diff_variant_emitted_on_mismatched_text() {
    let real = ShadowTurnOutput {
        assistant_text: "hello from real".into(),
        tool_calls: vec![],
        terminated: false,
    };
    let shadow = ShadowTurnOutput {
        assistant_text: "hello from shadow".into(),
        tool_calls: vec![],
        terminated: false,
    };

    let d = diff(&real, &shadow);
    assert!(!d.matched);
    assert_eq!(d.deltas.len(), 1);
    assert!(matches!(
        &d.deltas[0],
        ShadowDelta::TextDiff { real: r, shadow: s }
            if r == "hello from real" && s == "hello from shadow"
    ));
}

#[test]
fn diff_tool_call_mismatch_variant_emitted_on_different_tool_count() {
    let real = ShadowTurnOutput {
        assistant_text: "same".into(),
        tool_calls: vec!["read:deadbeef".into(), "write:cafebabe".into()],
        terminated: false,
    };
    let shadow = ShadowTurnOutput {
        assistant_text: "same".into(),
        tool_calls: vec!["read:deadbeef".into()],
        terminated: false,
    };

    let d = diff(&real, &shadow);
    assert!(!d.matched);
    assert_eq!(d.deltas.len(), 1);
    assert!(matches!(
        &d.deltas[0],
        ShadowDelta::ToolCallMismatch { real: r, shadow: s }
            if r.len() == 2 && s.len() == 1
    ));
}

#[test]
fn diff_termination_mismatch_variant_emitted() {
    let real = ShadowTurnOutput {
        assistant_text: "same".into(),
        tool_calls: vec![],
        terminated: true,
    };
    let shadow = ShadowTurnOutput {
        assistant_text: "same".into(),
        tool_calls: vec![],
        terminated: false,
    };

    let d = diff(&real, &shadow);
    assert!(!d.matched);
    assert!(matches!(
        &d.deltas[0],
        ShadowDelta::TerminationMismatch { real: true, shadow: false }
    ));
}

#[test]
fn serde_round_trip_for_shadow_turn_output() {
    let out = ShadowTurnOutput {
        assistant_text: "hello".into(),
        tool_calls: vec!["t1:abcd1234".into(), "t2:beef0000".into()],
        terminated: false,
    };

    let json = serde_json::to_string(&out).expect("serialize");
    let back: ShadowTurnOutput = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, out);
}
