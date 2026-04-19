//! First real sera-eval measurement: raw LM Studio vs full SERA harness.
//!
//! Loads the bundled `sera-internal` task corpus, runs each task under two
//! configurations, and writes results to `sera-eval.db` plus a markdown
//! report at `docs/sera-eval-first-run.md`.
//!
//! # Configurations
//!
//! - **raw**: direct `POST /v1/chat/completions` against LM Studio with a
//!   fixed system prompt and the task prompt as the only user turn. No
//!   memory, no tools, no transcript carry-over.
//! - **full**: `POST /api/chat` against the running SERA gateway at
//!   `$SERA_ENDPOINT`. Memory-seed items with `role: user` are replayed as
//!   prior /api/chat turns so the transcript carries them forward — this is
//!   the only memory surface the MVS binary exposes today. Between tasks we
//!   `docker restart $SERA_CONTAINER` to clear the shared per-agent session
//!   (the MVS binary has one session per agent, so cross-task contamination
//!   is otherwise unavoidable).
//!
//! # Scoring
//!
//! Assertions supported directly: `contains_any`, `contains_all`,
//! `not_contains`, `regex`. Unsupported kinds (`tool_called`, `file_written`,
//! `patch_applies`, `external_grader`) are marked as unsupported and, if a
//! task has **only** unsupported assertions, an LLM judge (the same LM
//! Studio endpoint) is asked for a Pass/Fail verdict with the task rationale
//! as rubric. Otherwise the direct assertions alone decide the verdict.
//!
//! # What this run measures (and does not)
//!
//! This is a baseline, not a victory lap. The MVS gateway does not yet wire
//! the ContextEngine, memory layer, skills autoloader, sandbox tiers, or
//! HITL plumbing into `/api/chat` — so tasks targeting those surfaces will
//! fail under `full` for a reason that is not the model's fault. The report
//! calls this out explicitly.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};

use sera_eval::task_def::{
    AssertionKind, MemorySeedItem, MetricSet, TaskDef, TaskResult, Verdict, parse_task_file,
};
use sera_eval::{EvalStore, HarnessConfig};

const MODEL: &str = "qwen/qwen3.6-35b-a3b";
const LM_STUDIO_BASE: &str = "http://127.0.0.1:1234/v1";
const SERA_ENDPOINT_DEFAULT: &str = "http://127.0.0.1:3001";
const SERA_CONTAINER_DEFAULT: &str = "rust-sera-1";
const SERA_API_KEY_DEFAULT: &str =
    "605d685d121d2f50daf1b1e3be84b3161ff80d16e56d60df4e5be35f9ebca639";
