pub mod codex;
pub mod claude;
pub mod gemini;
pub mod opencode;

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
/// 3. Add `Box::new(new_provider::NewProvider)` to `all_providers()`
/// 4. Add the variant to `ProviderKind` in `provider_registry.rs`
/// 5. Add the variant to the TypeScript `ProviderKind` union
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
}

// ── Registry ──────────────────────────────────────────────────────────────

/// Build the full set of supported providers. Add new providers here.
pub fn all_providers() -> Vec<Arc<dyn Provider>> {
    vec![
        Arc::new(opencode::OpenCodeProvider),
        Arc::new(codex::CodexProvider),
        Arc::new(claude::ClaudeProvider),
        Arc::new(gemini::GeminiProvider),
    ]
}

/// Look up a single provider by kind. Panics if the kind is unknown.
pub fn provider_for_kind(kind: ProviderKind) -> Arc<dyn Provider> {
    all_providers()
        .into_iter()
        .find(|p| p.kind() == kind)
        .unwrap_or_else(|| panic!("no provider registered for {:?}", kind))
}

/// Detect all providers in parallel (one thread per provider).
pub fn detect_all() -> Vec<DetectedProviderProfile> {
    let providers = all_providers();
    let handles: Vec<_> = providers
        .into_iter()
        .map(|p| std::thread::spawn(move || p.detect()))
        .collect();
    handles
        .into_iter()
        .map(|h| h.join().expect("provider detection should not panic"))
        .collect()
}
