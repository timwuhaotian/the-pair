use crate::config_paths::{opencode_auth_path, opencode_config_path};
use crate::path_env::{fallback_path_dirs, merge_path_entries, run_with_timeout};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

const CLI_PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Total time `opencode models` may spend waiting for the 2.x server to warm
/// up. Detection joins every provider thread, so this bounds the slowest one.
const OPENCODE_MODELS_BUDGET: Duration = Duration::from_secs(8);
const OPENCODE_WARMUP_RETRY_DELAY: Duration = Duration::from_millis(1500);
/// Extensions Windows can launch directly, in order of preference. Rust's
/// `Command` runs `.cmd`/`.bat` through `cmd.exe` by itself; the
/// extensionless npm shim next to them is a shell script it cannot run.
const WINDOWS_LAUNCHER_EXTENSIONS: [&str; 3] = [".exe", ".cmd", ".bat"];

/// How an installed `opencode` CLI expects a reasoning-effort variant to be
/// communicated on the turn command.
///
/// OpenCode has shipped two incompatible spellings across its major versions:
///
/// - **1.x** — `--model <id>` plus a separate `--variant <cli>` flag.
/// - **2.x** — `--variant` was removed; variants are now baked into the model
///   id as `provider/model#variant`. Verified against opencode 2.0.3
///   (2026-09-19): `--model, -m string   Model to use in the format
///   provider/model#variant`.
///
/// We probe `opencode run --help` once per process so the call site can pick
/// the spelling its installed copy understands. Older installs without any
/// variant support fall through to the bare model id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpencodeVariantSyntax {
    /// OpenCode 1.x: `--variant <cli>` is a separate flag.
    Flag,
    /// OpenCode 2.x: variants are baked into the model id with `#variant`.
    Suffix,
    /// The installed CLI exposes neither spelling; reasoning variants cannot
    /// be requested on this machine.
    Unsupported,
}

/// How long a failed capability probe is remembered before it is retried.
/// Long enough that a catalog build asking once per model doesn't re-run a
/// hanging probe for every row, short enough that one slow start doesn't hide
/// a capability for the rest of the session.
const FAILED_PROBE_RETRY_AFTER: Duration = Duration::from_secs(30);

static OPENCODE_VARIANT_SYNTAX: ProbeCache<OpencodeVariantSyntax> =
    ProbeCache::new(FAILED_PROBE_RETRY_AFTER);
static PI_MAX_THINKING_LEVEL_SUPPORT: ProbeCache<bool> = ProbeCache::new(FAILED_PROBE_RETRY_AFTER);

/// The last result of a capability probe for one resolved CLI binary. A
/// successful probe is kept for the session; a failed one (timeout, crash)
/// only for `retry_after`, and resolving a different binary re-probes.
struct ProbeCache<T> {
    retry_after: Duration,
    entry: Mutex<Option<ProbeEntry<T>>>,
}

struct ProbeEntry<T> {
    binary: PathBuf,
    value: Option<T>,
    probed_at: Instant,
}

impl<T: Copy> ProbeCache<T> {
    const fn new(retry_after: Duration) -> Self {
        Self {
            retry_after,
            entry: Mutex::new(None),
        }
    }

    fn get_or_probe(&self, binary: &Path, probe: impl FnOnce() -> Option<T>) -> Option<T> {
        {
            let entry = self.entry.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = entry.as_ref().filter(|entry| entry.binary == binary) {
                if entry.value.is_some() || entry.probed_at.elapsed() < self.retry_after {
                    return entry.value;
                }
            }
        }

        // Probe without holding the lock: it can take seconds.
        let value = probe();
        *self.entry.lock().unwrap_or_else(|e| e.into_inner()) = Some(ProbeEntry {
            binary: binary.to_path_buf(),
            value,
            probed_at: Instant::now(),
        });
        value
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Opencode,
    Codex,
    Claude,
    Gemini,
    Kimi,
    Pi,
    Kiro,
    Aider,
    Grok,
    Muse,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetectedModelOption {
    pub model_id: String,
    pub display_name: String,
    pub source_provider: Option<String>,
    pub family: Option<String>,
    pub subscription_label: String,
    pub supports_pair_execution: bool,
    pub runnable: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DetectedProviderProfile {
    pub kind: ProviderKind,
    pub installed: bool,
    pub authenticated: bool,
    pub runnable: bool,
    pub subscription_label: String,
    pub current_models: Vec<DetectedModelOption>,
    pub login_command: Option<String>,
    pub install_url: Option<String>,
    pub detected_at: u64,
}

fn non_empty_env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

pub(crate) fn homedir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let home = non_empty_env_path("USERPROFILE")
        .or_else(|| {
            ["APPDATA", "LOCALAPPDATA"]
                .into_iter()
                .find_map(|key| home_from_appdata(&non_empty_env_path(key)?))
        })
        .or_else(dirs::home_dir);
    #[cfg(not(target_os = "windows"))]
    let home = non_empty_env_path("HOME").or_else(dirs::home_dir);
    home.unwrap_or_default()
}

/// `%APPDATA%` is `<home>\AppData\Roaming` and `%LOCALAPPDATA%` is
/// `<home>\AppData\Local`, so the profile dir is their grandparent.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn home_from_appdata(appdata: &Path) -> Option<PathBuf> {
    appdata
        .parent()?
        .parent()
        .filter(|home| !home.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

pub(crate) fn cli_environment_overrides(home: &std::path::Path) -> Vec<(OsString, OsString)> {
    let mut overrides = Vec::new();

    if !home.as_os_str().is_empty() {
        overrides.push((OsString::from("HOME"), home.as_os_str().to_owned()));
        overrides.push((OsString::from("USERPROFILE"), home.as_os_str().to_owned()));

        #[cfg(target_os = "windows")]
        {
            if std::env::var_os("APPDATA").is_none() {
                overrides.push((
                    OsString::from("APPDATA"),
                    home.join("AppData/Roaming").into_os_string(),
                ));
            }
            if std::env::var_os("LOCALAPPDATA").is_none() {
                overrides.push((
                    OsString::from("LOCALAPPDATA"),
                    home.join("AppData/Local").into_os_string(),
                ));
            }
        }
    }

    if let Some(appdata) = std::env::var_os("APPDATA") {
        overrides.push((OsString::from("APPDATA"), appdata));
    }

    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA") {
        overrides.push((OsString::from("LOCALAPPDATA"), local_appdata));
    }

    let fallback_dirs = fallback_path_dirs(
        Some(home.to_path_buf()),
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        cfg!(target_os = "windows"),
    );
    let base_path = std::env::var_os("PATH").unwrap_or_default();
    if let Ok(path) = merge_path_entries(&base_path, &fallback_dirs) {
        overrides.push((OsString::from("PATH"), path));
    }

    overrides
}

fn prepare_cli_command(command: &mut Command, home: &std::path::Path) {
    for (key, value) in cli_environment_overrides(home) {
        command.env(key, value);
    }
}

/// Run a CLI probe and return its stdout, or `None` when it can't start, exits
/// non-zero or outlives `timeout`. stdout is drained while the probe runs, so
/// output larger than the pipe buffer can't stall it into the timeout.
fn capture_command_output_with_timeout(
    command_path: &Path,
    args: &[&str],
    home: &std::path::Path,
    timeout: Duration,
) -> Option<String> {
    let mut command = Command::new(command_path);
    command.args(args);
    prepare_cli_command(&mut command, home);

    let output = run_with_timeout(command, timeout)?;
    if !output.status?.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn opencode_variant_syntax_from_help(help_text: &str) -> OpencodeVariantSyntax {
    // OpenCode 2.x advertises the variant suffix in the `--model` help text
    // ("Model to use in the format provider/model#variant"); 1.x has neither
    // the suffix nor any reference to "#variant" in the help output.
    if help_text.contains("#variant") {
        OpencodeVariantSyntax::Suffix
    } else if help_text.contains("--variant") {
        OpencodeVariantSyntax::Flag
    } else {
        OpencodeVariantSyntax::Unsupported
    }
}

pub(crate) fn opencode_variant_syntax() -> OpencodeVariantSyntax {
    let Some(command_path) = which_binary("opencode") else {
        return OpencodeVariantSyntax::Unsupported;
    };

    OPENCODE_VARIANT_SYNTAX
        .get_or_probe(&command_path, || {
            capture_command_output_with_timeout(
                &command_path,
                &["run", "--help"],
                &homedir(),
                CLI_PROBE_TIMEOUT,
            )
            .map(|help_text| opencode_variant_syntax_from_help(&help_text))
        })
        .unwrap_or(OpencodeVariantSyntax::Unsupported)
}

/// Whether the installed `pi` CLI advertises the `max` thinking level in its
/// `--thinking` flag. Re-added in pi 0.80.6 (verified against pi 0.79.2 on
/// 2026-09-19, which lists only `off, minimal, low, medium, high, xhigh`).
/// We probe `pi --help` (once per binary, see `ProbeCache`) so the picker can
/// hide `max` on older installs where it would otherwise hard-fail at turn time.
pub(crate) fn pi_supports_max_thinking_level() -> bool {
    let Some(command_path) = which_binary("pi") else {
        return false;
    };

    PI_MAX_THINKING_LEVEL_SUPPORT
        .get_or_probe(&command_path, || {
            capture_command_output_with_timeout(
                &command_path,
                &["--help"],
                &homedir(),
                CLI_PROBE_TIMEOUT,
            )
            .map(|help_text| pi_help_lists_max_thinking_level(&help_text))
        })
        .unwrap_or(false)
}

fn pi_help_lists_max_thinking_level(help_text: &str) -> bool {
    // The thinking-flag help line enumerates accepted levels; presence of
    // "max" in that token list is the version's signal.
    help_text.contains("--thinking <level>")
        && extract_pi_thinking_levels(help_text)
            .is_some_and(|levels| levels.iter().any(|level| level == "max"))
}

fn extract_pi_thinking_levels(help_text: &str) -> Option<Vec<String>> {
    // The pi help block looks like:
    //   --thinking <level>   Set thinking level: off, minimal, low, medium, high, xhigh
    // We extract the comma-separated levels on the same line.
    let line = help_text
        .lines()
        .find(|line| line.contains("--thinking <level>"))?;
    let after_colon = line.split(':').nth(1)?;
    Some(
        after_colon
            .trim()
            .split(',')
            .map(|level| level.trim().to_string())
            .filter(|level| !level.is_empty())
            .collect(),
    )
}

/// Resolve `name` to a file this platform can launch: PATH first, then the
/// known install locations. On Windows that is `name.exe` / `.cmd` / `.bat`,
/// never npm's extensionless shell-script shim; on Unix an executable file.
pub fn which_binary(name: &str) -> Option<PathBuf> {
    let is_windows = cfg!(target_os = "windows");
    if let Some(path) = std::env::var_os("PATH").and_then(|value| {
        std::env::split_paths(&value)
            // A relative entry would resolve against the app's cwd.
            .filter(|dir| dir.is_absolute())
            .find_map(|dir| resolve_binary_in_dir(&dir, name, is_windows))
    }) {
        return Some(path);
    }

    if let Some(path) = resolve_binary_at_known_locations(name, &homedir()) {
        return Some(path);
    }

    #[cfg(target_os = "windows")]
    let cmd = "where";
    #[cfg(not(target_os = "windows"))]
    let cmd = "which";

    // Last resort: the platform's own lookup. `where` also lists the
    // extensionless npm shim, so keep only a path that can be launched.
    let mut command = Command::new(cmd);
    command.arg(name);
    let output = run_with_timeout(command, CLI_PROBE_TIMEOUT)?;
    if !output.status?.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .map(PathBuf::from)
        .find(|path| path.is_absolute() && is_launchable_file(path, is_windows))
}

fn which_binary_exists(name: &str) -> bool {
    which_binary(name).is_some()
}

fn safe_read_json<T: DeserializeOwned>(path: impl AsRef<std::path::Path>) -> Option<T> {
    let path = path.as_ref();
    if !path.exists() {
        return None;
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn detected_at_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn resolve_binary_at_known_locations(name: &str, home: &std::path::Path) -> Option<PathBuf> {
    resolve_binary_in_fallback_dirs(
        name,
        home,
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        cfg!(target_os = "windows"),
    )
}

/// Search the fallback install dirs in the same order `path_env` appends
/// them to PATH, so detection and spawning pick the same copy.
fn resolve_binary_in_fallback_dirs(
    name: &str,
    home: &Path,
    appdata: Option<PathBuf>,
    local_appdata: Option<PathBuf>,
    is_windows: bool,
) -> Option<PathBuf> {
    fallback_path_dirs(Some(home.to_path_buf()), appdata, local_appdata, is_windows)
        .iter()
        .find_map(|dir| resolve_binary_in_dir(dir, name, is_windows))
}

#[allow(dead_code)]
fn binary_exists_at_known_locations(name: &str, home: &std::path::Path) -> bool {
    resolve_binary_at_known_locations(name, home).is_some()
}

fn has_windows_launcher_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            WINDOWS_LAUNCHER_EXTENSIONS
                .iter()
                .any(|launcher| launcher[1..].eq_ignore_ascii_case(extension))
        })
}

/// Whether `path` is a file the OS can start: a launcher extension on
/// Windows, an executable bit on Unix. Directories never qualify.
fn is_launchable_file(path: &Path, is_windows: bool) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    if is_windows {
        return has_windows_launcher_extension(path);
    }
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = true;
    executable
}

fn resolve_binary_in_dir(dir: &std::path::Path, name: &str, is_windows: bool) -> Option<PathBuf> {
    if !is_windows {
        let candidate = dir.join(name);
        return is_launchable_file(&candidate, false).then_some(candidate);
    }

    let explicit = Path::new(name);
    if has_windows_launcher_extension(explicit) {
        let candidate = dir.join(explicit);
        return is_launchable_file(&candidate, true).then_some(candidate);
    }
    WINDOWS_LAUNCHER_EXTENSIONS
        .iter()
        .map(|extension| dir.join(format!("{name}{extension}")))
        .find(|candidate| is_launchable_file(candidate, true))
}

fn push_unique_model_id(model_ids: &mut Vec<String>, model_id: &str) {
    let trimmed = model_id.trim();
    if trimmed.is_empty() {
        return;
    }

    if !model_ids.iter().any(|existing| existing == trimmed) {
        model_ids.push(trimmed.to_string());
    }
}

fn extract_quoted_segments(line: &str) -> Vec<String> {
    line.split('"')
        .enumerate()
        .filter_map(|(index, segment)| (index % 2 == 1).then_some(segment.trim().to_string()))
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn collect_json_string_values(
    value: &serde_json::Value,
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, nested) in map {
                if interesting_keys.iter().any(|candidate| candidate == key) {
                    if let Some(string_value) = nested.as_str() {
                        if predicate(string_value) {
                            push_unique_model_id(model_ids, string_value);
                        }
                    }
                }
                collect_json_string_values(nested, interesting_keys, predicate, model_ids);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_string_values(item, interesting_keys, predicate, model_ids);
            }
        }
        _ => {}
    }
}

