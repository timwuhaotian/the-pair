use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Upper bound for capturing the login shell's PATH. Startup may wait on the
/// refresh, so a profile script that blocks (an `ssh-add` or keychain prompt)
/// must not stall the app.
const LOGIN_SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(3);

/// Printed around `$PATH` so banners and `echo`s from profile scripts can't be
/// mistaken for PATH entries.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const PATH_BEGIN_MARKER: &str = "__THE_PAIR_PATH_BEGIN__";
#[cfg(any(target_os = "macos", target_os = "linux"))]
const PATH_END_MARKER: &str = "__THE_PAIR_PATH_END__";

/// Stops a probe's output from growing without bound; the pipe keeps being
/// drained past this so the child never blocks on a full pipe.
const MAX_CAPTURED_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

/// After the child exits, how long to keep reading what is already in the
/// pipe even if the deadline has just passed.
const PIPE_DRAIN_GRACE: Duration = Duration::from_millis(100);

/// `CREATE_NO_WINDOW`: keeps console children of the windowless release build
/// from flashing a console window.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn apply_fallback_path() {
    let current = std::env::var_os("PATH").unwrap_or_default();

    if let Ok(merged) = merge_path_entries(&current, &current_fallback_dirs()) {
        std::env::set_var("PATH", merged);
    }
}

pub fn refresh_path_from_login_shell() {
    let Some(shell_path) = capture_login_shell_path() else {
        return;
    };
    let current = std::env::var_os("PATH").unwrap_or_default();

    if let Ok(merged) =
        login_shell_first_path(OsStr::new(&shell_path), &current, &current_fallback_dirs())
    {
        std::env::set_var("PATH", merged);
    }
}

fn current_fallback_dirs() -> Vec<PathBuf> {
    fallback_path_dirs(
        std::env::var_os(if cfg!(target_os = "windows") {
            "USERPROFILE"
        } else {
            "HOME"
        })
        .map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        cfg!(target_os = "windows"),
    )
}

pub(crate) fn fallback_path_dirs(
    home: Option<PathBuf>,
    appdata: Option<PathBuf>,
    local_appdata: Option<PathBuf>,
    is_windows: bool,
) -> Vec<PathBuf> {
    // An empty value would become a relative PATH entry, which the spawner
    // resolves against the user's repo (its cwd). Treat it as unset.
    let non_empty = |path: &PathBuf| !path.as_os_str().is_empty();
    let home = home.filter(non_empty);
    let appdata = appdata.filter(non_empty);
    let local_appdata = local_appdata.filter(non_empty);
    let mut dirs = Vec::new();

    // Per-user installer locations (Claude Code's native installer, the
    // opencode/kimi install scripts, `go install`, ...). Same layout on every
    // OS, e.g. `%USERPROFILE%\.local\bin\claude.exe` on Windows.
    if let Some(home) = &home {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("go/bin"));
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".volta/bin"));
        dirs.push(home.join(".opencode/bin"));
        dirs.push(home.join(".kimi-code/bin"));
    }

    if is_windows {
        if let Some(path) =
            appdata.or_else(|| home.as_ref().map(|path| path.join("AppData/Roaming")))
        {
            dirs.push(path.join("npm"));
        }

        if let Some(path) =
            local_appdata.or_else(|| home.as_ref().map(|path| path.join("AppData/Local")))
        {
            dirs.push(path.join("npm"));
        }
    } else {
        // Package-manager installs come before nvm: a CLI once installed under
        // an old nvm Node must not shadow the current Homebrew copy.
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/homebrew/sbin"),
            PathBuf::from("/usr/local/bin"),
        ]);

        if let Some(home) = &home {
            dirs.extend(nvm_default_bin_dir(home));
        }

        dirs.push(PathBuf::from("/usr/bin"));
    }

    dirs
}

/// PATH entries worth keeping: an empty or relative entry resolves against the
/// spawn cwd (the user's repo), which would let a repo-local file shadow a CLI.
fn is_usable_path_entry(entry: &Path) -> bool {
    !entry.as_os_str().is_empty() && entry.is_absolute()
}

fn dedup_path_entries(entries: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for entry in entries {
        if is_usable_path_entry(&entry) && !paths.contains(&entry) {
            paths.push(entry);
        }
    }
    paths
}