const SYSTEM_PROMPT_RAW: &str = "You are a helpful assistant.";
const TURN_TIMEOUT_SECS: u64 = 180;
const JUDGE_TIMEOUT_SECS: u64 = 60;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let sera_endpoint = env::var("SERA_ENDPOINT").unwrap_or_else(|_| SERA_ENDPOINT_DEFAULT.into());
    let sera_container =
        env::var("SERA_CONTAINER").unwrap_or_else(|_| SERA_CONTAINER_DEFAULT.into());
    let sera_api_key = env::var("SERA_API_KEY").unwrap_or_else(|_| SERA_API_KEY_DEFAULT.into());

    let skip_full = args.iter().any(|a| a == "--skip-full");
    let skip_raw = args.iter().any(|a| a == "--skip-raw");

    let repo_root = repo_root()?;
    let tasks_dir = repo_root.join("rust/crates/sera-eval/tasks");
    let db_path = repo_root.join("sera-eval.db");
    let report_path = repo_root.join("docs/sera-eval-first-run.md");

    eprintln!("sera-eval-first-run: repo_root={}", repo_root.display());
    eprintln!("  tasks_dir      = {}", tasks_dir.display());
    eprintln!("  db_path        = {}", db_path.display());
    eprintln!("  sera_endpoint  = {sera_endpoint}");
    eprintln!("  sera_container = {sera_container}");
    eprintln!("  model          = {MODEL}");

    let tasks = load_tasks(&tasks_dir)?;
    eprintln!("loaded {} tasks", tasks.len());

    // Health checks before we commit to a long run.
    let client = http_client()?;
    ensure_lm_studio_up(&client).await?;
    ensure_sera_up(&client, &sera_endpoint).await?;

    let store = EvalStore::open(&db_path)
        .with_context(|| format!("open eval store at {}", db_path.display()))?;

    let git_sha = git_sha(&repo_root).unwrap_or_else(|_| "unknown".into());
    let host = hostname().unwrap_or_else(|_| "unknown".into());
    let started_at = Utc::now().to_rfc3339();

    let raw_run_id = format!("run_{}", uuid::Uuid::new_v4().simple());
    let full_run_id = format!("run_{}", uuid::Uuid::new_v4().simple());

    if !skip_raw {
        store.insert_run(
            &raw_run_id,
            "sera-internal",
            MODEL,
            HarnessConfig::Raw,
            r#"{"source":"lm-studio","base":"http://127.0.0.1:1234/v1"}"#,
            &started_at,
            &git_sha,
            &host,
            Some("first-run: raw LM Studio baseline"),
        )?;
    }
    if !skip_full {
        store.insert_run(
            &full_run_id,
            "sera-internal",
            MODEL,
            HarnessConfig::Full,
            &serde_json::json!({
                "source": "sera-gateway",
                "endpoint": &sera_endpoint,
                "memory_seeding": "user-role seeds replayed as prior /api/chat turns",
                "per_task_reset": "docker restart between tasks"
            })
            .to_string(),
            &started_at,
            &git_sha,
            &host,
            Some("first-run: full SERA harness via /api/chat"),
        )?;
    }

    if !skip_raw {
        eprintln!("\n=== running RAW configuration ===");
        for (i, task) in tasks.iter().enumerate() {
            eprintln!("[raw {}/{}] {}", i + 1, tasks.len(), task.id);
            let result = raw_pipeline(&client, task).await;
            let (tr, err) = materialise_result(task, result);
            print_verdict(&tr, err.as_deref());
            store.insert_task_result(
                &format!("result_{}", uuid::Uuid::new_v4().simple()),
                &raw_run_id,
                &tr,
                &Utc::now().to_rfc3339(),
            )?;
        }
        store.finish_run(&raw_run_id, &Utc::now().to_rfc3339())?;
    }

    if !skip_full {
        eprintln!("\n=== running FULL configuration ===");
        for (i, task) in tasks.iter().enumerate() {
            eprintln!("[full {}/{}] {}", i + 1, tasks.len(), task.id);
            restart_sera(&sera_container)?;
            wait_for_sera(&client, &sera_endpoint).await?;
            warmup_sera(&client, &sera_endpoint, &sera_api_key).await?;
            let result = full_pipeline(&client, &sera_endpoint, &sera_api_key, task).await;
            let (tr, err) = materialise_result(task, result);
            print_verdict(&tr, err.as_deref());
            store.insert_task_result(
                &format!("result_{}", uuid::Uuid::new_v4().simple()),
                &full_run_id,
                &tr,
                &Utc::now().to_rfc3339(),
            )?;
        }
        store.finish_run(&full_run_id, &Utc::now().to_rfc3339())?;
    }

    eprintln!("\n=== generating report ===");
    let raw_opt = (!skip_raw).then_some(raw_run_id.as_str());
    let full_opt = (!skip_full).then_some(full_run_id.as_str());
    render_report(&db_path, &report_path, &tasks, raw_opt, full_opt, &git_sha)?;
    eprintln!("wrote {}", report_path.display());
    Ok(())
}

// ── Task loading ────────────────────────────────────────────────────────────

fn load_tasks(dir: &Path) -> Result<Vec<TaskDef>> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("read tasks dir {}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext == "yaml" || ext == "yml")
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let contents = fs::read_to_string(entry.path())?;
        out.push(parse_task_file(&contents)?);
    }
    Ok(out)
}

// ── Raw runner (LM Studio direct) ───────────────────────────────────────────

#[derive(Debug)]
struct RunOutcome {
    reply: String,
    prompt_tokens: u32,
    completion_tokens: u32,
    turns: u32,
    latency_ms: u64,
}

async fn run_raw(client: &reqwest::Client, task: &TaskDef) -> Result<RunOutcome> {
    let start = Instant::now();
    let body = serde_json::json!({
        "model": MODEL,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": SYSTEM_PROMPT_RAW},
            {"role": "user", "content": task.input.prompt},
        ],
    });
    let resp = client
        .post(format!("{LM_STUDIO_BASE}/chat/completions"))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let parsed: LmStudioResponse = resp.json().await?;
    let reply = parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .unwrap_or_default();
    Ok(RunOutcome {
        reply,
        prompt_tokens: parsed.usage.prompt_tokens,
        completion_tokens: parsed.usage.completion_tokens,
        turns: 1,
        latency_ms: start.elapsed().as_millis() as u64,
    })
}

