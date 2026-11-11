//! `bash`: run a shell command in the pair's workspace and read what it printed.
//!
//! This is the only tool that can change the machine it runs on, so its contract is
//! narrow. The command runs with the workspace as its working directory, never sees
//! stdin, is killed with its whole process tree when it outruns its timeout, and
//! answers with text the model can act on. A command that ran and failed is not an
//! error: the exit code travels in the text, so a failing `npm test` reaches the
//! model the way a passing one does.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use super::ToolContext;

/// Seconds a command may run before it is killed. A command still working at that
/// point returns what it printed, so the model can retry with a longer `timeout`
/// or in a shape that survives one.
const DEFAULT_TIMEOUT: u64 = 60;
/// The longest timeout a call may ask for, so no call waits forever.
const MAX_TIMEOUT: u64 = 300;
/// Output held in memory while a command runs, oldest bytes dropped first: the
/// model only ever reads the tail, so a chatty command cannot exhaust the app.
const MAX_CAPTURE: usize = 16 * 1024 * 1024;
/// What the model may see of a command's output: this many lines within this many
/// bytes, whichever cuts first.
const MAX_LINES: usize = 2000;
const MAX_BYTES: usize = 50 * 1024;
/// After a command ends, how long to read what it wrote. A job it left running can
/// hold the pipes open, so this is a grace period rather than a wait.
const DRAIN_GRACE: Duration = Duration::from_millis(500);
/// What a command that printed nothing returns.
const NO_OUTPUT: &str = "(no output)";
/// The same for a command that exited 0, so an empty result is not mistaken for a
/// failure to run.
const NO_OUTPUT_OK: &str = "(no output, exit code 0)";

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How a command ended.
enum Exit {
    Code(i32),
    Signal,
}

/// Runs one `bash` call in the pair's workspace.
pub async fn run(
    ctx: &ToolContext<'_>,
    args: &serde_json::Map<String, Value>,
) -> Result<String, String> {
    let Some(command) = args.get("command").and_then(Value::as_str) else {
        return Err("bash needs `command`".to_string());
    };
    run_within(ctx, command, timeout_of(args)).await
}

/// The seconds a call may run: `timeout` when the model asked for a sane one, the
/// default otherwise.
fn timeout_of(args: &serde_json::Map<String, Value>) -> Duration {
    let asked = match args.get("timeout") {
        Some(Value::Number(seconds)) => seconds.as_u64(),
        Some(Value::String(seconds)) => seconds.trim().parse().ok(),
        _ => None,
    };
    Duration::from_secs(asked.unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT))
}

/// Runs `command` under `limit`, which the tests shorten.
async fn run_within(
    ctx: &ToolContext<'_>,
    command: &str,
    limit: Duration,
) -> Result<String, String> {
    let mut running = match Running::spawn(ctx.directory, command) {
        Ok(running) => running,
        Err(error) => return Err(format!("bash could not start the command: {}", error)),
    };
    let exit = running.wait(limit).await;
    if exit.is_none() {
        running.kill();
    }
    let (captured, stopped) = running.finish().await;
    let mut text = tail(&captured.text(), captured.dropped);
    if text.is_empty() {
        text = match exit {
            Some(Exit::Code(0)) => NO_OUTPUT_OK.to_string(),
            _ => NO_OUTPUT.to_string(),
        };
    }
    match exit {
        None => text.push_str(&format!(
            "\n[Command was killed after {:.1}s without finishing]",
            limit.as_secs_f64()
        )),
        Some(Exit::Code(0)) => {}
        Some(Exit::Code(code)) => text.push_str(&format!("\n[Command exited with code {}]", code)),
        Some(Exit::Signal) => text.push_str("\n[Command was killed by a signal]"),
    }
    if stopped {
        text.push_str("\n[What the command left running was stopped]");
    }
    Ok(text)
}

/// A command this tool started: where its output goes until it is read, and how to
/// end it.
struct Running {
    child: Child,
    /// The shell's pid. On Unix it is also its process group's id, which is how one
    /// signal stops the shell together with everything it started.
    pid: u32,
    output: Arc<Mutex<Capture>>,
    readers: Vec<JoinHandle<()>>,
}