/// `base` in its own order, followed by any `extra_dirs` it is missing.
pub(crate) fn merge_path_entries(
    base: &OsStr,
    extra_dirs: &[PathBuf],
) -> Result<OsString, std::env::JoinPathsError> {
    std::env::join_paths(dedup_path_entries(
        std::env::split_paths(base).chain(extra_dirs.iter().cloned()),
    ))
}

/// The login shell's PATH first so the user's own order wins, then entries only
/// the app inherited, then the fallback dirs.
fn login_shell_first_path(
    shell_path: &OsStr,
    current: &OsStr,
    fallback_dirs: &[PathBuf],
) -> Result<OsString, std::env::JoinPathsError> {
    std::env::join_paths(dedup_path_entries(
        std::env::split_paths(shell_path)
            .chain(std::env::split_paths(current))
            .chain(fallback_dirs.iter().cloned()),
    ))
}

/// `v22.13.0` / `22.13.0` → `(22, 13, 0)`.
fn parse_node_version(name: &str) -> Option<(u64, u64, u64)> {
    let mut parts = name.strip_prefix('v').unwrap_or(name).split('.');
    let version = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(version)
}

/// The installed version an nvm version spec (`22`, `v22.13`, `v22.13.0`)
/// selects: the newest one matching every component given.
fn match_nvm_version<'a>(
    spec: &str,
    installed: &'a [((u64, u64, u64), String)],
) -> Option<&'a str> {
    let wanted: Vec<u64> = spec
        .strip_prefix('v')
        .unwrap_or(spec)
        .split('.')
        .map(|part| part.parse().ok())
        .collect::<Option<_>>()?;
    if wanted.is_empty() || wanted.len() > 3 {
        return None;
    }
    installed
        .iter()
        .filter(|((major, minor, patch), _)| {
            [*major, *minor, *patch]
                .iter()
                .zip(&wanted)
                .all(|(have, want)| have == want)
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, name)| name.as_str())
}

/// Follow `~/.nvm/alias/default` (aliases may chain, e.g. `lts/*` → `lts/jod`
/// → `v22.21.1`) to an installed version directory name.
fn resolve_nvm_default_alias(
    nvm_dir: &Path,
    installed: &[((u64, u64, u64), String)],
) -> Option<String> {
    let alias_dir = nvm_dir.join("alias");
    let mut alias = "default".to_string();
    for _ in 0..8 {
        // nvm writes LTS aliases lowercased (`lts/krypton`) but `default` may
        // name them as typed (`lts/Krypton`); case matters on Linux.
        let target = fs::read_to_string(alias_dir.join(&alias))
            .or_else(|_| fs::read_to_string(alias_dir.join(alias.to_lowercase())))
            .ok()?;
        let target = target.trim();
        if target.is_empty() {
            return None;
        }
        if let Some(name) = match_nvm_version(target, installed) {
            return Some(name.to_string());
        }
        if matches!(target, "node" | "stable") {
            return newest_nvm_version(installed).map(str::to_string);
        }
        alias = target.to_string();
    }
    None
}

fn newest_nvm_version(installed: &[((u64, u64, u64), String)]) -> Option<&str> {
    installed
        .iter()
        .max_by_key(|(version, _)| *version)
        .map(|(_, name)| name.as_str())
}

/// The bin dir of the Node that nvm activates by default, or of the newest
/// installed Node when the default alias can't be resolved. Only one version
/// is used: listing all of them in `read_dir` order let an arbitrary stale
/// copy of a CLI win.
fn nvm_default_bin_dir(home: &Path) -> Option<PathBuf> {
    let nvm_dir = home.join(".nvm");
    let versions_dir = nvm_dir.join("versions/node");
    let installed: Vec<((u64, u64, u64), String)> = fs::read_dir(&versions_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            Some((parse_node_version(&name)?, name))
        })
        .collect();

    let chosen = resolve_nvm_default_alias(&nvm_dir, &installed)
        .or_else(|| newest_nvm_version(&installed).map(str::to_string))?;
    Some(versions_dir.join(chosen).join("bin"))
}

/// Hide the console window a child would otherwise open on Windows.
fn hide_console_window(command: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = command;
}

/// What a child printed before it exited or was killed.
pub(crate) struct BoundedOutput {
    /// `None` when the child was still running at the deadline and got killed.
    pub status: Option<ExitStatus>,
    pub stdout: Vec<u8>,
}