#[derive(Debug, Deserialize)]
struct LmStudioResponse {
    choices: Vec<LmStudioChoice>,
    usage: LmStudioUsage,
}

#[derive(Debug, Deserialize)]
struct LmStudioChoice {
    message: LmStudioMessage,
}

#[derive(Debug, Deserialize)]
struct LmStudioMessage {
    content: String,
}

#[derive(Debug, Deserialize, Default)]
struct LmStudioUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
}

// ── Full runner (SERA /api/chat) ────────────────────────────────────────────

async fn run_full(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    task: &TaskDef,
) -> Result<RunOutcome> {
    let start = Instant::now();
    let mut turns: u32 = 0;

    // Replay user-role memory seeds as prior /api/chat turns so the
    // transcript carries them forward for the real prompt. Non-user roles
    // (system / tool / assistant) cannot be injected via /api/chat in the
    // MVS binary; those seeds are skipped and recorded in the report.
    for seed in &task.setup.memory_seed {
        if seed.role == "user" {
            call_sera_chat(client, endpoint, api_key, &seed.content).await?;
            turns += 1;
        }
    }

    let reply = call_sera_chat(client, endpoint, api_key, &task.input.prompt).await?;
    turns += 1;
    Ok(RunOutcome {
        reply,
        // MVS `/api/chat` does not report usage (returns zeros). Leave the
        // token counts at zero so the report can flag the gap instead of
        // silently overstating accuracy.
        prompt_tokens: 0,
        completion_tokens: 0,
        turns,
        latency_ms: start.elapsed().as_millis() as u64,
    })
}

async fn call_sera_chat(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    message: &str,
) -> Result<String> {
    let body = serde_json::json!({ "message": message });
    let resp = client
        .post(format!("{endpoint}/api/chat"))
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    let status = resp.status();
    let text = resp.text().await?;
    if !status.is_success() {
        return Err(anyhow!(
            "/api/chat returned {}: {}",
            status,
            text.chars().take(400).collect::<String>()
        ));
    }
    let parsed: SeraChatResponse = serde_json::from_str(&text)
        .with_context(|| format!("parse /api/chat response: {text}"))?;
    Ok(parsed.response)
}

#[derive(Debug, Deserialize)]
struct SeraChatResponse {
    response: String,
}

// ── Scoring ─────────────────────────────────────────────────────────────────

struct ScoringOutcome {
    verdict: Verdict,
    // Assertions that cannot be graded by this runner (tool traces, sandbox
    // filesystem, external grader). Recorded so the report can flag them.
    unsupported_assertions: Vec<AssertionKind>,
    // When true, the verdict came from the LLM judge, not direct matching.
    used_llm_judge: bool,
}

async fn score(client: &reqwest::Client, task: &TaskDef, reply: &str) -> Result<ScoringOutcome> {
    let mut direct_results: Vec<bool> = Vec::new();
    let mut unsupported: Vec<AssertionKind> = Vec::new();

    for a in &task.expected.assertions {
        match a.kind {
            AssertionKind::ContainsAny => {
                direct_results.push(a.values.iter().any(|v| reply.contains(v.as_str())));
            }
            AssertionKind::ContainsAll => {
                direct_results.push(a.values.iter().all(|v| reply.contains(v.as_str())));
            }
            AssertionKind::NotContains => {
                direct_results.push(!a.values.iter().any(|v| reply.contains(v.as_str())));
            }
            AssertionKind::Regex => {
                let pattern = a.values.first().cloned().unwrap_or_default();
                match Regex::new(&pattern) {
                    Ok(re) => direct_results.push(re.is_match(reply)),
                    Err(_) => direct_results.push(false),
                }
            }
            AssertionKind::ToolCalled
            | AssertionKind::FileWritten
            | AssertionKind::PatchApplies
            | AssertionKind::ExternalGrader => {
                unsupported.push(a.kind);
            }
        }
    }

    if direct_results.is_empty() && !unsupported.is_empty() {
        // Nothing to grade directly — fall back to LLM judge.
        let pass = llm_judge(client, task, reply).await?;
        return Ok(ScoringOutcome {
            verdict: if pass { Verdict::Pass } else { Verdict::Fail },
            unsupported_assertions: unsupported,
            used_llm_judge: true,
        });
    }

    // Any supported assertion failing → Fail. Unsupported ones do not
    // contribute to Pass but also do not flip Pass to Fail (they are
    // recorded so the report can flag them).
    let all_pass = !direct_results.is_empty() && direct_results.iter().all(|b| *b);
    Ok(ScoringOutcome {
        verdict: if all_pass { Verdict::Pass } else { Verdict::Fail },
        unsupported_assertions: unsupported,
        used_llm_judge: false,
    })
}