fn collect_model_ids_from_json_file(
    path: &std::path::Path,
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    if let Some(value) = safe_read_json::<serde_json::Value>(path) {
        collect_json_string_values(&value, interesting_keys, predicate, model_ids);
    }
}

fn collect_model_ids_from_jsonl_file(
    path: &std::path::Path,
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };

    // Transcripts run to tens of MB; only lines that mention one of the keys
    // can yield a value, so skip JSON-parsing the rest.
    let quoted_keys: Vec<String> = interesting_keys
        .iter()
        .map(|key| format!("\"{key}\""))
        .collect();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !quoted_keys.iter().any(|key| trimmed.contains(key.as_str())) {
            continue;
        }

        if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
            collect_json_string_values(&value, interesting_keys, predicate, model_ids);
        }
    }
}

fn collect_recent_files(
    root: &std::path::Path,
    extensions: &[&str],
    max_depth: usize,
    limit: usize,
) -> Vec<PathBuf> {
    fn visit_dir(
        dir: &std::path::Path,
        extensions: &[&str],
        max_depth: usize,
        depth: usize,
        files: &mut Vec<(std::time::SystemTime, PathBuf)>,
    ) {
        if depth > max_depth {
            return;
        }

        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_dir() {
                visit_dir(&path, extensions, max_depth, depth + 1, files);
                continue;
            }

            let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
                continue;
            };

            if !extensions.iter().any(|candidate| candidate == &extension) {
                continue;
            }

            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            files.push((modified, path));
        }
    }

    let mut files = Vec::new();
    visit_dir(root, extensions, max_depth, 0, &mut files);
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files
        .into_iter()
        .take(limit)
        .map(|(_, path)| path)
        .collect()
}

fn collect_model_ids_from_recent_files(
    root: &std::path::Path,
    extensions: &[&str],
    interesting_keys: &[&str],
    predicate: &dyn Fn(&str) -> bool,
    max_depth: usize,
    limit: usize,
    model_ids: &mut Vec<String>,
) {
    for path in collect_recent_files(root, extensions, max_depth, limit) {
        match path.extension().and_then(|value| value.to_str()) {
            Some("json") => {
                collect_model_ids_from_json_file(&path, interesting_keys, predicate, model_ids)
            }
            Some("jsonl") => {
                collect_model_ids_from_jsonl_file(&path, interesting_keys, predicate, model_ids)
            }
            _ => {}
        }
    }
}

fn collect_model_ids_from_toml_text(
    content: &str,
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    // Model ids only live at the top level, in `[profiles.*]`, and in
    // `[notice.model_migrations]`. Other tables hold look-alike strings, e.g.
    // `[tui] status_line = ["codex-version", …]`.
    let mut in_model_section = true;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            let section = line.trim_matches(|c| c == '[' || c == ']').trim();
            in_model_section =
                section.starts_with("profiles.") || section == "notice.model_migrations";
            continue;
        }
        if !in_model_section {
            continue;
        }

        if line.starts_with("model =") {
            if let Some(model_id) = line.split('"').nth(1) {
                if predicate(model_id) {
                    push_unique_model_id(model_ids, model_id);
                }
            }
            continue;
        }

        for model_id in extract_quoted_segments(line) {
            if predicate(&model_id) {
                push_unique_model_id(model_ids, &model_id);
            }
        }
    }
}

fn collect_model_ids_from_help_line(
    help_text: &str,
    predicate: &dyn Fn(&str) -> bool,
    model_ids: &mut Vec<String>,
) {
    // `claude --help` wraps the `--model` description across multiple indented
    // continuation lines (verified against claude-code 2.1.280), so the block
    // is stitched back together first. Only that block is scanned: the rest of
    // the help mentions look-alike quoted words (permission modes, output
    // formats) that are not models.
    let mut block: Option<String> = None;
    for raw_line in help_text.lines() {
        if let Some(buf) = block.as_mut() {
            // Continuation lines are indented with whitespace and don't start
            // with another flag. Otherwise we've left the description block.
            let trimmed = raw_line.trim_start();
            if raw_line.starts_with(char::is_whitespace)
                && !trimmed.is_empty()
                && !trimmed.starts_with('-')
            {
                buf.push(' ');
                buf.push_str(trimmed);
                continue;
            }
            break;
        }
        if raw_line.contains("--model") {
            block = Some(raw_line.to_string());
        }
    }
    let Some(block) = block else {
        return;
    };

    // Split on quotes and punctuation rather than pairing quotes: the prose
    // contains apostrophes ("a model's full name") that break quote pairing.
    for token in block.split(|c: char| {
        c.is_whitespace() || matches!(c, '\'' | '"' | '(' | ')' | ',' | '`')
    }) {
        let token = token.trim_end_matches(['.', ':', ';']);
        if predicate(token) {
            push_unique_model_id(model_ids, token);
        }
    }
}

fn is_plausible_model_id(value: &str) -> bool {
    if value.len() < 4 {
        return false;
    }
    let mut chars = value.chars();
    chars.next().is_some_and(|c| c.is_alphanumeric())
        && chars.all(|c| c.is_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '/')
}

pub fn beautify_claude_display_name(model_id: &str) -> String {
    let lower = model_id.to_lowercase();
    if !lower.starts_with("claude-") {
        return model_id.to_string();
    }

    let mut parts: Vec<String> = lower.split('-').map(|s| s.to_string()).collect();

    // Remove date suffix (8-digit segment at end)
    if let Some(last) = parts.last() {
        if last.len() == 8 && last.chars().all(|c| c.is_ascii_digit()) {
            parts.pop();
        }
    }

    // Handle old format: claude-{major}-{minor}-{name} (e.g., claude-3-5-sonnet)
    if parts.len() >= 4
        && parts[1].len() == 1
        && parts[2].len() == 1
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && parts[2].chars().all(|c| c.is_ascii_digit())
    {
        let minor = parts.remove(2);
        let major = parts.remove(1);
        parts.insert(1, format!("{}.{}", major, minor));
    }

    // Merge trailing version segments (e.g., "4" + "5" → "4.5")
    if parts.len() >= 2 {
        let last = parts[parts.len() - 1].clone();
        let prev = parts[parts.len() - 2].clone();
        if last.len() <= 2
            && prev.len() <= 2
            && last.chars().all(|c| c.is_ascii_digit())
            && prev.chars().all(|c| c.is_ascii_digit())
        {
            parts.pop();
            parts.pop();
            parts.push(format!("{}.{}", prev, last));
        }
    }

    // Capitalize each word
    parts
        .iter()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_detected_models(
    model_ids: Vec<String>,
    source_provider: &str,
    subscription_label: &str,
) -> Vec<DetectedModelOption> {
    model_ids
        .into_iter()
        .filter(|id| is_plausible_model_id(id))
        .map(|model_id| DetectedModelOption {
            display_name: model_id.clone(),
            model_id,
            source_provider: Some(source_provider.to_string()),
            family: None,
            subscription_label: subscription_label.to_string(),
            supports_pair_execution: true,
            runnable: true,
        })
        .collect()
}

fn is_codex_model_id(value: &str) -> bool {
    value.starts_with("gpt-")
        || value.starts_with("codex-")
        || value
            .strip_prefix('o')
            .and_then(|suffix| suffix.chars().next())
            .map(|ch| ch.is_ascii_digit())
            .unwrap_or(false)
}

/// Returns true for legacy `gemini-*` model IDs.  Kept for snapshot-recovery
/// paths that may encounter model IDs from the sunset Gemini CLI.
#[allow(dead_code)]
fn is_gemini_model_id(value: &str) -> bool {
    value.starts_with("gemini-")
}

fn is_claude_model_id(value: &str) -> bool {
    value.starts_with("claude-")
}

fn discover_codex_model_ids(home: &std::path::Path) -> Vec<String> {
    let predicate = |value: &str| is_codex_model_id(value);
    let mut model_ids = Vec::new();
    let config_path = home.join(".codex/config.toml");
    let models_cache_path = home.join(".codex/models_cache.json");

    collect_model_ids_from_json_file(&models_cache_path, &["slug"], &predicate, &mut model_ids);

    if let Ok(content) = fs::read_to_string(config_path) {
        collect_model_ids_from_toml_text(&content, &predicate, &mut model_ids);
    }

    collect_model_ids_from_recent_files(
        &home.join(".codex"),
        &["json", "jsonl"],
        &["model", "slug"],
        &predicate,
        2,
        20,
        &mut model_ids,
    );

    // The live catalog marks internal routes (e.g. `codex-auto-review`) as
    // `"visibility": "hide"`; they must not surface even when a recent session
    // file mentions them.
    let hidden = codex_hidden_model_slugs(&models_cache_path);
    model_ids.retain(|id| !hidden.contains(id));

    model_ids
}

fn codex_hidden_model_slugs(models_cache_path: &Path) -> std::collections::HashSet<String> {
    let Ok(content) = fs::read_to_string(models_cache_path) else {
        return Default::default();
    };
    let Ok(cache) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Default::default();
    };
    cache
        .get("models")
        .and_then(|models| models.as_array())
        .into_iter()
        .flatten()
        .filter(|model| model.get("visibility").and_then(|v| v.as_str()) == Some("hide"))
        .filter_map(|model| model.get("slug").and_then(|v| v.as_str()))
        .map(String::from)
        .collect()
}

