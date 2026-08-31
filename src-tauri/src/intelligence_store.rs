//! Cross-run intelligence: a local SQLite store aggregating run outcomes and
//! human interventions across all pairs, plus heuristic task tagging and
//! query commands that power the dashboard insights panel and creation-time
//! recommendations.
//!
//! Everything in this module degrades silently: DB open/corruption failures
//! are logged to stderr and produce empty results — never a crash, and writes
//! never block the run-completion path.

use crate::session_snapshot::{ensure_snapshot_dir, read_json, SessionSnapshotRecord, INDEX_FILE_NAME};
use crate::types::{
    AcceptanceRecord, AcceptanceRisk, AcceptanceVerdictDecision, Message, TurnTokenUsage,
};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const DB_FILE_NAME: &str = "intelligence.db";

/// Minimum number of runs a combo must have (among tag-overlapping runs)
/// before it can be recommended.
pub const MIN_RECOMMENDATION_SAMPLE: u64 = 2;

const MAX_RECOMMENDATIONS: usize = 3;

#[derive(Debug, Clone)]
pub struct RunRecord {
    pub run_id: String,
    pub pair_id: String,
    pub finished_at: u64,
    pub mentor_model: String,
    pub executor_model: String,
    pub provider_kind: String,
    /// "accept" / "reject" / None when no verdict was produced.
    pub verdict: Option<String>,
    /// "low" / "medium" / "high" / None.
    pub risk: Option<String>,
    pub iterations: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub duration_ms: u64,
    /// Comma-separated heuristic tags (see `tag_task`).
    pub task_tags: String,
    /// Comma-separated skill ids referenced by the task spec.
    pub skills: String,
    /// Stable hash of the workspace path (never the path itself).
    pub workspace_hash: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigStat {
    pub mentor_model: String,
    pub executor_model: String,
    pub provider_kind: String,
    pub runs: u64,
    pub successes: u64,
    pub success_rate: f64,
    pub avg_iterations: f64,
    pub avg_tokens: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InsightsSummary {
    pub total_runs: u64,
    pub successful_runs: u64,
    pub success_rate: f64,
    pub combos: Vec<ConfigStat>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigRecommendation {
    pub mentor_model: String,
    pub executor_model: String,
    pub runs: u64,
    pub successes: u64,
    pub success_rate: f64,
}

pub struct IntelligenceStore {
    conn: Connection,
}

impl IntelligenceStore {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create DB dir: {}", e))?;
        }
        let conn = Connection::open(path).map_err(|e| format!("Failed to open DB: {}", e))?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    fn init_schema(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS runs (
                    run_id TEXT PRIMARY KEY,
                    pair_id TEXT NOT NULL,
                    finished_at INTEGER NOT NULL,
                    mentor_model TEXT NOT NULL,
                    executor_model TEXT NOT NULL,
                    provider_kind TEXT NOT NULL,
                    verdict TEXT,
                    risk TEXT,
                    iterations INTEGER NOT NULL,
                    input_tokens INTEGER NOT NULL,
                    output_tokens INTEGER NOT NULL,
                    duration_ms INTEGER NOT NULL,
                    task_tags TEXT NOT NULL,
                    skills TEXT NOT NULL,
                    workspace_hash TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS interventions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id TEXT,
                    pair_id TEXT NOT NULL,
                    ts INTEGER NOT NULL,
                    kind TEXT NOT NULL,
                    outcome TEXT
                );",
            )
            .map_err(|e| format!("Failed to init intelligence schema: {}", e))
    }

    /// Idempotent by `run_id` — re-recording the same run replaces the row.
    pub fn record_run(&self, run: &RunRecord) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO runs (
                    run_id, pair_id, finished_at, mentor_model, executor_model,
                    provider_kind, verdict, risk, iterations, input_tokens,
                    output_tokens, duration_ms, task_tags, skills, workspace_hash
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    run.run_id,
                    run.pair_id,
                    run.finished_at as i64,
                    run.mentor_model,
                    run.executor_model,
                    run.provider_kind,
                    run.verdict,
                    run.risk,
                    run.iterations,
                    run.input_tokens as i64,
                    run.output_tokens as i64,
                    run.duration_ms as i64,
                    run.task_tags,
                    run.skills,
                    run.workspace_hash,
                ],
            )
            .map_err(|e| format!("Failed to record run: {}", e))?;
        Ok(())
    }

    pub fn record_intervention(
        &self,
        run_id: Option<&str>,
        pair_id: &str,
        kind: &str,
        outcome: Option<&str>,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO interventions (run_id, pair_id, ts, kind, outcome)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![run_id, pair_id, crate::util::now_millis() as i64, kind, outcome],
            )
            .map_err(|e| format!("Failed to record intervention: {}", e))?;
        Ok(())
    }

    pub fn run_count(&self) -> Result<u64, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM runs", [], |row| row.get::<_, i64>(0))
            .map(|count| count as u64)
            .map_err(|e| format!("Failed to count runs: {}", e))
    }

    pub fn insights_summary(&self) -> Result<InsightsSummary, String> {
        let (total_runs, successful_runs): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(CASE WHEN verdict = 'accept' THEN 1 ELSE 0 END), 0)
                 FROM runs",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| format!("Failed to summarize runs: {}", e))?;

        let mut stmt = self
            .conn
            .prepare(
                "SELECT mentor_model, executor_model, provider_kind,
                        COUNT(*) AS runs,
                        SUM(CASE WHEN verdict = 'accept' THEN 1 ELSE 0 END) AS successes,
                        AVG(iterations),
                        AVG(input_tokens + output_tokens)
                 FROM runs
                 GROUP BY mentor_model, executor_model, provider_kind
                 ORDER BY successes DESC, runs DESC",
            )
            .map_err(|e| format!("Failed to prepare combo query: {}", e))?;

        let combos = stmt
            .query_map([], |row| {
                let runs: i64 = row.get(3)?;
                let successes: i64 = row.get(4)?;
                Ok(ConfigStat {
                    mentor_model: row.get(0)?,
                    executor_model: row.get(1)?,
                    provider_kind: row.get(2)?,
                    runs: runs as u64,
                    successes: successes as u64,
                    success_rate: rate(successes, runs),
                    avg_iterations: row.get::<_, f64>(5)?,
                    avg_tokens: row.get::<_, f64>(6)?,
                })
            })
            .map_err(|e| format!("Failed to query combos: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read combos: {}", e))?;

        Ok(InsightsSummary {
            total_runs: total_runs as u64,
            successful_runs: successful_runs as u64,
            success_rate: rate(successful_runs, total_runs),
            combos,
        })
    }

    /// Rank combos by success rate among runs whose task tags overlap the
    /// query text's tags. Combos need at least `MIN_RECOMMENDATION_SAMPLE`
    /// matching runs; returns empty when nothing qualifies.
    pub fn recommend(&self, task_text: &str) -> Result<Vec<ConfigRecommendation>, String> {
        let skills = extract_skill_ids(task_text);
        let query_tags = tag_task(task_text, &skills, &[]);
        if query_tags.is_empty() {
            return Ok(Vec::new());
        }
        let query_set: BTreeSet<&str> = query_tags.iter().map(String::as_str).collect();

        let mut stmt = self
            .conn
            .prepare("SELECT mentor_model, executor_model, verdict, task_tags FROM runs")
            .map_err(|e| format!("Failed to prepare recommendation query: {}", e))?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| format!("Failed to query runs: {}", e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to read runs: {}", e))?;

        Ok(rank_recommendations(&rows, &query_set))
    }

    /// One-time backfill: scan per-pair snapshot JSON files (same pattern as
    /// `recent_activity.rs`) and insert historical runs. Only runs when the
    /// runs table is empty; INSERT OR REPLACE keeps it idempotent.
    pub fn backfill_from_snapshot_dir(&self, snapshot_dir: &Path) -> Result<u64, String> {
        if self.run_count()? > 0 {
            return Ok(0);
        }

        let entries = match fs::read_dir(snapshot_dir) {
            Ok(entries) => entries,
            Err(e) => return Err(format!("Failed to read snapshot dir: {}", e)),
        };

        let mut inserted = 0u64;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|name| name.to_str()) == Some(INDEX_FILE_NAME) {
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }

            let snapshot: SessionSnapshotRecord = match read_json(&path) {
                Ok(snapshot) => snapshot,
                Err(err) => {
                    eprintln!(
                        "[intelligence_store] Skipping unreadable snapshot {:?}: {}",
                        path, err
                    );
                    continue;
                }
            };

            let provider_kind = snapshot
                .mentor_provider
                .map(|kind| format!("{:?}", kind).to_lowercase())
                .unwrap_or_else(|| {
                    format!(
                        "{:?}",
                        crate::provider_adapter::ProviderAdapter::infer_provider_kind(
                            &snapshot.mentor_model
                        )
                    )
                    .to_lowercase()
                });

            for run in &snapshot.run_history {
                let record = run_record_from_summary(&snapshot, run, &provider_kind);
                if let Err(err) = self.record_run(&record) {
                    eprintln!(
                        "[intelligence_store] Failed to backfill run {}: {}",
                        record.run_id, err
                    );
                    continue;
                }
                inserted += 1;
            }
        }

        Ok(inserted)
    }
}

