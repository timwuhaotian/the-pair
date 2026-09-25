use crate::git_tracker::GitTracker;
use crate::types::{
    AcceptanceCheckRun, AcceptanceCheckStatus, AcceptanceNextAction, AcceptanceNextStep,
    AcceptanceRecord, AcceptanceRisk, AcceptanceVerdict, AcceptanceVerdictDecision, ModifiedFile,
};
use crate::util::now_millis;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::task::JoinHandle;

/// `git diff --check` is quick; anything slower than this is stuck.
const GIT_CHECK_TIMEOUT: Duration = Duration::from_secs(60);
/// Budget for `npm run typecheck` / `npm run test`. With `CI=true` watch-mode
/// runners (CRA, Jest, Vitest, Karma) exit on their own; this bounds anything
/// that still hangs so the pair can't wait forever.
const SCRIPT_CHECK_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// After a check exits (or is killed), how long to wait for its output pipes
/// to close — a background process it spawned may hold them open.
const OUTPUT_DRAIN_GRACE: Duration = Duration::from_secs(5);
/// Tail of stdout/stderr kept per check while it runs.
const MAX_CAPTURED_BYTES: usize = 64 * 1024;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Parses and validates a mentor review verdict through the quality gate.
///
/// The structured evidence format (`FILES_REVIEWED:` / `CHECKS:` / `CODE:`
/// lines) is optional — no prompt asks for it — so it is only enforced when
/// the mentor deliberately used all three markers at the start of a line.
/// Incidental text such as "EXIT CODE: 1" or "AUTOMATED CHECKS: 3 passed"
/// never causes a valid JSON verdict to be rejected.
pub fn parse_review_verdict_with_quality(raw: &str) -> Result<AcceptanceVerdict, String> {
    if let Some(evidence) = crate::quality_gate::extract_evidence(raw) {
        if let crate::quality_gate::QualityGateResult::Fail { reason } =
            crate::quality_gate::validate_review(&evidence)
        {
            return Err(format!("Review quality gate: {}", reason));
        }
    }
    parse_acceptance_verdict(raw)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AcceptanceCheckPlan {
    name: String,
    command: String,
    program: String,
    args: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LooseAcceptanceVerdict {
    verdict: AcceptanceVerdictDecision,
    risk: AcceptanceRisk,
    confidence: Option<f64>,
    #[serde(default)]
    issues: Vec<String>,
    evidence: Vec<String>,
    reasoning: Option<String>,
    summary: String,
    #[serde(alias = "next_step", alias = "next-step")]
    next_step: AcceptanceNextStep,
}

impl LooseAcceptanceVerdict {
    fn into_verdict(self) -> AcceptanceVerdict {
        let confidence = self.confidence.unwrap_or(match self.verdict {
            AcceptanceVerdictDecision::Pass => 1.0,
            AcceptanceVerdictDecision::Fail => 0.0,
        });
        let reasoning = self
            .reasoning
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.summary.clone());

        AcceptanceVerdict {
            verdict: self.verdict,
            risk: self.risk,
            confidence,
            issues: self.issues,
            evidence: self.evidence,
            reasoning,
            summary: self.summary,
            next_step: self.next_step,
        }
    }
}

impl AcceptanceCheckPlan {
    fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        let program = program.into();
        let command = std::iter::once(program.clone())
            .chain(args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");

        Self {
            name: command.clone(),
            command,
            program,
            args,
        }
    }
}

fn trim_output(text: &str, max_chars: usize) -> String {
    let trimmed = text.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= max_chars {
        return trimmed.to_string();
    }
    chars[chars.len() - max_chars..].iter().collect()
}

fn parse_package_json(workspace_root: &Path) -> Option<Value> {
    let raw = fs::read_to_string(workspace_root.join("package.json")).ok()?;
    serde_json::from_str(&raw).ok()
}

fn package_scripts(package_json: Option<&Value>) -> Vec<String> {
    package_json
        .and_then(|value| value.get("scripts"))
        .and_then(|value| value.as_object())
        .map(|scripts| scripts.keys().cloned().collect())
        .unwrap_or_default()
}

fn completion_signal_present(executor_output: &str) -> bool {
    let lower = executor_output.to_lowercase();
    [
        "done",
        "complete",
        "completed",
        "implemented",
        "fixed",
        "ready for review",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub fn classify_acceptance_risk(modified_files: &[ModifiedFile]) -> AcceptanceRisk {
    let total_files = modified_files.len();
    let has_delete_or_rename = modified_files.iter().any(|file| {
        matches!(
            file.status,
            crate::types::FileStatus::D | crate::types::FileStatus::R
        )
    });
    let has_migrations_or_schema = modified_files.iter().any(|file| {
        let path = file.path.to_lowercase();
        path.contains("migration") || path.contains("migrations") || path.contains("schema")
    });

    let mut has_backend = false;
    for file in modified_files {
        let path = file.path.to_lowercase();
        if path.contains("src-tauri/") || path.ends_with(".rs") {
            has_backend = true;
        }
    }

    if has_delete_or_rename || has_migrations_or_schema || total_files >= 12 {
        return AcceptanceRisk::High;
    }

    if total_files >= 6 || has_backend {
        return AcceptanceRisk::Medium;
    }

    AcceptanceRisk::Low
}

fn should_add_full_test(
    risk: &AcceptanceRisk,
    executor_output: &str,
    iteration: u32,
    max_iterations: u32,
) -> bool {
    if matches!(risk, AcceptanceRisk::Medium | AcceptanceRisk::High) {
        return true;
    }
    if completion_signal_present(executor_output) {
        return true;
    }
    max_iterations > 0 && iteration.saturating_add(1) >= max_iterations.saturating_sub(1)
}

/// `diff_base` is `HEAD`, or the empty tree in a repository without commits;
/// diffing against it (instead of the index) also covers staged changes.
fn build_acceptance_check_plan(
    workspace_root: &str,
    package_json_override: Option<&Value>,
    modified_files: &[ModifiedFile],
    executor_output: &str,
    iteration: u32,
    max_iterations: u32,
    diff_base: &str,
) -> Vec<AcceptanceCheckPlan> {
    let workspace_path = Path::new(workspace_root);
    let package_json = package_json_override
        .cloned()
        .or_else(|| parse_package_json(workspace_path));
    let scripts = package_scripts(package_json.as_ref());
    let risk = classify_acceptance_risk(modified_files);

    let mut checks = Vec::new();

    // Only run code-quality checks when the executor actually modified files.
    // Text-only outputs (smoke greetings, diagnostic answers) have nothing to
    // verify, and running `git diff --check` against an unchanged tree just
    // adds noise — or worse, surfaces env-level failures (exit 129) that the
    // mentor then has to explain away.
    if !modified_files.is_empty() {
        checks.push(AcceptanceCheckPlan::new(
            "git",
            vec![
                "diff".to_string(),
                diff_base.to_string(),
                "--check".to_string(),
            ],
        ));

        if scripts.iter().any(|script| script == "typecheck") {
            checks.push(AcceptanceCheckPlan::new(
                "npm",
                vec!["run".to_string(), "typecheck".to_string()],
            ));
        }

        if scripts.iter().any(|script| script == "test")
            && should_add_full_test(&risk, executor_output, iteration, max_iterations)
        {
            checks.push(AcceptanceCheckPlan::new(
                "npm",
                vec!["run".to_string(), "test".to_string()],
            ));
        }
    }

    checks
}

/// Classifies whether a non-zero exit code from `git diff --check` indicates a
/// real whitespace problem or an environment-level anomaly (signal kill, not a
/// git repo, etc.). Returning true means the failure is genuine; false means
/// the check could not produce a meaningful answer and should be reported as
/// `Skipped` so it doesn't poison the mentor's review.
fn is_actionable_git_check_failure(exit_code: Option<i32>, stderr: &str) -> bool {
    let stderr_lower = stderr.to_lowercase();
    if stderr_lower.contains("not a git repository") {
        return false;
    }
    match exit_code {
        // Exit 1 / 2 are the canonical "found whitespace errors" codes.
        Some(1) | Some(2) => true,
        // Signal-killed (128 + signo) or other environment failures aren't
        // about the diff itself — treat as inconclusive.
        Some(code) if code > 128 => false,
        // Unknown status (e.g., process terminated without exit code) → skip.
        None => false,
        // Any other unexpected non-zero code: skip rather than surface as a
        // hard fail. If a real whitespace problem occurs, exit 1 covers it.
        Some(_) => false,
    }
}

fn check_timeout(check: &AcceptanceCheckPlan) -> Duration {
    if check.program == "git" {
        GIT_CHECK_TIMEOUT
    } else {
        SCRIPT_CHECK_TIMEOUT
    }
}

/// Rust's `Command` only appends `.exe` when searching `PATH` on Windows, so
/// npm's `npm.cmd` shim is never found under the bare name `npm`.
fn resolve_program(program: &str) -> String {
    if cfg!(windows) && program == "npm" {
        "npm.cmd".to_string()
    } else {
        program.to_string()
    }
}

fn kill_process_tree(pid: u32) {
    #[cfg(unix)]
    {
        // Checks run in their own process group (pgid == pid), so this also
        // stops the test runner / watcher processes npm started.
        let _ = std::process::Command::new("kill")
            .args(["-KILL", "--", &format!("-{}", pid)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// Kills a check's whole process tree when dropped while armed — on timeout,
/// or when the caller abandons the check future (e.g. a pause or delete that
/// cancels it) — so watch-mode runners don't outlive the check.
struct ProcessTreeGuard {
    pid: Option<u32>,
}

impl ProcessTreeGuard {
    fn kill_now(&mut self) {
        if let Some(pid) = self.pid.take() {
            kill_process_tree(pid);
        }
    }

    fn disarm(&mut self) {
        self.pid = None;
    }
}

impl Drop for ProcessTreeGuard {
    fn drop(&mut self) {
        self.kill_now();
    }
}

/// Reads `reader` to EOF, keeping only the last ~`MAX_CAPTURED_BYTES`.
async fn capture_tail<R>(mut reader: R, sink: Arc<StdMutex<Vec<u8>>>)
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let mut buffer = sink.lock().unwrap_or_else(|e| e.into_inner());
                buffer.extend_from_slice(&chunk[..read]);
                if buffer.len() > MAX_CAPTURED_BYTES * 2 {
                    let excess = buffer.len() - MAX_CAPTURED_BYTES;
                    buffer.drain(..excess);
                }
            }
        }
    }
}

/// Waits until `deadline` for the output readers. Finished readers are removed
/// (a completed `JoinHandle` must not be polled again); true when none remain.
async fn join_readers(readers: &mut Vec<JoinHandle<()>>, deadline: tokio::time::Instant) -> bool {
    let mut pending = Vec::new();
    for mut handle in readers.drain(..) {
        if tokio::time::timeout_at(deadline, &mut handle)
            .await
            .is_err()
        {
            pending.push(handle);
        }
    }
    *readers = pending;
    readers.is_empty()
}

fn take_captured(sink: &Arc<StdMutex<Vec<u8>>>) -> String {
    let bytes = std::mem::take(&mut *sink.lock().unwrap_or_else(|e| e.into_inner()));
    trim_output(&String::from_utf8_lossy(&bytes), 4_000)
}

async fn run_check(workspace_root: &Path, check: &AcceptanceCheckPlan) -> AcceptanceCheckRun {
    run_check_with_timeout(workspace_root, check, check_timeout(check)).await
}

async fn run_check_with_timeout(
    workspace_root: &Path,
    check: &AcceptanceCheckPlan,
    limit: Duration,
) -> AcceptanceCheckRun {
    let started = Instant::now();
    let mut command = Command::new(resolve_program(&check.program));
    command
        .args(&check.args)
        .current_dir(workspace_root)
        // Non-interactive runs: CRA / Jest / Vitest / Karma skip watch mode.
        .env("CI", "true")
        // Don't take index.lock away from the agents.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            // The tool isn't installed or not on PATH: the check is
            // inconclusive, not evidence against the executor's work.
            return AcceptanceCheckRun {
                name: check.name.clone(),
                command: check.command.clone(),
                status: AcceptanceCheckStatus::Skipped,
                exit_code: None,
                duration_ms: started.elapsed().as_millis() as u64,
                summary: format!("{} could not start", check.command),
                stdout: String::new(),
                stderr: error.to_string(),
            };
        }
    };
    let mut tree = ProcessTreeGuard { pid: child.id() };

    let stdout_sink = Arc::new(StdMutex::new(Vec::new()));
    let stderr_sink = Arc::new(StdMutex::new(Vec::new()));
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(tokio::spawn(capture_tail(stdout, stdout_sink.clone())));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(tokio::spawn(capture_tail(stderr, stderr_sink.clone())));
    }

    let wait_result = tokio::time::timeout(limit, child.wait()).await;
    if wait_result.is_err() {
        tree.kill_now();
        let _ = child.start_kill();
        let _ = tokio::time::timeout(OUTPUT_DRAIN_GRACE, child.wait()).await;
    }

    // A background process left behind by the check (same process group) can
    // keep the pipes open; stop it rather than stall the review.
    if !join_readers(
        &mut readers,
        tokio::time::Instant::now() + OUTPUT_DRAIN_GRACE,
    )
    .await
    {
        tree.kill_now();
        join_readers(
            &mut readers,
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await;
        for reader in &readers {
            reader.abort();
        }
    }
    tree.disarm();

    let stdout = take_captured(&stdout_sink);
    let stderr = take_captured(&stderr_sink);
    let duration_ms = started.elapsed().as_millis() as u64;

    let exit_status = match wait_result {
        Err(_elapsed) => {
            return AcceptanceCheckRun {
                name: check.name.clone(),
                command: check.command.clone(),
                status: AcceptanceCheckStatus::Failed,
                exit_code: None,
                duration_ms,
                summary: format!(
                    "{} timed out after {}s and was stopped",
                    check.command,
                    limit.as_secs()
                ),
                stdout,
                stderr,
            };
        }
        Ok(Err(error)) => {
            return AcceptanceCheckRun {
                name: check.name.clone(),
                command: check.command.clone(),
                status: AcceptanceCheckStatus::Skipped,
                exit_code: None,
                duration_ms,
                summary: format!("{} could not complete", check.command),
                stdout,
                stderr: if stderr.is_empty() {
                    error.to_string()
                } else {
                    stderr
                },
            };
        }
        Ok(Ok(status)) => status,
    };

    let success = exit_status.success();
    let exit_code = exit_status.code();

    // Special-case `git diff --check`: env/signal failures shouldn't be
    // reported as `failed`, only genuine whitespace errors should.
    let is_git_diff_check = check.program == "git"
        && check.args.first().map(|s| s.as_str()) == Some("diff")
        && check.args.iter().any(|s| s == "--check");

    let status = if success {
        AcceptanceCheckStatus::Passed
    } else if is_git_diff_check && !is_actionable_git_check_failure(exit_code, &stderr) {
        AcceptanceCheckStatus::Skipped
    } else {
        AcceptanceCheckStatus::Failed
    };

    let summary = match status {
        AcceptanceCheckStatus::Passed => format!("{} passed", check.command),
        AcceptanceCheckStatus::Skipped => {
            format!("{} skipped (no actionable diff)", check.command)
        }
        AcceptanceCheckStatus::Failed => format!("{} failed", check.command),
    };

    AcceptanceCheckRun {
        name: check.name.clone(),
        command: check.command.clone(),
        status,
        exit_code,
        duration_ms,
        summary,
        stdout,
        stderr,
    }
}

fn extract_json_candidates(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let mut candidates = Vec::new();
    if trimmed.is_empty() {
        return candidates;
    }

    candidates.push(trimmed.to_string());

    if trimmed.starts_with("```") {
        if let Some((_, rest)) = trimmed.split_once('\n') {
            if let Some(end) = rest.rfind("```") {
                candidates.push(rest[..end].trim().to_string());
            }
        }
    }

    let chars: Vec<char> = trimmed.chars().collect();
    for i in 0..chars.len() {
        if chars[i] != '{' {
            continue;
        }

        let mut depth = 0;
        let mut in_string = false;
        let mut escaped = false;

        for j in i..chars.len() {
            let ch = chars[j];

            if in_string {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    in_string = false;
                }
                continue;
            }

            if ch == '"' {
                in_string = true;
                continue;
            }

            if ch == '{' {
                depth += 1;
            } else if ch == '}' {
                depth -= 1;
                if depth == 0 {
                    candidates.push(chars[i..=j].iter().collect());
                    break;
                }
            }
        }
    }

    candidates
}

