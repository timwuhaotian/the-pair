use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

pub fn refresh_path_from_login_shell() {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let base = if let Some(shell_path) = capture_login_shell_path() {
        OsString::from(shell_path)
    } else {
        current.clone()
    };

    let extra_dirs = fallback_path_dirs(
        std::env::var_os(if cfg!(target_os = "windows") {
            "USERPROFILE"
        } else {
            "HOME"
        })
        .map(PathBuf::from),
        std::env::var_os("APPDATA").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        cfg!(target_os = "windows"),
    );

    if let Ok(merged) = merge_path_entries(&base, &extra_dirs) {
        std::env::set_var("PATH", merged);
    }
}

pub(crate) fn fallback_path_dirs(
    home: Option<PathBuf>,
    appdata: Option<PathBuf>,
    local_appdata: Option<PathBuf>,
    is_windows: bool,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if is_windows {
        if let Some(path) = appdata
            .clone()
            .or_else(|| home.as_ref().map(|path| path.join("AppData/Roaming")))
        {
            dirs.push(path.join("npm"));
        }

        if let Some(path) = local_appdata
            .clone()
            .or_else(|| home.as_ref().map(|path| path.join("AppData/Local")))
        {
            dirs.push(path.join("npm"));
        }
    } else {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/opt/homebrew/sbin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
        ]);

        if let Some(home) = home {
            dirs.push(home.join(".local/bin"));
            dirs.push(home.join("go/bin"));
            dirs.push(home.join(".npm-global/bin"));
            dirs.push(home.join(".volta/bin"));
            dirs.extend(nvm_bin_dirs(&home));
        }
    }

    dirs
}

fn merge_path_entries(
    base: &OsString,
    extra_dirs: &[PathBuf],
) -> Result<OsString, std::env::JoinPathsError> {
    let mut paths: Vec<PathBuf> = std::env::split_paths(base).collect();
    for dir in extra_dirs {
        if !paths.iter().any(|existing| existing == dir) {
            paths.push(dir.clone());
        }
    }
    std::env::join_paths(paths)
}

fn nvm_bin_dirs(home: &std::path::Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let nvm_versions = home.join(".nvm/versions/node");
    if let Ok(entries) = fs::read_dir(nvm_versions) {
        for entry in entries.flatten() {
            dirs.push(entry.path().join("bin"));
        }
    }
    dirs
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub fn capture_login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".to_string()
        } else {
            "/bin/bash".to_string()
        }
    });

    let output = Command::new(shell)
        .arg("-lc")
        .arg("printf '%s' \"$PATH\"")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let path = String::from_utf8(output.stdout).ok()?;
    let trimmed = path.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn capture_login_shell_path() -> Option<String> {
    None
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
        }
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
    }
}