fn discover_claude_model_ids(home: &std::path::Path, command_path: Option<&Path>) -> Vec<String> {
    let mut model_ids = Vec::new();

    if let Some(command_path) = command_path {
        if let Some(help_text) = capture_claude_help_text(home, command_path) {
            // Bare aliases ('opus', 'sonnet') are skipped: provider inference
            // routes model ids by their `claude-` prefix, so an alias would be
            // spawned through the wrong CLI.
            let help_predicate =
                |value: &str| is_claude_model_id(value) && is_plausible_model_id(value);
            collect_model_ids_from_help_line(&help_text, &help_predicate, &mut model_ids);
        }
    }

    let history_predicate = |value: &str| is_claude_model_id(value);
    collect_model_ids_from_recent_files(
        &home.join(".claude/projects"),
        &["json", "jsonl"],
        &["model"],
        &history_predicate,
        3,
        25,
        &mut model_ids,
    );

    model_ids
}

fn capture_claude_help_text(home: &std::path::Path, command_path: &Path) -> Option<String> {
    capture_command_output_with_timeout(command_path, &["--help"], home, CLI_PROBE_TIMEOUT)
}

fn claude_credentials_paths(home: &std::path::Path) -> Vec<PathBuf> {
    let mut paths = vec![home.join(".claude/.credentials.json")];

    if let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) {
        paths.push(appdata.join("Claude/.credentials.json"));
    }

    if let Some(local_appdata) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        paths.push(local_appdata.join("Claude/.credentials.json"));
    }

    paths
}

fn has_claude_credentials(home: &std::path::Path) -> bool {
    claude_credentials_paths(home)
        .into_iter()
        .any(|path| path.exists() && safe_read_json::<serde_json::Value>(&path).is_some())
}

/// Major version from `opencode --version` (`opencode v2.0.3` → 2).
fn opencode_major_version(bin_path: &Path) -> Option<u32> {
    let output =
        capture_command_output_with_timeout(bin_path, &["--version"], &homedir(), CLI_PROBE_TIMEOUT)?;
    parse_opencode_major_version(&output)
}

fn parse_opencode_major_version(output: &str) -> Option<u32> {
    output.split_whitespace().find_map(|token| {
        let (major, rest) = token.trim_start_matches('v').split_once('.')?;
        if rest.is_empty() || !major.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        major.parse().ok()
    })
}