fn rate(successes: i64, runs: i64) -> f64 {
    if runs <= 0 {
        0.0
    } else {
        successes as f64 / runs as f64
    }
}

fn rank_recommendations(
    rows: &[(String, String, Option<String>, String)],
    query_tags: &BTreeSet<&str>,
) -> Vec<ConfigRecommendation> {
    struct ComboAcc {
        runs: u64,
        successes: u64,
    }

    let mut combos: HashMap<(String, String), ComboAcc> = HashMap::new();
    for (mentor_model, executor_model, verdict, task_tags) in rows {
        let overlaps = task_tags
            .split(',')
            .filter(|tag| !tag.is_empty())
            .any(|tag| query_tags.contains(tag));
        if !overlaps {
            continue;
        }
        let acc = combos
            .entry((mentor_model.clone(), executor_model.clone()))
            .or_insert(ComboAcc {
                runs: 0,
                successes: 0,
            });
        acc.runs += 1;
        if verdict.as_deref() == Some("accept") {
            acc.successes += 1;
        }
    }

    let mut recommendations: Vec<ConfigRecommendation> = combos
        .into_iter()
        .filter(|(_, acc)| acc.runs >= MIN_RECOMMENDATION_SAMPLE)
        .map(|((mentor_model, executor_model), acc)| ConfigRecommendation {
            mentor_model,
            executor_model,
            runs: acc.runs,
            successes: acc.successes,
            success_rate: rate(acc.successes as i64, acc.runs as i64),
        })
        .collect();

    recommendations.sort_by(|a, b| {
        b.success_rate
            .partial_cmp(&a.success_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.runs.cmp(&a.runs))
    });
    recommendations.truncate(MAX_RECOMMENDATIONS);
    recommendations
}