/// Run `command` with stdin closed, stdout captured and stderr discarded, for at
/// most `timeout`.
///
/// stdout is drained on a helper thread while the child runs. Reading it only
/// after exit deadlocks once the child fills the pipe buffer (64 KiB on
/// macOS): it blocks on write, never exits, and runs into the timeout. The
/// helper stops at EOF, and waiting for it is capped by the same deadline, so
/// a daemon that inherited the pipe can't hold the caller either.
pub(crate) fn run_with_timeout(mut command: Command, timeout: Duration) -> Option<BoundedOutput> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console_window(&mut command);

    let deadline = Instant::now() + timeout;
    let mut child = command.spawn().ok()?;
    let Some(mut pipe) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };

    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let (eof_tx, eof_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    let mut buffer = sink.lock().unwrap_or_else(|e| e.into_inner());
                    let room = MAX_CAPTURED_OUTPUT_BYTES.saturating_sub(buffer.len());
                    buffer.extend_from_slice(&chunk[..read.min(room)]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        let _ = eof_tx.send(());
    });

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };

    if status.is_some() {
        let drain = deadline
            .saturating_duration_since(Instant::now())
            .max(PIPE_DRAIN_GRACE);
        let _ = eof_rx.recv_timeout(drain);
    }
    let stdout = std::mem::take(&mut *captured.lock().unwrap_or_else(|e| e.into_inner()));
    Some(BoundedOutput { status, stdout })
}

/// The PATH printed between the markers, ignoring whatever else the shell's
/// startup files wrote to stdout.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn extract_marked_path(output: &str) -> Option<String> {
    let start = output.find(PATH_BEGIN_MARKER)? + PATH_BEGIN_MARKER.len();
    let end = start + output[start..].find(PATH_END_MARKER)?;
    let path = output[start..end].trim();
    (!path.is_empty()).then(|| path.to_string())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn capture_login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "/bin/zsh".to_string()
            } else {
                "/bin/bash".to_string()
            }
        });

    let mut command = Command::new(shell);
    command.arg("-lc").arg(format!(
        "printf '%s%s%s' '{PATH_BEGIN_MARKER}' \"$PATH\" '{PATH_END_MARKER}'"
    ));
    // Own process group: a profile script that reads the terminal (when the
    // app runs from one) is stopped instead of taking over its input.
    std::os::unix::process::CommandExt::process_group(&mut command, 0);

    let output = run_with_timeout(command, LOGIN_SHELL_PATH_TIMEOUT)?;
    extract_marked_path(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "windows")]
