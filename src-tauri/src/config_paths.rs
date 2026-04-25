use std::path::{Path, PathBuf};

pub fn build_opencode_config_path(base: impl AsRef<Path>, is_windows: bool) -> PathBuf {
    let base = base.as_ref();
    if is_windows {
        base.join("opencode").join("opencode.json")
    } else {
        base.join(".config/opencode/opencode.json")
    }
}

pub fn build_opencode_auth_path(base: impl AsRef<Path>, is_windows: bool) -> PathBuf {
    let base = base.as_ref();
    if is_windows {
        base.join("opencode").join("auth.json")
    } else {
        base.join(".local/share/opencode/auth.json")
    }
}

pub fn opencode_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA").or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData/Roaming").into_os_string())
        })?;
        return Some(build_opencode_config_path(PathBuf::from(base), true));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var_os("HOME")?;
        Some(build_opencode_config_path(PathBuf::from(base), false))
    }
}

pub fn opencode_auth_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA").or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(|home| PathBuf::from(home).join("AppData/Roaming").into_os_string())
        })?;
        return Some(build_opencode_auth_path(PathBuf::from(base), true));
    }

    #[cfg(not(target_os = "windows"))]
    {
        let base = std::env::var_os("HOME")?;
        Some(build_opencode_auth_path(PathBuf::from(base), false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_opencode_config_path_uses_unix_layout_when_requested() {
        let path = build_opencode_config_path("/Users/alex", false);

        assert_eq!(
            path,
            PathBuf::from("/Users/alex/.config/opencode/opencode.json")
        );
    }

    #[test]
    fn build_opencode_config_path_uses_windows_layout_when_requested() {
        let path = build_opencode_config_path("/Users/alex/AppData/Roaming", true);

        assert_eq!(
            path,
            PathBuf::from("/Users/alex/AppData/Roaming/opencode/opencode.json")
        );
    }

    #[test]
    fn build_opencode_auth_path_uses_unix_layout_when_requested() {
        let path = build_opencode_auth_path("/Users/alex", false);

        assert_eq!(
            path,
            PathBuf::from("/Users/alex/.local/share/opencode/auth.json")
        );
    }

    #[test]
    fn build_opencode_auth_path_uses_windows_layout_when_requested() {
        let path = build_opencode_auth_path("/Users/alex/AppData/Roaming", true);

        assert_eq!(
            path,
            PathBuf::from("/Users/alex/AppData/Roaming/opencode/auth.json")
        );
    }
}