async fn llm_judge(client: &reqwest::Client, task: &TaskDef, reply: &str) -> Result<bool> {
    let rubric = task
        .expected
        .assertions
        .iter()
        .map(|a| format!("- {:?}: {:?}", a.kind, a.values))
        .collect::<Vec<_>>()
        .join("\n");
    let prompt = format!(
        "You are grading an AI agent's answer to a task. Reply with a single word — PASS or FAIL.\n\n\
         Task: {title}\n\
         Prompt: {prompt}\n\
         Rubric (assertions the agent must satisfy):\n{rubric}\n\
         Rationale from task designer:\n{rationale}\n\n\
         Agent's answer:\n---\n{reply}\n---\n\n\
         Does the answer satisfy the rubric? Reply PASS or FAIL.",
        title = task.title,
        prompt = task.input.prompt,
        rubric = rubric,
        rationale = task.rationale,
        reply = reply,
    );
    let body = serde_json::json!({
        "model": MODEL,
        "temperature": 0,
        "messages": [
            {"role": "system", "content": "You are a strict grader. Reply with PASS or FAIL only."},
            {"role": "user", "content": prompt},
        ],
    });
    let judge_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(JUDGE_TIMEOUT_SECS))
        .build()?;
    let _ = client; // keep signature stable even though we use a dedicated client for the judge
    let resp = judge_client
        .post(format!("{LM_STUDIO_BASE}/chat/completions"))
        .json(&body)
        .send()
        .await?
        .error_for_status()?;
    let parsed: LmStudioResponse = resp.json().await?;
    let verdict = parsed
        .choices
        .into_iter()
        .next()
        .map(|c| c.message.content.to_uppercase())
        .unwrap_or_default();
    Ok(verdict.contains("PASS") && !verdict.contains("FAIL"))
}

// ── Result assembly ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct Transcript<'a> {
    prompt: &'a str,
    reply: &'a str,
    unsupported_assertions: Vec<String>,
    used_llm_judge: bool,
    error: Option<&'a str>,
}

fn materialise_result(
    task: &TaskDef,
    run: Result<(RunOutcome, ScoringOutcome)>,
) -> (TaskResult, Option<String>) {
    match run {
        Ok((outcome, scoring)) => {
            let metrics = MetricSet {
                turns: outcome.turns,
                prompt_tokens: outcome.prompt_tokens,
                completion_tokens: outcome.completion_tokens,
                latency_ms: outcome.latency_ms,
                tool_calls_total: 0,
                tool_calls_valid: 0,
                memory_precision: None,
                cost_usd: 0.0,
            };
            let transcript = Transcript {
                prompt: &task.input.prompt,
                reply: &outcome.reply,
                unsupported_assertions: scoring
                    .unsupported_assertions
                    .iter()
                    .map(|k| format!("{k:?}"))
                    .collect(),
                used_llm_judge: scoring.used_llm_judge,
                error: None,
            };
            (
                TaskResult {
                    task_id: task.id.clone(),
                    verdict: scoring.verdict,
                    metrics,
                    transcript: serde_json::to_value(&transcript).unwrap_or(serde_json::Value::Null),
                    error_message: None,
                },
                None,
            )
        }
        Err(e) => {
            let msg = format!("{e:#}");
            let transcript = Transcript {
                prompt: &task.input.prompt,
                reply: "",
                unsupported_assertions: vec![],
                used_llm_judge: false,
                error: Some(&msg),
            };
            (
                TaskResult {
                    task_id: task.id.clone(),
                    verdict: Verdict::Error,
                    metrics: MetricSet::default(),
                    transcript: serde_json::to_value(&transcript).unwrap_or(serde_json::Value::Null),
                    error_message: Some(msg.clone()),
                },
                Some(msg),
            )
        }
    }
}

