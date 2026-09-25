//! Where OpenCode keeps its global config and credentials.
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

fn home_dir() -> Option<PathBuf> {
    Some(crate::provider_registry::homedir()).filter(|home| !home.as_os_str().is_empty())
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

/// Global config directories OpenCode may read, most specific last: the XDG
/// default, then `$OPENCODE_CONFIG_DIR` when set (2.x reads it instead of the
/// default, 1.x in addition to it).
pub fn opencode_config_dirs() -> Vec<PathBuf> {
    let home = home_dir();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in [
        build_opencode_config_dir(
            None,
            env_path("XDG_CONFIG_HOME").as_deref(),
            home.as_deref(),
        ),
        env_path("OPENCODE_CONFIG_DIR"),
    ]
    .into_iter()
    .flatten()
    {
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs
}

pub fn opencode_auth_path() -> Option<PathBuf> {
    let primary =
        build_opencode_auth_path(env_path("XDG_DATA_HOME").as_deref(), home_dir().as_deref());
    prefer_existing(primary, legacy_windows_path(OPENCODE_AUTH_FILE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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

    #[test]
    fn auth_path_uses_xdg_data_home_or_local_share() {
        let home = Some(Path::new("/Users/alex"));
        assert_eq!(
            build_opencode_auth_path(None, home),
            Some(PathBuf::from("/Users/alex/.local/share/opencode/auth.json"))
        );
        assert_eq!(
            build_opencode_auth_path(Some(Path::new("/xdg/data")), home),
            Some(PathBuf::from("/xdg/data/opencode/auth.json"))
        );
        assert_eq!(build_opencode_auth_path(None, None), None);
    }

    #[test]
    fn legacy_file_is_used_only_when_the_real_location_has_none() {
        let root = std::env::temp_dir().join(format!("the-pair-test-{}", uuid::Uuid::new_v4()));
        let primary = root.join("home/.config/opencode/opencode.json");
        let legacy = root.join("AppData/Roaming/opencode/opencode.json");

        // Nothing exists yet: point at the location OpenCode reads.
        assert_eq!(
            prefer_existing(Some(primary.clone()), Some(legacy.clone())),
            Some(primary.clone())
        );

        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::write(&legacy, "{}").unwrap();
        assert_eq!(
            prefer_existing(Some(primary.clone()), Some(legacy.clone())),
            Some(legacy.clone())
        );

        fs::create_dir_all(primary.parent().unwrap()).unwrap();
        fs::write(&primary, "{}").unwrap();
        assert_eq!(
            prefer_existing(Some(primary.clone()), Some(legacy)),
            Some(primary)
        );

        fs::remove_dir_all(&root).ok();
    }

    // Unix only: on Windows the home dir comes from USERPROFILE and a legacy
    // `%APPDATA%\opencode` file on the machine would take part.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn opencode_paths_follow_the_environment() {
        const KEYS: [&str; 4] = [
            "OPENCODE_CONFIG_DIR",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "HOME",
        ];
        let _guard = crate::test_env::lock_env();
        let saved: Vec<(&str, Option<std::ffi::OsString>)> = KEYS
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect();
        let home = std::env::temp_dir().join(format!("the-pair-test-{}", uuid::Uuid::new_v4()));

        std::env::set_var("HOME", &home);
        std::env::set_var("OPENCODE_CONFIG_DIR", "");
        std::env::set_var("XDG_CONFIG_HOME", "/xdg/config");
        std::env::set_var("XDG_DATA_HOME", "/xdg/data");
        let xdg = (opencode_config_path(), opencode_auth_path());
        let xdg_dirs = opencode_config_dirs();

        std::env::set_var("OPENCODE_CONFIG_DIR", "/custom/opencode");
        let overridden = opencode_config_path();
        let overridden_dirs = opencode_config_dirs();

        for key in ["OPENCODE_CONFIG_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME"] {
            std::env::remove_var(key);
        }
        let defaults = (opencode_config_path(), opencode_auth_path());

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }

        assert_eq!(
            xdg,
            (
                Some(PathBuf::from("/xdg/config/opencode/opencode.json")),
                Some(PathBuf::from("/xdg/data/opencode/auth.json")),
            )
        );
        assert_eq!(
            overridden,
            Some(PathBuf::from("/custom/opencode/opencode.json"))
        );
        assert_eq!(xdg_dirs, vec![PathBuf::from("/xdg/config/opencode")]);
        assert_eq!(
            overridden_dirs,
            vec![
                PathBuf::from("/xdg/config/opencode"),
                PathBuf::from("/custom/opencode")
            ]
        );
        assert_eq!(
            defaults,
            (
                Some(home.join(".config/opencode/opencode.json")),
                Some(home.join(".local/share/opencode/auth.json")),
            )
        );
    }
}
