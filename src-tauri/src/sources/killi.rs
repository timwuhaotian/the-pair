//! killi's own configuration: `~/.killi/config.toml` and `~/.killi/chatgpt.json`.
//!
//! Shape (verified against killi's `config.rs` and `config/resolve.rs`):
//!
//! ```toml
//! default_model = "anthropic/claude-sonnet-4-5"
//! [providers.anthropic]
//! type = "anthropic"
//! api_key = "sk-ant-..."
//! [providers.opencode-go]
//! type = "openai"
//! base_url = "https://opencode.ai/inference/go/openai/v1"
//! api_key_env = "OPENCODE_API_KEY"
//! session_header = "x-opencode-session"
//! [providers.opencode-go.custom_headers]
//! x-extra = "1"
//! [models."anthropic/claude-sonnet-4-5"]
//! provider = "anthropic"
//! model = "claude-sonnet-4-5"
//! max_context_size = 200000
//! max_output_size = 64000
//! capabilities = ["image_in", "thinking"]
//! support_efforts = ["low", "high"]
//! default_effort = "high"
//! [models."opencode-go/claude-haiku-5-5"]
//! provider = "opencode-go"
//! model = "claude-haiku-5-5"
//! protocol = "anthropic"
//! base_url = "https://opencode.ai/inference/go/anthropic"
//! ```
//!
//! A model key is `provider/model`: the prefix names the provider (its `provider`
//! field says the same), the rest is the model. A model's own `base_url` and
//! `protocol` win over its provider's, so one provider name can spread across
//! several endpoints and each endpoint becomes a draft carrying the provider's
//! accounts. `capabilities` has no place on a model route, so it is left out.
//!
//! `chatgpt.json` keeps the ChatGPT plan tokens `killi login chatgpt` saved, in
//! Unix seconds; they become a Responses account on `api.openai.com/v1`. The
//! `chatgpt` provider the config declares carries no key, so it is skipped there
//! and rebuilt from that file.

use super::*;
use crate::accounts::registry::ProviderDraft;
use crate::accounts::types::{HeaderPair, ModelRoute, ModelSource, OAuthToken};
use std::collections::BTreeMap;

const CONFIG_FILE: &str = "config.toml";
const CHATGPT_FILE: &str = "chatgpt.json";
/// Where a ChatGPT plan's requests go.
const CHATGPT_BASE_URL: &str = "https://api.openai.com/v1";
/// The scope that lets a signed-in token use the plan.
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";

/// One killi provider and the endpoints its models resolve to.
struct ProviderGroup {
    name: String,
    /// The provider's own `base_url`, when it names one.
    base_url: Option<String>,
    protocol: Protocol,
    /// One draft per endpoint, the provider's own first.
    drafts: Vec<ProviderDraft>,
}

impl ProviderGroup {
    fn new(
        name: &str,
        display_name: &str,
        protocol: Protocol,
        base_url: Option<String>,
        accounts: Vec<AccountDraft>,
    ) -> Self {
        let resolved = base_url
            .clone()
            .unwrap_or_else(|| protocol.default_base_url().to_string());
        ProviderGroup {
            name: name.to_string(),
            base_url,
            protocol,
            drafts: vec![ProviderDraft {
                protocol,
                base_url: resolved,
                vendor: None,
                display_name: Some(display_name.to_string()),
                accounts,
                models: Vec::new(),
            }],
        }
    }

    /// The draft at this endpoint, opened with the provider's own accounts when a
    /// model names a URL or protocol of its own.
    fn draft_at(&mut self, protocol: Protocol, base_url: &str) -> &mut ProviderDraft {
        if let Some(position) = self
            .drafts
            .iter()
            .position(|draft| draft.protocol == protocol && draft.base_url == base_url)
        {
            return &mut self.drafts[position];
        }
        let position = self.drafts.len();
        let mut extra = self.drafts[0].clone();
        extra.protocol = protocol;
        extra.base_url = base_url.to_string();
        extra.models.clear();
        self.drafts.push(extra);
        &mut self.drafts[position]
    }
}