fn print_verdict(result: &TaskResult, err: Option<&str>) {
    let verdict = result.verdict.as_str();
    let extra = err.map(|e| format!(" ({e})")).unwrap_or_default();
    eprintln!(
        "  -> {} turns={} p_tok={} c_tok={} latency_ms={}{}",
        verdict,
        result.metrics.turns,
        result.metrics.prompt_tokens,
        result.metrics.completion_tokens,
        result.metrics.latency_ms,
        extra
    );
}

// Small wrapper so run_raw / run_full + scoring share the same signature.
async fn run_and_score(
    client: &reqwest::Client,
    task: &TaskDef,
    outcome: RunOutcome,
) -> Result<(RunOutcome, ScoringOutcome)> {
    let scoring = score(client, task, &outcome.reply).await?;
    Ok((outcome, scoring))
}

// ── Reporting ───────────────────────────────────────────────────────────────

#[derive(Debug)]
#[allow(dead_code)]
struct RunSummary {
    run_id: String,
    harness: String,
    n_tasks: u32,
    n_pass: u32,
    n_fail: u32,
    n_error: u32,
    avg_turns: f64,
    avg_prompt_tokens: f64,
    avg_completion_tokens: f64,
    avg_latency_ms: f64,
    per_task: Vec<TaskRow>,
}

#[derive(Debug)]
#[allow(dead_code)]
struct TaskRow {
    task_id: String,
    verdict: String,
    turns: u32,
    prompt_tokens: u32,
    completion_tokens: u32,
    latency_ms: u64,
    note: String,
}

