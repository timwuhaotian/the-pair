//! Credential persistence.
//!
//! Everything lives in one file under the app data dir, written 0600 through a
//! temp-file + rename so a crash mid-write cannot truncate the registry. The
//! OAuth services additionally keep their token in its own file (killi's
//! approach) so a token refresh never rewrites the whole registry.

use super::types::{ModelRoute, ModelSource, Provider};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub const FILE_NAME: &str = "credentials.json";
pub const FILE_VERSION: u32 = 1;
pub const OAUTH_DIR: &str = "oauth";
/// The `source` every mock endpoint's account carries, so a mock registry can
/// be told apart from a real one on the way to disk.
const MOCK_SOURCE: &str = "mock";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialFile {
    pub version: u32,
    pub providers: Vec<Provider>,
}

/// Serializes read-modify-write cycles across Tauri commands.
static STORE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn global_lock() -> &'static Mutex<()> {
    STORE_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn store_path(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// A file the app cannot read in full still lets it boot: what can be read is
/// loaded, and the user can re-run the import for the rest. Nothing is lost by
/// that — [`save`] keeps a copy of such a file before it writes over it.
///
/// Under `THE_PAIR_E2E_MOCK` a machine with no credentials still gets a
/// registry, so the desktop e2e suite can create a pair without a real API key.
/// It used to get the same from the deleted CLI provider probes. Those endpoints
/// are merged in on every read and dropped on every write, so the first command
/// that saves cannot freeze them into the user's real credential file.
pub fn load(dir: &Path) -> Vec<Provider> {
    let stored = read_stored(dir);
    if !crate::util::is_mock_mode() {
        return stored;
    }
    let mut providers = mock_registry();
    for provider in stored {
        if !providers.iter().any(|kept| kept.id == provider.id) {
            providers.push(provider);
        }
    }
    providers
}

fn read_stored(dir: &Path) -> Vec<Provider> {
    let path = store_path(dir);
    let Ok(bytes) = fs::read(&path) else {
        return Vec::new();
    };
    // A registry restored from a backup can arrive world-readable; tightening it
    // on read keeps that exposure from outliving the build that allowed it.
    restrict_file(&path);
    parse_stored(&bytes).providers
}

/// What a credential file's bytes hold, as far as this build can tell.
struct Parsed {
    providers: Vec<Provider>,
    /// False when the file says more than `providers` does: it is not JSON, it
    /// was written by a newer build, or an entry did not parse. Writing the
    /// registry back would then erase something.
    whole: bool,
}

/// Reads a credential file leniently: one entry this build cannot parse must
/// not hide every other key the user stored.
///
/// A file from a newer build is not read at all — its entries may mean something
/// this build would get wrong — and a file that is not JSON has nothing to read.
fn parse_stored(bytes: &[u8]) -> Parsed {
    let nothing = Parsed {
        providers: Vec::new(),
        whole: false,
    };
    let Ok(file) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return nothing;
    };
    let version = file.get("version").and_then(serde_json::Value::as_u64);
    let Some(entries) = file.get("providers").and_then(serde_json::Value::as_array) else {
        return nothing;
    };
    if version.map_or(true, |version| version > u64::from(FILE_VERSION)) {
        return nothing;
    }
    let providers: Vec<Provider> = entries
        .iter()
        .filter_map(|entry| serde_json::from_value(entry.clone()).ok())
        .collect();
    Parsed {
        whole: providers.len() == entries.len(),
        providers,
    }
}

