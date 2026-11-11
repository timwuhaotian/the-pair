//! Where OpenCode and Muse keep their global config and credentials.
//!
//! This mirrors opencode's own resolution (its bundled `xdg-basedir` logic),
//! which is the same on every platform, Windows included:
//!
//! - config: `$OPENCODE_CONFIG_DIR/opencode.json`, else
//!   `${XDG_CONFIG_HOME:-~/.config}/opencode/opencode.json`
//! - auth: `${XDG_DATA_HOME:-~/.local/share}/opencode/auth.json`
//!
//! Earlier versions of this app looked under `%APPDATA%\opencode` on Windows.
//! OpenCode never reads that, but a file the app created there is still used
//! when the real location has none, so existing setups keep working.

use std::path::{Path, PathBuf};

const OPENCODE_CONFIG_FILE: &str = "opencode.json";
const OPENCODE_AUTH_FILE: &str = "auth.json";

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn non_empty_env_path(key: &str) -> Option<PathBuf> {
    env_path(key).filter(|value| !value.as_os_str().is_empty())
}

/// The user's home directory, from the environment first so a test (or a
/// shell with a stale `HOME`) can point the importers somewhere else.
pub fn home_dir() -> Option<PathBuf> {
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
    home.filter(|home| !home.as_os_str().is_empty())
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

/// `%APPDATA%\opencode\<file>`, the location older builds used on Windows.
fn legacy_windows_path(file: &str) -> Option<PathBuf> {
    if !cfg!(target_os = "windows") {
        return None;
    }
    let appdata =
        env_path("APPDATA").or_else(|| home_dir().map(|home| home.join("AppData/Roaming")))?;
    Some(appdata.join("opencode").join(file))
}

fn build_opencode_config_dir(
    config_dir_override: Option<&Path>,
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    match config_dir_override {
        Some(dir) => Some(dir.to_path_buf()),
        None => Some(
            xdg_config_home
                .map(Path::to_path_buf)
                .or_else(|| home.map(|home| home.join(".config")))?
                .join("opencode"),
        ),
    }
}

fn build_opencode_config_path(
    config_dir_override: Option<&Path>,
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Option<PathBuf> {
    build_opencode_config_dir(config_dir_override, xdg_config_home, home)
        .map(|dir| dir.join(OPENCODE_CONFIG_FILE))
}

fn build_opencode_auth_path(xdg_data_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let data_home = xdg_data_home
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".local/share")))?;
    Some(data_home.join("opencode").join(OPENCODE_AUTH_FILE))
}

/// The path OpenCode reads, unless only the legacy file exists.
fn prefer_existing(primary: Option<PathBuf>, legacy: Option<PathBuf>) -> Option<PathBuf> {
    match (primary, legacy) {
        (Some(primary), _) if primary.exists() => Some(primary),
        (_, Some(legacy)) if legacy.exists() => Some(legacy),
        (primary, _) => primary,
    }
}

pub fn opencode_config_path() -> Option<PathBuf> {
    let primary = build_opencode_config_path(
        env_path("OPENCODE_CONFIG_DIR").as_deref(),
        env_path("XDG_CONFIG_HOME").as_deref(),
        home_dir().as_deref(),
    );
    prefer_existing(primary, legacy_windows_path(OPENCODE_CONFIG_FILE))
}

pub fn opencode_auth_path() -> Option<PathBuf> {
    let primary =
        build_opencode_auth_path(env_path("XDG_DATA_HOME").as_deref(), home_dir().as_deref());
    prefer_existing(primary, legacy_windows_path(OPENCODE_AUTH_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_defaults_to_dot_config_on_every_platform() {
        assert_eq!(
            build_opencode_config_path(None, None, Some(Path::new("/Users/alex"))),
            Some(PathBuf::from("/Users/alex/.config/opencode/opencode.json"))
        );
        assert_eq!(build_opencode_config_path(None, None, None), None);
    }

    #[test]
    fn config_path_honors_xdg_config_home_and_the_opencode_override() {
        let home = Some(Path::new("/Users/alex"));
        assert_eq!(
            build_opencode_config_path(None, Some(Path::new("/xdg/config")), home),
            Some(PathBuf::from("/xdg/config/opencode/opencode.json"))
        );
        // OPENCODE_CONFIG_DIR names the opencode dir itself, not its parent.
        assert_eq!(
            build_opencode_config_path(
                Some(Path::new("/custom/opencode-config")),
                Some(Path::new("/xdg/config")),
                home,
            ),
            Some(PathBuf::from("/custom/opencode-config/opencode.json"))
        );
    }
}