pub fn scan() -> ImportCandidate {
    match killi_home() {
        Some(home) => scan_from(&home),
        None => ImportCandidate {
            source_id: "killi".into(),
            source_name: "killi".into(),
            config_path: None,
            drafts: Vec::new(),
            notes: vec!["No home directory, so no killi config to read.".into()],
            error: None,
        },
    }
}

pub fn scan_from(root: &Path) -> ImportCandidate {
    let path = root.join(CONFIG_FILE);
    let mut candidate = ImportCandidate {
        source_id: "killi".into(),
        source_name: "killi".into(),
        config_path: Some(path.clone()),
        drafts: Vec::new(),
        notes: Vec::new(),
        error: None,
    };

    let config = match read_toml(&path) {
        Ok(config) => config,
        Err(error) => {
            candidate.error = Some(error);
            return candidate;
        }
    };

    let mut groups: Vec<ProviderGroup> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    if let Some(config) = &config {
        if let Some(toml::Value::Table(providers)) = config.get("providers") {
            for (name, provider) in providers {
                let Some(table) = provider.as_table() else {
                    continue;
                };
                match read_provider(name, table) {
                    Some(group) => groups.push(group),
                    None => skipped.push(name.clone()),
                }
            }
        }
    }

    // The ChatGPT plan's tokens are kept outside the config, so its group is built
    // before the models are attached and `chatgpt`-provider models land on it.
    match read_json(&root.join(CHATGPT_FILE)) {
        Ok(Some(value)) => match chatgpt_account(&value) {
            Ok(account) => groups.push(ProviderGroup::new(
                "chatgpt",
                "ChatGPT (killi)",
                Protocol::Responses,
                Some(CHATGPT_BASE_URL.to_string()),
                vec![account],
            )),
            Err(note) => candidate.notes.push(note),
        },
        Ok(None) => {}
        Err(error) => candidate.error = Some(error),
    }

    let mut imported: Vec<String> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    if let Some(config) = &config {
        if let Some(toml::Value::Table(models)) = config.get("models") {
            for (alias, model) in models {
                let Some(table) = model.as_table() else {
                    continue;
                };
                let provider = toml_str(table, "provider").or_else(|| split_alias(alias).0);
                let Some(group) = provider
                    .as_deref()
                    .and_then(|provider| groups.iter_mut().find(|group| group.name == provider))
                else {
                    dropped.push(alias.clone());
                    continue;
                };
                let protocol = toml_str(table, "protocol")
                    .map(|kind| protocol_for(&kind, ""))
                    .unwrap_or(group.protocol);
                let base_url = toml_str(table, "base_url")
                    .or_else(|| group.base_url.clone())
                    .unwrap_or_else(|| protocol.default_base_url().to_string());
                let id = toml_str(table, "model")
                    .or_else(|| split_alias(alias).1)
                    .unwrap_or_else(|| alias.clone());
                let route = ModelRoute {
                    id: id.clone(),
                    display_name: toml_str(table, "display_name").unwrap_or(id),
                    context_window: toml_u64(table, "max_context_size"),
                    max_output: toml_u64(table, "max_output_size"),
                    efforts: toml_strings(table, "support_efforts"),
                    default_effort: toml_str(table, "default_effort"),
                    price: None,
                    source: ModelSource::Imported,
                };
                let draft = group.draft_at(protocol, &base_url);
                imported.push(alias.clone());
                if draft.models.iter().any(|model| model.id == route.id) {
                    continue;
                }
                draft.models.push(route);
            }
        }
    }

    if !skipped.is_empty() {
        candidate.notes.push(format!(
            "Skipped providers with no API key in the config: {}",
            skipped.join(", ")
        ));
    }
    if !dropped.is_empty() {
        candidate.notes.push(format!(
            "Left out models whose provider wasn't imported: {}",
            dropped.join(", ")
        ));
    }
    let default_model = config
        .as_ref()
        .and_then(|config| toml_str(config, "default_model"));
    if let Some(default_model) = default_model {
        if !imported.iter().any(|alias| *alias == default_model) {
            candidate.notes.push(format!(
                "default_model `{}` was not imported.",
                default_model
            ));
        }
    }

    candidate.drafts = groups.into_iter().flat_map(|group| group.drafts).collect();
    candidate
}

