//! Process environment variables.
//!
//! killi treats a live variable as the strongest signal about a credential, so
//! `ANTHROPIC_API_KEY` set in the shell shows up even when no agent config
//! mentions Anthropic. The variable is referenced, not copied, so rotating it
//! in the shell takes effect without a re-scan.
//!
//! A launch from Finder or `open` hands the app launchd's environment, which no
//! profile has written to, so the shell's own variables are missing from the
//! process. They are asked for once, before the first scan, and only the ones
//! the process is missing are filled in.

use super::*;
use crate::accounts::registry::ProviderDraft;
use std::ffi::OsStr;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Once;
use std::time::{Duration, Instant};

/// Upper bound for capturing the login shell's environment, all attempts
/// together. A scan waits on it, so a profile that blocks (an `ssh-add` or
/// keychain prompt) must not stall the import. Typical profiles finish well
/// under a second.
const LOGIN_SHELL_ENV_BUDGET: Duration = Duration::from_secs(5);

/// Share of the budget the interactive `-ilc` attempt may use; the rest is kept
/// for the plain `-lc` fallback.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const INTERACTIVE_SHELL_ENV_TIMEOUT: Duration = Duration::from_secs(3);

/// The variable the shell is told to write `printenv`'s output to. Passing the
/// path through the environment keeps it out of the command line, where a
/// quoting mistake would turn into a broken redirect.
const ENV_FILE_VAR: &str = "THE_PAIR_SHELL_ENV_FILE";

/// Fills in the variables the login shell has and this process does not.
///
/// The app launched from the Finder inherits launchd's environment: no profile
/// runs there, so `ANTHROPIC_API_KEY` exported from `.zshrc` reads as unset and
/// every env-backed credential is reported missing. The capture runs once — the
/// same shell `path_env` asks for PATH — and never overwrites a variable the
/// process already has, which was set deliberately.
///
/// Answers with the shell's `PATH` on the call that did the capture, and `None`
/// on every later one. Startup hands it to `path_env`, so the one capture
/// serves both: asking the shell a second time just for `PATH` would run the
/// user's whole profile again before the window appears.
pub fn load_login_shell_env() -> Option<String> {
    static ONCE: Once = Once::new();
    let mut shell_path = None;
    ONCE.call_once(|| {
        let pairs = capture_login_shell_env();
        shell_path = pairs
            .iter()
            .find(|(name, _)| name == "PATH")
            .map(|(_, value)| value.clone())
            .filter(|value| !value.trim().is_empty());
        apply_login_shell_env(pairs);
    });
    shell_path
}

/// Writes the captured pairs into the process, skipping the ones already set.
/// A blank value counts as missing: `live_env` treats it as unset too.
fn apply_login_shell_env(pairs: Vec<(String, String)>) {
    for (name, value) in pairs {
        let present = std::env::var_os(&name).is_some_and(|set| !set.is_empty());
        if !present {
            std::env::set_var(name, value);
        }
    }
}

/// The login shell's whole environment.
fn capture_login_shell_env() -> Vec<(String, String)> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        capture_shell_env(
            &login_shell(),
            &[],
            INTERACTIVE_SHELL_ENV_TIMEOUT,
            LOGIN_SHELL_ENV_BUDGET,
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // Windows sources no profile: the app's own environment is all there is.
        Vec::new()
    }
}

/// `$SHELL`, or the platform's own when it names nothing.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|shell| !shell.trim().is_empty())
        .unwrap_or_else(|| {
            if cfg!(target_os = "macos") {
                "/bin/zsh".to_string()
            } else {
                "/bin/bash".to_string()
            }
        })
}

/// The environment a terminal would have. An interactive login shell (`-ilc`, as
/// the `shell-env` package does) also reads `.zshrc` / `.bashrc`, where API keys
/// are usually exported; a plain login shell (`-lc`) skips those files.
///
/// If the interactive attempt fails, is stopped or prints no variables (a
/// `.zshrc` that execs tmux, exits or blocks), `-lc` gets what is left of
/// `budget`.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn capture_shell_env(
    shell: &str,
    envs: &[(&str, &OsStr)],
    interactive_timeout: Duration,
    budget: Duration,
) -> Vec<(String, String)> {
    let started = Instant::now();
    if let Some(pairs) = run_shell_for_env(shell, "-ilc", envs, interactive_timeout.min(budget)) {
        return pairs;
    }
    let remaining = budget.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Vec::new();
    }
    run_shell_for_env(shell, "-lc", envs, remaining).unwrap_or_default()
}