/// What a command has written so far, stdout and stderr merged in the order it
/// arrived.
#[derive(Default)]
struct Capture {
    bytes: Vec<u8>,
    /// Bytes dropped from the front to stay under `MAX_CAPTURE`.
    dropped: usize,
}

impl Capture {
    /// Takes a chunk of a command's output, dropping the oldest bytes once the
    /// capture is full.
    fn append(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
        if let Some(excess) = self
            .bytes
            .len()
            .checked_sub(MAX_CAPTURE)
            .filter(|&size| size > 0)
        {
            self.bytes.drain(..excess);
            self.dropped += excess;
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }
}

impl Running {
    /// Starts `command` under the platform's shell, in `directory`, with stdin
    /// closed: a command that reads it would otherwise wait on a terminal that is
    /// not there.
    fn spawn(directory: &Path, command: &str) -> std::io::Result<Self> {
        let (shell, flag) = if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        };
        let mut spawned = Command::new(shell);
        spawned
            .arg(flag)
            .arg(command)
            .current_dir(directory)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            // A pager or a colorizer would bury the output the model has to read.
            .env("NO_COLOR", "1")
            .env("TERM", "dumb")
            .env("PAGER", "cat")
            .env("GIT_PAGER", "cat")
            .kill_on_drop(true);
        #[cfg(unix)]
        spawned.process_group(0);
        #[cfg(windows)]
        spawned.creation_flags(CREATE_NO_WINDOW);
        let mut child = spawned.spawn()?;
        let pid = child.id().unwrap_or_default();

        let output = Arc::new(Mutex::new(Capture::default()));
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(tokio::spawn(capture(stdout, Arc::clone(&output))));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(tokio::spawn(capture(stderr, Arc::clone(&output))));
        }
        Ok(Self {
            child,
            pid,
            output,
            readers,
        })
    }

    /// Waits for the command to finish; `None` when it outran `limit`.
    async fn wait(&mut self, limit: Duration) -> Option<Exit> {
        match tokio::time::timeout(limit, self.child.wait()).await {
            Ok(Ok(status)) => Some(status.code().map_or(Exit::Signal, Exit::Code)),
            _ => None,
        }
    }

    /// Ends the command and everything it started. On Unix the command runs in a
    /// process group of its own, so one signal reaches the shell's jobs too; on
    /// Windows the tree is walked instead.
    fn kill(&mut self) {
        #[cfg(unix)]
        {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", "--", &format!("-{}", self.pid)])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        #[cfg(windows)]
        {
            let _ = std::process::Command::new("taskkill")
                .args(["/T", "/F", "/PID", &self.pid.to_string()])
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
        let _ = self.child.start_kill();
    }

    /// Reaps the command and reads what it wrote, so neither a kill nor a job left
    /// holding the pipes keeps the call from returning. The answer is what the
    /// command printed and whether something had to be stopped to get it.
    async fn finish(&mut self) -> (Capture, bool) {
        let _ = tokio::time::timeout(DRAIN_GRACE, self.child.wait()).await;
        let mut stopped = false;
        if !join(&mut self.readers, DRAIN_GRACE).await {
            // Something the command started still holds its output pipes: it is not
            // coming back to this call, so stop it rather than leak it.
            stopped = true;
            self.kill();
            join(&mut self.readers, DRAIN_GRACE).await;
        }
        for reader in &self.readers {
            reader.abort();
        }
        self.readers.clear();
        let capture =
            std::mem::take(&mut *self.output.lock().unwrap_or_else(PoisonError::into_inner));
        (capture, stopped)
    }
}

/// A call dropped before it finished — its turn was paused, killed or
/// superseded — takes its command with it. `kill_on_drop` alone would only end
/// the shell, leaving whatever the shell started running in the workspace.
impl Drop for Running {
    fn drop(&mut self) {
        // Still running means it was never reaped, so the pid is still its own.
        if matches!(self.child.try_wait(), Ok(None)) {
            self.kill();
        }
        for reader in &self.readers {
            reader.abort();
        }
    }
}

/// Reads `stream` to the end into `sink`, so a command's output is all there
/// however long it runs and whichever of the two pipes it arrives on.
async fn capture<R>(mut stream: R, sink: Arc<Mutex<Capture>>)
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(read) => sink
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .append(&chunk[..read]),
        }
    }
}