pub fn parse_acceptance_verdict(raw: &str) -> Result<AcceptanceVerdict, String> {
    let mut last_error = "Acceptance verdict was empty".to_string();
    for candidate in extract_json_candidates(raw) {
        let parsed =
            serde_json::from_str::<AcceptanceVerdict>(&candidate).or_else(|strict_error| {
                last_error = strict_error.to_string();
                serde_json::from_str::<LooseAcceptanceVerdict>(&candidate)
                    .map(LooseAcceptanceVerdict::into_verdict)
            });

        match parsed {
            Ok(verdict) => match validate_acceptance_verdict(verdict) {
                Ok(validated) => return Ok(validated),
                Err(error) => return Err(error),
            },
            Err(error) => {
                last_error = error.to_string();
            }
        }
    }

    Err(last_error)
}

fn validate_acceptance_verdict(verdict: AcceptanceVerdict) -> Result<AcceptanceVerdict, String> {
    if verdict.confidence < 0.0 || verdict.confidence > 1.0 {
        return Err(format!(
            "Invalid confidence value: {}. Must be between 0.0 and 1.0",
            verdict.confidence
        ));
    }

    if matches!(verdict.next_step.action, AcceptanceNextAction::Continue)
        && verdict.next_step.instructions.is_empty()
    {
        return Err("Acceptance verdict requires instructions for continue".to_string());
    }
    if matches!(verdict.next_step.action, AcceptanceNextAction::Finish)
        && !verdict.next_step.instructions.is_empty()
    {
        return Err("Acceptance verdict cannot include instructions when finishing".to_string());
    }
    // `verdict` and `nextStep.action` are independent axes: `verdict` judges the
    // executor's latest output, while `action` tracks whether the overall task is
    // done. A correct step in a multi-step task is legitimately `pass` + `continue`
    // (good work, more to do), so we do NOT force `pass` to pair with `finish`.
    // `fail` + `finish` stays rejected: finishing on a failing step with no
    // follow-up would stall the loop (`should_stop_iteration` needs `pass`, and the
    // executor follow-up needs `continue`, leaving no actionable next turn), so we
    // ask the mentor to restate instead.
    if matches!(verdict.verdict, AcceptanceVerdictDecision::Fail)
        && !matches!(verdict.next_step.action, AcceptanceNextAction::Continue)
    {
        return Err("Acceptance fail verdict must use nextStep.action continue".to_string());
    }
    // `should_stop_iteration` only stops at or above the threshold. A
    // low-confidence finish would otherwise be accepted but never stop the
    // loop, and its (empty) instructions would be sent to the executor.
    if matches!(verdict.next_step.action, AcceptanceNextAction::Finish)
        && verdict.confidence < CONFIDENCE_THRESHOLD
    {
        return Err(format!(
            "Acceptance verdict can only finish with confidence >= {} (got {}). If work remains, use nextStep.action continue with concrete instructions; only raise confidence if the whole task is verified complete.",
            CONFIDENCE_THRESHOLD, verdict.confidence
        ));
    }

    Ok(verdict)
}