// ---------------------------------------------------------------------------
// Heuristic task tagging (pure functions, unit-testable)
// ---------------------------------------------------------------------------

const TAG_KEYWORDS: &[(&str, &[&str])] = &[
    (
        "fix",
        &[
            "fix", "bug", "broken", "crash", "error", "regression", "hotfix", "repair",
        ],
    ),
    (
        "feat",
        &["add", "feature", "implement", "create", "support", "introduce", "build"],
    ),
    (
        "refactor",
        &["refactor", "cleanup", "simplify", "rename", "restructure", "dedupe"],
    ),
    ("test", &["test", "coverage", "e2e"]),
    (
        "docs",
        &["doc", "docs", "documentation", "readme", "changelog"],
    ),
    (
        "chore",
        &["chore", "bump", "dependency", "dependencies", "upgrade", "lint", "format"],
    ),
];

/// Derive heuristic tags for a task: keyword categories from the spec text,
/// `skill:<id>` for each referenced skill, and `ext:<ext>` for each file
/// extension touched by the run's diff. Deterministic ordering, deduplicated.
pub fn tag_task(spec: &str, skill_ids: &[String], file_paths: &[String]) -> Vec<String> {
    let lower = spec.to_lowercase();
    let tokens: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();

    let mut tags = Vec::new();
    for (tag, keywords) in TAG_KEYWORDS {
        let matched = keywords.iter().any(|keyword| {
            tokens
                .iter()
                .any(|token| *token == *keyword || token.starts_with(keyword))
        });
        if matched {
            tags.push((*tag).to_string());
        }
    }

    for skill_id in skill_ids {
        let tag = format!("skill:{}", skill_id);
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }

    let mut extensions = BTreeSet::new();
    for path in file_paths {
        if let Some(ext) = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase())
        {
            if !ext.is_empty() && ext.len() <= 6 && ext.chars().all(|c| c.is_alphanumeric()) {
                extensions.insert(ext);
            }
        }
    }
    for ext in extensions {
        tags.push(format!("ext:{}", ext));
    }

    tags
}