/// Waits up to `grace` for the output readers. Finished handles are let go — a
/// completed handle must not be polled again — and the answer is whether any were
/// still running at the deadline.
async fn join(readers: &mut Vec<JoinHandle<()>>, grace: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + grace;
    let mut pending = Vec::new();
    for mut reader in readers.drain(..) {
        if tokio::time::timeout_at(deadline, &mut reader)
            .await
            .is_err()
        {
            pending.push(reader);
        }
    }
    *readers = pending;
    readers.is_empty()
}

/// Keeps the last `MAX_LINES` lines within `MAX_BYTES` of what a command printed,
/// cut on a character boundary, and says which lines came back. `dropped` counts
/// the bytes the running capture had already let go of.
fn tail(output: &str, dropped: usize) -> String {
    let output = output.trim_end_matches('\n');
    if output.is_empty() {
        return String::new();
    }
    let total = output.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let mut kept = 0;
    let mut bytes = 0;
    for line in output.rsplit('\n') {
        let next = line.len() + 1;
        if kept == MAX_LINES || (bytes + next > MAX_BYTES && kept > 0) {
            break;
        }
        bytes += next;
        kept += 1;
    }
    let shown = cut(&output[output.len() - (bytes - 1)..], MAX_BYTES);
    let first = total - kept + 1;
    if first == 1 && dropped == 0 && shown.len() == output.len() {
        return output.to_string();
    }
    let lost = if dropped > 0 {
        format!(" The first {} bytes were not kept.", dropped)
    } else {
        String::new()
    };
    format!(
        "[Showing lines {}-{} of {}.{}]\n{}",
        first, total, total, lost, shown
    )
}

