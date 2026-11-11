//! Discovering model configuration inside other coding agents' installs.
//!
//! Every importer is read-only and returns [`ImportCandidate`] drafts; nothing
//! is written until the user confirms in the import wizard. Scans are pure
//! functions of the filesystem so they are unit-testable against temp dirs.

pub mod claude;
pub mod codex;
pub mod env;
pub mod gemini;
pub mod killi;
pub mod kimi;
pub mod omp;
pub mod opencode;
pub mod pi;

use crate::accounts::registry::{AccountDraft, ProviderDraft};
use crate::accounts::types::{AccountKind, Protocol, Secret};
use std::path::{Path, PathBuf};

/// One coding agent whose model configuration The Pair can read.
pub struct ImportSource {
    pub id: &'static str,
    pub name: &'static str,
    /// The config file that proves this agent is set up on the machine.
    pub detect: fn() -> Option<PathBuf>,
    pub scan: fn() -> ImportCandidate,
}

/// What a scan found, before anything is written.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportCandidate {
    pub source_id: String,
    pub source_name: String,
    pub config_path: Option<PathBuf>,
    pub drafts: Vec<ProviderDraft>,
    /// Things the user should know (skipped subscriptions, dropped defaults).
    pub notes: Vec<String>,
    pub error: Option<String>,
}

/// The importable agents, in the order the wizard lists them.
pub fn sources() -> Vec<ImportSource> {
    vec![
        ImportSource {
            id: "kimi",
            name: "Kimi Code",
            detect: detect_kimi,
            scan: kimi::scan,
        },
        ImportSource {
            id: "pi",
            name: "pi",
            detect: detect_pi,
            scan: pi::scan,
        },
        ImportSource {
            id: "omp",
            name: "omp",
            detect: detect_omp,
            scan: omp::scan,
        },
        ImportSource {
            id: "opencode",
            name: "opencode",
            detect: detect_opencode,
            scan: opencode::scan,
        },
        ImportSource {
            id: "codex",
            name: "Codex",
            detect: detect_codex,
            scan: codex::scan,
        },
        ImportSource {
            id: "claude",
            name: "Claude Code",
            detect: detect_claude,
            scan: claude::scan,
        },
        ImportSource {
            id: "gemini",
            name: "Gemini CLI",
            detect: detect_gemini,
            scan: gemini::scan,
        },
        ImportSource {
            id: "killi",
            name: "killi",
            detect: detect_killi,
            scan: killi::scan,
        },
        ImportSource {
            id: "env",
            name: "Environment variables",
            detect: detect_env,
            scan: env::scan,
        },
    ]
}

pub fn find_source(id: &str) -> Option<ImportSource> {
    sources().into_iter().find(|source| source.id == id)
}

/// Runs one source and stamps it with the registry's id and display name, so
/// importers never have to repeat them.
fn run_source(source: &ImportSource) -> ImportCandidate {
    let mut candidate = (source.scan)();
    candidate.source_id = source.id.to_string();
    candidate.source_name = source.name.to_string();
    candidate
}

/// Scans the named sources. Unknown ids are ignored so a stale saved selection
/// cannot break the wizard.
pub fn scan(ids: &[String]) -> Vec<ImportCandidate> {
    // A GUI launch sources no profile, so the shell's variables are asked for
    // before anything reads them.
    env::load_login_shell_env();
    ids.iter()
        .filter_map(|id| find_source(id))
        .map(|source| run_source(&source))
        .collect()
}