/// Extract skill ids referenced by a task spec. The SkillPicker inserts
/// "Load the <id> skill and …" into the spec; match that pattern.
pub fn extract_skill_ids(spec: &str) -> Vec<String> {
    let lower = spec.to_lowercase();
    let tokens: Vec<&str> = lower.split_whitespace().collect();
    let mut skills = Vec::new();
    for window in tokens.windows(4) {
        if window[0] == "load" && window[1] == "the" && window[3].starts_with("skill") {
            let id = window[2].trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
            if !id.is_empty() && !skills.contains(&id.to_string()) {
                skills.push(id.to_string());
            }
        }
    }
    skills
}

/// Stable, privacy-preserving hash of a workspace path (FNV-1a 64-bit, hex).
pub fn workspace_hash(path: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in path.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{:016x}", hash)
}

fn verdict_strings(acceptance: Option<&AcceptanceRecord>) -> (Option<String>, Option<String>) {
    match acceptance {
        Some(record) => {
            let verdict = record.verdict.as_ref().map(|v| match v.verdict {
                AcceptanceVerdictDecision::Pass => "accept".to_string(),
                AcceptanceVerdictDecision::Fail => "reject".to_string(),
            });
            let risk = Some(
                match record.risk {
                    AcceptanceRisk::Low => "low",
                    AcceptanceRisk::Medium => "medium",
                    AcceptanceRisk::High => "high",
                }
                .to_string(),
            );
            (verdict, risk)
        }
        None => (None, None),
    }
}

fn sum_token_usage(messages: &[Message]) -> (u64, u64) {
    let mut input = 0u64;
    let mut output = 0u64;
    for message in messages {
        if let Some(TurnTokenUsage {
            input_tokens,
            output_tokens,
            ..
        }) = message.token_usage.as_ref()
        {
            input += input_tokens.unwrap_or(0);
            output += output_tokens;
        }
    }
    (input, output)
}

/// Build a `RunRecord` from the live broker state at run completion. Called by
/// the process spawner's run-completion path.
pub fn build_run_record(
    pair_id: &str,
    started_at: u64,
    finished_at: u64,
    mentor_model: &str,
    executor_model: &str,
    provider_kind: &str,
    task_spec: &str,
    state: &crate::types::PairState,
) -> RunRecord {
    let skills = extract_skill_ids(task_spec);
    let file_paths: Vec<String> = state
        .modified_files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    let (input_tokens, output_tokens) = sum_token_usage(&state.messages);
    let (verdict, risk) = verdict_strings(state.latest_acceptance.as_ref());

    RunRecord {
        run_id: format!("{}-run@{}", pair_id, started_at),
        pair_id: pair_id.to_string(),
        finished_at,
        mentor_model: mentor_model.to_string(),
        executor_model: executor_model.to_string(),
        provider_kind: provider_kind.to_string(),
        verdict,
        risk,
        iterations: state.iteration,
        input_tokens,
        output_tokens,
        duration_ms: finished_at.saturating_sub(started_at),
        task_tags: tag_task(task_spec, &skills, &file_paths).join(","),
        skills: skills.join(","),
        workspace_hash: workspace_hash(&state.directory),
    }
}