/// The last `max` bytes of `text`, on a character boundary so a wide character is
/// never cut in half.
fn cut(text: &str, max: usize) -> &str {
    let mut keep = max.min(text.len());
    while !text.is_char_boundary(text.len() - keep) {
        keep -= 1;
    }
    &text[text.len() - keep..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolPolicy;
    use serde_json::json;
    use std::fs;
    use std::time::Instant;

    /// A workspace of its own, removed when the test ends.
    struct Workspace {
        path: std::path::PathBuf,
    }

    impl Workspace {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("the-pair-bash-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("workspace");
            Self { path }
        }

        fn context(&self) -> ToolContext<'_> {
            ToolContext {
                directory: &self.path,
                policy: ToolPolicy::Full,
            }
        }
    }

    impl Drop for Workspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    /// The arguments a model sends for `bash`: one command line, and nothing else.
    fn command(line: &str) -> serde_json::Map<String, Value> {
        let mut args = serde_json::Map::new();
        args.insert("command".to_string(), Value::String(line.to_string()));
        args
    }

    /// Tool arguments holding one value under `key`.
    fn argument(key: &str, value: Value) -> serde_json::Map<String, Value> {
        let mut args = serde_json::Map::new();
        args.insert(key.to_string(), value);
        args
    }

    /// `first; second` in this platform's shell.
    fn sequence(first: &str, second: &str) -> String {
        if cfg!(windows) {
            format!("{} & {}", first, second)
        } else {
            format!("{}; {}", first, second)
        }
    }

    /// Prints `text` on stderr.
    fn to_stderr(text: &str) -> String {
        if cfg!(windows) {
            format!("echo {} 1>&2", text)
        } else {
            format!("echo {} >&2", text)
        }
    }

    /// Prints the working directory.
    fn working_directory() -> &'static str {
        if cfg!(windows) {
            "cd"
        } else {
            "pwd"
        }
    }

    /// Prints nothing and succeeds.
    fn silent() -> &'static str {
        if cfg!(windows) {
            "ver > nul"
        } else {
            "true"
        }
    }

    /// Reads its stdin to the end.
    fn reads_stdin() -> &'static str {
        if cfg!(windows) {
            "more"
        } else {
            "cat"
        }
    }

    /// Runs for far longer than any test's timeout.
    fn long_running() -> &'static str {
        if cfg!(windows) {
            "ping -n 60 127.0.0.1 > nul"
        } else {
            "sleep 60"
        }
    }

    /// Prints `count` numbered lines.
    fn numbered_lines(count: usize) -> String {
        if cfg!(windows) {
            format!("for /l %i in (1,1,{}) do @echo line %i", count)
        } else {
            format!(
                "i=1; while [ $i -le {} ]; do echo \"line $i\"; i=$((i+1)); done",
                count
            )
        }
    }

    #[tokio::test]
    async fn a_command_runs_and_returns_what_it_printed() {
        let workspace = Workspace::new();
        let text = run(&workspace.context(), &command("echo hello"))
            .await
            .expect("echo runs");
        assert_eq!(text.trim(), "hello");
    }

    #[tokio::test]
    async fn the_command_runs_with_the_workspace_as_its_working_directory() {
        let workspace = Workspace::new();
        fs::write(workspace.path.join("marker.txt"), "here").expect("marker");
        let list = if cfg!(windows) { "dir /b" } else { "ls" };
        let text = run(
            &workspace.context(),
            &command(&sequence(working_directory(), list)),
        )
        .await
        .expect("pwd runs");
        let printed = text.lines().next().unwrap_or_default().trim();
        #[cfg(unix)]
        assert_eq!(
            fs::canonicalize(printed).expect("the printed directory exists"),
            fs::canonicalize(&workspace.path).expect("the workspace exists"),
            "the command ran somewhere else"
        );
        #[cfg(windows)]
        assert!(
            printed.ends_with(workspace.path.file_name().unwrap().to_str().unwrap()),
            "the command ran somewhere else: {}",
            printed
        );
        assert!(text.contains("marker.txt"), "{}", text);
    }

    #[tokio::test]
    async fn a_non_zero_exit_code_is_reported_in_the_text_not_as_an_error() {
        let workspace = Workspace::new();
        let text = run(
            &workspace.context(),
            &command(&sequence("echo done", "exit 3")),
        )
        .await
        .expect("a failed command is a result, not a failure to run");
        assert_eq!(text, "done\n[Command exited with code 3]");
    }

    #[tokio::test]
    async fn stderr_arrives_in_the_same_text_as_stdout() {
        let workspace = Workspace::new();
        let text = run(
            &workspace.context(),
            &command(&sequence("echo out", &to_stderr("err"))),
        )
        .await
        .expect("the command runs");
        assert!(text.contains("out"), "{}", text);
        assert!(text.contains("err"), "{}", text);
    }

    #[tokio::test]
    async fn a_command_that_reads_stdin_gets_eof_at_once() {
        let workspace = Workspace::new();
        let started = Instant::now();
        let text = run(&workspace.context(), &command(reads_stdin()))
            .await
            .expect("the command runs");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the command waited on an inherited stdin"
        );
        assert_eq!(text, NO_OUTPUT_OK);
    }

    #[tokio::test]
    async fn a_command_that_prints_nothing_says_so() {
        let workspace = Workspace::new();
        let text = run(&workspace.context(), &command(silent()))
            .await
            .expect("the command runs");
        assert_eq!(text, NO_OUTPUT_OK);
    }

    #[tokio::test]
    async fn a_missing_command_is_an_error_the_model_can_act_on() {
        let workspace = Workspace::new();
        let error = run(&workspace.context(), &serde_json::Map::new())
            .await
            .expect_err("there is no command to run");
        assert!(error.contains("needs `command`"), "{}", error);
    }

    #[tokio::test]
    async fn a_command_that_outruns_its_timeout_is_killed_and_says_so() {
        let workspace = Workspace::new();
        let started = Instant::now();
        let text = run_within(
            &workspace.context(),
            long_running(),
            Duration::from_millis(300),
        )
        .await
        .expect("a killed command still returns what it printed");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the call outlived its timeout"
        );
        assert!(
            text.contains("killed after 0.3s without finishing"),
            "{}",
            text
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_timeout_kills_the_whole_process_tree() {
        let workspace = Workspace::new();
        let text = run_within(
            &workspace.context(),
            "sleep 60 & echo $!; wait",
            Duration::from_millis(300),
        )
        .await
        .expect("the command is killed");
        let pid = text
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .parse::<i32>()
            .expect("the job's pid");
        let mut alive = true;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(50));
            alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("kill runs")
                .success();
            if !alive {
                break;
            }
        }
        assert!(!alive, "the job the shell started outlived the command");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_call_dropped_mid_command_takes_the_process_tree_with_it() {
        // What a paused turn does: the call's future is dropped where it is.
        let workspace = Workspace::new();
        let context = workspace.context();
        tokio::select! {
            _ = run_within(&context, "sleep 60 & echo $! > job.pid; wait", Duration::from_secs(30)) => {
                panic!("the command cannot have finished")
            }
            _ = tokio::time::sleep(Duration::from_millis(400)) => {}
        }
        let pid = fs::read_to_string(workspace.path.join("job.pid"))
            .expect("the job wrote its pid")
            .trim()
            .parse::<i32>()
            .expect("the job's pid");
        let mut alive = true;
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(50));
            alive = std::process::Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .expect("kill runs")
                .success();
            if !alive {
                break;
            }
        }
        assert!(!alive, "the job outlived the call that was dropped");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_job_left_holding_the_pipes_does_not_hold_the_command() {
        let workspace = Workspace::new();
        let started = Instant::now();
        let text = run_within(
            &workspace.context(),
            "echo started; sleep 60 &",
            Duration::from_secs(60),
        )
        .await
        .expect("the command runs");
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "a job left running held the command's output open"
        );
        assert!(text.contains("started"), "{}", text);
        assert!(text.contains("was stopped"), "{}", text);
    }

    #[tokio::test]
    async fn a_huge_output_is_cut_to_its_tail_with_a_marker() {
        let workspace = Workspace::new();
        let text = run(&workspace.context(), &command(&numbered_lines(3000)))
            .await
            .expect("the command runs");
        assert!(
            text.starts_with("[Showing lines 1001-3000 of 3000.]"),
            "{}",
            &text[..80.min(text.len())]
        );
        assert!(text.ends_with("line 3000"), "{}", &text[text.len() - 40..]);
        assert_eq!(text.lines().count(), MAX_LINES + 1);
    }

    #[test]
    fn the_tail_is_cut_by_bytes_on_a_character_boundary() {
        // One line of wide characters: the byte budget cuts, the characters survive.
        let text = tail(&"界".repeat(MAX_BYTES), 0);
        assert!(
            text.starts_with("[Showing lines 1-1 of 1.]"),
            "{}",
            &text[..40]
        );
        let body = text
            .split_once('\n')
            .expect("the marker is on its own line")
            .1;
        assert!(body.len() <= MAX_BYTES, "{} bytes", body.len());
        assert!(
            body.chars().all(|c| c == '界'),
            "a character was cut in half"
        );
    }

    #[test]
    fn a_dropped_head_is_said_out_loud() {
        let text = tail("b\nc\n", 5);
        assert!(
            text.contains("The first 5 bytes were not kept."),
            "{}",
            text
        );
    }

    #[test]
    fn a_chatty_command_cannot_exhaust_memory() {
        let mut capture = Capture::default();
        capture.append(&[b'a'; MAX_CAPTURE]);
        assert_eq!(capture.dropped, 0);
        capture.append(b"bc");
        assert_eq!(capture.dropped, 2);
        assert_eq!(capture.bytes.len(), MAX_CAPTURE);
        assert_eq!(capture.text(), format!("{}bc", "a".repeat(MAX_CAPTURE - 2)));
    }

    #[test]
    fn the_timeout_argument_is_clamped_to_a_sane_range() {
        let asked = |timeout: Value| timeout_of(&argument("timeout", timeout)).as_secs();
        assert_eq!(
            timeout_of(&serde_json::Map::new()).as_secs(),
            DEFAULT_TIMEOUT
        );
        assert_eq!(asked(json!(45)), 45);
        assert_eq!(asked(json!("45")), 45);
        assert_eq!(asked(json!(0)), 1);
        assert_eq!(asked(json!(99999)), MAX_TIMEOUT);
        assert_eq!(asked(json!("soon")), DEFAULT_TIMEOUT);
        assert_eq!(asked(json!(true)), DEFAULT_TIMEOUT);
    }
}