/// One `[providers.<name>]` table, or `None` when killi signs in to it itself and
/// the config holds no key to import.
fn read_provider(name: &str, table: &toml::Table) -> Option<ProviderGroup> {
    let kind = toml_str(table, "type").unwrap_or_default();
    let inline_key = toml_str(table, "api_key");
    let env_key = toml_str(table, "api_key_env");
    if kind == "chatgpt" || (inline_key.is_none() && env_key.is_none()) {
        return None;
    }

    let base_url = toml_str(table, "base_url");
    let protocol = protocol_for(&kind, base_url.as_deref().unwrap_or(""));
    let mut headers = custom_headers(table);
    // The session header carries the conversation's id, which is only known per
    // request, so only its name is stored — as the opencode-go flow does.
    if let Some(header) = toml_str(table, "session_header") {
        headers.push(HeaderPair {
            name: header,
            value: String::new(),
        });
    }
    let tool_names = tool_names(table);
    let accounts = secret_from_key_fields(inline_key, env_key)
        .and_then(|secret| api_key_account(name.to_string(), "killi", secret))
        .map(|mut account| {
            account.headers = headers.clone();
            account.tool_names = tool_names.clone();
            account
        })
        .into_iter()
        .collect();

    Some(ProviderGroup::new(
        name,
        &format!("{} (killi)", name),
        protocol,
        base_url,
        accounts,
    ))
}

/// killi spells a ChatGPT plan `chatgpt`; its other names, including its own
/// `openai_responses`, match [`protocol_from_type`].
fn protocol_for(kind: &str, base_url: &str) -> Protocol {
    match kind.trim().to_ascii_lowercase().as_str() {
        "chatgpt" => Protocol::Responses,
        other => protocol_from_type(other, base_url),
    }
}