fn load_summary(store: &rusqlite::Connection, run_id: &str) -> Result<RunSummary> {
    let mut stmt = store.prepare(
        "SELECT harness FROM eval_runs WHERE id = ?1",
    )?;
    let harness: String = stmt.query_row([run_id], |r| r.get(0))?;

    let mut stmt = store.prepare(
        "SELECT task_id, verdict, turns, prompt_tokens, completion_tokens,
                latency_ms, transcript_json, error_message
         FROM eval_task_results
         WHERE run_id = ?1
         ORDER BY task_id",
    )?;
    let rows = stmt.query_map([run_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;

    let mut n_tasks = 0u32;
    let mut n_pass = 0u32;
    let mut n_fail = 0u32;
    let mut n_error = 0u32;
    let mut sum_turns = 0u64;
    let mut sum_pt = 0u64;
    let mut sum_ct = 0u64;
    let mut sum_lat = 0u64;
    let mut per_task = Vec::new();

    for row in rows {
        let (task_id, verdict, turns, pt, ct, lat, transcript_json, err) = row?;
        n_tasks += 1;
        match verdict.as_str() {
            "pass" => n_pass += 1,
            "fail" => n_fail += 1,
            _ => n_error += 1,
        }
        sum_turns += turns as u64;
        sum_pt += pt as u64;
        sum_ct += ct as u64;
        sum_lat += lat as u64;

        let mut note = String::new();
        if let Some(e) = &err {
            note = format!("error: {}", e.chars().take(80).collect::<String>());
        } else if let Ok(v) = serde_json::from_str::<serde_json::Value>(&transcript_json) {
            let unsupported = v
                .get("unsupported_assertions")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let judged = v
                .get("used_llm_judge")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            let mut parts = Vec::new();
            if !unsupported.is_empty() {
                parts.push(format!("unsupported: {}", unsupported.join(", ")));
            }
            if judged {
                parts.push("llm-judge".into());
            }
            note = parts.join("; ");
        }

        per_task.push(TaskRow {
            task_id,
            verdict,
            turns: turns as u32,
            prompt_tokens: pt as u32,
            completion_tokens: ct as u32,
            latency_ms: lat as u64,
            note,
        });
    }

    let n_f = n_tasks.max(1) as f64;
    Ok(RunSummary {
        run_id: run_id.to_string(),
        harness,
        n_tasks,
        n_pass,
        n_fail,
        n_error,
        avg_turns: sum_turns as f64 / n_f,
        avg_prompt_tokens: sum_pt as f64 / n_f,
        avg_completion_tokens: sum_ct as f64 / n_f,
        avg_latency_ms: sum_lat as f64 / n_f,
        per_task,
    })
}

fn render_report(
    db_path: &Path,
    out_path: &Path,
    tasks: &[TaskDef],
    raw_run_id: Option<&str>,
    full_run_id: Option<&str>,
    git_sha: &str,
) -> Result<()> {
    let conn = rusqlite::Connection::open(db_path)?;
    let raw = raw_run_id.map(|id| load_summary(&conn, id)).transpose()?;
    let full = full_run_id.map(|id| load_summary(&conn, id)).transpose()?;

    let mut md = String::new();
    md.push_str("# sera-eval — First Real Measurement\n\n");
    md.push_str(&format!(
        "> Generated by `sera-eval-first-run` at `{}` against git `{}`.\n",
        Utc::now().to_rfc3339(),
        git_sha
    ));
    md.push_str(&format!("> Model: `{MODEL}` — LM Studio local.\n"));
    md.push_str("> Suite: `sera-internal` (10 bundled tasks).\n\n");
    md.push_str("See [`docs/sera-eval-design.md`](sera-eval-design.md) for the harness design.\n\n");

    md.push_str("## Headline\n\n");
    match (&raw, &full) {
        (Some(r), Some(f)) => {
            let raw_rate = pct(r.n_pass, r.n_tasks);
            let full_rate = pct(f.n_pass, f.n_tasks);
            let delta = (f.n_pass as i64) - (r.n_pass as i64);
            md.push_str(&format!(
                "**Raw LM Studio:** {}/{} pass ({:.0}%). \
                 **Full SERA harness:** {}/{} pass ({:.0}%). \
                 **Δ = {:+} tasks** ({:+.0} pp).\n\n",
                r.n_pass, r.n_tasks, raw_rate,
                f.n_pass, f.n_tasks, full_rate,
                delta, full_rate - raw_rate
            ));
        }
        (Some(r), None) => {
            md.push_str(&format!(
                "**Raw LM Studio:** {}/{} pass ({:.0}%). Full run skipped.\n\n",
                r.n_pass, r.n_tasks, pct(r.n_pass, r.n_tasks)
            ));
        }
        (None, Some(f)) => {
            md.push_str(&format!(
                "**Full SERA harness:** {}/{} pass ({:.0}%). Raw run skipped.\n\n",
                f.n_pass, f.n_tasks, pct(f.n_pass, f.n_tasks)
            ));
        }
        (None, None) => md.push_str("No runs — both configurations skipped.\n\n"),
    }

    md.push_str("## Summary table\n\n");
    md.push_str("| Config | Pass | Fail | Error | Avg turns | Avg prompt tok | Avg completion tok | Avg latency |\n");
    md.push_str("|--------|------|------|-------|-----------|----------------|--------------------|-------------|\n");
    if let Some(r) = &raw {
        md.push_str(&format_summary_row("raw", r));
    }
    if let Some(f) = &full {
        md.push_str(&format_summary_row("full", f));
    }
    md.push('\n');

    md.push_str("## Per-task results\n\n");
    md.push_str("| Task | Raw | Full | Raw lat | Full lat | Raw turns | Full turns | Notes |\n");
    md.push_str("|------|-----|------|---------|----------|-----------|------------|-------|\n");
    for task in tasks {
        let raw_row = raw
            .as_ref()
            .and_then(|r| r.per_task.iter().find(|x| x.task_id == task.id));
        let full_row = full
            .as_ref()
            .and_then(|f| f.per_task.iter().find(|x| x.task_id == task.id));
        let raw_verdict = raw_row.map(|x| x.verdict.as_str()).unwrap_or("—");
        let full_verdict = full_row.map(|x| x.verdict.as_str()).unwrap_or("—");
        let raw_lat = raw_row
            .map(|x| format!("{:.1}s", x.latency_ms as f64 / 1000.0))
            .unwrap_or_else(|| "—".into());
        let full_lat = full_row
            .map(|x| format!("{:.1}s", x.latency_ms as f64 / 1000.0))
            .unwrap_or_else(|| "—".into());
        let raw_turns = raw_row
            .map(|x| x.turns.to_string())
            .unwrap_or_else(|| "—".into());
        let full_turns = full_row
            .map(|x| x.turns.to_string())
            .unwrap_or_else(|| "—".into());
        let notes = task_notes(task, raw_row, full_row);
        md.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} |\n",
            task.id,
            verdict_emoji(raw_verdict),
            verdict_emoji(full_verdict),
            raw_lat,
            full_lat,
            raw_turns,
            full_turns,
            notes
        ));
    }
    md.push('\n');

    md.push_str("## What the harness can and cannot grade today\n\n");
    let unsupported_seen = collect_unsupported(&[raw.as_ref(), full.as_ref()]);
    if unsupported_seen.is_empty() {
        md.push_str("All assertion kinds used by the sample corpus were gradable.\n\n");
    } else {
        md.push_str(
            "The following assertion kinds in the sample corpus are not yet gradable by \
             the first-run binary — they are recorded in the transcript as `unsupported` \
             and, when a task has *only* unsupported assertions, an LLM judge is used:\n\n",
        );
        for k in &unsupported_seen {
            md.push_str(&format!("- `{}`\n", k));
        }
        md.push('\n');
    }

    md.push_str("## Caveats and gaps in this run\n\n");
    md.push_str(
        "- **Memory seeding is shallow.** The full-harness runner replays `role: user` \
         memory-seed items as prior `/api/chat` turns so the transcript carries them \
         forward. Seeds with `role: system`, `role: tool`, or `role: assistant` cannot \
         be injected via the MVS `/api/chat` binary and are silently skipped. Tasks \
         whose correctness depends on those roles will fail for a reason that is not \
         the model's fault.\n",
    );
    md.push_str(
        "- **Tool use is not wired through `/api/chat` yet.** The MVS binary does not \
         expose grep / bash / patch / request_approval tools to the model, so tasks with \
         `tool_called` assertions cannot pass via direct matching in the full run.\n",
    );
    md.push_str(
        "- **Token accounting is zero under full.** `/api/chat` returns `usage: {0,0,0}` \
         today. The raw run uses LM Studio's `usage` field directly. Context-efficiency \
         comparisons from this run are therefore not meaningful.\n",
    );
    md.push_str(
        "- **Single shared session per agent.** The MVS binary keeps one persistent \
         session per agent. Between tasks we `docker restart` the container so the \
         session starts clean — this adds ~20s per task and is why full-run latency is \
         high.\n",
    );
    md.push_str(
        "- **Sandbox tiers / HITL / egress allowlist are not enforced here.** Tasks \
         targeting those surfaces can only surface as \"the model happened to say the \
         magic word\" and should be re-run once the harness plumbs them in.\n",
    );
    md.push_str(
        "- **`latency_ms` under full includes the `docker restart` between tasks? No.** \
         The restart happens *before* the run starts and is excluded from the measured \
         latency. `latency_ms` is `/api/chat` request-to-response only, including any \
         memory-seed replay turns.\n",
    );

    md.push_str("\n## How to reproduce\n\n");
    md.push_str(
        "```bash\n\
         # from repo root; SERA gateway must be running at :3001 and LM Studio at :1234\n\
         cd rust && cargo run -p sera-eval --bin sera-eval-first-run\n\
         ```\n\n",
    );
    md.push_str(
        "Results are appended to `sera-eval.db` in the repo root and the markdown \
         report is regenerated at `docs/sera-eval-first-run.md`. Pass `--skip-full` or \
         `--skip-raw` to run only one configuration.\n",
    );

    fs::create_dir_all(out_path.parent().unwrap_or(Path::new(".")))?;
    fs::write(out_path, md)?;
    Ok(())
}