/// `printenv` inside a login shell, written to a file rather than a pipe.
///
/// A pipe would stay open as long as a background job the profile started still
/// holds it, and waiting for that would stall the scan; with the output in a
/// file the child's exit is all there is to wait for.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn run_shell_for_env(
    shell: &str,
    flags: &str,
    envs: &[(&str, &OsStr)],
    timeout: Duration,
) -> Option<Vec<(String, String)>> {
    let dir = std::env::temp_dir().join(format!("the-pair-shell-env-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).ok()?;
    let file = dir.join("printenv");
    let mut command = Command::new(shell);
    command
        .arg(flags)
        .arg(format!("printenv > \"${ENV_FILE_VAR}\""));
    command.env(ENV_FILE_VAR, &file);
    // Oh My Zsh would otherwise check for (and prompt about) updates.
    command.env("DISABLE_AUTO_UPDATE", "true");
    command.envs(envs.iter().copied());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    detach_from_terminal(&mut command);

    let exited = command
        .spawn()
        .ok()
        .and_then(|mut child| wait_bounded(&mut child, timeout));
    let pairs = std::fs::read_to_string(&file)
        .map(|text| parse_printenv(&text))
        .unwrap_or_default();
    std::fs::remove_dir_all(&dir).ok();
    exited
        .filter(|status| status.success())
        .and_then(|_| (!pairs.is_empty()).then_some(pairs))
}

/// Waits for `child` for at most `timeout`. `None` when it had to be stopped: a
/// shell blocked in `wait` takes the background job holding it with it.
fn wait_bounded(child: &mut std::process::Child, timeout: Duration) -> Option<ExitStatus> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Some(status),
            Ok(None) => {}
            Err(_) => {
                stop(child);
                return None;
            }
        }
        if Instant::now() >= deadline {
            stop(child);
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Stops the child and everything it started: it leads its own session, so its
/// process group is the whole tree a blocking profile built.
fn stop(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        const SIGKILL: i32 = 9;
        if let Ok(pid) = i32::try_from(child.id()) {
            // Never let a bogus id turn into kill(0)/kill(-1).
            if pid > 1 {
                // SAFETY: kill(2) takes plain integers and touches no memory; a
                // negative pid addresses the process group.
                unsafe { kill(-pid, SIGKILL) };
            }
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Starts the child in a session of its own, without a controlling terminal: an
/// interactive shell run from a terminal otherwise tries to take the terminal
/// over, gets stopped by SIGTTIN/SIGTTOU and runs into the timeout.
#[cfg(unix)]
fn detach_from_terminal(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    extern "C" {
        fn setsid() -> i32;
    }
    // SAFETY: setsid(2) is async-signal-safe, allocates nothing and only runs
    // in the forked child before exec. Failure (already a group leader, which a
    // fresh fork never is) leaves the child as it was.
    unsafe {
        command.pre_exec(|| {
            setsid();
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn detach_from_terminal(_command: &mut Command) {}

/// `printenv`'s `NAME=value` lines. Only names that could be variables are
/// kept, so a startup file's chatter can't become one.
fn parse_printenv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (name, value) = line.split_once('=')?;
            let is_name = !name.is_empty()
                && name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_');
            is_name.then(|| (name.to_string(), value.to_string()))
        })
        .collect()
}

pub fn scan() -> ImportCandidate {
    let mut candidate = ImportCandidate {
        source_id: "env".into(),
        source_name: "Environment variables".into(),
        config_path: None,
        drafts: Vec::new(),
        notes: Vec::new(),
        error: None,
    };

    // Several variables can name the same endpoint (GEMINI_API_KEY and
    // GOOGLE_API_KEY both point at generativelanguage). The first one wins so
    // the account list does not fill with duplicates.
    let mut seen: Vec<(&str, &str)> = Vec::new();
    for (name, protocol, base_url) in ENV_KEYS {
        if live_env(name).is_none() {
            continue;
        }
        if seen
            .iter()
            .any(|(existing_url, _)| existing_url == base_url)
        {
            candidate.notes.push(format!(
                "{} points at the same endpoint as an earlier variable, so it was skipped.",
                name
            ));
            continue;
        }
        seen.push((base_url, name));

        let Some(account) =
            api_key_account((*name).to_string(), "env", Secret::Env((*name).to_string()))
        else {
            continue;
        };
        candidate.drafts.push(ProviderDraft {
            protocol: *protocol,
            base_url: (*base_url).to_string(),
            vendor: None,
            display_name: None,
            accounts: vec![account],
            models: Vec::new(),
        });
    }

    if candidate.drafts.is_empty() && candidate.notes.is_empty() {
        candidate
            .notes
            .push("No well-known provider API key is set in this shell.".into());
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The env lock plus the login shell's variables already in the process, so
    /// a test neither races the one-time capture nor sees it land halfway
    /// through its assertions.
    fn isolated_env() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::test_env::lock_env();
        crate::sources::env::load_login_shell_env();
        guard
    }

    #[test]
    fn a_live_variable_becomes_an_env_account() {
        let _guard = isolated_env();
        let saved = std::env::var_os("ANTHROPIC_API_KEY");
        std::env::set_var("ANTHROPIC_API_KEY", "sk-ant-test");
        std::env::remove_var("OPENAI_API_KEY");

        let candidate = scan();
        let draft = candidate
            .drafts
            .iter()
            .find(|draft| draft.base_url == "https://api.anthropic.com")
            .expect("anthropic draft");
        assert_eq!(draft.accounts.len(), 1);
        assert_eq!(
            draft.accounts[0].secret,
            Secret::Env("ANTHROPIC_API_KEY".into())
        );
        assert!(draft.accounts[0].secret.is_present());

        match saved {
            Some(value) => std::env::set_var("ANTHROPIC_API_KEY", value),
            None => std::env::remove_var("ANTHROPIC_API_KEY"),
        }
    }

    #[test]
    fn unset_variables_produce_nothing() {
        let _guard = isolated_env();
        let saved: Vec<_> = ENV_KEYS
            .iter()
            .map(|(name, _, _)| (*name, std::env::var_os(name)))
            .collect();
        for (name, _, _) in ENV_KEYS {
            std::env::remove_var(name);
        }

        let candidate = scan();
        assert!(candidate.drafts.is_empty());
        assert!(!candidate.notes.is_empty());

        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    #[test]
    fn two_variables_for_one_endpoint_collapse() {
        let _guard = isolated_env();
        let saved_gemini = std::env::var_os("GEMINI_API_KEY");
        let saved_google = std::env::var_os("GOOGLE_API_KEY");
        std::env::set_var("GEMINI_API_KEY", "a");
        std::env::set_var("GOOGLE_API_KEY", "b");

        let candidate = scan();
        let google_drafts: Vec<_> = candidate
            .drafts
            .iter()
            .filter(|draft| draft.base_url.contains("generativelanguage"))
            .collect();
        assert_eq!(google_drafts.len(), 1);
        assert!(candidate
            .notes
            .iter()
            .any(|note| note.contains("GOOGLE_API_KEY")));

        match saved_gemini {
            Some(value) => std::env::set_var("GEMINI_API_KEY", value),
            None => std::env::remove_var("GEMINI_API_KEY"),
        }
        match saved_google {
            Some(value) => std::env::set_var("GOOGLE_API_KEY", value),
            None => std::env::remove_var("GOOGLE_API_KEY"),
        }
    }

    #[test]
    fn printenv_lines_are_parsed_and_chatter_ignored() {
        let parsed = parse_printenv(
            "ANTHROPIC_API_KEY=sk-ant-1\nnot a pair\nweird name=x\n=\nHOME=/Users/alex\n",
        );
        assert_eq!(
            parsed,
            vec![
                ("ANTHROPIC_API_KEY".to_string(), "sk-ant-1".to_string()),
                ("HOME".to_string(), "/Users/alex".to_string()),
            ]
        );
    }

    #[test]
    fn a_captured_variable_only_fills_in_what_is_missing() {
        let _guard = isolated_env();
        let names = [
            "THE_PAIR_TEST_SHELL_FILL",
            "THE_PAIR_TEST_SHELL_ADD",
            "THE_PAIR_TEST_SHELL_BLANK",
        ];
        let saved: Vec<_> = names
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect();
        std::env::set_var("THE_PAIR_TEST_SHELL_FILL", "from-the-app");
        std::env::remove_var("THE_PAIR_TEST_SHELL_ADD");
        std::env::set_var("THE_PAIR_TEST_SHELL_BLANK", "");

        apply_login_shell_env(
            names
                .iter()
                .map(|name| (name.to_string(), "from-the-shell".to_string()))
                .collect(),
        );

        assert_eq!(
            std::env::var("THE_PAIR_TEST_SHELL_FILL").unwrap(),
            "from-the-app",
            "a variable the app has must not be overwritten"
        );
        assert_eq!(
            std::env::var("THE_PAIR_TEST_SHELL_ADD").unwrap(),
            "from-the-shell"
        );
        assert_eq!(
            std::env::var("THE_PAIR_TEST_SHELL_BLANK").unwrap(),
            "from-the-shell",
            "a blank value counts as missing"
        );

        for (name, value) in saved {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    #[test]
    fn loading_the_shell_env_twice_changes_nothing() {
        let _guard = isolated_env();
        let saved = std::env::var_os("THE_PAIR_TEST_SHELL_ONCE");
        std::env::remove_var("THE_PAIR_TEST_SHELL_ONCE");

        load_login_shell_env();
        let after_first = std::env::var_os("THE_PAIR_TEST_SHELL_ONCE");
        std::env::set_var("THE_PAIR_TEST_SHELL_ONCE", "set-by-the-test");
        load_login_shell_env();

        assert_eq!(
            std::env::var("THE_PAIR_TEST_SHELL_ONCE").unwrap(),
            "set-by-the-test",
            "the capture runs once per process"
        );
        assert!(after_first.is_none() || after_first == saved);

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_SHELL_ONCE", value),
            None => std::env::remove_var("THE_PAIR_TEST_SHELL_ONCE"),
        }
    }

    /// A throwaway home holding the given startup files.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn shell_home(files: &[(&str, &str)]) -> std::path::PathBuf {
        let home =
            std::env::temp_dir().join(format!("the-pair-shell-env-{}", uuid::Uuid::new_v4()));
        for (name, content) in files {
            let path = home.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, content).unwrap();
        }
        home
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn shell_env_includes_what_a_zshrc_exports() {
        if !std::path::Path::new("/bin/zsh").exists() {
            return;
        }
        let home = shell_home(&[
            (".zshenv", "echo zshenv says hi\n"),
            (
                ".zprofile",
                "export THE_PAIR_TEST_SHELL_PROFILE=sk-profile\n",
            ),
            // Prints, reads stdin (closed), then exports, as an installer does.
            (
                ".zshrc",
                "echo rc noise\nread -r answer\nexport THE_PAIR_TEST_SHELL_RC=sk-rc\n",
            ),
        ]);

        let env = capture_shell_env(
            "/bin/zsh",
            &[("ZDOTDIR", home.as_os_str()), ("HOME", home.as_os_str())],
            Duration::from_secs(3),
            Duration::from_secs(5),
        );
        let value = |name: &str| {
            env.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(value("THE_PAIR_TEST_SHELL_RC"), Some("sk-rc"));
        assert_eq!(value("THE_PAIR_TEST_SHELL_PROFILE"), Some("sk-profile"));
        assert_eq!(value("HOME"), Some(home.to_string_lossy().as_ref()));

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn shell_env_falls_back_to_a_login_shell_when_zshrc_blocks() {
        if !std::path::Path::new("/bin/zsh").exists() {
            return;
        }
        let home = shell_home(&[
            (
                ".zprofile",
                "export THE_PAIR_TEST_SHELL_PROFILE=sk-profile\n",
            ),
            (".zshrc", "sleep 30 &\nwait\n"),
        ]);

        let started = Instant::now();
        let env = capture_shell_env(
            "/bin/zsh",
            &[("ZDOTDIR", home.as_os_str()), ("HOME", home.as_os_str())],
            Duration::from_millis(700),
            Duration::from_secs(4),
        );
        assert!(started.elapsed() < Duration::from_secs(4));
        let value = env
            .iter()
            .find(|(key, _)| key == "THE_PAIR_TEST_SHELL_PROFILE")
            .map(|(_, value)| value.as_str());
        assert_eq!(value, Some("sk-profile"), "the -lc fallback still answers");

        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    fn shell_env_is_empty_for_a_missing_shell() {
        let started = Instant::now();
        assert!(capture_shell_env(
            "/nonexistent/the-pair-shell",
            &[],
            Duration::from_secs(3),
            Duration::from_secs(5),
        )
        .is_empty());
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