/// Scans every source that looks set up on this machine.
pub fn scan_detected() -> Vec<ImportCandidate> {
    env::load_login_shell_env();
    sources()
        .into_iter()
        .filter(|source| (source.detect)().is_some())
        .map(|source| run_source(&source))
        .collect()
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

fn present(path: PathBuf) -> Option<PathBuf> {
    path.exists().then_some(path)
}

/// An environment variable naming a directory, when it is set and not blank.
pub(super) fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn home_dir() -> Option<PathBuf> {
    crate::config_paths::home_dir()
}

fn detect_kimi() -> Option<PathBuf> {
    present(kimi::config_path(&home_dir()?))
}

fn detect_pi() -> Option<PathBuf> {
    present(pi::config_path(&home_dir()?))
}

fn detect_omp() -> Option<PathBuf> {
    let home = home_dir()?;
    ["models.yml", "models.yaml", "models.json"]
        .into_iter()
        .map(|name| home.join(".omp/agent").join(name))
        .find(|path| path.exists())
}

fn detect_opencode() -> Option<PathBuf> {
    crate::config_paths::opencode_config_path().filter(|path| path.exists())
}

fn detect_codex() -> Option<PathBuf> {
    present(home_dir()?.join(".codex/config.toml"))
}

fn detect_claude() -> Option<PathBuf> {
    present(home_dir()?.join(".claude/settings.json"))
}

fn detect_gemini() -> Option<PathBuf> {
    present(home_dir()?.join(".gemini/settings.json"))
}

fn detect_killi() -> Option<PathBuf> {
    present(killi_home()?.join("config.toml"))
}

/// `~/.killi`, overridable for tests through `KILLI_HOME`.
pub fn killi_home() -> Option<PathBuf> {
    std::env::var_os("KILLI_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".killi")))
}

fn detect_env() -> Option<PathBuf> {
    ENV_KEYS
        .iter()
        .any(|(key, _, _)| live_env(key).is_some())
        .then(|| PathBuf::from("<environment>"))
}

/// Environment variables that name a well-known provider endpoint.
pub const ENV_KEYS: &[(&str, Protocol, &str)] = &[
    (
        "ANTHROPIC_API_KEY",
        Protocol::Anthropic,
        "https://api.anthropic.com",
    ),
    (
        "OPENAI_API_KEY",
        Protocol::Responses,
        "https://api.openai.com/v1",
    ),
    (
        "GEMINI_API_KEY",
        Protocol::OpenAiChat,
        "https://generativelanguage.googleapis.com/v1beta/openai",
    ),
    (
        "GOOGLE_API_KEY",
        Protocol::OpenAiChat,
        "https://generativelanguage.googleapis.com/v1beta/openai",
    ),
    ("XAI_API_KEY", Protocol::OpenAiChat, "https://api.x.ai/v1"),
    (
        "MOONSHOT_API_KEY",
        Protocol::OpenAiChat,
        "https://api.moonshot.cn/v1",
    ),
    (
        "DEEPSEEK_API_KEY",
        Protocol::OpenAiChat,
        "https://api.deepseek.com",
    ),
    (
        "OPENROUTER_API_KEY",
        Protocol::OpenAiChat,
        "https://openrouter.ai/api/v1",
    ),
    (
        "ZAI_API_KEY",
        Protocol::OpenAiChat,
        "https://api.z.ai/api/paas/v4",
    ),
    (
        "GROQ_API_KEY",
        Protocol::OpenAiChat,
        "https://api.groq.com/openai/v1",
    ),
    (
        "MISTRAL_API_KEY",
        Protocol::OpenAiChat,
        "https://api.mistral.ai/v1",
    ),
    (
        "COMMANDCODE_API_KEY",
        Protocol::CommandCode,
        "https://api.commandcode.ai",
    ),
];

/// Maps a config's provider `type` onto a wire protocol.
pub fn protocol_from_type(kind: &str, base_url: &str) -> Protocol {
    let host = host_of(base_url);
    match kind.trim().to_ascii_lowercase().as_str() {
        "anthropic" | "claude" => Protocol::Anthropic,
        "responses" | "openai-responses" | "openai_responses" => Protocol::Responses,
        "commandcode" | "command-code" => Protocol::CommandCode,
        "openai" | "chat" | "openai-compatible" | "openai_compatible" => {
            // opencode's `@ai-sdk/openai` points at api.openai.com, the same
            // endpoint the env source maps to Responses: one endpoint must not
            // become two providers because two agents spell it differently.
            if host.contains("openai.com") {
                Protocol::Responses
            } else {
                Protocol::OpenAiChat
            }
        }
        _ => {
            if host.contains("anthropic.com") {
                Protocol::Anthropic
            } else if host.contains("commandcode.ai") {
                Protocol::CommandCode
            } else if host.contains("openai.com") {
                Protocol::Responses
            } else {
                Protocol::OpenAiChat
            }
        }
    }
}

/// The host of a URL, lowercased, without scheme, port or path.
pub fn host_of(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    let without_scheme = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(&trimmed);
    let without_path = without_scheme.split('/').next().unwrap_or(without_scheme);
    without_path
        .split(':')
        .next()
        .unwrap_or(without_path)
        .to_string()
}

/// A set, non-blank process variable.
pub fn live_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

pub fn read_json(path: &Path) -> Result<Option<serde_json::Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {}", path.display(), error)),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| format!("invalid JSON in {}: {}", path.display(), error))
}

pub fn read_toml(path: &Path) -> Result<Option<toml::Table>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {}", path.display(), error)),
    };
    toml::from_str(&text)
        .map(Some)
        .map_err(|error| format!("invalid TOML in {}: {}", path.display(), error))
}