pub const CONFIDENCE_THRESHOLD: f64 = 0.8;

pub fn should_stop_iteration(verdict: &AcceptanceVerdict) -> bool {
    matches!(verdict.verdict, AcceptanceVerdictDecision::Pass)
        && matches!(verdict.next_step.action, AcceptanceNextAction::Finish)
        && verdict.confidence >= CONFIDENCE_THRESHOLD
}

/// Fresh working-tree changes first, plus anything the pair state already
/// knew about (e.g. files the executor has since committed).
fn merge_modified_files(known: &[ModifiedFile], fresh: Vec<ModifiedFile>) -> Vec<ModifiedFile> {
    let mut seen: HashSet<String> = fresh.iter().map(|file| file.path.clone()).collect();
    let mut merged = fresh;
    for file in known {
        if seen.insert(file.path.clone()) {
            merged.push(file.clone());
        }
    }
    merged
}

/// The pair state's list is only refreshed by a 5 s poll, so it can miss
/// edits made right before the executor finished. Re-read the working tree.
async fn refresh_modified_files(
    workspace_root: &Path,
    known: &[ModifiedFile],
) -> Vec<ModifiedFile> {
    let directory = workspace_root.to_string_lossy().to_string();
    let fresh = tokio::task::spawn_blocking(move || GitTracker::collect_modified_files(&directory))
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    merge_modified_files(known, fresh)
}