fn format_summary_row(label: &str, s: &RunSummary) -> String {
    format!(
        "| `{label}` | {} | {} | {} | {:.1} | {:.0} | {:.0} | {:.1}s |\n",
        s.n_pass,
        s.n_fail,
        s.n_error,
        s.avg_turns,
        s.avg_prompt_tokens,
        s.avg_completion_tokens,
        s.avg_latency_ms / 1000.0,
    )
}

fn task_notes(task: &TaskDef, raw: Option<&TaskRow>, full: Option<&TaskRow>) -> String {
    let mut parts = Vec::new();
    let kinds: HashSet<_> = task
        .expected
        .assertions
        .iter()
        .map(|a| format!("{:?}", a.kind))
        .collect();
    if task
        .setup
        .memory_seed
        .iter()
        .any(|s: &MemorySeedItem| s.role != "user")
    {
        parts.push("non-user seed skipped".into());
    }
    let unsupported_kinds: Vec<&str> = kinds
        .iter()
        .filter_map(|k| match k.as_str() {
            "ToolCalled" | "FileWritten" | "PatchApplies" | "ExternalGrader" => Some(k.as_str()),
            _ => None,
        })
        .collect();
    if !unsupported_kinds.is_empty() {
        parts.push(format!("uses {}", unsupported_kinds.join(", ")));
    }
    for (label, row) in [("raw", raw), ("full", full)] {
        if let Some(r) = row
            && r.note.contains("llm-judge")
        {
            parts.push(format!("{label}: llm-judge"));
        }
    }
    parts.join(" · ")
}