/// `[providers.<name>.custom_headers]`, as the file lists them.
fn custom_headers(table: &toml::Table) -> Vec<HeaderPair> {
    table
        .get("custom_headers")
        .and_then(toml::Value::as_table)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| {
                    Some(HeaderPair {
                        name: name.clone(),
                        value: value.as_str()?.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `tool_names`: killi's name mapped to the name the API is sent and answers with.
fn tool_names(table: &toml::Table) -> Option<BTreeMap<String, String>> {
    let names: BTreeMap<String, String> = table
        .get("tool_names")
        .and_then(toml::Value::as_table)?
        .iter()
        .filter_map(|(from, to)| Some((from.clone(), to.as_str()?.to_string())))
        .collect();
    (!names.is_empty()).then_some(names)
}

/// `"anthropic/claude-sonnet-4-5"` → the provider and the model the key names.
fn split_alias(alias: &str) -> (Option<String>, Option<String>) {
    match alias.split_once('/') {
        Some((provider, model)) if !provider.is_empty() && !model.is_empty() => {
            (Some(provider.to_string()), Some(model.to_string()))
        }
        _ => (None, None),
    }
}

/// killi's `~/.killi/chatgpt.json` as a ChatGPT plan account, in Unix seconds.
/// `Err` says why the tokens can't be used.
fn chatgpt_account(value: &serde_json::Value) -> Result<AccountDraft, String> {
    let access_token = json_str(value, "access_token")
        .ok_or_else(|| "killi's chatgpt.json has no access token.".to_string())?;
    let scopes: Vec<&str> = value
        .get("scopes")
        .and_then(serde_json::Value::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    // Without the plan's scope the token names the account and no more.
    if !scopes.iter().any(|scope| *scope == PLAN_SCOPE) {
        return Err(format!(
            "killi's ChatGPT sign-in has no `{}` scope, so it can't use the plan.",
            PLAN_SCOPE
        ));
    }
    let token = OAuthToken {
        access_token,
        refresh_token: json_str(value, "refresh_token"),
        expires_at: json_u64(value, "saved_at")
            .unwrap_or_default()
            .saturating_add(json_u64(value, "expires_in").unwrap_or_default())
            .saturating_mul(1000),
        token_type: "Bearer".to_string(),
        scope: None,
        account_label: json_str(value, "email"),
    };
    // The same margin killi itself refreshes at: a token with less than that
    // left would be Ready on import and dead on the first request.
    if token.expires_within(crate::util::now_millis(), OAuthToken::REFRESH_MARGIN_MS) {
        let note = "killi's ChatGPT sign-in has expired or is about to; run `killi login chatgpt` \
                    to renew it.";
        return Err(note.to_string());
    }
    Ok(AccountDraft {
        kind: AccountKind::ChatGptOAuth,
        label: "ChatGPT (killi)".to_string(),
        source: "killi".to_string(),
        secret: Secret::OAuth(Box::new(token)),
        headers: Vec::new(),
        tool_names: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::registry::upsert_drafts;
    use crate::accounts::types::AccountStatus;
    use std::fs;

    fn write(root: &Path, name: &str, body: &str) {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    fn root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("the-pair-killi-{}", uuid::Uuid::new_v4()))
    }

    /// The status the registry gives a draft's first account.
    fn status_of(draft: &ProviderDraft) -> AccountStatus {
        let mut providers = Vec::new();
        upsert_drafts(&mut providers, std::slice::from_ref(draft));
        providers[0].accounts[0].status
    }

    #[test]
    fn imports_providers_their_accounts_and_their_models() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
default_model = "anthropic/claude-sonnet-4-5"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
[providers.opencode-go]
type = "openai"
base_url = "https://opencode.ai/inference/go/openai/v1"
api_key = "sk-go-1"
session_header = "x-opencode-session"
tool_names = { bash = "run_command" }
[providers.opencode-go.custom_headers]
x-extra = "1"
[models."anthropic/claude-sonnet-4-5"]
provider = "anthropic"
model = "claude-sonnet-4-5"
display_name = "Claude Sonnet 4.5"
max_context_size = 200000
max_output_size = 64000
capabilities = ["image_in", "thinking"]
support_efforts = ["low", "high"]
default_effort = "high"
[models."opencode-go/kimi-k2"]
provider = "opencode-go"
model = "kimi-k2"
"#,
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        assert!(candidate.notes.is_empty(), "{:?}", candidate.notes);
        assert_eq!(candidate.drafts.len(), 2);

        let anthropic = candidate
            .drafts
            .iter()
            .find(|draft| draft.protocol == Protocol::Anthropic)
            .expect("anthropic draft");
        assert_eq!(anthropic.base_url, "https://api.anthropic.com");
        assert_eq!(anthropic.display_name.as_deref(), Some("anthropic (killi)"));
        let account = &anthropic.accounts[0];
        assert_eq!(account.kind, AccountKind::ApiKey);
        assert_eq!(account.source, "killi");
        assert_eq!(account.secret, Secret::ApiKey("sk-ant-1".into()));
        assert_eq!(status_of(anthropic), AccountStatus::Ready);
        assert!(account.headers.is_empty());
        assert!(account.tool_names.is_none());

        let model = &anthropic.models[0];
        assert_eq!(model.id, "claude-sonnet-4-5");
        assert_eq!(model.display_name, "Claude Sonnet 4.5");
        assert_eq!(model.context_window, Some(200_000));
        assert_eq!(model.max_output, Some(64_000));
        assert_eq!(model.efforts, vec!["low", "high"]);
        assert_eq!(model.default_effort.as_deref(), Some("high"));
        assert_eq!(model.source, ModelSource::Imported);

        let go = candidate
            .drafts
            .iter()
            .find(|draft| draft.base_url.contains("opencode.ai"))
            .expect("opencode-go draft");
        assert_eq!(go.protocol, Protocol::OpenAiChat);
        assert_eq!(go.models.len(), 1);
        assert_eq!(
            go.accounts[0].headers.len(),
            2,
            "custom header + session header"
        );
        assert_eq!(go.accounts[0].headers[0].name, "x-extra");
        assert_eq!(go.accounts[0].headers[1].name, "x-opencode-session");
        assert!(go.accounts[0].headers[1].value.is_empty());
        assert_eq!(
            go.accounts[0]
                .tool_names
                .as_ref()
                .unwrap()
                .get("bash")
                .map(String::as_str),
            Some("run_command")
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_models_own_url_and_protocol_open_their_own_draft() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.opencode-go]
type = "openai"
base_url = "https://opencode.ai/inference/go/openai/v1"
api_key = "sk-go-1"
[models."opencode-go/claude-haiku-5-5"]
provider = "opencode-go"
model = "claude-haiku-5-5"
protocol = "anthropic"
base_url = "https://opencode.ai/inference/go/anthropic"
[models."opencode-go/muse-spark-1.3"]
provider = "opencode-go"
model = "muse-spark-1.3"
protocol = "openai_responses"
base_url = "https://opencode.ai/inference/go/responses"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 3, "one draft per endpoint");
        let own = candidate
            .drafts
            .iter()
            .find(|draft| draft.base_url.ends_with("/openai/v1"))
            .expect("the provider's own endpoint");
        assert!(own.models.is_empty());
        assert_eq!(own.accounts.len(), 1);

        let anthropic = candidate
            .drafts
            .iter()
            .find(|draft| draft.protocol == Protocol::Anthropic)
            .expect("the model's own endpoint");
        assert_eq!(
            anthropic.base_url,
            "https://opencode.ai/inference/go/anthropic"
        );
        assert_eq!(
            anthropic.display_name.as_deref(),
            Some("opencode-go (killi)")
        );
        assert_eq!(
            anthropic.accounts.len(),
            1,
            "the provider's key follows its models"
        );
        assert_eq!(anthropic.models.len(), 1);
        assert_eq!(anthropic.models[0].id, "claude-haiku-5-5");

        let responses = candidate
            .drafts
            .iter()
            .find(|draft| draft.protocol == Protocol::Responses)
            .expect("openai_responses is the Responses API");
        assert_eq!(responses.models[0].id, "muse-spark-1.3");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn skips_providers_killi_signs_in_to_and_says_so() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.chatgpt]
type = "chatgpt"
[providers.keyless]
type = "openai"
base_url = "https://llm.internal.example/v1"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].accounts.len(), 1);
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("chatgpt"),
            "{}",
            candidate.notes[0]
        );
        assert!(
            candidate.notes[0].contains("keyless"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_config_is_not_an_error() {
        let root = root();
        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert!(candidate.notes.is_empty());
        assert_eq!(candidate.config_path, Some(root.join(CONFIG_FILE)));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_config_reports_an_error() {
        let root = root();
        write(&root, CONFIG_FILE, "this is not = = toml");
        let candidate = scan_from(&root);
        assert!(candidate.error.is_some());
        assert!(candidate.drafts.is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn env_key_reference_is_kept_as_a_reference() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key = "sk-inline"
api_key_env = "THE_PAIR_TEST_KILLI_KEY"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_KILLI_KEY");
        std::env::set_var("THE_PAIR_TEST_KILLI_KEY", "sk-live");

        let candidate = scan_from(&root);
        let account = &candidate.drafts[0].accounts[0];
        assert_eq!(account.kind, AccountKind::Env);
        assert_eq!(
            account.secret,
            Secret::Env("THE_PAIR_TEST_KILLI_KEY".into())
        );
        assert_eq!(status_of(&candidate.drafts[0]), AccountStatus::Ready);

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_KILLI_KEY", value),
            None => std::env::remove_var("THE_PAIR_TEST_KILLI_KEY"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unset_env_var_leaves_the_provider_without_an_account() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.moonshot]
type = "openai"
base_url = "https://api.moonshot.cn/v1"
api_key_env = "THE_PAIR_TEST_KILLI_UNSET"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("THE_PAIR_TEST_KILLI_UNSET");
        std::env::remove_var("THE_PAIR_TEST_KILLI_UNSET");

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert!(candidate.drafts[0].accounts.is_empty());

        match saved {
            Some(value) => std::env::set_var("THE_PAIR_TEST_KILLI_UNSET", value),
            None => std::env::remove_var("THE_PAIR_TEST_KILLI_UNSET"),
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn two_aliases_for_one_model_are_imported_once() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
[models."anthropic/claude-sonnet-4-5"]
provider = "anthropic"
model = "claude-sonnet-4-5"
max_context_size = 200000
[models."other/claude-sonnet-4-5"]
provider = "anthropic"
model = "claude-sonnet-4-5"
max_output_size = 64000
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(candidate.drafts[0].models[0].context_window, Some(200_000));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn entries_that_are_not_tables_are_ignored() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers]
bare = 1
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
[models]
bare = 2
[models."nope"]
provider = "gone"
model = "whatever"
[models."anthropic/claude-sonnet-4-5"]
provider = "anthropic"
model = "claude-sonnet-4-5"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("nope"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_model_key_names_the_provider_and_the_model_when_the_fields_do_not() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
[models."anthropic/claude-sonnet-4-5"]
max_context_size = 200000
[models."bare"]
model = "bare-model"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        let models: Vec<&str> = candidate.drafts[0]
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect();
        assert_eq!(models, vec!["claude-sonnet-4-5"]);
        assert_eq!(candidate.drafts[0].models[0].context_window, Some(200_000));
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("bare"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_default_model_that_was_not_imported_is_noted() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
default_model = "chatgpt/gpt-6.1-terra"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
[models."anthropic/claude-sonnet-4-5"]
provider = "anthropic"
model = "claude-sonnet-4-5"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("chatgpt/gpt-6.1-terra")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_default_model_deduped_onto_another_alias_is_not_noted() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
default_model = "other/claude-sonnet-4-5"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
[models."anthropic/claude-sonnet-4-5"]
provider = "anthropic"
model = "claude-sonnet-4-5"
[models."other/claude-sonnet-4-5"]
provider = "anthropic"
model = "claude-sonnet-4-5"
"#,
        );

        let candidate = scan_from(&root);
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].models.len(), 1);
        assert!(
            !candidate
                .notes
                .iter()
                .any(|note| note.contains("default_model")),
            "the alias was imported, even though its route was deduped: {:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_token_inside_the_refresh_margin_is_not_imported() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            "default_model = \"chatgpt/gpt-6.1-terra\"\n",
        );
        let now = crate::util::now_millis() / 1000;
        write(
            &root,
            CHATGPT_FILE,
            &format!(
                r#"{{"access_token": "at-1", "expires_in": 60, "saved_at": {},
                    "scopes": ["{}"]}}"#,
                now - 60,
                PLAN_SCOPE
            ),
        );

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(
            candidate.notes.iter().any(|note| note.contains("expired")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn imports_the_chatgpt_plan_tokens() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.chatgpt]
type = "chatgpt"
[models."chatgpt/gpt-6.1-terra"]
provider = "chatgpt"
model = "gpt-6.1-terra"
protocol = "chatgpt"
max_context_size = 500000
"#,
        );
        let saved_at = crate::util::now_millis() / 1000 + 7200;
        write(
            &root,
            CHATGPT_FILE,
            &format!(
                r#"{{"email": "me@example.com", "access_token": "at-1", "refresh_token": "rt-1",
                    "expires_in": 3600, "saved_at": {}, "scopes": ["{}", "openid"]}}"#,
                saved_at, PLAN_SCOPE
            ),
        );

        let candidate = scan_from(&root);
        assert!(candidate.error.is_none());
        // The config's own chatgpt entry carries no key, so it is the one thing
        // worth a note here; the plan itself came from chatgpt.json.
        assert_eq!(candidate.notes.len(), 1, "{:?}", candidate.notes);
        assert!(
            candidate.notes[0].contains("chatgpt"),
            "{}",
            candidate.notes[0]
        );
        assert_eq!(candidate.drafts.len(), 1);
        let draft = &candidate.drafts[0];
        assert_eq!(draft.protocol, Protocol::Responses);
        assert_eq!(draft.base_url, CHATGPT_BASE_URL);
        assert_eq!(draft.display_name.as_deref(), Some("ChatGPT (killi)"));
        assert_eq!(
            draft.models.len(),
            1,
            "chatgpt models land on the plan's endpoint"
        );
        assert_eq!(draft.models[0].id, "gpt-6.1-terra");
        assert_eq!(draft.models[0].context_window, Some(500_000));

        let account = &draft.accounts[0];
        assert_eq!(account.kind, AccountKind::ChatGptOAuth);
        assert_eq!(account.source, "killi");
        let Secret::OAuth(token) = &account.secret else {
            panic!("the plan's tokens are an OAuth secret");
        };
        assert_eq!(token.access_token, "at-1");
        assert_eq!(token.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(token.expires_at, (saved_at + 3600) * 1000);
        assert_eq!(token.account_label.as_deref(), Some("me@example.com"));
        assert_eq!(status_of(draft), AccountStatus::Ready);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_expired_chatgpt_token_is_not_imported() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            "default_model = \"chatgpt/gpt-6.1-terra\"\n",
        );
        write(
            &root,
            CHATGPT_FILE,
            r#"{"access_token": "at-1", "expires_in": 3600, "saved_at": 1000,
                "scopes": ["chatgpt.tokens.use.direct"]}"#,
        );

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(candidate.error.is_none());
        assert!(
            candidate.notes.iter().any(|note| note.contains("expired")),
            "{:?}",
            candidate.notes
        );
        assert!(
            candidate
                .notes
                .iter()
                .any(|note| note.contains("default_model")),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_chatgpt_token_without_the_plan_scope_is_not_imported() {
        let root = root();
        write(
            &root,
            CHATGPT_FILE,
            r#"{"access_token": "at-1", "expires_in": 3600, "saved_at": 4000000000,
                "scopes": ["openid", "email"]}"#,
        );

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert!(
            candidate.notes.iter().any(|note| note.contains(PLAN_SCOPE)),
            "{:?}",
            candidate.notes
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_chatgpt_file_without_an_access_token_is_not_imported() {
        let root = root();
        write(
            &root,
            CHATGPT_FILE,
            r#"{"expires_in": 3600, "saved_at": 4000000000,
                "scopes": ["chatgpt.tokens.use.direct"]}"#,
        );

        let candidate = scan_from(&root);
        assert!(candidate.drafts.is_empty());
        assert_eq!(candidate.notes.len(), 1);
        assert!(
            candidate.notes[0].contains("access token"),
            "{}",
            candidate.notes[0]
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn malformed_chatgpt_json_reports_an_error() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
"#,
        );
        write(&root, CHATGPT_FILE, "{not json");

        let candidate = scan_from(&root);
        assert!(candidate.error.is_some());
        assert_eq!(candidate.drafts.len(), 1, "the config still imports");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_reads_the_home_killi_home_names() {
        let root = root();
        write(
            &root,
            CONFIG_FILE,
            r#"
[providers.anthropic]
type = "anthropic"
api_key = "sk-ant-1"
"#,
        );
        let _guard = crate::test_env::lock_env();
        let saved = std::env::var_os("KILLI_HOME");
        std::env::set_var("KILLI_HOME", &root);

        let candidate = scan();
        assert_eq!(candidate.source_id, "killi");
        assert_eq!(candidate.drafts.len(), 1);
        assert_eq!(candidate.drafts[0].base_url, "https://api.anthropic.com");

        match saved {
            Some(value) => std::env::set_var("KILLI_HOME", value),
            None => std::env::remove_var("KILLI_HOME"),
        }
        fs::remove_dir_all(&root).ok();
    }
}