async fn resolve_diff_base(workspace_root: &Path) -> String {
    let directory = workspace_root.to_string_lossy().to_string();
    tokio::task::spawn_blocking(move || crate::git_tracker::diff_base(&directory))
        .await
        .unwrap_or_else(|_| "HEAD".to_string())
}

pub async fn run_acceptance_checks(
    workspace_root: &Path,
    modified_files: &[ModifiedFile],
    executor_output: &str,
    iteration: u32,
    max_iterations: u32,
) -> AcceptanceRecord {
    let started_at = now_millis();
    let modified_files = refresh_modified_files(workspace_root, modified_files).await;
    let diff_base = resolve_diff_base(workspace_root).await;
    let checks = build_acceptance_check_plan(
        &workspace_root.to_string_lossy(),
        None,
        &modified_files,
        executor_output,
        iteration,
        max_iterations,
        &diff_base,
    );

    let mut runs = Vec::with_capacity(checks.len());
    for check in &checks {
        runs.push(run_check(workspace_root, check).await);
    }

    let passed = runs
        .iter()
        .filter(|run| matches!(run.status, AcceptanceCheckStatus::Passed))
        .count();
    let failed = runs
        .iter()
        .filter(|run| matches!(run.status, AcceptanceCheckStatus::Failed))
        .count();
    let skipped = runs
        .iter()
        .filter(|run| matches!(run.status, AcceptanceCheckStatus::Skipped))
        .count();

    AcceptanceRecord {
        iteration,
        risk: classify_acceptance_risk(&modified_files),
        checks: runs,
        summary: format!("{} passed, {} failed, {} skipped", passed, failed, skipped),
        started_at,
        finished_at: now_millis(),
        verdict: None,
        raw_verdict: None,
        error: None,
        repair_attempts: 0,
    }
}