fn collect_unsupported(summaries: &[Option<&RunSummary>]) -> Vec<String> {
    let mut set: HashSet<String> = HashSet::new();
    for s in summaries.iter().flatten() {
        for t in &s.per_task {
            if t.note.contains("unsupported:") {
                for piece in t.note.split(';') {
                    if let Some(rest) = piece.trim().strip_prefix("unsupported:") {
                        for k in rest.split(',') {
                            set.insert(k.trim().to_string());
                        }
                    }
                }
            }
        }
    }
    let mut out: Vec<String> = set.into_iter().filter(|s| !s.is_empty()).collect();
    out.sort();
    out
}

fn verdict_emoji(v: &str) -> &'static str {
    match v {
        "pass" => "✅",
        "fail" => "❌",
        "error" => "⚠️",
        "—" => "—",
        _ => "?",
    }
}

fn pct(num: u32, denom: u32) -> f64 {
    if denom == 0 {
        0.0
    } else {
        (num as f64 / denom as f64) * 100.0
    }
}

// ── Health + reset helpers ──────────────────────────────────────────────────

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(TURN_TIMEOUT_SECS))
        .build()?)
}

async fn ensure_lm_studio_up(client: &reqwest::Client) -> Result<()> {
    let resp = client
        .get(format!("{LM_STUDIO_BASE}/models"))
        .send()
        .await
        .context("LM Studio /v1/models unreachable — is LM Studio running on :1234?")?;
    if !resp.status().is_success() {
        return Err(anyhow!("LM Studio /v1/models returned {}", resp.status()));
    }
    Ok(())
}

async fn ensure_sera_up(client: &reqwest::Client, endpoint: &str) -> Result<()> {
    let resp = client
        .get(format!("{endpoint}/api/health"))
        .send()
        .await
        .with_context(|| format!("SERA gateway {endpoint}/api/health unreachable"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("SERA /api/health returned {}", resp.status()));
    }
    Ok(())
}

fn restart_sera(container: &str) -> Result<()> {
    let out = Command::new("docker")
        .args(["restart", container])
        .output()
        .with_context(|| format!("docker restart {container}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "docker restart {container} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}

/// After a container restart the `/api/health` route returns 200 before the
/// runtime harness has finished connecting to LM Studio. Calling `/api/chat`
/// in that window yields a successful HTTP 200 with an empty `response`
/// field — a silent failure that looked like a real-but-instant model reply
/// in the first recorded run. The warmup posts a trivial message and waits
/// for a non-empty reply before we start measuring real tasks.
async fn warmup_sera(client: &reqwest::Client, endpoint: &str, api_key: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(90);
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match call_sera_chat(client, endpoint, api_key, "ping").await {
            Ok(reply) if !reply.trim().is_empty() => {
                eprintln!("  (warmup ok after {attempt} attempt(s))");
                return Ok(());
            }
            Ok(_) | Err(_) => {
                if Instant::now() > deadline {
                    return Err(anyhow!(
                        "harness warmup did not produce a non-empty reply within 90s"
                    ));
                }
                tokio::time::sleep(Duration::from_millis(1500)).await;
            }
        }
    }
}

async fn wait_for_sera(client: &reqwest::Client, endpoint: &str) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(resp) = client
            .get(format!("{endpoint}/api/health"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            && resp.status().is_success()
        {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(anyhow!(
                "SERA gateway did not become ready at {endpoint} within 60s"
            ));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

fn repo_root() -> Result<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("git rev-parse --show-toplevel")?;
    if !out.status.success() {
        return Err(anyhow!("git rev-parse failed"));
    }
    Ok(PathBuf::from(String::from_utf8(out.stdout)?.trim()))
}

fn git_sha(root: &Path) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()?;
    if !out.status.success() {
        return Err(anyhow!("git rev-parse HEAD failed"));
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

fn hostname() -> Result<String> {
    let out = Command::new("hostname").output()?;
    if !out.status.success() {
        return Err(anyhow!("hostname failed"));
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_string())
}

async fn raw_pipeline(
    client: &reqwest::Client,
    task: &TaskDef,
) -> Result<(RunOutcome, ScoringOutcome)> {
    let outcome = run_raw(client, task).await?;
    run_and_score(client, task, outcome).await
}

async fn full_pipeline(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
    task: &TaskDef,
) -> Result<(RunOutcome, ScoringOutcome)> {
    let outcome = run_full(client, endpoint, api_key, task).await?;
    run_and_score(client, task, outcome).await
}