/// Keeps a copy of a credential file this build could not read in full, before
/// [`save`] replaces it.
///
/// [`load`] hands back only what it could read, so the registry about to be
/// written is that plus whatever the user just changed — and the atomic replace
/// would erase every key and sign-in the file held beyond it: a downgrade from a
/// newer build, or one entry in a shape this build does not know. The copy sits
/// next to the store, owner-only like it. A copy that cannot be written fails
/// the save: losing the edit is recoverable, losing the stored keys is not.
fn preserve_unreadable(dir: &Path) -> Result<(), String> {
    let path = store_path(dir);
    let Ok(bytes) = fs::read(&path) else {
        return Ok(());
    };
    if parse_stored(&bytes).whole {
        return Ok(());
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let copy = dir.join(format!("credentials.unreadable-{}.json", millis));
    write_private(&copy, &bytes).map_err(|error| {
        format!(
            "{} could not be read in full, and backing it up before saving failed: {}",
            path.display(),
            error
        )
    })?;
    eprintln!(
        "[accounts] {} could not be read in full; a copy was kept at {} before saving",
        path.display(),
        copy.display()
    );
    Ok(())
}

/// Whether an endpoint only exists to serve the e2e mock.
fn is_mock_provider(provider: &Provider) -> bool {
    provider
        .accounts
        .iter()
        .any(|account| account.source == MOCK_SOURCE)
}

/// What actually reaches the credential file: everything but the mock
/// endpoints, which are re-derived on every read.
fn stored_providers(providers: &[Provider]) -> Vec<Provider> {
    if !crate::util::is_mock_mode() {
        return providers.to_vec();
    }
    providers
        .iter()
        .filter(|provider| !is_mock_provider(provider))
        .cloned()
        .collect()
}

/// Two endpoints with a credential apiece: one Anthropic-shaped, one
/// OpenAI-Chat-shaped, each listing a model a mock turn can be pointed at.
fn mock_registry() -> Vec<Provider> {
    use crate::accounts::types::{Account, AccountKind, AccountStatus, Protocol, Secret};
    let endpoint = |id: &str, protocol: Protocol, base_url: &str, model: &str| Provider {
        id: format!("{}-1", id),
        protocol,
        base_url: base_url.into(),
        vendor: id.into(),
        display_name: format!("{} (mock)", id),
        accounts: vec![Account {
            id: format!("{}-1-a", id),
            kind: AccountKind::ApiKey,
            label: id.into(),
            source: MOCK_SOURCE.into(),
            status: AccountStatus::Ready,
            headers: Vec::new(),
            tool_names: None,
            secret: Secret::ApiKey("mock-key".into()),
        }],
        models: vec![ModelRoute {
            id: model.into(),
            display_name: model.into(),
            context_window: None,
            max_output: None,
            efforts: Vec::new(),
            default_effort: None,
            price: None,
            source: ModelSource::Manual,
        }],
    };
    vec![
        endpoint(
            "anthropic",
            Protocol::Anthropic,
            "https://api.anthropic.com",
            "claude-sonnet-4-5",
        ),
        endpoint(
            "openai",
            Protocol::OpenAiChat,
            "https://api.openai.com/v1",
            "gpt-4o-mini",
        ),
    ]
}

pub fn save(dir: &Path, providers: &[Provider]) -> Result<(), String> {
    preserve_unreadable(dir)?;
    let file = CredentialFile {
        version: FILE_VERSION,
        providers: stored_providers(providers),
    };
    let text = serde_json::to_string_pretty(&file)
        .map_err(|error| format!("Failed to serialize credentials: {}", error))?;
    write_private(&store_path(dir), text.as_bytes())
}

/// Writes `bytes` to `path` with 0600 permissions, atomically.
///
/// The temp file is created 0600 from the start — `fs::write` would open it with
/// the default umask and leave a world-readable window holding the plaintext
/// keys — and it is removed again if anything fails, so a half-written secret
/// never survives the attempt.
pub fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("No parent directory for {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create {}: {}", parent.display(), error))?;
    restrict_dir(parent);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut temp = path.with_extension(format!("tmp-{}", nanos));
    let mut attempt = 0;
    loop {
        match create_private(&temp, bytes) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && attempt < 8 => {
                attempt += 1;
                temp = path.with_extension(format!("tmp-{}-{}", nanos, attempt));
            }
            Err(error) => {
                let _ = fs::remove_file(&temp);
                return Err(format!("Failed to write {}: {}", temp.display(), error));
            }
        }
    }
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(format!(
            "Failed to move {} into place: {}",
            temp.display(),
            error
        ));
    }
    restrict_file(path);
    Ok(())
}

fn create_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = open_private(path)?;
    file.write_all(bytes)
}

#[cfg(unix)]
fn open_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_private(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// `oauth/<name>.json` inside the app data dir.
pub fn oauth_path(dir: &Path, service: &str) -> PathBuf {
    dir.join(OAUTH_DIR).join(format!("{}.json", service))
}

#[cfg(unix)]
fn restrict_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
}

#[cfg(not(unix))]
fn restrict_dir(_path: &Path) {}