pub fn build_mentor_acceptance_prompt(
    task_spec: &str,
    executor_result: &str,
    acceptance: &AcceptanceRecord,
) -> String {
    let parts: Vec<String> = vec![
        "You're reviewing the other agent's latest work in an automated pair-programming workflow. Read the executor output and the automated check results below, then return your assessment as a JSON block the workflow can parse.".to_string(),
        "".to_string(),
        "For this review turn, focus on analysis — no need to run commands or edit files.".to_string(),
        "".to_string(),
        "Reply with a JSON object using this schema (the orchestrator parses it):".to_string(),
        "".to_string(),
        "{".to_string(),
        "  \"verdict\": \"pass\" | \"fail\",".to_string(),
        "  \"risk\": \"low\" | \"medium\" | \"high\",".to_string(),
        "  \"confidence\": 0.0-1.0,".to_string(),
        "  \"issues\": [\"...\"],".to_string(),
        "  \"evidence\": [\"...\"],".to_string(),
        "  \"reasoning\": \"...\",".to_string(),
        "  \"summary\": \"...\",".to_string(),
        "  \"nextStep\": {".to_string(),
        "    \"action\": \"continue\" | \"finish\",".to_string(),
        "    \"instructions\": [\"...\"]".to_string(),
        "  }".to_string(),
        "}".to_string(),
        "".to_string(),
        "Notes:".to_string(),
        "- `verdict` judges the executor's latest output: \"pass\" if it's correct, \"fail\" if it needs rework. `nextStep.action` is a separate axis that tracks whether the overall task is finished.".to_string(),
        "- These two are independent. In a multi-step task, a correct step that still has follow-up work is \"verdict\": \"pass\" with \"action\": \"continue\" plus the next instructions. Avoid marking a good step \"fail\" just because later steps remain.".to_string(),
        "- Use \"action\": \"finish\" only when the entire task is complete; confidence ≥ 0.8 is required to finish.".to_string(),
        "- If nextStep.action is \"continue\", include concrete instructions for what the executor should do next.".to_string(),
        "- If nextStep.action is \"finish\", instructions should be an empty array.".to_string(),
        "- When you finish the workflow (action \"finish\"), add TASK_COMPLETE on its own line after the JSON so the orchestrator knows to stop.".to_string(),
        "".to_string(),
        "TASK".to_string(),
        task_spec.trim().to_string(),
        "".to_string(),
        "EXECUTOR OUTPUT".to_string(),
        executor_result.trim().to_string(),
        "".to_string(),
        "AUTOMATED CHECKS".to_string(),
        serde_json::to_string_pretty(acceptance).unwrap_or_else(|_| "{}".to_string()),
    ];

    parts.join("\n")
}

pub fn build_mentor_acceptance_repair_prompt(error: &str) -> String {
    format!(
        "The orchestrator couldn't parse your previous reply as the expected assessment JSON. Reply again with the JSON block in the schema described earlier — same fields, valid JSON, no surrounding prose.\n\n\
Parser error: {}",
        error.trim()
    )
}

pub fn build_executor_acceptance_followup_prompt(
    _task_spec: &str,
    _previous_executor_result: &str,
    verdict: &AcceptanceVerdict,
    _acceptance: &AcceptanceRecord,
) -> String {
    let mut lines = vec![
        "The reviewer asked for some adjustments to your previous turn. Carry them out and report what you did.".to_string(),
        "".to_string(),
        "A few constraints for this turn:".to_string(),
        "- Treat each instruction below as a direct task to carry out.".to_string(),
        "- For text-only instructions, output exactly the text the reviewer asked for — no commentary, no completion markers. Do not append TASK_COMPLETE (only the reviewer ends the workflow).".to_string(),
        "- If a tool is unavailable, fall back to a close text-based equivalent and briefly note the limitation only if it blocks you.".to_string(),
        "".to_string(),
        "ADJUSTMENTS".to_string(),
    ];

    for (index, instruction) in verdict.next_step.instructions.iter().enumerate() {
        lines.push(format!("{}. {}", index + 1, instruction));
    }

    lines.join("\n")
}

