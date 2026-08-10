pub mod codex;
pub mod claude;
pub mod gemini;
pub mod kimi;
pub mod kiro;
pub mod opencode;
pub mod pi;

use crate::provider_adapter::{
    CwdStrategy, InputTransport, OutputTransport, PermissionStrategy, ProviderRuntimeSpec,
    ProviderTurnCommand, ProviderTurnRequest, SessionStrategy,
};
use crate::provider_registry::{DetectedProviderProfile, ProviderKind};
use serde_json::Value;
use std::sync::Arc;

use crate::types::TurnTokenUsage;

/// The central abstraction for a CLI provider. Each supported CLI tool (opencode,
/// codex, claude, gemini, ...) implements this trait in its own module file.
///
/// Adding a new provider:
/// 1. Create `providers/new_provider.rs` implementing `Provider`
/// 2. Add `pub mod new_provider;` to this file
/// 3. Add `Arc::new(new_provider::NewProvider)` to `all_providers()`
/// 4. Add the variant to `ProviderKind` in `provider_registry.rs`, plus a
///    `detect_*` fn, and extend `infer_provider_kind` in `provider_adapter.rs`
/// 5. Add the variant to the TypeScript `ProviderKind` union in `types.ts`
/// 6. Extend the frontend string lists that enumerate providers:
///    `providerResolution.ts` (inference), `modelResolution.ts`
///    (`stripProviderPrefix`), `modelCatalogGrouping.ts` (`PROVIDER_PRIORITY`),
///    and `providerSetup.ts` (login command + install URL maps)
pub trait Provider: Send + Sync {
    // ── Identity ──────────────────────────────────────────────────────────

    /// The enum variant identifying this provider.
    fn kind(&self) -> ProviderKind;

    /// The CLI binary name (e.g. "claude", "codex").
    fn executable(&self) -> &str;

    // ── CLI Command Construction ──────────────────────────────────────────

    /// The runtime transport/strategy spec for this provider. Default
    /// implementation builds from the trait methods; override for complex
    /// providers (e.g. Gemini dual-backend).
    fn runtime_spec(&self) -> ProviderRuntimeSpec {
        ProviderRuntimeSpec {
            executable: self.executable().into(),
            input_transport: InputTransport::Stdio,
            output_transport: OutputTransport::JsonEvents,
            session_strategy: SessionStrategy::NewFirst,
            permission_strategy: PermissionStrategy::Auto,
            cwd_strategy: CwdStrategy::Worktree,
        }
    }

    /// Build the CLI command for a single turn.
    fn build_turn_command(&self, request: &ProviderTurnRequest) -> ProviderTurnCommand;

    // ── Event Parsing ─────────────────────────────────────────────────────

    /// Extract token usage from a JSON event. Return `None` if the event
    /// doesn't contain usage data.
    fn extract_token_usage(&self, event: &Value) -> Option<TurnTokenUsage>;

    /// Provider-specific JSON candidate extraction. Return `Some(vec)` to
    /// override the generic text extraction, or `None` to fall through to
    /// the default.
    fn collect_json_candidates(&self, _event: &Value) -> Option<Vec<String>> {
        None
    }

    /// Extract a provider-specific error from a turn event (e.g. Claude Code
    /// `result` with `is_error: true`). Return `None` if no error.
    fn extract_error_detail(&self, _event: &Value) -> Option<String> {
        None
    }

    /// Whether to suppress stderr forwarding to the broker log.
    fn suppress_stderr(&self) -> bool {
        false
    }

    /// Whether to suppress plain-output line logging.
    fn suppress_plain_output_logging(&self) -> bool {
        false
    }

    // ── Detection ─────────────────────────────────────────────────────────

    /// Detect whether this provider is installed, authenticated, and which
    /// models are available. Runs in its own thread from `detect_all()`.
    fn detect(&self) -> DetectedProviderProfile;

    // ── Model Metadata ────────────────────────────────────────────────────

    /// The brand slug used for canonical key computation (e.g. "openai",
    /// "anthropic").
    fn brand(&self) -> &str;

    /// Human-readable provider label for the model picker (e.g. "Claude Code").
    fn provider_label(&self) -> &str;

    /// Billing kind string (e.g. "byok", "plan").
    fn billing_kind(&self) -> &str;

    /// Billing display label (e.g. "Pay as you go").
    fn billing_label(&self) -> &str;

    /// Access label (e.g. "ChatGPT plan", "Claude Code login").
    fn access_label(&self, source_provider_label: &str) -> String;