fn run_record_from_summary(
    snapshot: &SessionSnapshotRecord,
    run: &crate::session_snapshot::SnapshotRunSummary,
    provider_kind: &str,
) -> RunRecord {
    let skills = extract_skill_ids(&run.spec);
    let (input_tokens, output_tokens) = sum_token_usage(&run.messages);
    let (verdict, risk) = verdict_strings(run.latest_acceptance.as_ref());
    let finished_at = run.finished_at.unwrap_or(run.started_at);

    RunRecord {
        run_id: run.id.clone(),
        pair_id: snapshot.pair_id.clone(),
        finished_at,
        mentor_model: run.mentor_model.clone(),
        executor_model: run.executor_model.clone(),
        provider_kind: provider_kind.to_string(),
        verdict,
        risk,
        iterations: run.iterations,
        input_tokens,
        output_tokens,
        duration_ms: finished_at.saturating_sub(run.started_at),
        task_tags: tag_task(&run.spec, &skills, &[]).join(","),
        skills: skills.join(","),
        workspace_hash: workspace_hash(&snapshot.directory),
    }
}

// ---------------------------------------------------------------------------
// App-facing helpers — all failures are logged to stderr and swallowed.
// ---------------------------------------------------------------------------

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {}", e))?;
    dir.push(DB_FILE_NAME);
    Ok(dir)
}

fn open_for_app(app: &AppHandle) -> Result<IntelligenceStore, String> {
    IntelligenceStore::open(&db_path(app)?)
}

pub fn record_run_safely(app: &AppHandle, run: &RunRecord) {
    match open_for_app(app).and_then(|store| store.record_run(run)) {
        Ok(()) => {}
        Err(err) => eprintln!("[intelligence_store] Failed to record run: {}", err),
    }
}

pub fn record_intervention_safely(app: &AppHandle, pair_id: &str, kind: &str, outcome: Option<&str>) {
    match open_for_app(app).and_then(|store| store.record_intervention(None, pair_id, kind, outcome))
    {
        Ok(()) => {}
        Err(err) => eprintln!("[intelligence_store] Failed to record intervention: {}", err),
    }
}