pub fn canonical_acceptance_verdict_json(verdict: &AcceptanceVerdict) -> String {
    serde_json::to_string_pretty(verdict).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use crate::types::AcceptanceNextStep;

    use super::*;
    use crate::types::{AcceptanceRisk, AcceptanceVerdict, AcceptanceVerdictDecision, FileStatus};

    #[test]
    fn parse_acceptance_verdict_handles_embedded_json() {
        let verdict = super::parse_acceptance_verdict(
            "Here is the structured review:\n{\n  \"verdict\": \"fail\",\n  \"risk\": \"high\",\n  \"confidence\": 0.75,\n  \"issues\": [\"npm run typecheck failed\"],\n  \"evidence\": [\"Type error in src/main.ts\"],\n  \"reasoning\": \"Type errors prevent task completion\",\n  \"summary\": \"The task still has type errors\",\n  \"nextStep\": {\n    \"action\": \"continue\",\n    \"instructions\": [\"Fix the TS error\", \"Re-run typecheck\"]\n  }\n}\nThanks.",
        )
        .expect("verdict should parse");

        assert_eq!(verdict.verdict, AcceptanceVerdictDecision::Fail);
        assert_eq!(verdict.risk, AcceptanceRisk::High);
        assert!((verdict.confidence - 0.75).abs() < 0.001);
        assert_eq!(verdict.issues, vec!["npm run typecheck failed"]);
        assert_eq!(verdict.evidence, vec!["Type error in src/main.ts"]);
        assert_eq!(verdict.reasoning, "Type errors prevent task completion");
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Continue);
        assert_eq!(
            verdict.next_step.instructions,
            vec![
                "Fix the TS error".to_string(),
                "Re-run typecheck".to_string()
            ]
        );
    }

    #[test]
    fn parse_acceptance_verdict_accepts_minimal_display_schema() {
        let verdict = super::parse_acceptance_verdict(
            r#"{
                "verdict": "pass",
                "risk": "low",
                "evidence": ["Executor rejected the fake task"],
                "summary": "Executor stayed focused on real project work",
                "nextStep": {
                    "action": "finish",
                    "instructions": []
                }
            }"#,
        )
        .expect("minimal verdict should parse");

        assert_eq!(verdict.verdict, AcceptanceVerdictDecision::Pass);
        assert_eq!(verdict.risk, AcceptanceRisk::Low);
        assert_eq!(verdict.confidence, 1.0);
        assert!(verdict.issues.is_empty());
        assert_eq!(
            verdict.reasoning,
            "Executor stayed focused on real project work"
        );
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Finish);
    }

    #[test]
    fn parse_acceptance_verdict_rejects_continue_without_instructions() {
        let error = super::parse_acceptance_verdict(
            r#"{
                "verdict": "fail",
                "risk": "medium",
                "confidence": 0.65,
                "issues": ["tests are still failing"],
                "evidence": ["test output"],
                "reasoning": "Need another iteration to fix tests",
                "summary": "Need another iteration",
                "nextStep": {
                    "action": "continue",
                    "instructions": []
                }
            }"#,
        )
        .expect_err("continue without instructions should fail");

        assert!(error.contains("instructions"));
    }

    #[test]
    fn parse_acceptance_verdict_accepts_pass_with_continue_action() {
        // A correct step in a multi-step task: the latest output passes, but more
        // work remains, so the mentor continues with the next instructions. Verdict
        // and next action are independent axes, so this must parse cleanly.
        let verdict = super::parse_acceptance_verdict(
            r#"{
                "verdict": "pass",
                "risk": "low",
                "confidence": 0.95,
                "issues": [],
                "evidence": ["Only one of three chat rounds completed"],
                "reasoning": "Round 1 was correct; more chat rounds are required",
                "summary": "Round 1 accepted, continuing to round 2",
                "nextStep": {
                    "action": "continue",
                    "instructions": ["Send round 2"]
                }
            }"#,
        )
        .expect("pass + continue should parse after decoupling verdict from action");

        assert_eq!(verdict.verdict, AcceptanceVerdictDecision::Pass);
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Continue);
        assert_eq!(
            verdict.next_step.instructions,
            vec!["Send round 2".to_string()]
        );
        // It passed a step but isn't finishing, so the loop must keep going.
        assert!(!super::should_stop_iteration(&verdict));
    }

    #[test]
    fn parse_acceptance_verdict_rejects_fail_with_finish_action() {
        let error = super::parse_acceptance_verdict(
            r#"{
                "verdict": "fail",
                "risk": "low",
                "confidence": 0.65,
                "issues": ["Task is incomplete"],
                "evidence": ["Only one of three chat rounds completed"],
                "reasoning": "More chat rounds are required",
                "summary": "Task is not complete yet",
                "nextStep": {
                    "action": "finish",
                    "instructions": []
                }
            }"#,
        )
        .expect_err("fail verdict cannot finish");

        assert!(error.contains("fail"));
        assert!(error.contains("continue"));
    }

    #[test]
    fn build_executor_acceptance_followup_prompt_requires_exact_text_output() {
        let verdict = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Fail,
            risk: AcceptanceRisk::Low,
            confidence: 0.9,
            issues: vec![],
            evidence: vec!["Greeting 2/3 received".to_string()],
            reasoning: "One more greeting is required".to_string(),
            summary: "One more greeting is required".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Continue,
                instructions: vec!["Send Greeting 3/3".to_string()],
            },
        };
        let acceptance = AcceptanceRecord {
            iteration: 2,
            risk: AcceptanceRisk::Low,
            checks: vec![],
            summary: "1 passed, 0 failed".to_string(),
            started_at: 100,
            finished_at: 200,
            verdict: Some(verdict.clone()),
            raw_verdict: None,
            error: None,
            repair_attempts: 0,
        };

        let prompt = super::build_executor_acceptance_followup_prompt(
            "Smoke greeting task",
            "Greeting 2/3",
            &verdict,
            &acceptance,
        );

        assert!(prompt.contains("output exactly the text the reviewer asked for"));
        assert!(prompt.contains("Do not append TASK_COMPLETE"));
        assert!(prompt.contains("TASK_COMPLETE"));
        assert!(prompt.contains("Send Greeting 3/3"));
        assert!(!prompt.contains("Greeting 2/3 received"));
        assert!(!prompt.contains("One more greeting is required"));
        assert!(!prompt.contains("### ACCEPTANCE REPORT"));
        assert!(!prompt.contains("### PREVIOUS EXECUTOR RESULT"));
        assert!(!prompt.contains("### ROLE:"));
    }

    #[test]
    fn build_acceptance_check_plan_prefers_fast_checks_and_adds_test_when_needed() {
        let package_json = serde_json::json!({
            "scripts": {
                "test": "node --test",
                "typecheck": "tsc --noEmit"
            }
        });

        let checks = super::build_acceptance_check_plan(
            "/workspace",
            Some(&package_json),
            &[ModifiedFile {
                path: "src/renderer/src/App.tsx".to_string(),
                status: FileStatus::M,
                display_path: "src/renderer/src/App.tsx".to_string(),
            }],
            "Done. The feature is implemented and ready for review.",
            8,
            10,
            "HEAD",
        );

        let names: Vec<_> = checks.iter().map(|check| check.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["git diff HEAD --check", "npm run typecheck", "npm run test"]
        );
    }

    #[test]
    fn build_acceptance_check_plan_skips_all_checks_when_no_files_modified() {
        let package_json = serde_json::json!({
            "scripts": {
                "test": "node --test",
                "typecheck": "tsc --noEmit"
            }
        });

        let checks = super::build_acceptance_check_plan(
            "/workspace",
            Some(&package_json),
            &[],
            "Greeting 1/3",
            1,
            20,
            "HEAD",
        );

        assert!(
            checks.is_empty(),
            "text-only outputs with no modified files should not trigger code-quality checks, got: {:?}",
            checks
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn is_actionable_git_check_failure_classifies_signal_and_repo_errors_as_skipped() {
        // Genuine whitespace failures should remain actionable.
        assert!(super::is_actionable_git_check_failure(Some(1), ""));
        assert!(super::is_actionable_git_check_failure(Some(2), ""));

        // Signal-killed (128+) and env-level failures should not.
        assert!(!super::is_actionable_git_check_failure(Some(129), ""));
        assert!(!super::is_actionable_git_check_failure(Some(143), ""));
        assert!(!super::is_actionable_git_check_failure(None, ""));
        assert!(!super::is_actionable_git_check_failure(
            Some(128),
            "fatal: not a git repository (or any of the parent directories): .git"
        ));
    }

    #[test]
    fn parse_acceptance_verdict_handles_greeting_pass_verdict() {
        let verdict = super::parse_acceptance_verdict(
            r#"{
                "verdict": "pass",
                "risk": "low",
                "confidence": 0.95,
                "evidence": ["Greeting 3/3 received", "All greetings completed successfully"],
                "summary": "All three greetings have been sent",
                "nextStep": {
                    "action": "finish",
                    "instructions": []
                }
            }"#,
        )
        .expect("greeting pass verdict should parse");

        assert_eq!(verdict.verdict, AcceptanceVerdictDecision::Pass);
        assert_eq!(verdict.risk, AcceptanceRisk::Low);
        assert!((verdict.confidence - 0.95).abs() < 0.001);
        assert!(verdict
            .evidence
            .contains(&"Greeting 3/3 received".to_string()));
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Finish);
        assert!(verdict.next_step.instructions.is_empty());
    }

    #[test]
    fn parse_acceptance_verdict_handles_greeting_fail_verdict() {
        let verdict = super::parse_acceptance_verdict(
            r#"{
                "verdict": "fail",
                "risk": "low",
                "confidence": 0.6,
                "issues": ["Only 2 of 3 greetings completed"],
                "evidence": ["Greeting 1/3 received", "Greeting 2/3 received"],
                "summary": "Missing final greeting",
                "nextStep": {
                    "action": "continue",
                    "instructions": ["Send Greeting 3/3"]
                }
            }"#,
        )
        .expect("greeting fail verdict should parse without error");

        assert_eq!(verdict.verdict, AcceptanceVerdictDecision::Fail);
        assert_eq!(verdict.risk, AcceptanceRisk::Low);
        assert!((verdict.confidence - 0.6).abs() < 0.001);
        assert!(verdict
            .evidence
            .contains(&"Greeting 1/3 received".to_string()));
        assert!(verdict
            .evidence
            .contains(&"Greeting 2/3 received".to_string()));
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Continue);
        assert_eq!(
            verdict.next_step.instructions,
            vec!["Send Greeting 3/3".to_string()]
        );
    }

    #[test]
    fn should_stop_iteration_returns_true_for_finish_action() {
        let verdict = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Medium,
            confidence: 0.85,
            issues: vec![],
            evidence: vec!["Greeting 3/3 received".to_string()],
            reasoning: "All greetings completed".to_string(),
            summary: "Task complete".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Finish,
                instructions: vec![],
            },
        };
        assert!(super::should_stop_iteration(&verdict));
    }

    #[test]
    fn should_stop_iteration_requires_pass_and_high_confidence() {
        // Pass with high confidence - should stop
        let high_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Low,
            confidence: 0.85,
            issues: vec![],
            evidence: vec!["All tests pass".to_string()],
            reasoning: "Implementation is complete".to_string(),
            summary: "Ready to finish".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Finish,
                instructions: vec![],
            },
        };
        assert!(should_stop_iteration(&high_confidence));

        // Pass with low confidence - should NOT stop
        let low_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Medium,
            confidence: 0.75,
            issues: vec!["Minor concerns".to_string()],
            evidence: vec![],
            reasoning: "Some uncertainty".to_string(),
            summary: "Needs more work".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Continue,
                instructions: vec!["Refactor".to_string()],
            },
        };
        assert!(!should_stop_iteration(&low_confidence));

        // Fail with high confidence - should NOT stop
        let fail_high_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Fail,
            risk: AcceptanceRisk::High,
            confidence: 0.95,
            issues: vec!["Tests failing".to_string()],
            evidence: vec![],
            reasoning: "Critical errors".to_string(),
            summary: "Cannot finish".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Continue,
                instructions: vec!["Fix tests".to_string()],
            },
        };
        assert!(!should_stop_iteration(&fail_high_confidence));

        // Edge case: exactly 0.8 confidence - should stop
        let threshold_confidence = AcceptanceVerdict {
            verdict: AcceptanceVerdictDecision::Pass,
            risk: AcceptanceRisk::Low,
            confidence: 0.8,
            issues: vec![],
            evidence: vec!["All good".to_string()],
            reasoning: "At threshold".to_string(),
            summary: "Ready".to_string(),
            next_step: AcceptanceNextStep {
                action: AcceptanceNextAction::Finish,
                instructions: vec![],
            },
        };
        assert!(should_stop_iteration(&threshold_confidence));
    }

    const PASS_FINISH_JSON: &str = r#"{
        "verdict": "pass",
        "risk": "low",
        "confidence": 0.9,
        "evidence": ["AUTOMATED CHECKS: 3 passed", "EXIT CODE: 0"],
        "summary": "Done",
        "nextStep": { "action": "finish", "instructions": [] }
    }"#;

    #[test]
    fn quality_gate_ignores_incidental_marker_text() {
        // Markers inside JSON strings or mid-line prose must not reject a valid verdict.
        let verdict = super::parse_review_verdict_with_quality(PASS_FINISH_JSON)
            .expect("markers inside strings are incidental");
        assert_eq!(verdict.next_step.action, AcceptanceNextAction::Finish);

        let with_prose = format!(
            "CHECKS: ran npm test, all green\nNo code changes needed.\n{}\nTASK_COMPLETE",
            PASS_FINISH_JSON
        );
        super::parse_review_verdict_with_quality(&with_prose)
            .expect("a lone CHECKS: line is not the evidence format");
    }

    #[test]
    fn quality_gate_still_rejects_deliberate_but_empty_evidence() {
        let raw = format!(
            "FILES_REVIEWED:\nCHECKS: types\nCODE: fn main()\n{}",
            PASS_FINISH_JSON
        );
        let error = super::parse_review_verdict_with_quality(&raw).expect_err("empty files list");
        assert!(error.contains("quality gate"), "{}", error);
    }

    #[test]
    fn parse_acceptance_verdict_rejects_low_confidence_finish() {
        let error = super::parse_acceptance_verdict(
            r#"{
                "verdict": "pass",
                "risk": "low",
                "confidence": 0.6,
                "evidence": ["looks fine"],
                "summary": "Probably done",
                "nextStep": { "action": "finish", "instructions": [] }
            }"#,
        )
        .expect_err("finish below the stop threshold must trigger the repair prompt");
        assert!(error.contains("confidence"), "{}", error);
        assert!(error.contains("continue"), "{}", error);
    }

    #[test]
    fn merge_modified_files_prefers_fresh_entries_and_keeps_known_ones() {
        let file = |path: &str, status: FileStatus| ModifiedFile {
            path: path.to_string(),
            status,
            display_path: path.to_string(),
        };
        let merged = super::merge_modified_files(
            &[
                file("a.rs", FileStatus::M),
                file("committed.rs", FileStatus::M),
            ],
            vec![
                file("a.rs", FileStatus::D),
                file("new.rs", FileStatus::Untracked),
            ],
        );
        let paths: Vec<&str> = merged.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["a.rs", "new.rs", "committed.rs"]);
        assert!(matches!(merged[0].status, FileStatus::D));
    }

    #[test]
    fn resolve_program_only_rewrites_npm_on_windows() {
        let expected = if cfg!(windows) { "npm.cmd" } else { "npm" };
        assert_eq!(super::resolve_program("npm"), expected);
        assert_eq!(super::resolve_program("git"), "git");
    }

    fn unique_dir(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "the-pair-acceptance-{}-{}-{}",
            label,
            std::process::id(),
            nanos
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn run_check_reports_spawn_failures_as_skipped() {
        let dir = unique_dir("spawn");
        let plan = AcceptanceCheckPlan::new("the-pair-no-such-binary-xyz", vec!["run".to_string()]);
        let run = super::run_check(&dir, &plan).await;
        assert_eq!(run.status, AcceptanceCheckStatus::Skipped);
        assert!(run.summary.contains("could not start"), "{}", run.summary);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_check_sets_ci_and_closes_stdin() {
        let dir = unique_dir("ci");
        let plan = AcceptanceCheckPlan::new(
            "sh",
            vec![
                "-c".to_string(),
                "read line; echo \"CI=$CI stdin_rc=$?\"".to_string(),
            ],
        );
        let run = super::run_check(&dir, &plan).await;
        assert!(run.stdout.contains("CI=true"), "{}", run.stdout);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_check_times_out_and_kills_the_whole_process_tree() {
        let dir = unique_dir("timeout");
        let pid_file = dir.join("child.pid");
        let plan = AcceptanceCheckPlan::new(
            "sh",
            vec![
                "-c".to_string(),
                format!(
                    "sleep 60 & echo $! > '{}'; echo started; wait",
                    pid_file.display()
                ),
            ],
        );

        let started = std::time::Instant::now();
        let run =
            super::run_check_with_timeout(&dir, &plan, std::time::Duration::from_secs(1)).await;
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        assert_eq!(run.status, AcceptanceCheckStatus::Failed);
        assert!(run.summary.contains("timed out"), "{}", run.summary);
        assert!(run.stdout.contains("started"), "{}", run.stdout);

        // The background grandchild was killed along with the shell.
        let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
        let mut alive = true;
        for _ in 0..50 {
            let status = std::process::Command::new("kill")
                .args(["-0", &pid])
                .stderr(Stdio::null())
                .status()
                .unwrap();
            if !status.success() {
                alive = false;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!alive, "grandchild {} survived the timeout", pid);
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_check_does_not_stall_on_background_processes_holding_output() {
        let dir = unique_dir("straggler");
        let pid_file = dir.join("bg.pid");
        // The shell exits immediately, but its background child keeps stdout open.
        let plan = AcceptanceCheckPlan::new(
            "sh",
            vec![
                "-c".to_string(),
                // Only stdout stays open, so the stderr reader finishes first and
                // must not be polled again after the grace period.
                format!(
                    "sleep 60 2>/dev/null & echo $! > '{}'; echo done",
                    pid_file.display()
                ),
            ],
        );

        let started = std::time::Instant::now();
        let run = super::run_check(&dir, &plan).await;
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        assert_eq!(run.status, AcceptanceCheckStatus::Passed);
        assert!(run.stdout.contains("done"), "{}", run.stdout);

        let pid = fs::read_to_string(&pid_file).unwrap().trim().to_string();
        let mut alive = true;
        for _ in 0..50 {
            let status = std::process::Command::new("kill")
                .args(["-0", &pid])
                .stderr(Stdio::null())
                .status()
                .unwrap();
            if !status.success() {
                alive = false;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!alive, "background process {} survived", pid);
        let _ = fs::remove_dir_all(&dir);
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[tokio::test]
    async fn run_acceptance_checks_sees_fresh_and_staged_whitespace_errors() {
        let dir = unique_dir("diff-check");
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.name", "Test"]);
        git(&dir, &["config", "user.email", "test@example.com"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);

        // Unborn HEAD: a staged whitespace error is still caught.
        fs::write(dir.join("a.txt"), "trailing   \n").unwrap();
        git(&dir, &["add", "a.txt"]);
        // The pair state's (stale) list is empty; the fresh status must be used.
        let record = super::run_acceptance_checks(&dir, &[], "", 1, 0).await;
        assert_eq!(record.checks.len(), 1, "{:?}", record.checks);
        assert_eq!(record.checks[0].status, AcceptanceCheckStatus::Failed);

        // With a commit: staged changes are checked against HEAD.
        fs::write(dir.join("a.txt"), "clean\n").unwrap();
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "-q", "--no-verify", "-m", "init"]);
        fs::write(dir.join("b.txt"), "bad   \n").unwrap();
        git(&dir, &["add", "b.txt"]);
        let record = super::run_acceptance_checks(&dir, &[], "", 1, 0).await;
        assert_eq!(record.checks[0].command, "git diff HEAD --check");
        assert_eq!(record.checks[0].status, AcceptanceCheckStatus::Failed);

        let _ = fs::remove_dir_all(&dir);
    }
}