/// Run `opencode models`. On 2.x the command talks to a shared background
/// server that returns an empty or partial list while it is still starting
/// (observed on 2.0.3: 0 → 59 → 87 routes over ~5s), so poll briefly until the
/// list stops growing. All attempts share `OPENCODE_MODELS_BUDGET`, so a CLI
/// that keeps timing out can't hold up detection for 4 × 3s plus the sleeps.
fn capture_opencode_models(bin_path: &Path, retry_while_warming: bool) -> Option<String> {
    let line_count = |output: &Option<String>| {
        output.as_deref().map_or(0, |text| {
            text.lines().filter(|line| !line.trim().is_empty()).count()
        })
    };
    let attempts = if retry_while_warming { 4 } else { 1 };
    let deadline = Instant::now() + OPENCODE_MODELS_BUDGET;
    let mut best: Option<String> = None;
    for attempt in 0..attempts {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let output = capture_command_output_with_timeout(
            bin_path,
            &["models"],
            &homedir(),
            CLI_PROBE_TIMEOUT.min(remaining),
        );
        if line_count(&output) > line_count(&best) {
            best = output;
        } else if line_count(&best) > 0 {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if attempt + 1 == attempts
            || remaining <= OPENCODE_WARMUP_RETRY_DELAY + Duration::from_secs(1)
        {
            break;
        }
        thread::sleep(OPENCODE_WARMUP_RETRY_DELAY);
    }
    best
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenCodeConfig {
    /// opencode 1.x key.
    pub provider: Option<HashMap<String, ProviderConfig>>,
    /// opencode 2.x renamed `provider` to `providers`.
    pub providers: Option<HashMap<String, ProviderConfig>>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderConfig {
    pub options: Option<ProviderOptions>,
    pub models: Option<HashMap<String, ModelConfig>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProviderOptions {
    #[serde(rename = "apiKey")]
    pub api_key: Option<String>,
    #[serde(rename = "baseURL")]
    pub base_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModelConfig {
    pub name: Option<String>,
}

pub struct ProviderRegistry;

impl ProviderRegistry {
    pub fn detect_all() -> Vec<DetectedProviderProfile> {
        crate::providers::detect_all()
    }

    pub fn detect_all_mock() -> Vec<DetectedProviderProfile> {
        vec![
            DetectedProviderProfile {
                kind: ProviderKind::Opencode,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "opencode/glm-5-turbo".to_string(),
                    display_name: "GLM-5 Turbo (Mock)".to_string(),
                    source_provider: Some("opencode".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                login_command: None,
                install_url: None,
                detected_at: detected_at_now(),
            },
            DetectedProviderProfile {
                kind: ProviderKind::Claude,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "claude-sonnet-4-20250514".to_string(),
                    display_name: "Claude Sonnet 4 (Mock)".to_string(),
                    source_provider: Some("claude".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                login_command: None,
                install_url: None,
                detected_at: detected_at_now(),
            },
            // Appended last so the default model selection (first selectable
            // entry) stays on the opencode mock and existing e2e specs are
            // unaffected. The alias mirrors a real Kimi Code KAT route.
            DetectedProviderProfile {
                kind: ProviderKind::Kimi,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "wanqing-streamlake/kat-coder-pro-v2.5".to_string(),
                    display_name: "KAT Coder Pro (Mock)".to_string(),
                    source_provider: Some("kimi".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                login_command: None,
                install_url: None,
                detected_at: detected_at_now(),
            },
            DetectedProviderProfile {
                kind: ProviderKind::Pi,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "anthropic/claude-sonnet-4".to_string(),
                    display_name: "Claude Sonnet 4 via Pi (Mock)".to_string(),
                    source_provider: Some("pi".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                login_command: None,
                install_url: None,
                detected_at: detected_at_now(),
            },
            DetectedProviderProfile {
                kind: ProviderKind::Kiro,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "claude-sonnet-4-5".to_string(),
                    display_name: "Claude Sonnet 4.5 via Kiro (Mock)".to_string(),
                    source_provider: Some("kiro".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                login_command: None,
                install_url: None,
                detected_at: detected_at_now(),
            },
            DetectedProviderProfile {
                kind: ProviderKind::Aider,
                installed: true,
                authenticated: true,
                runnable: true,
                subscription_label: "mock".to_string(),
                current_models: vec![DetectedModelOption {
                    model_id: "claude-sonnet-4-6".to_string(),
                    display_name: "Claude Sonnet 4.6 via Aider (Mock)".to_string(),
                    source_provider: Some("aider".to_string()),
                    family: None,
                    subscription_label: "mock".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                }],
                login_command: None,
                install_url: None,
                detected_at: detected_at_now(),
            },
        ]
    }

    pub fn detect_opencode() -> DetectedProviderProfile {
        let opencode_path = which_binary("opencode");
        let installed = opencode_path.is_some();

        let mut models = Vec::new();
        let mut authenticated = false;

        // 1. Detect from ~/.config/opencode/opencode.json (user custom models)
        if let Some(config_path) = opencode_config_path().filter(|path| path.exists()) {
            authenticated = true;
            if let Some(config) = safe_read_json::<OpenCodeConfig>(config_path) {
                for providers in [config.provider, config.providers].into_iter().flatten() {
                    for (provider_id, provider_data) in providers {
                        if let Some(model_list) = provider_data.models {
                            for (model_id, model_config) in model_list {
                                let display_name =
                                    model_config.name.unwrap_or_else(|| model_id.clone());
                                models.push(DetectedModelOption {
                                    model_id: format!("{}/{}", provider_id, model_id),
                                    display_name,
                                    source_provider: Some(provider_id.clone()),
                                    family: None,
                                    subscription_label: "custom-provider".into(),
                                    supports_pair_execution: true,
                                    runnable: true,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 2. Detect from ~/.local/share/opencode/auth.json (internal providers via /connect)
        let mut internal_providers = Vec::new();
        if let Some(auth_path) = opencode_auth_path().filter(|path| path.exists()) {
            authenticated = true;
            if let Some(auth_data) = safe_read_json::<serde_json::Value>(auth_path) {
                if let Some(obj) = auth_data.as_object() {
                    for provider_id in obj.keys() {
                        internal_providers.push(provider_id.clone());
                    }
                }
            }
        }

        // 3. Detect from 'opencode models' command output
        if installed {
            let bin_path = opencode_path.expect("opencode path should be resolved");
            // opencode 2.x keeps credentials in its SQLite store (auth.json is no
            // longer written) and `opencode models` lists only connected or
            // configured providers, so every listed route is runnable. 1.x
            // listed the whole models.dev catalog and still needs the filter.
            let lists_only_connected = opencode_major_version(&bin_path)
                .is_some_and(|major| major >= 2);
            if let Some(content) = capture_opencode_models(&bin_path, lists_only_connected) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }

                    // Check if we already added this model from config
                    if models.iter().any(|m| m.model_id == line) {
                        continue;
                    }

                    let parts: Vec<&str> = line.split('/').collect();
                    if parts.len() >= 2 {
                        let provider_id = parts[0];
                        let model_name = parts[1..].join("/");

                        // Check if this model belongs to an authenticated provider (either internal or custom).
                        // The "opencode" provider_id represents zen-backed models that are available
                        // whenever opencode is installed — they don't appear in auth.json.
                        let is_authenticated = lists_only_connected
                            || provider_id == "opencode"
                            || internal_providers.contains(&provider_id.to_string())
                            || models
                                .iter()
                                .any(|m| m.source_provider.as_deref() == Some(provider_id));

                        if is_authenticated {
                            if lists_only_connected {
                                authenticated = true;
                            }
                            // Derive family from model name for OpenCode models
                            // e.g., "minimax-m2.5" -> "minimax", "claude-3-5-sonnet" -> "claude"
                            let family = if provider_id == "opencode" {
                                model_name.split('-').next().map(|s| s.to_string())
                            } else {
                                None
                            };

                            models.push(DetectedModelOption {
                                model_id: line.to_string(),
                                display_name: model_name,
                                source_provider: Some(provider_id.to_string()),
                                family,
                                subscription_label: if provider_id == "opencode" {
                                    "zen-backed".into()
                                } else {
                                    "internal-provider".into()
                                },
                                supports_pair_execution: true,
                                runnable: true,
                            });
                        }
                    }
                }
            }
        }

        DetectedProviderProfile {
            kind: ProviderKind::Opencode,
            installed,
            authenticated,
            runnable: installed,
            subscription_label: "multi-provider".into(),
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_codex() -> DetectedProviderProfile {
        let installed = which_binary_exists("codex");

        let homedir = homedir();
        let auth_path = homedir.join(".codex/auth.json");
        let config_path = homedir.join(".codex/config.toml");

        let authenticated = auth_path.exists();
        let subscription_label = "subscription-backed".to_string();
        let models = if installed {
            build_detected_models(
                discover_codex_model_ids(&homedir),
                "openai",
                &subscription_label,
            )
        } else {
            Vec::new()
        };

        DetectedProviderProfile {
            kind: ProviderKind::Codex,
            installed,
            authenticated,
            runnable: installed && (authenticated || config_path.exists()),
            subscription_label,
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_claude() -> DetectedProviderProfile {
        let claude_path = which_binary("claude");
        let installed = claude_path.is_some();

        let homedir = homedir();
        let mut authenticated = false;
        let mut subscription_label = "api-backed".to_string();
        let mut model_ids = Vec::new();

        if installed {
            let bin_path = claude_path.expect("claude path should be resolved");
            if let Some(status_str) = capture_command_output_with_timeout(
                &bin_path,
                &["auth", "status"],
                &homedir,
                CLI_PROBE_TIMEOUT,
            ) {
                if let Ok(status) = serde_json::from_str::<serde_json::Value>(&status_str) {
                    if status
                        .get("loggedIn")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        authenticated = true;
                        subscription_label = "subscription-backed".to_string();
                    }
                }
            }

            if !authenticated && has_claude_credentials(&homedir) {
                authenticated = true;
                subscription_label = "subscription-backed".to_string();
            }

            // Also check for ANTHROPIC_API_KEY env var as fallback auth
            if !authenticated && std::env::var("ANTHROPIC_API_KEY").is_ok() {
                authenticated = true;
            }

            model_ids = discover_claude_model_ids(&homedir, Some(&bin_path));
        }

        let mut claude_models = build_detected_models(model_ids, "anthropic", &subscription_label);
        for model in &mut claude_models {
            model.display_name = beautify_claude_display_name(&model.model_id);
        }

        DetectedProviderProfile {
            kind: ProviderKind::Claude,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label,
            current_models: claude_models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_gemini() -> DetectedProviderProfile {
        // Antigravity CLI (`agy`) is Google's successor to the Gemini CLI,
        // which stopped serving requests on 2026-06-18.
        if let Some(agy_bin) = which_binary("agy") {
            let models = discover_antigravity_model_ids(&agy_bin);
            let authenticated = !models.is_empty();
            return DetectedProviderProfile {
                kind: ProviderKind::Gemini,
                installed: true,
                authenticated,
                runnable: authenticated,
                subscription_label: "antigravity-backed".into(),
                current_models: models,
                login_command: None,
                install_url: None,
                detected_at: detected_at_now(),
            };
        }

        // agy not found - report as not installed.
        DetectedProviderProfile {
            kind: ProviderKind::Gemini,
            installed: false,
            authenticated: false,
            runnable: false,
            subscription_label: "antigravity-backed".into(),
            current_models: Vec::new(),
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_kimi() -> DetectedProviderProfile {
        let installed = which_binary_exists("kimi");
        let models = if installed {
            discover_kimi_models(&homedir())
        } else {
            Vec::new()
        };
        // Kimi Code only lists model aliases in its config after the user has
        // logged in or configured a provider, so a non-empty catalog doubles
        // as the authentication signal (same heuristic as Antigravity).
        let authenticated = !models.is_empty();

        DetectedProviderProfile {
            kind: ProviderKind::Kimi,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label: "kimi-code".into(),
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_muse() -> DetectedProviderProfile {
        let installed = which_binary_exists("muse");
        let models = if installed {
            discover_muse_models(&homedir())
        } else {
            Vec::new()
        };
        // `muse login --help` states META_API_KEY takes priority over the stored
        // account login, so either signal counts as authenticated.
        let authenticated = installed && muse_authenticated(&homedir());

        DetectedProviderProfile {
            kind: ProviderKind::Muse,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label: "muse".into(),
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_pi() -> DetectedProviderProfile {
        let pi_bin = which_binary("pi");
        let installed = pi_bin.is_some();
        let models = if let Some(ref bin) = pi_bin {
            discover_pi_models(bin)
        } else {
            Vec::new()
        };
        // Pi exposes available models only after at least one provider is
        // configured, so a non-empty catalog means the user is authenticated.
        let authenticated = !models.is_empty();

        DetectedProviderProfile {
            kind: ProviderKind::Pi,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label: "pi".into(),
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_kiro() -> DetectedProviderProfile {
        let kiro_bin = which_binary("kiro-cli");
        let installed = kiro_bin.is_some();

        // Auth: check KIRO_API_KEY env var or kiro-cli whoami output.
        let has_api_key = std::env::var("KIRO_API_KEY")
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let logged_in = has_api_key
            || kiro_bin
                .as_ref()
                .map(|bin| {
                    let out = capture_command_output_with_timeout(
                        bin,
                        &["whoami"],
                        &homedir(),
                        CLI_PROBE_TIMEOUT,
                    )
                    .unwrap_or_default();
                    out.contains("Logged in")
                })
                .unwrap_or(false);
        let authenticated = logged_in;
        // `chat --list-models` starts Kiro's browser login flow when signed out
        // (kiro-cli 2.23.0), so only list models once auth is confirmed.
        let models = match kiro_bin.as_ref() {
            Some(bin) if authenticated => discover_kiro_models(bin),
            _ => Vec::new(),
        };

        DetectedProviderProfile {
            kind: ProviderKind::Kiro,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label: "kiro".into(),
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_aider() -> DetectedProviderProfile {
        let installed = which_binary_exists("aider");
        let homedir = homedir();

        // Aider is BYOK — auth means having an API key available. The config
        // file alone may specify a model but without a key the provider can't
        // run. Besides the environment, aider loads keys from
        // `~/.aider/oauth-keys.env` (written by its OpenRouter sign-in) and
        // `~/.env` (aider-chat 0.86.2 `main.py`).
        const AIDER_KEY_VARS: [&str; 6] = [
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
            "GEMINI_API_KEY",
            "DEEPSEEK_API_KEY",
            "OPENROUTER_API_KEY",
            "AZURE_API_KEY",
        ];
        let has_env_key = AIDER_KEY_VARS
            .iter()
            .any(|k| std::env::var(k).map(|v| !v.is_empty()).unwrap_or(false));
        let file_has_key = |path: PathBuf| {
            fs::read_to_string(path)
                .map(|content| dotenv_defines_any(&content, &AIDER_KEY_VARS))
                .unwrap_or(false)
        };

        let authenticated = has_env_key
            || file_has_key(homedir.join(".aider/oauth-keys.env"))
            || file_has_key(homedir.join(".env"));
        let subscription_label = "byok".to_string();
        let models = if installed && authenticated {
            discover_aider_models(&homedir)
        } else {
            Vec::new()
        };

        DetectedProviderProfile {
            kind: ProviderKind::Aider,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label,
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }

    pub fn detect_grok() -> DetectedProviderProfile {
        let installed = which_binary_exists("grok");
        let homedir = homedir();

        // Grok Build caches OAuth credentials in `~/.grok/auth.json`; headless
        // environments can instead authenticate with `XAI_API_KEY`.
        let authenticated = homedir.join(".grok/auth.json").exists()
            || std::env::var("XAI_API_KEY")
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false);

        let models = if installed {
            grok_model_options(&homedir)
        } else {
            Vec::new()
        };

        DetectedProviderProfile {
            kind: ProviderKind::Grok,
            installed,
            authenticated,
            runnable: installed && authenticated,
            subscription_label: "xai".into(),
            current_models: models,
            login_command: None,
            install_url: None,
            detected_at: detected_at_now(),
        }
    }
}

/// Discover models from Antigravity CLI via `agy models`. The CLI prints one
/// `slug<TAB>Display Name` pair per line (e.g. `gemini-3.7-flash-high<TAB>Gemini
/// 3.7 Flash (High)`); the slug is the value `--model` accepts. We surface only
/// the Gemini-family slugs here: agy also offers Claude/GPT models, but those
/// are owned by the native Claude/Codex providers and would misroute if
/// inferred from the display name.
fn discover_antigravity_model_ids(agy_bin: &Path) -> Vec<DetectedModelOption> {
    // agy models may hit the network on first run; give it more headroom than the
    // default 3s CLI probe.
    let output = capture_command_output_with_timeout(
        agy_bin,
        &["models"],
        &homedir(),
        Duration::from_secs(8),
    )
    .unwrap_or_default();

    parse_antigravity_model_lines(&output)
}

/// Parse the `agy models` output into model options. The CLI prints one
/// `slug<TAB>Display Name` pair per line; the slug is what `--model` accepts.
fn parse_antigravity_model_lines(output: &str) -> Vec<DetectedModelOption> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| line.split_once('\t'))
        .filter(|(slug, _display)| slug.to_ascii_lowercase().starts_with("gemini-"))
        .map(|(slug, display)| DetectedModelOption {
            model_id: slug.to_string(),
            display_name: display.trim().to_string(),
            source_provider: Some("google".to_string()),
            family: Some("gemini".to_string()),
            subscription_label: "antigravity-backed".to_string(),
            supports_pair_execution: true,
            runnable: true,
        })
        .collect()
}

/// Discover Kimi Code model aliases from `$KIMI_CODE_HOME/config.toml`
/// (default `~/.kimi-code/config.toml`). Every
/// `[models."<alias>"]` section is a runnable `--model` value; `display_name`
/// keys inside a section provide the human-readable label.
/// Model ids Muse Code is known to serve. Muse ships no `models list`
/// subcommand and silently accepts unknown `--model` values (a bogus id still
/// completes a run), so the catalog cannot be probed from the CLI. These two
/// ids are seeded and then unioned with whatever the user has configured, so a
/// newer model appears as soon as they select it in Muse itself.
const MUSE_SEED_MODELS: &[&str] = &["muse-spark-1.3", "muse-spark-1.2"];

fn muse_settings_path(home: &std::path::Path) -> PathBuf {
    home.join(".config/muse/settings.json")
}

#[derive(Deserialize)]
struct MuseSettings {
    #[serde(default)]
    model: Option<String>,
}

#[derive(Deserialize)]
struct MuseAuth {
    #[serde(default)]
    providers: HashMap<String, serde_json::Value>,
}

/// Muse is authenticated when `META_API_KEY` is set (it takes priority over the
/// account login) or `auth.json` holds a provider credential record.
fn muse_authenticated(home: &std::path::Path) -> bool {
    if std::env::var("META_API_KEY")
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return true;
    }

    safe_read_json::<MuseAuth>(home.join(".config/muse/auth.json"))
        .is_some_and(|auth| !auth.providers.is_empty())
}

fn muse_model_option(model_id: &str) -> DetectedModelOption {
    DetectedModelOption {
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
        source_provider: Some("meta".to_string()),
        family: None,
        subscription_label: "muse".to_string(),
        supports_pair_execution: true,
        runnable: true,
    }
}

/// Seed catalog unioned with the user's configured model. The configured model
/// leads so the picker defaults to what Muse itself would use.
fn discover_muse_models(home: &std::path::Path) -> Vec<DetectedModelOption> {
    let configured = safe_read_json::<MuseSettings>(muse_settings_path(home))
        .and_then(|settings| settings.model)
        .map(|model| model.trim().to_string())
        .filter(|model| !model.is_empty());

    let mut models: Vec<DetectedModelOption> = Vec::new();
    if let Some(configured) = configured {
        models.push(muse_model_option(&configured));
    }
    for seed in MUSE_SEED_MODELS {
        if !models.iter().any(|model| model.model_id == *seed) {
            models.push(muse_model_option(seed));
        }
    }
    models
}

fn discover_kimi_models(home: &std::path::Path) -> Vec<DetectedModelOption> {
    // Kimi Code keeps its config under `KIMI_CODE_HOME`, falling back to
    // `~/.kimi-code` when the variable is unset (kimi-code 2.0.2 docs).
    let kimi_home = std::env::var_os("KIMI_CODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".kimi-code"));
    let config_path = kimi_home.join("config.toml");
    match fs::read_to_string(config_path) {
        Ok(content) => parse_kimi_model_aliases(&content),
        Err(_) => Vec::new(),
    }
}

fn parse_kimi_model_aliases(content: &str) -> Vec<DetectedModelOption> {
    let mut models: Vec<DetectedModelOption> = Vec::new();
    let mut current_alias: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            current_alias = line
                .strip_prefix("[models.")
                .and_then(|rest| rest.strip_suffix(']'))
                .map(|key| key.trim().trim_matches('"').to_string())
                // A leftover quote means a nested sub-table header
                // (e.g. `[models."x".extras]`), not a model alias.
                .filter(|alias| !alias.is_empty() && !alias.contains('"'));

            if let Some(alias) = current_alias.as_ref() {
                if !models.iter().any(|model| &model.model_id == alias) {
                    models.push(DetectedModelOption {
                        display_name: alias.clone(),
                        model_id: alias.clone(),
                        source_provider: Some("kimi".to_string()),
                        family: None,
                        subscription_label: "kimi-code".to_string(),
                        supports_pair_execution: true,
                        runnable: true,
                    });
                }
            }
            continue;
        }

        if let (Some(alias), true) = (current_alias.as_ref(), line.starts_with("display_name")) {
            if let Some(display_name) = line.split('"').nth(1).filter(|name| !name.is_empty()) {
                if let Some(model) = models.iter_mut().find(|model| &model.model_id == alias) {
                    model.display_name = display_name.to_string();
                }
            }
        }
    }

    models
}

/// Build Grok Build's model list: the built-in models (Grok Build 1.0.40's
/// bundled `default_models.json`, default `grok-4.6`) plus any custom models
/// declared in `~/.grok/config.toml`. Custom `[model.<alias>]` sections are
/// addressed by alias through `-m`; `name` provides the display label.
fn grok_model_options(home: &std::path::Path) -> Vec<DetectedModelOption> {
    let mut models: Vec<DetectedModelOption> = [("grok-4.6", "Grok 4.6"), ("grok-4.5", "Grok 4.5")]
        .into_iter()
        .map(|(model_id, display_name)| DetectedModelOption {
            model_id: model_id.to_string(),
            display_name: display_name.to_string(),
            source_provider: Some("xai".to_string()),
            family: None,
            subscription_label: "xai".to_string(),
            supports_pair_execution: true,
            runnable: true,
        })
        .collect();

    if let Ok(content) = fs::read_to_string(home.join(".grok/config.toml")) {
        for model in parse_grok_config_models(&content) {
            if !models.iter().any(|m| m.model_id == model.model_id) {
                models.push(model);
            }
        }
    }

    models
}

/// Parse `[model.<alias>]` sections from a Grok Build `config.toml`. The alias
/// is the value `-m` accepts; `name = "…"` (when present) is the display name.
/// Non-model tables (`[models]`, `[mcp…]`, …) are ignored.
fn parse_grok_config_models(content: &str) -> Vec<DetectedModelOption> {
    let mut models: Vec<DetectedModelOption> = Vec::new();
    let mut current_alias: Option<String> = None;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') {
            current_alias = line
                .strip_prefix("[model.")
                .and_then(|rest| rest.strip_suffix(']'))
                .map(|key| key.trim().trim_matches('"').to_string())
                // A leftover quote means a nested sub-table header
                // (e.g. `[model."x".extra]`), not a model alias.
                .filter(|alias| !alias.is_empty() && !alias.contains('"'));
            if let Some(alias) = current_alias.clone() {
                models.push(DetectedModelOption {
                    display_name: alias.clone(),
                    model_id: alias,
                    source_provider: Some("xai".to_string()),
                    family: None,
                    subscription_label: "xai".to_string(),
                    supports_pair_execution: true,
                    runnable: true,
                });
            }
            continue;
        }

        if let (Some(alias), true) = (current_alias.as_ref(), line.starts_with("name")) {
            if let Some(name) = line.split('"').nth(1).filter(|name| !name.is_empty()) {
                if let Some(model) = models
                    .iter_mut()
                    .rev()
                    .find(|model| model.model_id == *alias)
                {
                    model.display_name = name.to_string();
                }
            }
        }
    }

    models
}

/// Discover Pi models via `pi --list-models`. The CLI prints a padded table
/// (verified against pi 0.79.2, 2026-09-23):
///
/// ```text
/// provider   model                      context  max-out  thinking  images
/// anthropic  claude-3-5-haiku-20241022  200K     8.2K     no        yes
/// ```
///
/// `--model` takes `provider/model`, so each row maps to that id. Models from
/// all configured providers are surfaced.
fn discover_pi_models(pi_bin: &Path) -> Vec<DetectedModelOption> {
    // pi --list-models may need to query provider catalogs; give it more
    // headroom than the default 3s CLI probe.
    let output = capture_command_output_with_timeout(
        pi_bin,
        &["--list-models"],
        &homedir(),
        Duration::from_secs(8),
    )
    .unwrap_or_default();

    parse_pi_list_models(&output)
}

/// Parse the `pi --list-models` table. Rows are only trusted after the
/// `provider  model …` header, so notices like "No models available" and any
/// banner text above the table never become model ids.
fn parse_pi_list_models(output: &str) -> Vec<DetectedModelOption> {
    let mut seen_header = false;
    output
        .lines()
        .filter_map(|line| {
            let mut columns = line.split_whitespace();
            let (provider, model) = (columns.next()?, columns.next()?);
            if !seen_header {
                seen_header = provider == "provider" && model == "model";
                return None;
            }
            let model_id = format!("{provider}/{model}");
            Some(DetectedModelOption {
                model_id: model_id.clone(),
                display_name: model_id,
                source_provider: Some(provider.to_string()),
                family: None,
                subscription_label: "pi".to_string(),
                supports_pair_execution: true,
                runnable: true,
            })
        })
        .collect()
}

/// Discover Kiro models via `kiro-cli chat --list-models`. The CLI prints a
/// header plus one row per model (kiro-cli 2.23.0), the default marked `*`:
///
/// ```text
/// Available models (* = default):
/// * auto                1.00x credits   Models chosen by task
///   claude-sonnet-4.5   1.30x credits   The Claude Sonnet 4.5 model
/// ```
fn discover_kiro_models(kiro_bin: &Path) -> Vec<DetectedModelOption> {
    let output = capture_command_output_with_timeout(
        kiro_bin,
        &["chat", "--list-models"],
        &homedir(),
        Duration::from_secs(8),
    )
    .unwrap_or_default();

    parse_kiro_model_list(&output)
}

fn parse_kiro_model_list(output: &str) -> Vec<DetectedModelOption> {
    let mut models: Vec<DetectedModelOption> = Vec::new();
    for line in output.lines().map(str::trim) {
        if line.is_empty()
            || line.ends_with(':')
            || line.starts_with("No ")
            || line.starts_with("Error")
        {
            continue;
        }
        let Some(model_id) = line
            .trim_start_matches(|c: char| c == '*' || c == '-' || c.is_whitespace())
            .split_whitespace()
            .next()
            .filter(|id| is_plausible_model_id(id))
        else {
            continue;
        };
        if models.iter().any(|model| model.model_id == model_id) {
            continue;
        }
        models.push(DetectedModelOption {
            model_id: model_id.to_string(),
            display_name: model_id.to_string(),
            source_provider: Some("kiro".to_string()),
            family: None,
            subscription_label: "kiro".to_string(),
            supports_pair_execution: true,
            runnable: true,
        });
    }
    models
}

/// Whether a dotenv file assigns a non-empty value to any of `keys`
/// (`KEY=value`, optionally prefixed with `export`).
fn dotenv_defines_any(content: &str, keys: &[&str]) -> bool {
    content.lines().any(|line| {
        let line = line.trim_start();
        let line = line.strip_prefix("export ").unwrap_or(line).trim_start();
        keys.iter().any(|key| {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix('='))
                .is_some_and(|value| !value.trim().trim_matches(['"', '\'']).is_empty())
        })
    })
}

/// Discover Aider models from `~/.aider.conf.yml`. Aider's `--list-models`
/// only searches its built-in catalog of known models, not what the user can
/// run, so we parse the config file for a `model:` key and also surface a
/// small static fallback list of common BYOK models so the picker is never
/// empty when the user has an API key configured.
fn discover_aider_models(home: &std::path::Path) -> Vec<DetectedModelOption> {
    let subscription_label = "byok".to_string();
    let mut models = Vec::new();

    // 1. Parse ~/.aider.conf.yml for a configured model.
    let config_path = home.join(".aider.conf.yml");
    if let Ok(content) = fs::read_to_string(&config_path) {
        for raw_line in content.lines() {
            let line = raw_line.trim();
            // Model ids may themselves contain `:` (Bedrock `…-v1:0`, Ollama
            // `qwen:7b`), so only the key prefix is stripped.
            if let Some(rest) = line.strip_prefix("model:") {
                let model_id = rest.trim().trim_matches(|c| c == '"' || c == '\'');
                if !model_id.is_empty() && is_plausible_model_id(&model_id.replace(':', "-")) {
                    models.push(DetectedModelOption {
                        model_id: model_id.to_string(),
                        display_name: model_id.to_string(),
                        source_provider: Some("aider".to_string()),
                        family: None,
                        subscription_label: subscription_label.clone(),
                        supports_pair_execution: true,
                        runnable: true,
                    });
                }
            }
        }
    }

    // 2. Static fallback: common models users are likely to have keys for.
    // Only added if the config didn't already list them. The IDs resolve in
    // aider-chat 0.86.2's model metadata.
    let fallbacks = [
        "claude-sonnet-5",
        "claude-opus-5",
        "claude-haiku-4-5",
        "gpt-5.4",
        "gpt-5.4-mini",
        "gemini-2.5-pro",
        // litellm has no provider for the bare `deepseek-coder-v3`; this is
        // the DeepSeek id aider-chat 0.86.2 knows.
        "deepseek/deepseek-chat",
    ];
    for model_id in &fallbacks {
        if !models.iter().any(|m| &m.model_id == model_id) {
            models.push(DetectedModelOption {
                model_id: model_id.to_string(),
            display_name: model_id.to_string(),
            source_provider: Some("aider".to_string()),
            family: None,
            subscription_label: subscription_label.clone(),
            supports_pair_execution: true,
            runnable: true,
            });
        }
    }

    models
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::time::Duration;
    use uuid::Uuid;

    #[test]
    fn opencode_help_detects_variant_syntax() {
        // OpenCode 2.x: variant suffix on --model id.
        assert_eq!(
            opencode_variant_syntax_from_help(
                "Usage: opencode run [flags] [<message>]\n  --model, -m string   Model to use in the format provider/model#variant\n"
            ),
            OpencodeVariantSyntax::Suffix
        );
        // OpenCode 1.x: separate --variant flag.
        assert_eq!(
            opencode_variant_syntax_from_help(
                "Usage: opencode run [message]\n  --variant  model variant\n"
            ),
            OpencodeVariantSyntax::Flag
        );
        // No variant support at all.
        assert_eq!(
            opencode_variant_syntax_from_help(
                "Usage: opencode run [message]\n  --model  model id\n"
            ),
            OpencodeVariantSyntax::Unsupported
        );
    }

    #[test]
    fn extract_pi_thinking_levels_parses_help_line() {
        // pi 0.79.2 help (verified 2026-09-19): no `max`.
        let levels = extract_pi_thinking_levels(
            "  --thinking <level>             Set thinking level: off, minimal, low, medium, high, xhigh\n",
        )
        .expect("thinking line should parse");
        assert_eq!(
            levels,
            vec!["off", "minimal", "low", "medium", "high", "xhigh"]
        );
        assert!(!levels.iter().any(|level| level == "max"));

        // pi 0.80.6+ re-adds `max` (GPT-5.6 / adaptive Claude models).
        let levels = extract_pi_thinking_levels(
            "  --thinking <level>             Set thinking level: off, minimal, low, medium, high, xhigh, max\n",
        )
        .expect("thinking line should parse");
        assert!(levels.iter().any(|level| level == "max"));
    }

    #[test]
    fn parse_antigravity_model_lines_uses_slug_ids_and_display_names() {
        let output = "gemini-3.7-flash-high\tGemini 3.7 Flash (High)\n\
                      gemini-3.5-flash-low\tGemini 3.5 Flash (Low)\n\
                      claude-sonnet-4-6\tClaude Sonnet 4.6 (Thinking)\n\
                      gpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n\
                      fetching models...\n";

        let models = parse_antigravity_model_lines(output);

        assert_eq!(models.len(), 2, "only Gemini-family slugs should surface");
        assert_eq!(models[0].model_id, "gemini-3.7-flash-high");
        assert_eq!(models[0].display_name, "Gemini 3.7 Flash (High)");
        assert_eq!(models[1].model_id, "gemini-3.5-flash-low");
        assert_eq!(models[1].display_name, "Gemini 3.5 Flash (Low)");
        assert!(models.iter().all(|m| m.runnable && m.supports_pair_execution));
    }

    #[test]
    fn parse_opencode_major_version_reads_semver_major() {
        assert_eq!(parse_opencode_major_version("opencode v2.0.3\n"), Some(2));
        assert_eq!(parse_opencode_major_version("1.18.30"), Some(1));
        assert_eq!(parse_opencode_major_version("opencode dev build"), None);
        assert_eq!(parse_opencode_major_version(""), None);
    }

    #[test]
    fn opencode_config_accepts_v1_and_v2_provider_keys() {
        let v1: OpenCodeConfig =
            serde_json::from_str(r#"{"provider": {"ollama": {"models": {"qwen": {}}}}}"#)
                .expect("1.x config parses");
        assert!(v1.provider.is_some() && v1.providers.is_none());

        let v2: OpenCodeConfig =
            serde_json::from_str(r#"{"providers": {"ollama": {"models": {"qwen": {}}}}}"#)
                .expect("2.x config parses");
        assert!(v2.providers.is_some() && v2.provider.is_none());
    }

    #[test]
    fn claude_help_parser_reads_only_the_model_block() {
        // Layout of `claude --help` 2.1.280: the `--model` description wraps,
        // contains an apostrophe, and is followed by flags quoting other words.
        let help = "\
  --permission-mode <mode>              Permission mode ('acceptEdits', 'auto',
                                        'plan')
  --model <model>                       Model for the current session. Provide
                                        an alias for the latest model (e.g.
                                        'fable', 'opus', or 'sonnet') or a
                                        model's full name (e.g.
                                        'claude-fable-5').
  -n, --name <name>                     Set a display name ('reviewer')
  --output-format <format>              'text', 'json', or 'stream-json'
";
        let predicate = |value: &str| is_claude_model_id(value) && is_plausible_model_id(value);
        let mut ids = Vec::new();
        collect_model_ids_from_help_line(help, &predicate, &mut ids);
        assert_eq!(ids, vec!["claude-fable-5".to_string()]);

        let mut none = Vec::new();
        collect_model_ids_from_help_line("  --verbose  'claude-x-1'\n", &predicate, &mut none);
        assert!(none.is_empty(), "no --model block means no help-derived models");
    }

    #[test]
    fn discover_aider_models_keeps_colons_in_configured_model_ids() {
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_home).expect("failed to create temp home");
        fs::write(
            temp_home.join(".aider.conf.yml"),
            "model: bedrock/anthropic.claude-3-5-sonnet-20240620-v1:0\n",
        )
        .expect("failed to write aider config");

        let models = discover_aider_models(&temp_home);
        let _ = fs::remove_dir_all(&temp_home);

        assert_eq!(
            models[0].model_id,
            "bedrock/anthropic.claude-3-5-sonnet-20240620-v1:0"
        );
        assert!(models.iter().any(|m| m.model_id == "deepseek/deepseek-chat"));
        assert!(!models.iter().any(|m| m.model_id == "deepseek-coder-v3"));
    }

    #[test]
    fn dotenv_defines_any_matches_assigned_keys_only() {
        let keys = ["OPENROUTER_API_KEY", "OPENAI_API_KEY"];
        assert!(dotenv_defines_any("OPENROUTER_API_KEY=sk-or-1\n", &keys));
        assert!(dotenv_defines_any("# keys\nexport OPENAI_API_KEY=\"sk-1\"\n", &keys));
        assert!(!dotenv_defines_any("OPENAI_API_KEY=\nOTHER=1\n", &keys));
        assert!(!dotenv_defines_any("OPENAI_API_KEY_OLD=sk-1\n", &keys));
    }

    #[test]
    fn parse_kiro_model_list_reads_first_column() {
        let output = "Available models (* = default):\n\
                      * auto                1.00x credits   Models chosen by task\n\
                      \x20 claude-sonnet-4.5   1.30x credits   The Claude Sonnet 4.5 model\n\
                      \x20 claude-haiku-4.5    0.40x credits   The Claude Haiku 4.5 model\n";
        let ids: Vec<String> = parse_kiro_model_list(output)
            .into_iter()
            .map(|model| model.model_id)
            .collect();
        assert_eq!(ids, vec!["auto", "claude-sonnet-4.5", "claude-haiku-4.5"]);
    }

    #[test]
    fn parse_pi_list_models_reads_table_rows_as_provider_model_ids() {
        // Verbatim `pi --list-models` layout (pi 0.79.2), incl. trailing padding.
        let output = "provider              model                       context  max-out  thinking  images\n\
                      anthropic             claude-3-5-haiku-20241022   200K     8.2K     no        yes   \n\
                      zai                   glm-4.5-air                 128K     96K      yes       no    \n";

        let models = parse_pi_list_models(output);

        let ids: Vec<&str> = models.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(ids, vec!["anthropic/claude-3-5-haiku-20241022", "zai/glm-4.5-air"]);
        assert_eq!(models[1].source_provider.as_deref(), Some("zai"));

        assert!(parse_pi_list_models("No models available. Configure a provider.\n").is_empty());
    }

    #[test]
    fn parse_kimi_model_aliases_reads_sections_and_display_names() {
        let config = r#"
default_permission_mode = "yolo"
default_model = "ark-coding-plan/glm-5.2"

[providers.ark-coding-plan]
type = "openai"
api_key = "secret"
base_url = "https://example.com/v1"

[models."ark-coding-plan/glm-5.2"]
provider = "ark-coding-plan"
model = "glm-5.2"
display_name = "GLM-5.2 (Ark)"

[models."kimi-code/kimi-for-coding"]
provider = "managed:kimi-code"
model = "kimi-for-coding"
"#;

        let models = parse_kimi_model_aliases(config);
        let ids: Vec<&str> = models.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(ids, vec!["ark-coding-plan/glm-5.2", "kimi-code/kimi-for-coding"]);
        assert_eq!(models[0].display_name, "GLM-5.2 (Ark)");
        // No display_name key → the alias itself is the label.
        assert_eq!(models[1].display_name, "kimi-code/kimi-for-coding");
        assert!(models.iter().all(|m| m.runnable && m.supports_pair_execution));
    }

    #[test]
    fn parse_kimi_model_aliases_ignores_non_model_sections_and_subtables() {
        let config = r#"
[providers.zai]
display_name = "not a model"

[models."kimi-code/k3".extras]
display_name = "sub-table, not an alias"

[models]
display_name = "bare table, not an alias"
"#;

        assert!(parse_kimi_model_aliases(config).is_empty());
    }

    #[test]
    fn parse_grok_config_models_reads_sections_and_names() {
        let config = r#"
[models]
default = "my-model"

[model.my-model]
model = "model-id"
base_url = "https://api.example.com/v1"
name = "Display Name"
env_key = "API_KEY"

[model.pi-proxy]
model = "pi-4.6"
"#;

        let models = parse_grok_config_models(config);
        let ids: Vec<&str> = models.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(ids, vec!["my-model", "pi-proxy"]);
        assert_eq!(models[0].display_name, "Display Name");
        // No `name` key → the alias itself is the label.
        assert_eq!(models[1].display_name, "pi-proxy");
        assert!(models.iter().all(|m| m.runnable && m.supports_pair_execution));
    }

    #[test]
    fn parse_grok_config_models_ignores_non_model_sections_and_subtables() {
        let config = r#"
[mcp.linear]
name = "not a model"

[model."x".extra]
name = "sub-table, not an alias"

[personas.reviewer]
name = "bare table, not an alias"
"#;

        assert!(parse_grok_config_models(config).is_empty());
    }

    #[cfg(unix)]
    fn write_executable_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = dir.join(name);
        fs::write(&path, contents).expect("failed to write test script");
        let mut perms = fs::metadata(&path)
            .expect("failed to read script metadata")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).expect("failed to mark test script executable");
        path
    }

    fn muse_temp_home(settings: Option<&str>) -> PathBuf {
        let home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let config_dir = home.join(".config/muse");
        fs::create_dir_all(&config_dir).expect("failed to create temp muse config dir");
        if let Some(settings) = settings {
            fs::write(config_dir.join("settings.json"), settings)
                .expect("failed to write muse settings");
        }
        home
    }

    #[test]
    fn discover_muse_models_falls_back_to_the_seed_catalog() {
        // Muse ships no `models list` and accepts unknown ids silently, so the
        // seed is the only catalog available without user config.
        let home = muse_temp_home(None);
        let models = discover_muse_models(&home);

        let ids: Vec<&str> = models.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(ids, vec!["muse-spark-1.3", "muse-spark-1.2"]);
        assert!(models.iter().all(|m| m.supports_pair_execution && m.runnable));

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn discover_muse_models_puts_the_configured_model_first() {
        // A model the user selected in Muse itself must appear even though it
        // is not in the seed — that is the whole point of reading settings.
        let home = muse_temp_home(Some(
            r#"{"schema_version":1,"provider":"meta","model":"muse-spark-9.9","reasoning_effort":"xhigh"}"#,
        ));
        let models = discover_muse_models(&home);

        let ids: Vec<&str> = models.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["muse-spark-9.9", "muse-spark-1.3", "muse-spark-1.2"]
        );

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn discover_muse_models_does_not_duplicate_a_configured_seed_model() {
        let home = muse_temp_home(Some(r#"{"model":"muse-spark-1.3"}"#));
        let models = discover_muse_models(&home);

        let ids: Vec<&str> = models.iter().map(|m| m.model_id.as_str()).collect();
        assert_eq!(ids, vec!["muse-spark-1.3", "muse-spark-1.2"]);

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn muse_authentication_accepts_api_key_or_stored_credentials() {
        let _guard = crate::test_env::lock_env();
        let home = muse_temp_home(None);

        // Neither signal present.
        std::env::remove_var("META_API_KEY");
        assert!(!muse_authenticated(&home));

        // An empty key is not a credential.
        std::env::set_var("META_API_KEY", "   ");
        assert!(!muse_authenticated(&home));

        // `muse login --help`: META_API_KEY takes priority over account login.
        std::env::set_var("META_API_KEY", "sk-test");
        assert!(muse_authenticated(&home));
        std::env::remove_var("META_API_KEY");

        // Stored account credentials alone are enough.
        fs::write(
            home.join(".config/muse/auth.json"),
            r#"{"schema_version":1,"providers":{"meta":{"mechanism":"oauth","storage":"keychain"}}}"#,
        )
        .expect("failed to write muse auth");
        assert!(muse_authenticated(&home));

        // An empty provider map is not authentication.
        fs::write(
            home.join(".config/muse/auth.json"),
            r#"{"schema_version":1,"providers":{}}"#,
        )
        .expect("failed to rewrite muse auth");
        assert!(!muse_authenticated(&home));

        fs::remove_dir_all(&home).ok();
    }

    #[cfg(unix)]
    #[test]
    fn detect_opencode_does_not_invent_models_without_local_sources() {
        let _guard = crate::test_env::lock_env();
        // Config/auth locations must come from the temp HOME only.
        let original_xdg: Vec<(&str, Option<OsString>)> =
            ["OPENCODE_CONFIG_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect();
        for (key, _) in &original_xdg {
            std::env::remove_var(key);
        }
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let opencode_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        fs::create_dir_all(&opencode_dir).expect("failed to create temp opencode dir");

        write_executable_script(
            &opencode_dir,
            "opencode",
            r#"#!/bin/sh
if [ "$1" = "models" ]; then
  exit 0
fi
exit 0
"#,
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", opencode_dir.display(), existing.to_string_lossy())
        } else {
            opencode_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);

        let profile = ProviderRegistry::detect_opencode();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        for (key, value) in original_xdg {
            if let Some(value) = value {
                std::env::set_var(key, value);
            }
        }

        assert!(profile.installed);
        assert!(
            profile.current_models.is_empty(),
            "OpenCode should not invent fallback models when no local sources are present"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_claude_lists_models_even_when_auth_status_is_logged_out() {
        let _guard = crate::test_env::lock_env();
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let bin_dir = temp_root.join("bin");
        fs::create_dir_all(&bin_dir).expect("failed to create temp bin dir");

        write_executable_script(
            &bin_dir,
            "claude",
            r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  printf '%s\n' '{"loggedIn":false,"authMethod":"none","apiProvider":"firstParty"}'
elif [ "$1" = "--help" ]; then
  printf '%s\n' "  --model <model>  Use aliases like 'sonnet' and full names like 'claude-sonnet-4-6'"
else
  exit 0
fi
"#,
        );

        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", bin_dir.display(), existing.to_string_lossy())
        } else {
            bin_dir.display().to_string()
        };

        std::env::set_var("PATH", new_path);
        std::env::remove_var("ANTHROPIC_API_KEY");

        let profile = ProviderRegistry::detect_claude();

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        if let Some(value) = original_api_key {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        }

        assert!(profile.installed);
        assert!(!profile.authenticated);
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-sonnet-4-6"),
            "logged-out Claude Code should still expose help-discovered models"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_codex_reads_models_from_config_toml() {
        let _guard = crate::test_env::lock_env();
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let codex_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let codex_config_dir = temp_home.join(".codex");
        fs::create_dir_all(&codex_dir).expect("failed to create temp codex dir");
        fs::create_dir_all(&codex_config_dir).expect("failed to create temp codex config dir");

        write_executable_script(
            &codex_dir,
            "codex",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(
            codex_config_dir.join("config.toml"),
            r#"
model = "gpt-5.4"

[profiles.fast]
model = "gpt-5.4-mini"

[tui]
status_line = ["codex-version", "model"]

[notice.model_migrations]
"gpt-5.1-codex-mini" = "gpt-5.4"
"#,
        )
        .expect("failed to write codex config");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", codex_dir.display(), existing.to_string_lossy())
        } else {
            codex_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);

        let profile = ProviderRegistry::detect_codex();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert!(profile.installed);
        assert!(profile.runnable);
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-5.4-mini"),
            "Codex should expose profile models declared in config.toml"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-5.4"),
            "Codex should expose the primary configured model"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-5.1-codex-mini"),
            "Codex should preserve migration-linked model ids seen in config"
        );
        assert!(
            !profile
                .current_models
                .iter()
                .any(|model| model.model_id == "codex-version"),
            "[tui] status_line items are not models"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_codex_reads_models_from_local_cache() {
        let _guard = crate::test_env::lock_env();
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let codex_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let codex_config_dir = temp_home.join(".codex");
        fs::create_dir_all(&codex_dir).expect("failed to create temp codex dir");
        fs::create_dir_all(&codex_config_dir).expect("failed to create temp codex config dir");

        write_executable_script(
            &codex_dir,
            "codex",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(codex_config_dir.join("config.toml"), r#"model = "gpt-5.4""#)
            .expect("failed to write codex config");

        fs::write(
            codex_config_dir.join("models_cache.json"),
            r#"{
  "models": [
    { "slug": "gpt-9-coder-preview" },
    { "slug": "codex-ultra-latest" },
    { "slug": "codex-auto-review", "visibility": "hide" }
  ]
}"#,
        )
        .expect("failed to write codex models cache");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", codex_dir.display(), existing.to_string_lossy())
        } else {
            codex_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);

        let profile = ProviderRegistry::detect_codex();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "gpt-9-coder-preview"),
            "Codex should discover model slugs from the local models cache"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "codex-ultra-latest"),
            "Codex should preserve additional cache-backed model ids"
        );
        assert!(
            !profile
                .current_models
                .iter()
                .any(|model| model.model_id == "codex-auto-review"),
            "models the cache marks hidden must not surface"
        );
    }

    #[cfg(unix)]
    #[test]
    fn binary_exists_at_known_locations_finds_gemini_in_nvm_layout() {
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let gemini_dir = temp_root.join(".nvm/versions/node/v24.14.0/bin");
        fs::create_dir_all(&gemini_dir).expect("failed to create temp gemini dir");

        write_executable_script(
            &gemini_dir,
            "gemini",
            r#"#!/bin/sh
exit 0
"#,
        );

        assert!(
            binary_exists_at_known_locations("gemini", &temp_root),
            "gemini should be discoverable in a common NVM layout even when PATH is empty"
        );
    }

    #[cfg(unix)]
    #[test]
    fn which_binary_returns_resolved_path_in_nvm_layout() {
        let _guard = crate::test_env::lock_env();
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let claude_dir = temp_root.join(".nvm/versions/node/v24.14.0/bin");
        fs::create_dir_all(&claude_dir).expect("failed to create temp claude dir");
        // A name no real install provides: Homebrew/`/usr/local` rank ahead of
        // nvm, so a real `opencode` on the test machine would win.
        let cli_name = format!("the-pair-test-cli-{}", Uuid::new_v4().simple());

        write_executable_script(
            &claude_dir,
            &cli_name,
            r#"#!/bin/sh
exit 0
"#,
        );

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");

        std::env::set_var("HOME", &temp_root);
        std::env::set_var("PATH", "/usr/bin:/bin");

        let resolved = which_binary(&cli_name);

        if let Some(value) = original_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_path {
            std::env::set_var("PATH", value);
        } else {
            std::env::remove_var("PATH");
        }

        assert_eq!(
            resolved,
            Some(claude_dir.join(&cli_name)),
            "which_binary should return resolved binary path, not bool"
        );
        fs::remove_dir_all(&temp_root).ok();
    }

    #[test]
    fn windows_fallback_lookup_finds_global_npm_bins() {
        // The home-derived default (no APPDATA/LOCALAPPDATA values given).
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let roaming_npm_dir = temp_root.join("AppData/Roaming/npm");
        fs::create_dir_all(&roaming_npm_dir).expect("failed to create roaming npm dir");
        fs::write(roaming_npm_dir.join("claude.cmd"), "@echo off\r\n").expect("failed to seed cmd");

        assert_eq!(
            resolve_binary_in_fallback_dirs("claude", &temp_root, None, None, true),
            Some(roaming_npm_dir.join("claude.cmd")),
            "claude should be discoverable in the standard Windows global npm directory"
        );

        fs::remove_dir_all(&temp_root).ok();
    }

    #[test]
    fn windows_fallback_lookup_uses_custom_appdata_paths() {
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let roaming = temp_root.join("enterprise/roaming");
        let local = temp_root.join("enterprise/local");
        fs::create_dir_all(roaming.join("npm")).expect("failed to create roaming npm dir");
        fs::create_dir_all(local.join("npm")).expect("failed to create local npm dir");
        fs::write(local.join("npm/opencode.cmd"), "@echo off\r\n")
            .expect("failed to seed local binary");

        assert_eq!(
            resolve_binary_in_fallback_dirs(
                "opencode",
                &temp_root.join("home"),
                Some(roaming),
                Some(local.clone()),
                true,
            ),
            Some(local.join("npm/opencode.cmd"))
        );

        fs::remove_dir_all(&temp_root).ok();
    }

    #[test]
    fn windows_lookup_prefers_launchers_over_the_extensionless_npm_shim() {
        // npm writes `claude` (a sh script), `claude.cmd` and `claude.ps1`.
        // Windows can't execute the bare script, so it must never be picked.
        let bin_dir = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&bin_dir).expect("failed to create temp windows bin dir");
        fs::write(bin_dir.join("claude"), "#!/bin/sh\n").expect("failed to seed bare shim");
        fs::write(bin_dir.join("claude.ps1"), "# ps1\n").expect("failed to seed ps1 shim");
        assert_eq!(resolve_binary_in_dir(&bin_dir, "claude", true), None);

        fs::write(bin_dir.join("claude.bat"), "@echo off\r\n").expect("failed to seed bat");
        assert_eq!(
            resolve_binary_in_dir(&bin_dir, "claude", true),
            Some(bin_dir.join("claude.bat"))
        );

        fs::write(bin_dir.join("claude.cmd"), "@echo off\r\n").expect("failed to seed cmd");
        assert_eq!(
            resolve_binary_in_dir(&bin_dir, "claude", true),
            Some(bin_dir.join("claude.cmd"))
        );

        fs::write(bin_dir.join("claude.exe"), "MZ").expect("failed to seed exe");
        assert_eq!(
            resolve_binary_in_dir(&bin_dir, "claude", true),
            Some(bin_dir.join("claude.exe"))
        );

        // A name that already carries a launcher extension is taken as is.
        assert_eq!(
            resolve_binary_in_dir(&bin_dir, "claude.cmd", true),
            Some(bin_dir.join("claude.cmd"))
        );

        fs::remove_dir_all(&bin_dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn unix_lookup_skips_non_executable_files_and_directories() {
        let bin_dir = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        fs::create_dir_all(bin_dir.join("codex")).expect("failed to create dir named like a CLI");
        fs::write(bin_dir.join("claude"), "#!/bin/sh\n").expect("failed to seed plain file");
        assert_eq!(resolve_binary_in_dir(&bin_dir, "claude", false), None);
        assert_eq!(resolve_binary_in_dir(&bin_dir, "codex", false), None);

        let script = write_executable_script(&bin_dir, "claude", "#!/bin/sh\nexit 0\n");
        assert_eq!(
            resolve_binary_in_dir(&bin_dir, "claude", false),
            Some(script)
        );

        fs::remove_dir_all(&bin_dir).ok();
    }

    #[test]
    fn home_from_appdata_is_the_grandparent_of_appdata() {
        assert_eq!(
            home_from_appdata(Path::new("/Users/alex/AppData/Roaming")),
            Some(PathBuf::from("/Users/alex"))
        );
        assert_eq!(
            home_from_appdata(Path::new("/Users/alex/AppData/Local")),
            Some(PathBuf::from("/Users/alex"))
        );
        assert_eq!(home_from_appdata(Path::new("Roaming")), None);
    }

    #[test]
    fn probe_cache_keeps_successes_and_retries_failures() {
        let calls = std::cell::Cell::new(0);
        let binary = Path::new("/usr/local/bin/pi");

        // A failure is retried once `retry_after` has passed ...
        let retrying = ProbeCache::<bool>::new(Duration::ZERO);
        let fail = || {
            calls.set(calls.get() + 1);
            None
        };
        assert_eq!(retrying.get_or_probe(binary, fail), None);
        assert_eq!(retrying.get_or_probe(binary, fail), None);
        assert_eq!(calls.get(), 2, "a failed probe must not stick");

        // ... and a success is kept for good, even with a zero retry window.
        let succeed = || {
            calls.set(calls.get() + 1);
            Some(true)
        };
        assert_eq!(retrying.get_or_probe(binary, succeed), Some(true));
        assert_eq!(retrying.get_or_probe(binary, succeed), Some(true));
        assert_eq!(calls.get(), 3);

        // A different binary (e.g. after the PATH refresh) is probed afresh.
        assert_eq!(
            retrying.get_or_probe(Path::new("/opt/homebrew/bin/pi"), || Some(false)),
            Some(false)
        );

        // Within the retry window a failure is remembered, so a catalog build
        // asking once per model doesn't re-run a hanging probe for every row.
        let remembering = ProbeCache::<bool>::new(Duration::from_secs(60));
        calls.set(0);
        assert_eq!(remembering.get_or_probe(binary, fail), None);
        assert_eq!(remembering.get_or_probe(binary, succeed), None);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn pi_help_parser_detects_the_max_thinking_level() {
        assert!(pi_help_lists_max_thinking_level(
            "  --thinking <level>   Set thinking level: off, minimal, low, medium, high, xhigh, max\n"
        ));
        assert!(!pi_help_lists_max_thinking_level(
            "  --thinking <level>   Set thinking level: off, minimal, low, medium, high, xhigh\n"
        ));
        assert!(!pi_help_lists_max_thinking_level("pi [options]\n"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn binary_exists_at_known_locations_uses_custom_appdata_env_paths() {
        let _guard = crate::test_env::lock_env();
        let temp_root = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let roaming = temp_root.join("enterprise/roaming");
        let local = temp_root.join("enterprise/local");
        let original_appdata = std::env::var_os("APPDATA");
        let original_local_appdata = std::env::var_os("LOCALAPPDATA");
        fs::create_dir_all(roaming.join("npm")).expect("failed to create roaming npm dir");
        fs::create_dir_all(local.join("npm")).expect("failed to create local npm dir");
        fs::write(roaming.join("npm/opencode.cmd"), "@echo off\r\n")
            .expect("failed to seed roaming binary");

        std::env::set_var("APPDATA", &roaming);
        std::env::set_var("LOCALAPPDATA", &local);

        let found = binary_exists_at_known_locations("opencode", &temp_root.join("home"));

        if let Some(value) = original_appdata {
            std::env::set_var("APPDATA", value);
        } else {
            std::env::remove_var("APPDATA");
        }

        if let Some(value) = original_local_appdata {
            std::env::set_var("LOCALAPPDATA", value);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }

        assert!(
            found,
            "Windows fallback lookup should honor APPDATA/LOCALAPPDATA env values"
        );
    }

    #[test]
    fn cli_environment_overrides_preserve_custom_appdata_values() {
        let _guard = crate::test_env::lock_env();
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let roaming = temp_home.join("enterprise/Roaming");
        let local = temp_home.join("enterprise/Local");
        let original_appdata = std::env::var_os("APPDATA");
        let original_local_appdata = std::env::var_os("LOCALAPPDATA");

        std::env::set_var("APPDATA", &roaming);
        std::env::set_var("LOCALAPPDATA", &local);

        let overrides: HashMap<_, _> = cli_environment_overrides(&temp_home).into_iter().collect();

        if let Some(value) = original_appdata {
            std::env::set_var("APPDATA", value);
        } else {
            std::env::remove_var("APPDATA");
        }

        if let Some(value) = original_local_appdata {
            std::env::set_var("LOCALAPPDATA", value);
        } else {
            std::env::remove_var("LOCALAPPDATA");
        }

        assert_eq!(
            overrides.get(&OsString::from("HOME")),
            Some(&temp_home.as_os_str().to_owned())
        );
        assert_eq!(
            overrides.get(&OsString::from("USERPROFILE")),
            Some(&temp_home.as_os_str().to_owned())
        );
        assert_eq!(
            overrides.get(&OsString::from("APPDATA")),
            Some(&roaming.as_os_str().to_owned())
        );
        assert_eq!(
            overrides.get(&OsString::from("LOCALAPPDATA")),
            Some(&local.as_os_str().to_owned())
        );

        let path_override = overrides.get(&OsString::from("PATH"));
        assert!(
            path_override.is_some(),
            "PATH should be propagated to CLI child env"
        );
        #[cfg(unix)]
        {
            assert!(
                std::env::split_paths(path_override.unwrap()).any(|path| {
                    path == temp_home.join(".local/bin")
                        || path == temp_home.join("go/bin")
                        || path == temp_home.join(".npm-global/bin")
                        || path == temp_home.join(".volta/bin")
                }),
                "PATH should include common Unix fallback directories"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn detect_claude_reads_models_from_help_and_recent_history() {
        let _guard = crate::test_env::lock_env();
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let claude_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let claude_projects_dir = temp_home.join(".claude/projects/example");
        fs::create_dir_all(&claude_dir).expect("failed to create temp claude dir");
        fs::create_dir_all(&claude_projects_dir).expect("failed to create temp claude project dir");

        write_executable_script(
            &claude_dir,
            "claude",
            r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  printf '%s\n' '{"loggedIn":true,"authMethod":"oauth"}'
elif [ "$1" = "--help" ]; then
  printf '%s\n' "  --model <model>  Use aliases like 'sonnet' and full names like 'claude-sonnet-9-9'"
else
  exit 0
fi
"#,
        );

        fs::write(
            claude_projects_dir.join("session.jsonl"),
            "{\"message\":{\"model\":\"claude-opus-9-1\"}}\n",
        )
        .expect("failed to write recent claude history");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", claude_dir.display(), existing.to_string_lossy())
        } else {
            claude_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);
        std::env::remove_var("ANTHROPIC_API_KEY");

        let profile = ProviderRegistry::detect_claude();

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_api_key {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        }

        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-sonnet-9-9"),
            "Claude should discover full model names from CLI help output"
        );
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-opus-9-1"),
            "Claude should discover full model names from recent local history"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_claude_uses_credentials_file_when_auth_status_fails() {
        let _guard = crate::test_env::lock_env();
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let claude_dir = temp_home.join(".nvm/versions/node/v24.14.0/bin");
        let credentials_dir = temp_home.join(".claude");
        fs::create_dir_all(&claude_dir).expect("failed to create temp claude dir");
        fs::create_dir_all(&credentials_dir).expect("failed to create temp credentials dir");

        write_executable_script(
            &claude_dir,
            "claude",
            r#"#!/bin/sh
if [ "$1" = "auth" ] && [ "$2" = "status" ]; then
  exit 1
elif [ "$1" = "--help" ]; then
  printf '%s\n' "  --model <model>  Use aliases like 'sonnet' and full names like 'claude-sonnet-4-7'"
else
  exit 0
fi
"#,
        );

        fs::write(
            credentials_dir.join(".credentials.json"),
            r#"{"accessToken":"test-token"}"#,
        )
        .expect("failed to write claude credentials");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", claude_dir.display(), existing.to_string_lossy())
        } else {
            claude_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);
        std::env::remove_var("ANTHROPIC_API_KEY");

        let profile = ProviderRegistry::detect_claude();

        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }

        if let Some(value) = original_api_key {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        }

        assert!(profile.installed);
        assert!(profile.authenticated);
        assert!(profile.runnable);
        assert_eq!(profile.subscription_label, "subscription-backed");
        assert!(
            profile
                .current_models
                .iter()
                .any(|model| model.model_id == "claude-sonnet-4-7"),
            "Claude should still discover models when auth falls back to local credentials"
        );
    }

    #[cfg(unix)]
    #[test]
    fn command_output_timeout_returns_none_for_slow_command() {
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_home).expect("failed to create temp home");
        let script = write_executable_script(
            &temp_home,
            "slow-provider",
            r#"#!/bin/sh
sleep 2
printf '%s\n' 'eventual output'
"#,
        );

        let started = std::time::Instant::now();
        let output = capture_command_output_with_timeout(
            &script,
            &["--help"],
            &temp_home,
            Duration::from_millis(100),
        );

        assert!(output.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "slow provider command should be bounded by timeout"
        );

        let _ = fs::remove_dir_all(temp_home);
    }

    #[cfg(unix)]
    #[test]
    fn command_output_keeps_probe_output_larger_than_the_pipe_buffer() {
        // `opencode models` (1.x) and `pi --list-models` can print more than
        // the 64 KiB pipe buffer; reading only after exit used to stall the
        // probe into its timeout and lose every model.
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_home).expect("failed to create temp home");
        let script = write_executable_script(
            &temp_home,
            "chatty-provider",
            r#"#!/bin/sh
i=0
while [ $i -lt 3000 ]; do
  printf 'provider-%s/model-with-a-fairly-long-identifier-%s\n' "$i" "$i"
  i=$((i + 1))
done
"#,
        );

        let output = capture_command_output_with_timeout(
            &script,
            &["models"],
            &temp_home,
            Duration::from_secs(10),
        )
        .expect("large probe output should be captured, not time out");

        assert!(output.len() > 64 * 1024, "only {} bytes", output.len());
        assert_eq!(output.lines().count(), 3000);
        assert_eq!(
            output.lines().last(),
            Some("provider-2999/model-with-a-fairly-long-identifier-2999")
        );

        let _ = fs::remove_dir_all(temp_home);
    }

    #[cfg(unix)]
    #[test]
    fn command_output_is_none_for_a_failing_probe() {
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&temp_home).expect("failed to create temp home");
        let script = write_executable_script(
            &temp_home,
            "broken-provider",
            "#!/bin/sh\necho partial\nexit 3\n",
        );

        assert_eq!(
            capture_command_output_with_timeout(
                &script,
                &["--help"],
                &temp_home,
                Duration::from_secs(5),
            ),
            None
        );

        let _ = fs::remove_dir_all(temp_home);
    }

    // ── Aider detection tests ──────────────────────────────────────────────

    #[cfg(unix)]
    #[test]
    fn detect_aider_reads_models_from_config() {
        let _guard = crate::test_env::lock_env();
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let bin_dir = temp_home.join(".local/bin");
        fs::create_dir_all(&bin_dir).expect("failed to create temp bin dir");

        write_executable_script(
            &bin_dir,
            "aider",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(
            temp_home.join(".aider.conf.yml"),
            "model: claude-sonnet-4-6\n",
        )
        .expect("failed to write aider config");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let original_openai_key = std::env::var_os("OPENAI_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", bin_dir.display(), existing.to_string_lossy())
        } else {
            bin_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");

        let profile = ProviderRegistry::detect_aider();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        if let Some(value) = original_api_key {
            std::env::set_var("ANTHROPIC_API_KEY", value);
        } else {
            std::env::remove_var("ANTHROPIC_API_KEY");
        }
        if let Some(value) = original_openai_key {
            std::env::set_var("OPENAI_API_KEY", value);
        } else {
            std::env::remove_var("OPENAI_API_KEY");
        }

        assert!(profile.installed);
        assert!(profile.authenticated);
        assert!(profile.runnable);
        assert!(
            profile
                .current_models
                .iter()
                .any(|m| m.model_id == "claude-sonnet-4-6"),
            "Aider should surface the model configured in ~/.aider.conf.yml"
        );
        // Fallback models should also be present.
        assert!(
            profile
                .current_models
                .iter()
                .any(|m| m.model_id == "gpt-5.4"),
            "Aider should include static fallback models for BYOK"
        );
    }

    #[cfg(unix)]
    #[test]
    fn detect_aider_not_authenticated_without_env_key() {
        let _guard = crate::test_env::lock_env();
        let temp_home = std::env::temp_dir().join(format!("the-pair-test-{}", Uuid::new_v4()));
        let bin_dir = temp_home.join(".local/bin");
        fs::create_dir_all(&bin_dir).expect("failed to create temp bin dir");

        write_executable_script(
            &bin_dir,
            "aider",
            r#"#!/bin/sh
exit 0
"#,
        );

        fs::write(
            temp_home.join(".aider.conf.yml"),
            "model: claude-sonnet-4-6\n",
        )
        .expect("failed to write aider config");

        let original_home = std::env::var_os("HOME");
        let original_path = std::env::var_os("PATH");
        let original_api_key = std::env::var_os("ANTHROPIC_API_KEY");
        let original_openai_key = std::env::var_os("OPENAI_API_KEY");
        let original_gemini_key = std::env::var_os("GEMINI_API_KEY");
        let original_deepseek_key = std::env::var_os("DEEPSEEK_API_KEY");
        let original_openrouter_key = std::env::var_os("OPENROUTER_API_KEY");
        let original_azure_key = std::env::var_os("AZURE_API_KEY");
        let new_path = if let Some(existing) = &original_path {
            format!("{}:{}", bin_dir.display(), existing.to_string_lossy())
        } else {
            bin_dir.display().to_string()
        };

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PATH", new_path);
        // Remove all known API key env vars.
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("DEEPSEEK_API_KEY");
        std::env::remove_var("OPENROUTER_API_KEY");
        std::env::remove_var("AZURE_API_KEY");

        let profile = ProviderRegistry::detect_aider();

        if let Some(home) = original_home {
            std::env::set_var("HOME", home);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(path) = original_path {
            std::env::set_var("PATH", path);
        } else {
            std::env::remove_var("PATH");
        }
        for (key, original) in [
            ("ANTHROPIC_API_KEY", original_api_key),
            ("OPENAI_API_KEY", original_openai_key),
            ("GEMINI_API_KEY", original_gemini_key),
            ("DEEPSEEK_API_KEY", original_deepseek_key),
            ("OPENROUTER_API_KEY", original_openrouter_key),
            ("AZURE_API_KEY", original_azure_key),
        ] {
            if let Some(value) = original {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }

        assert!(profile.installed);
        assert!(
            !profile.authenticated,
            "Aider should not be authenticated without any API key env var"
        );
        assert!(!profile.runnable);
        assert!(
            profile.current_models.is_empty(),
            "Aider should not list models when not authenticated"
        );
    }
}