    /// Reasoning effort levels offered for a model, or `None` to hide the
    /// control.
    fn reasoning_effort_levels(&self, _model_id: &str) -> Option<Vec<String>> {
        None
    }

    /// Whether unavailable models should be filtered out of the catalog
    /// (true for OpenCode's long tail).
    fn should_filter_unavailable_models(&self) -> bool {
        false
    }

    /// Post-process a model display name (e.g. Claude's beautifier). Default
    /// returns the input unchanged.
    fn normalize_model_display_name(&self, display_name: &str) -> String {
        display_name.to_string()
    }

    // ── Provider Login / Install Guidance ─────────────────────────────────

    /// The CLI login command for this provider (e.g. `"claude login"`).
    /// Returns `None` if no simple login command exists.
    fn login_command(&self) -> Option<String> {
        None
    }

    /// A URL where the user can install this CLI tool.
    /// Returns `None` if not applicable.
    fn install_url(&self) -> Option<String> {
        None
    }
}

// ── Registry ──────────────────────────────────────────────────────────────

/// Build the full set of supported providers. Add new providers here.
pub fn all_providers() -> Vec<Arc<dyn Provider>> {
    vec![
        Arc::new(opencode::OpenCodeProvider),
        Arc::new(codex::CodexProvider),
        Arc::new(claude::ClaudeProvider),
        Arc::new(gemini::GeminiProvider),
        Arc::new(kimi::KimiProvider),
        Arc::new(pi::PiProvider),
        Arc::new(kiro::KiroProvider),
    ]
}

/// Look up a single provider by kind. Returns `None` if the kind is unknown.
pub fn provider_for_kind(kind: ProviderKind) -> Option<Arc<dyn Provider>> {
    all_providers().into_iter().find(|p| p.kind() == kind)
}

/// Detect all providers in parallel (one thread per provider).
/// Enriches each profile with `login_command` and `install_url` from the
/// Provider trait so the frontend can show actionable Sign In / Install buttons.
pub fn detect_all() -> Vec<DetectedProviderProfile> {
    let providers = all_providers();
    let handles: Vec<_> = providers
        .into_iter()
        .map(|p| {
            let login_command = p.login_command();
            let install_url = p.install_url();
            std::thread::spawn(move || {
                let mut profile = p.detect();
                profile.login_command = login_command;
                profile.install_url = install_url;
                profile
            })
        })
        .collect();
    handles
        .into_iter()
        .map(|h| h.join().expect("provider detection should not panic"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_provider_returns_login_command_and_install_url() {
        let provider = claude::ClaudeProvider;
        assert_eq!(provider.login_command().as_deref(), Some("claude login"));
        assert!(provider.install_url().is_some());
        assert!(provider.install_url().unwrap().contains("claude"));
    }

    #[test]
    fn codex_provider_returns_login_command_and_install_url() {
        let provider = codex::CodexProvider;
        assert_eq!(provider.login_command().as_deref(), Some("codex auth"));
        assert!(provider.install_url().is_some());
    }

    #[test]
    fn opencode_provider_returns_login_command_and_install_url() {
        let provider = opencode::OpenCodeProvider;
        assert_eq!(
            provider.login_command().as_deref(),
            Some("opencode auth login")
        );
        assert!(provider.install_url().is_some());
    }

    #[test]
    fn gemini_provider_returns_login_command() {
        let provider = gemini::GeminiProvider;
        // Gemini login command depends on agy vs legacy, but should always be Some
        let login = provider.login_command();
        assert!(login.is_some(), "Gemini should always have a login command");
        let cmd = login.unwrap();
        assert!(
            cmd.starts_with("agy") || cmd.starts_with("gemini"),
            "Unexpected gemini login command: {}",
            cmd
        );
    }

    #[test]
    fn kimi_provider_returns_login_command_and_install_url() {
        let provider = kimi::KimiProvider;
        assert_eq!(provider.login_command().as_deref(), Some("kimi login"));
        assert!(provider.install_url().is_some());
        assert!(provider.install_url().unwrap().contains("kimi"));
    }

    #[test]
    fn all_providers_have_login_or_install_info() {
        for provider in all_providers() {
            let kind = provider.kind();
            let login = provider.login_command();
            let install = provider.install_url();
            assert!(
                login.is_some() || install.is_some(),
                "Provider {:?} should have at least login_command or install_url",
                kind
            );
        }
    }
}