#[cfg(unix)]
fn restrict_file(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::types::{
        Account, AccountKind, AccountStatus, Protocol, Provider as ProviderEntry, Secret,
    };

    fn sample() -> Vec<ProviderEntry> {
        vec![ProviderEntry {
            id: "anthropic-1".into(),
            protocol: Protocol::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            vendor: "anthropic".into(),
            display_name: "Anthropic".into(),
            accounts: vec![Account {
                id: "anthropic-1-a".into(),
                kind: AccountKind::ApiKey,
                label: "claude".into(),
                source: "claude".into(),
                status: AccountStatus::Ready,
                headers: vec![],
                tool_names: None,
                secret: Secret::ApiKey("sk-ant-test".into()),
            }],
            models: vec![],
        }]
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("the-pair-store-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trips_providers() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        save(&dir, &sample()).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded, sample());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_file_loads_empty() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        assert!(load(&dir).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_loads_empty_rather_than_failing_the_app() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        fs::write(store_path(&dir), "{ not json").unwrap();
        assert!(load(&dir).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn future_version_is_ignored() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        fs::write(
            store_path(&dir),
            serde_json::json!({"version": 99, "providers": sample()}).to_string(),
        )
        .unwrap();
        assert!(load(&dir).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    /// The copies `save` kept of a file it could not read in full.
    fn kept_copies(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("credentials.unreadable-"))
            })
            .collect()
    }

    #[test]
    fn saving_over_a_newer_file_keeps_a_copy_of_it() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        // After a downgrade the file reads as empty; adding one key must not
        // erase everything the newer build had stored.
        let newer = serde_json::json!({"version": 99, "providers": sample()}).to_string();
        fs::write(store_path(&dir), &newer).unwrap();
        assert!(load(&dir).is_empty());

        save(&dir, &sample()).unwrap();

        let copies = kept_copies(&dir);
        assert_eq!(copies.len(), 1, "{:?}", copies);
        assert_eq!(fs::read_to_string(&copies[0]).unwrap(), newer);
        assert_eq!(load(&dir), sample());

        // The file is whole now, so the next save has nothing to keep.
        save(&dir, &sample()).unwrap();
        assert_eq!(kept_copies(&dir).len(), 1);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_over_a_corrupt_file_keeps_a_copy_of_it() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        fs::write(store_path(&dir), "{ not json").unwrap();

        save(&dir, &sample()).unwrap();

        let copies = kept_copies(&dir);
        assert_eq!(copies.len(), 1);
        assert_eq!(fs::read_to_string(&copies[0]).unwrap(), "{ not json");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&copies[0]).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the copy holds keys too");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn one_unreadable_entry_does_not_hide_the_others() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        let mut entries = serde_json::to_value(sample()).unwrap();
        entries
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"id": "future-1", "protocol": "somethingNew"}));
        let text = serde_json::json!({"version": FILE_VERSION, "providers": entries}).to_string();
        fs::write(store_path(&dir), &text).unwrap();

        // The entry this build knows still loads.
        assert_eq!(load(&dir), sample());

        // And the one it does not know survives the next save, in the copy.
        save(&dir, &load(&dir)).unwrap();
        let copies = kept_copies(&dir);
        assert_eq!(copies.len(), 1);
        assert!(fs::read_to_string(&copies[0]).unwrap().contains("future-1"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_whole_file_and_a_missing_one_need_no_copy() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        save(&dir, &sample()).unwrap();
        save(&dir, &sample()).unwrap();
        assert!(kept_copies(&dir).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn private_files_are_user_only_on_unix() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        save(&dir, &sample()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store_path(&dir)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "credential file is world-readable");
            let dir_mode = fs::metadata(&dir).unwrap().permissions().mode();
            assert_eq!(dir_mode & 0o777, 0o700);
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_new_secret_file_is_user_only_from_the_moment_it_exists() {
        let dir = temp_dir();
        let path = dir.join("nested/token.json");
        fs::create_dir_all(dir.join("nested")).unwrap();
        create_private(&path, b"sk-ant-test").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "temp file is world-readable");
        }
        assert_eq!(fs::read(&path).unwrap(), b"sk-ant-test".to_vec());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_failed_write_leaves_no_secret_temp_file_behind() {
        let dir = temp_dir();
        // A directory cannot be renamed over, so the move fails after the temp
        // file has been written.
        let path = dir.join("credentials.json");
        fs::create_dir(&path).unwrap();
        assert!(write_private(&path, b"sk-ant-test").is_err());
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(
            leftovers,
            vec![std::ffi::OsString::from("credentials.json")]
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn loading_tightens_a_loose_credential_file() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        save(&dir, &sample()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // A registry restored from a backup, or written by an older build.
            fs::set_permissions(store_path(&dir), fs::Permissions::from_mode(0o644)).unwrap();
        }
        assert_eq!(load(&dir), sample());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store_path(&dir)).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "file stayed world-readable");
        }
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mock_endpoints_are_never_written_to_disk() {
        let dir = temp_dir();
        let _guard = crate::test_env::lock_env();
        std::env::set_var("THE_PAIR_E2E_MOCK", "true");

        assert_eq!(load(&dir), mock_registry());
        // Saving what was loaded — what every command does — must not freeze the
        // fixture into the credential file.
        save(&dir, &load(&dir)).unwrap();
        let text = fs::read_to_string(store_path(&dir)).unwrap();
        assert!(!text.contains("mock"), "mock registry persisted: {}", text);
        assert_eq!(load(&dir), mock_registry());

        // A real endpoint added next to them survives, and keeps them company.
        let mut providers = load(&dir);
        let mut real = sample()[0].clone();
        real.id = "manual-1".into();
        real.accounts[0].id = "manual-1-a".into();
        providers.push(real);
        save(&dir, &providers).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.len(), 3);
        assert!(loaded.iter().any(|provider| provider.id == "manual-1"));
        assert!(loaded.iter().any(|provider| provider.id == "anthropic-1"));

        std::env::remove_var("THE_PAIR_E2E_MOCK");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn writes_are_atomic_and_leave_no_temp_files() {
        let dir = temp_dir();
        write_private(&dir.join("nested/oauth/chatgpt.json"), b"{}").unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir.join("nested/oauth"))
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(leftovers, vec![std::ffi::OsString::from("chatgpt.json")]);
        assert_eq!(
            std::fs::read(dir.join("nested/oauth/chatgpt.json")).unwrap(),
            b"{}".to_vec()
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn oauth_path_is_namespaced_per_service() {
        let dir = Path::new("/data");
        assert_eq!(
            oauth_path(dir, "chatgpt"),
            PathBuf::from("/data/oauth/chatgpt.json")
        );
    }
}