pub fn capture_login_shell_path() -> Option<String> {
    let mut command = Command::new("cmd.exe");
    command.arg("/c").arg("@echo off & echo %PATH%");

    let output = run_with_timeout(command, LOGIN_SHELL_PATH_TIMEOUT)?;
    if !output.status.is_some_and(|status| status.success()) {
        return None;
    }

    let path = String::from_utf8_lossy(&output.stdout);
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn capture_login_shell_path_returns_a_value_on_unix_desktops() {
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let path = capture_login_shell_path().expect("expected login shell PATH");
            assert!(!path.trim().is_empty());
            assert!(path.contains('/'));
            assert!(!path.contains("__THE_PAIR_PATH"));
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn capture_login_shell_path_returns_a_value_on_windows_desktops() {
        let path = capture_login_shell_path().expect("expected login shell PATH");
        assert!(!path.trim().is_empty());
        assert!(path.contains(';'));
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn extract_marked_path_ignores_profile_output_around_the_markers() {
        let output = format!(
            "Welcome back!\nLast login: today{PATH_BEGIN_MARKER}/opt/homebrew/bin:/usr/bin{PATH_END_MARKER}bye\n"
        );
        assert_eq!(
            extract_marked_path(&output).as_deref(),
            Some("/opt/homebrew/bin:/usr/bin")
        );
        assert_eq!(extract_marked_path("/usr/bin:/bin"), None);
        assert_eq!(
            extract_marked_path(&format!("{PATH_BEGIN_MARKER}/usr/bin")),
            None,
            "a PATH cut off by the timeout must not be used"
        );
        assert_eq!(
            extract_marked_path(&format!("{PATH_BEGIN_MARKER}  {PATH_END_MARKER}")),
            None
        );
    }

    #[cfg(unix)]
    fn sh(script: &str) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(script);
        command
    }

    #[test]
    #[cfg(unix)]
    fn run_with_timeout_drains_output_larger_than_the_pipe_buffer() {
        // ~200 KB: far past the 64 KiB pipe buffer that used to stall the child.
        let started = Instant::now();
        let output = run_with_timeout(
            sh("i=0; while [ $i -lt 4000 ]; do echo provider/model-name-padding-padding-padding-$i; i=$((i+1)); done"),
            Duration::from_secs(10),
        )
        .expect("spawn /bin/sh");

        assert!(output.status.is_some_and(|status| status.success()));
        let text = String::from_utf8(output.stdout).expect("utf-8 output");
        assert_eq!(text.lines().count(), 4000);
        assert!(text.len() > 64 * 1024);
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    #[cfg(unix)]
    fn run_with_timeout_kills_a_child_that_outlives_the_deadline() {
        let started = Instant::now();
        let output = run_with_timeout(sh("echo early; sleep 5"), Duration::from_millis(300))
            .expect("spawn /bin/sh");

        assert!(output.status.is_none(), "child should have been killed");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    #[cfg(unix)]
    fn run_with_timeout_returns_when_a_grandchild_keeps_stdout_open() {
        // The backgrounded sleep inherits stdout, so EOF only arrives when it
        // exits. The exited child's own output must still come back, bounded
        // by the deadline.
        let started = Instant::now();
        let output = run_with_timeout(sh("sleep 5 & echo done"), Duration::from_millis(500))
            .expect("spawn /bin/sh");

        assert!(output.status.is_some_and(|status| status.success()));
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "done");
        assert!(started.elapsed() < Duration::from_secs(3));
    }

    #[test]
    fn fallback_dirs_include_windows_global_npm_locations() {
        let dirs = fallback_path_dirs(
            Some(PathBuf::from(r"C:\Users\alex")),
            Some(PathBuf::from(r"C:\Users\alex\AppData\Roaming")),
            Some(PathBuf::from(r"C:\Users\alex\AppData\Local")),
            true,
        );

        let rendered: Vec<String> = dirs
            .iter()
            .map(|dir| dir.to_string_lossy().replace('\\', "/"))
            .collect();

        assert!(rendered.contains(&"C:/Users/alex/AppData/Roaming/npm".to_string()));
        assert!(rendered.contains(&"C:/Users/alex/AppData/Local/npm".to_string()));
    }

    #[test]
    fn fallback_dirs_include_common_unix_user_bin_locations() {
        let dirs = fallback_path_dirs(Some(PathBuf::from("/Users/alex")), None, None, false);

        assert!(dirs.contains(&PathBuf::from("/Users/alex/.local/bin")));
        assert!(dirs.contains(&PathBuf::from("/Users/alex/go/bin")));
        assert!(dirs.contains(&PathBuf::from("/Users/alex/.npm-global/bin")));
        assert!(dirs.contains(&PathBuf::from("/Users/alex/.volta/bin")));
        assert!(dirs.contains(&PathBuf::from("/Users/alex/.opencode/bin")));
        assert!(dirs.contains(&PathBuf::from("/Users/alex/.kimi-code/bin")));
        assert_eq!(dirs.len(), 10);
    }

    #[test]
    fn fallback_dirs_skip_home_relative_entries_when_home_is_empty() {
        let unix = fallback_path_dirs(Some(PathBuf::new()), None, None, false);
        assert!(!unix.is_empty());
        assert!(unix.iter().all(|dir| dir.is_absolute()), "{unix:?}");

        let windows = fallback_path_dirs(
            Some(PathBuf::new()),
            Some(PathBuf::new()),
            Some(PathBuf::new()),
            true,
        );
        assert!(windows.is_empty(), "{windows:?}");
    }

    fn temp_home() -> PathBuf {
        let home = std::env::temp_dir().join(format!("the-pair-path-env-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&home).expect("create temp home");
        home
    }

    fn install_node_versions(home: &Path, versions: &[&str]) {
        for version in versions {
            fs::create_dir_all(home.join(".nvm/versions/node").join(version).join("bin"))
                .expect("create nvm version dir");
        }
    }

    fn write_nvm_alias(home: &Path, alias: &str, target: &str) {
        let path = home.join(".nvm/alias").join(alias);
        fs::create_dir_all(path.parent().unwrap()).expect("create alias dir");
        fs::write(path, format!("{target}\n")).expect("write alias");
    }

    fn nvm_dirs(dirs: &[PathBuf]) -> Vec<&PathBuf> {
        dirs.iter()
            .filter(|dir| dir.to_string_lossy().contains(".nvm"))
            .collect()
    }

    #[test]
    fn fallback_dirs_use_only_the_newest_nvm_version_without_a_default_alias() {
        let home = temp_home();
        install_node_versions(
            &home,
            &["v22.13.0", "v24.14.0", "v22.22.1", "not-a-version"],
        );

        let dirs = fallback_path_dirs(Some(home.clone()), None, None, false);
        assert_eq!(
            nvm_dirs(&dirs),
            vec![&home.join(".nvm/versions/node/v24.14.0/bin")]
        );

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn fallback_dirs_follow_the_nvm_default_alias() {
        let home = temp_home();
        install_node_versions(&home, &["v22.13.0", "v24.14.0", "v22.22.1"]);

        // A major-only alias picks the newest matching install.
        write_nvm_alias(&home, "default", "22");
        let dirs = fallback_path_dirs(Some(home.clone()), None, None, false);
        assert_eq!(
            nvm_dirs(&dirs),
            vec![&home.join(".nvm/versions/node/v22.22.1/bin")]
        );

        // Chained LTS aliases resolve down to the exact version.
        write_nvm_alias(&home, "default", "lts/*");
        write_nvm_alias(&home, "lts/*", "lts/jod");
        write_nvm_alias(&home, "lts/jod", "v22.13.0");
        let dirs = fallback_path_dirs(Some(home.clone()), None, None, false);
        assert_eq!(
            nvm_dirs(&dirs),
            vec![&home.join(".nvm/versions/node/v22.13.0/bin")]
        );

        // `nvm alias default lts/Jod` keeps the case; the LTS file is lowercase.
        write_nvm_alias(&home, "default", "lts/Jod");
        let dirs = fallback_path_dirs(Some(home.clone()), None, None, false);
        assert_eq!(
            nvm_dirs(&dirs),
            vec![&home.join(".nvm/versions/node/v22.13.0/bin")]
        );

        // A default that is not installed falls back to the newest version.
        write_nvm_alias(&home, "default", "v18");
        let dirs = fallback_path_dirs(Some(home.clone()), None, None, false);
        assert_eq!(
            nvm_dirs(&dirs),
            vec![&home.join(".nvm/versions/node/v24.14.0/bin")]
        );

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn fallback_dirs_put_homebrew_before_nvm() {
        let home = temp_home();
        install_node_versions(&home, &["v24.14.0"]);

        let dirs = fallback_path_dirs(Some(home.clone()), None, None, false);
        let position = |dir: &Path| dirs.iter().position(|entry| entry == dir).unwrap();
        let nvm = position(&home.join(".nvm/versions/node/v24.14.0/bin"));
        assert!(position(Path::new("/opt/homebrew/bin")) < nvm);
        assert!(position(Path::new("/usr/local/bin")) < nvm);
        assert!(nvm < position(Path::new("/usr/bin")));

        fs::remove_dir_all(&home).ok();
    }

    #[test]
    #[cfg(unix)]
    fn login_shell_path_takes_precedence_over_inherited_and_fallback_entries() {
        let merged = login_shell_first_path(
            OsStr::new("/opt/homebrew/bin:/Users/alex/.nvm/versions/node/v24.14.0/bin:/usr/bin"),
            OsStr::new(
                "/usr/bin:/bin:/Users/alex/.nvm/versions/node/v22.13.0/bin:/opt/homebrew/bin",
            ),
            &[PathBuf::from("/usr/local/bin"), PathBuf::from("/usr/bin")],
        )
        .expect("join PATH");

        assert_eq!(
            merged,
            OsString::from(
                "/opt/homebrew/bin:/Users/alex/.nvm/versions/node/v24.14.0/bin:/usr/bin:/bin:/Users/alex/.nvm/versions/node/v22.13.0/bin:/usr/local/bin"
            )
        );
    }

    #[test]
    #[cfg(unix)]
    fn merged_path_drops_empty_and_relative_entries() {
        let merged = merge_path_entries(
            OsStr::new("/usr/bin::node_modules/.bin:/bin"),
            &[
                PathBuf::from(".local/bin"),
                PathBuf::from("/opt/homebrew/bin"),
            ],
        )
        .expect("join PATH");

        assert_eq!(merged, OsString::from("/usr/bin:/bin:/opt/homebrew/bin"));
    }
}