/// Backfill historical runs from per-pair snapshots once at startup. Only
/// inserts when the runs table is empty.
pub fn backfill_if_empty(app: &AppHandle) {
    let result = open_for_app(app).and_then(|store| {
        let snapshot_dir = ensure_snapshot_dir(app)?;
        store.backfill_from_snapshot_dir(&snapshot_dir)
    });
    match result {
        Ok(inserted) if inserted > 0 => {
            println!(
                "[intelligence_store] Backfilled {} historical runs",
                inserted
            );
        }
        Ok(_) => {}
        Err(err) => eprintln!("[intelligence_store] Backfill failed: {}", err),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands — never error out to the frontend; empty results on failure.
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_insights_summary(app: AppHandle) -> InsightsSummary {
    match open_for_app(&app).and_then(|store| store.insights_summary()) {
        Ok(summary) => summary,
        Err(err) => {
            eprintln!("[intelligence_store] get_insights_summary failed: {}", err);
            InsightsSummary::default()
        }
    }
}

#[tauri::command]
pub fn get_recommendation(app: AppHandle, task_text: String) -> Vec<ConfigRecommendation> {
    match open_for_app(&app).and_then(|store| store.recommend(&task_text)) {
        Ok(recommendations) => recommendations,
        Err(err) => {
            eprintln!("[intelligence_store] get_recommendation failed: {}", err);
            Vec::new()
        }
    }
}

#[tauri::command]
pub fn record_intervention(
    app: AppHandle,
    pair_id: String,
    kind: String,
    outcome: Option<String>,
) {
    record_intervention_safely(&app, &pair_id, &kind, outcome.as_deref());
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(test_name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "the-pair-intel-test-{}-{}",
            test_name,
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir.join(DB_FILE_NAME)
    }

    fn sample_run(run_id: &str, mentor: &str, executor: &str, verdict: Option<&str>) -> RunRecord {
        RunRecord {
            run_id: run_id.to_string(),
            pair_id: "pair-1".to_string(),
            finished_at: 1_700_000_000_000,
            mentor_model: mentor.to_string(),
            executor_model: executor.to_string(),
            provider_kind: "claude".to_string(),
            verdict: verdict.map(String::from),
            risk: Some("low".to_string()),
            iterations: 4,
            input_tokens: 1000,
            output_tokens: 500,
            duration_ms: 60_000,
            task_tags: "fix,ext:rs".to_string(),
            skills: String::new(),
            workspace_hash: workspace_hash("/tmp/workspace"),
        }
    }

    #[test]
    fn record_run_is_idempotent() {
        let path = temp_db_path("idempotent");
        let store = IntelligenceStore::open(&path).unwrap();

        store.record_run(&sample_run("run-1", "claude-a", "codex-b", Some("accept"))).unwrap();
        store.record_run(&sample_run("run-1", "claude-a", "codex-b", Some("accept"))).unwrap();

        assert_eq!(store.run_count().unwrap(), 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn insights_summary_aggregates_per_combo() {
        let path = temp_db_path("aggregation");
        let store = IntelligenceStore::open(&path).unwrap();

        store.record_run(&sample_run("run-1", "claude-a", "codex-b", Some("accept"))).unwrap();
        store.record_run(&sample_run("run-2", "claude-a", "codex-b", Some("reject"))).unwrap();
        store.record_run(&sample_run("run-3", "kimi-x", "codex-b", Some("accept"))).unwrap();

        let summary = store.insights_summary().unwrap();
        assert_eq!(summary.total_runs, 3);
        assert_eq!(summary.successful_runs, 2);
        assert!((summary.success_rate - 2.0 / 3.0).abs() < 1e-9);
        assert_eq!(summary.combos.len(), 2);

        let combo_a = summary
            .combos
            .iter()
            .find(|c| c.mentor_model == "claude-a")
            .unwrap();
        assert_eq!(combo_a.runs, 2);
        assert_eq!(combo_a.successes, 1);
        assert!((combo_a.success_rate - 0.5).abs() < 1e-9);
        assert!((combo_a.avg_iterations - 4.0).abs() < 1e-9);
        assert!((combo_a.avg_tokens - 1500.0).abs() < 1e-9);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn recommendation_ranks_by_success_rate_with_min_sample() {
        let path = temp_db_path("recommendation");
        let store = IntelligenceStore::open(&path).unwrap();

        // Combo A: 3 runs, 2 accepts (tagged fix)
        for (id, verdict) in [
            ("a1", Some("accept")),
            ("a2", Some("accept")),
            ("a3", Some("reject")),
        ] {
            store.record_run(&sample_run(id, "mentor-a", "exec-a", verdict)).unwrap();
        }
        // Combo B: 2 runs, 2 accepts (tagged fix) — higher success rate, fewer runs
        store.record_run(&sample_run("b1", "mentor-b", "exec-b", Some("accept"))).unwrap();
        store.record_run(&sample_run("b2", "mentor-b", "exec-b", Some("accept"))).unwrap();
        // Combo C: only 1 run — below the minimum sample
        store.record_run(&sample_run("c1", "mentor-c", "exec-c", Some("accept"))).unwrap();
        // Combo D: enough runs but non-overlapping tags
        let mut other = sample_run("d1", "mentor-d", "exec-d", Some("accept"));
        other.task_tags = "docs".to_string();
        store.record_run(&other).unwrap();
        other.run_id = "d2".to_string();
        store.record_run(&other).unwrap();

        let recs = store.recommend("fix the broken parser").unwrap();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].mentor_model, "mentor-b");
        assert_eq!(recs[0].runs, 2);
        assert!((recs[0].success_rate - 1.0).abs() < 1e-9);
        assert_eq!(recs[1].mentor_model, "mentor-a");
        assert!((recs[1].success_rate - 2.0 / 3.0).abs() < 1e-9);

        // No overlap → empty
        assert!(store.recommend("polish the painting").unwrap().is_empty());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn record_and_read_interventions() {
        let path = temp_db_path("interventions");
        let store = IntelligenceStore::open(&path).unwrap();

        store
            .record_intervention(None, "pair-1", "plan_approved", None)
            .unwrap();
        store
            .record_intervention(None, "pair-1", "retry", Some("verdict parse failed"))
            .unwrap();

        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM interventions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn backfill_imports_snapshot_run_history() {
        let root = std::env::temp_dir().join(format!(
            "the-pair-intel-backfill-{}",
            uuid::Uuid::new_v4()
        ));
        let snapshot_dir = root.join("pair-snapshots");
        fs::create_dir_all(&snapshot_dir).unwrap();

        // Minimal snapshot fixture: only the fields the backfill reads matter,
        // but the record must deserialize, so write a full JSON document.
        // (Raw string instead of serde_json::json! — the big literal exceeds
        // the macro recursion limit.)
        let fixture = r#"{
            "snapshotVersion": 2,
            "savedAt": 1700000100000,
            "pairId": "pair-backfill",
            "name": "Backfill Pair",
            "directory": "/tmp/backfill-workspace",
            "spec": "fix the broken login",
            "status": "finished",
            "iterations": 5,
            "maxIterations": 10,
            "turn": "mentor",
            "mentorProvider": "claude",
            "mentorModel": "claude-sonnet",
            "executorProvider": "codex",
            "executorModel": "gpt-5-codex",
            "pendingMentorModel": null,
            "pendingExecutorModel": null,
            "mentorReasoningEffort": null,
            "executorReasoningEffort": null,
            "messages": [],
            "mentorActivity": {
                "phase": "idle", "label": "Idle", "startedAt": 0, "updatedAt": 0, "outputLineCount": 0
            },
            "executorActivity": {
                "phase": "idle", "label": "Idle", "startedAt": 0, "updatedAt": 0, "outputLineCount": 0
            },
            "mentorCpu": 0.0,
            "mentorMemMb": 0.0,
            "executorCpu": 0.0,
            "executorMemMb": 0.0,
            "cpuUsage": 0.0,
            "memUsage": 0.0,
            "modifiedFiles": [],
            "gitTracking": { "available": false },
            "automationMode": "full-auto",
            "acceptanceHistory": [],
            "currentTurnCard": null,
            "runCount": 2,
            "runHistory": [
                {
                    "id": "pair-backfill-run-1",
                    "spec": "fix the broken login",
                    "status": "finished",
                    "startedAt": 1700000000000,
                    "finishedAt": 1700000060000,
                    "mentorModel": "claude-sonnet",
                    "executorModel": "gpt-5-codex",
                    "iterations": 5,
                    "messages": [
                        {
                            "id": "m1",
                            "timestamp": 1700000001000,
                            "from": "mentor",
                            "to": "executor",
                            "type": "plan",
                            "content": "plan",
                            "iteration": 1,
                            "tokenUsage": {
                                "outputTokens": 200,
                                "inputTokens": 800,
                                "lastUpdatedAt": 1700000001000,
                                "source": "final",
                                "provider": null
                            }
                        }
                    ],
                    "totalOutputTokens": 200,
                    "latestAcceptance": {
                        "iteration": 5,
                        "risk": "low",
                        "checks": [],
                        "summary": "ok",
                        "startedAt": 1700000050000,
                        "finishedAt": 1700000060000,
                        "verdict": {
                            "verdict": "pass",
                            "risk": "low",
                            "confidence": 0.9,
                            "issues": [],
                            "evidence": [],
                            "reasoning": "looks good",
                            "summary": "done",
                            "nextStep": { "action": "finish", "instructions": [] }
                        }
                    }
                },
                {
                    "id": "pair-backfill-run-2",
                    "spec": "fix the settings page crash",
                    "status": "finished",
                    "startedAt": 1700000100000,
                    "finishedAt": 1700000160000,
                    "mentorModel": "claude-sonnet",
                    "executorModel": "gpt-5-codex",
                    "iterations": 3,
                    "messages": [],
                    "latestAcceptance": null
                }
            ],
            "currentRunStartedAt": 1700000100000,
            "currentRunFinishedAt": 1700000160000,
            "createdAt": 1699000000000,
            "providerSessions": {
                "mentorSessionId": null,
                "executorSessionId": null,
                "runGeneration": 0,
                "isSmokeTest": false
            },
            "branch": null,
            "repoPath": null,
            "worktreePath": null,
            "planGate": false
        }"#;
        fs::write(snapshot_dir.join("pair-backfill.json"), fixture).unwrap();
        // Index file must be skipped by the scan.
        fs::write(snapshot_dir.join(INDEX_FILE_NAME), "[]").unwrap();

        let store = IntelligenceStore::open(&root.join(DB_FILE_NAME)).unwrap();
        let inserted = store.backfill_from_snapshot_dir(&snapshot_dir).unwrap();
        assert_eq!(inserted, 2);
        assert_eq!(store.run_count().unwrap(), 2);

        // Second backfill is a no-op (guard: only when empty).
        let inserted_again = store.backfill_from_snapshot_dir(&snapshot_dir).unwrap();
        assert_eq!(inserted_again, 0);

        let summary = store.insights_summary().unwrap();
        assert_eq!(summary.total_runs, 2);
        assert_eq!(summary.successful_runs, 1);
        let combo = &summary.combos[0];
        assert_eq!(combo.mentor_model, "claude-sonnet");
        assert_eq!(combo.executor_model, "gpt-5-codex");
        assert_eq!(combo.provider_kind, "claude");
        assert_eq!(combo.runs, 2);
        assert!((combo.avg_tokens - 500.0).abs() < 1e-9); // (1000 + 0) / 2

        // Backfilled tags feed recommendations.
        let recs = store.recommend("fix the crash in the signup flow").unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].mentor_model, "claude-sonnet");
        assert!((recs[0].success_rate - 0.5).abs() < 1e-9);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tag_task_detects_keywords_skills_and_extensions() {
        let tags = tag_task(
            "Fix the broken tests and update the README",
            &["brainstorming".to_string()],
            &["src/main.rs".to_string(), "README.md".to_string()],
        );
        assert!(tags.contains(&"fix".to_string()));
        assert!(tags.contains(&"test".to_string()));
        assert!(tags.contains(&"docs".to_string()));
        assert!(tags.contains(&"skill:brainstorming".to_string()));
        assert!(tags.contains(&"ext:rs".to_string()));
        assert!(tags.contains(&"ext:md".to_string()));
        assert!(!tags.contains(&"feat".to_string()));
    }

    #[test]
    fn tag_task_matches_word_stems_not_substrings() {
        // "address" must not trigger the "add" keyword via substring matching —
        // but it does start with "add", so check a genuinely unrelated word.
        let tags = tag_task("polish the hallway mirror", &[], &[]);
        assert!(tags.is_empty());

        let tags = tag_task("adding a new feature", &[], &[]);
        assert!(tags.contains(&"feat".to_string()));
    }

    #[test]
    fn extract_skill_ids_parses_skill_picker_insertions() {
        let skills = extract_skill_ids("Load the brainstorming skill and fix the bug");
        assert_eq!(skills, vec!["brainstorming".to_string()]);
        assert!(extract_skill_ids("no skills here").is_empty());
    }

    #[test]
    fn workspace_hash_is_stable_and_hides_path() {
        let hash = workspace_hash("/Users/alice/secret-project");
        assert_eq!(hash, workspace_hash("/Users/alice/secret-project"));
        assert_ne!(hash, workspace_hash("/Users/alice/other-project"));
        assert!(!hash.contains("alice"));
    }
}