/// A JSON object's string field, when it is set and not blank.
pub fn json_str(value: &serde_json::Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}

pub fn json_u64(value: &serde_json::Value, field: &str) -> Option<u64> {
    value.get(field).and_then(serde_json::Value::as_u64)
}

/// A TOML table's string field, when it is set and not blank.
pub fn toml_str(table: &toml::Table, field: &str) -> Option<String> {
    table
        .get(field)
        .and_then(toml::Value::as_str)
        .map(str::to_string)
        .filter(|text| !text.is_empty())
}

pub fn toml_u64(table: &toml::Table, field: &str) -> Option<u64> {
    table
        .get(field)
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok())
}

/// A string array from TOML.
pub fn toml_strings(table: &toml::Table, field: &str) -> Vec<String> {
    table
        .get(field)
        .and_then(toml::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(toml::Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Reads `KEY=value` lines, ignoring comments and `export` prefixes.
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            let value = unquote_dotenv_value(value.trim());
            if key.is_empty() || value.is_empty() {
                return None;
            }
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

/// One matched pair of quotes comes off a value, then an unquoted ` # comment`
/// is cut off it. Stripping every quote character would leave a corrupt key
/// (`KEY="a" # c` -> `a" # c`), and a comment read as part of the value would
/// be sent as one.
fn unquote_dotenv_value(value: &str) -> String {
    let without_comment = match value.find(" #") {
        Some(index) => value[..index].trim_end(),
        None => value,
    };
    let quote = without_comment.chars().next();
    let quoted = matches!(quote, Some('"') | Some('\''))
        && without_comment.chars().count() >= 2
        && without_comment.ends_with(quote.unwrap_or_default());
    match quote.filter(|_| quoted) {
        Some(quote) => {
            without_comment[quote.len_utf8()..without_comment.len() - quote.len_utf8()].to_string()
        }
        None => without_comment.to_string(),
    }
}

/// An api-key account, or an env reference when the config names a variable.
///
/// killi's precedence: a live process variable wins over the value in the file,
/// and a reference is stored instead of a copy so rotation keeps working. We
/// copy, so an env-backed key is recorded as [`AccountKind::Env`] when the
/// variable is live and as a literal key otherwise.
pub fn secret_from_key_fields(
    inline_key: Option<String>,
    env_key: Option<String>,
) -> Option<Secret> {
    if let Some(name) = env_key.as_ref().filter(|name| live_env(name).is_some()) {
        return Some(Secret::Env(name.clone()));
    }
    inline_key.map(Secret::ApiKey)
}

/// Builds an api-key account draft, or `None` when there is no credential.
pub fn api_key_account(label: String, source: &str, secret: Secret) -> Option<AccountDraft> {
    if matches!(secret, Secret::None) {
        return None;
    }
    Some(AccountDraft {
        kind: match &secret {
            Secret::Env(_) => AccountKind::Env,
            _ => AccountKind::ApiKey,
        },
        label,
        source: source.to_string(),
        secret,
        headers: Vec::new(),
        tool_names: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ids_are_unique_and_cover_every_importer() {
        let ids: Vec<&str> = sources().into_iter().map(|source| source.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate source id");
        assert!(ids.contains(&"kimi"));
        assert!(ids.contains(&"killi"));
        assert!(ids.contains(&"env"));
    }

    #[test]
    fn a_scan_is_stamped_with_the_source_registrys_names() {
        let source = find_source("kimi").unwrap();
        let candidate = run_source(&source);
        assert_eq!(candidate.source_id, "kimi");
        assert_eq!(candidate.source_name, "Kimi Code");
    }

    #[test]
    fn unknown_source_ids_are_ignored() {
        // The scan entry point fills the process env from the login shell, so
        // the env lock keeps that from landing inside another test's assertions.
        let _guard = crate::test_env::lock_env();
        env::load_login_shell_env();
        assert!(scan(&["nope".to_string()]).is_empty());
        assert!(find_source("kimi").is_some());
        assert!(find_source("nope").is_none());
    }

    #[test]
    fn protocol_from_type_prefers_the_declared_kind() {
        assert_eq!(protocol_from_type("anthropic", ""), Protocol::Anthropic);
        assert_eq!(protocol_from_type("responses", ""), Protocol::Responses);
        assert_eq!(
            protocol_from_type("command-code", ""),
            Protocol::CommandCode
        );
        assert_eq!(protocol_from_type("openai", ""), Protocol::OpenAiChat);
    }

    #[test]
    fn protocol_from_type_knows_every_spelling_of_responses() {
        assert_eq!(
            protocol_from_type("openai-responses", ""),
            Protocol::Responses
        );
        assert_eq!(
            protocol_from_type("openai_responses", ""),
            Protocol::Responses
        );
    }

    #[test]
    fn an_openai_kind_at_openai_com_is_the_responses_api() {
        // opencode's `@ai-sdk/openai` and the env source's `OPENAI_API_KEY`
        // name one endpoint; both must resolve to the same protocol for the
        // (protocol, base_url) dedupe to merge them.
        assert_eq!(
            protocol_from_type("openai", "https://api.openai.com/v1"),
            Protocol::Responses
        );
        assert_eq!(
            protocol_from_type("openai-compatible", "https://api.openai.com/v1"),
            Protocol::Responses
        );
        assert_eq!(
            protocol_from_type("openai", "https://api.moonshot.cn/v1"),
            Protocol::OpenAiChat
        );
    }

    #[test]
    fn protocol_falls_back_to_the_host() {
        assert_eq!(
            protocol_from_type("", "https://api.anthropic.com"),
            Protocol::Anthropic
        );
        assert_eq!(
            protocol_from_type("custom", "https://api.openai.com/v1"),
            Protocol::Responses
        );
        assert_eq!(
            protocol_from_type("custom", "https://llm.internal.example/v1"),
            Protocol::OpenAiChat
        );
    }

    #[test]
    fn host_of_strips_scheme_port_and_path() {
        assert_eq!(
            host_of("https://API.anthropic.com:443/v1"),
            "api.anthropic.com"
        );
        assert_eq!(host_of("llm.internal.example"), "llm.internal.example");
    }

    #[test]
    fn dotenv_parsing_skips_comments_and_quotes() {
        let parsed = parse_dotenv(
            "# comment\n\nexport ANTHROPIC_API_KEY=\"sk-ant-1\"\nGEMINI_API_KEY='abc'\nBROKEN\n",
        );
        assert_eq!(
            parsed,
            vec![
                ("ANTHROPIC_API_KEY".to_string(), "sk-ant-1".to_string()),
                ("GEMINI_API_KEY".to_string(), "abc".to_string()),
            ]
        );
    }

    #[test]
    fn dotenv_parsing_cuts_inline_comments_and_keeps_one_quote_pair() {
        let parsed = parse_dotenv(
            "GEMINI_API_KEY=sk-file # the gateway key\nGOOGLE_API_KEY=\"sk-quoted\" # and a note\n\
             MOONSHOT_API_KEY='sk-single' # another\nUNQUOTED_HASH=abc#not-a-comment\n\
             EMPTY_QUOTES=\"\"\n",
        );
        assert_eq!(
            parsed,
            vec![
                ("GEMINI_API_KEY".to_string(), "sk-file".to_string()),
                ("GOOGLE_API_KEY".to_string(), "sk-quoted".to_string()),
                ("MOONSHOT_API_KEY".to_string(), "sk-single".to_string()),
                ("UNQUOTED_HASH".to_string(), "abc#not-a-comment".to_string()),
            ],
            "one matched quote pair comes off, and a comment is not part of the key"
        );
    }

    #[test]
    fn live_env_variable_beats_an_inline_key() {
        let _guard = crate::test_env::lock_env();
        std::env::set_var("THE_PAIR_TEST_KEY", "from-env");
        assert_eq!(
            secret_from_key_fields(Some("inline".into()), Some("THE_PAIR_TEST_KEY".into())),
            Some(Secret::Env("THE_PAIR_TEST_KEY".into()))
        );
        std::env::remove_var("THE_PAIR_TEST_KEY");
        assert_eq!(
            secret_from_key_fields(Some("inline".into()), Some("THE_PAIR_TEST_KEY".into())),
            Some(Secret::ApiKey("inline".into()))
        );
        assert_eq!(secret_from_key_fields(None, None), None);
    }

    #[test]
    fn api_key_account_drops_empty_secrets() {
        assert!(api_key_account("x".into(), "kimi", Secret::None).is_none());
        let account = api_key_account("x".into(), "kimi", Secret::ApiKey("k".into())).unwrap();
        assert_eq!(account.kind, AccountKind::ApiKey);
        assert_eq!(account.source, "kimi");
    }
}
