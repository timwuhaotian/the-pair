# Plan: Add Grok Build CLI provider

Feature branch: `feature/grok-build-cli` (worktree). Base: `origin/main` @ 582b471.

## Goal

Support xAI's Grok Build CLI (`grok`) as a ninth provider kind in The Pair, following the
"Adding a New Provider" checklist in `AGENTS.md`.

## CLI facts (verified 2026-09-19 against xai-org/grok-build headless-mode docs)

- Headless turn: `grok -p <prompt>`; flags: `-m/--model`, `-r/--resume <id>`,
  `--output-format streaming-json`, `--yolo`, `--tools <allowlist>`,
  `--reasoning-effort <level>`, `--cwd <path>`.
- streaming-json events: `{"type":"text","data":…}`, `{"type":"usage","usage":{…}}`,
  `{"type":"end","sessionId":…,"usage":{…}}`, `{"type":"error","message":…}`.
- Auth: `~/.grok/auth.json` (OAuth) or `XAI_API_KEY`. Login: `grok login`.
- Custom models: `[model.<alias>]` sections in `~/.grok/config.toml`.

## Tasks

### Task 1 — Rust: registry plumbing

File: `src-tauri/src/provider_registry.rs`

1. Add `Grok` variant to `ProviderKind`.
2. Add `ProviderRegistry::detect_grok()`: installed = `which_binary_exists("grok")`;
   authenticated = `~/.grok/auth.json` exists or `XAI_API_KEY` env non-empty;
   models (only when installed) = static `grok-4.6` + custom aliases parsed from
   `~/.grok/config.toml`.
3. Add `parse_grok_config_models(content)` helper (TOML-lite, modeled on
   `parse_kimi_model_aliases`): `[model.<alias>]` starts a model (`model_id` = alias);
   `name = "…"` sets display name; `model = "…"` ignored for identity.
4. Unit tests: parser (basic + display names + ignores other tables), inference is
   covered in Task 3.
   Run: `cargo test -q --manifest-path src-tauri/Cargo.toml grok` → pass. Commit.

### Task 2 — Rust: Provider trait implementation

Files: `src-tauri/src/providers/grok.rs` (new), `src-tauri/src/providers/mod.rs`

1. `grok.rs`: `GrokProvider` implementing `Provider`:
   - `build_turn_command`: strip `grok/` prefix; leading-dash prompt guard (`\n` prepend,
     same as kimi); args `["-p", prompt, "--output-format", "streaming-json", "-m", model]`;
     mentor → `--tools read_file,grep,list_dir`; both roles → `--yolo`;
     `--resume <sid>` when session; `--reasoning-effort <level>` when set.
   - `extract_token_usage`: `type=="usage"` → Live, `type=="end"` → Final;
     output = `usage.output_tokens`; input = `input_tokens + cache_read_input_tokens +
cache_creation_input_tokens` (grok reports uncached-only input).
   - `collect_json_candidates`: always `Some(vec)`; only `type=="text"` events contribute
     `data`; everything else (thought/tool_call/usage/plan/end/error) → empty.
   - `extract_error_detail`: `type=="error"` → `message`.
   - `reasoning_effort_levels`: `["low","medium","high"]`.
   - Metadata: brand `xai`, label `Grok Build`, billing `byok`/`Pay as you go`,
     access `Grok Build login`, login `grok login`,
     install `https://github.com/xai-org/grok-build`.
   - `suppress_stderr`/`suppress_plain_output_logging` = true (mirror claude: stdout is
     pure NDJSON; stderr carries updater notices).
2. `mod.rs`: `pub mod grok;`, add to `all_providers()`, add login/install test.
3. Unit tests in `grok.rs`: executor command shape, mentor has read-only allowlist and no
   write tools, resume flag, effort flag, leading-dash guard, prefix strip, text-only
   candidates, usage live/final, error detail.
   Run: `cargo test -q --manifest-path src-tauri/Cargo.toml grok` → pass. Commit.

### Task 3 — Rust: inference + session id + brand map

Files: `src-tauri/src/provider_adapter.rs`, `src-tauri/src/process_spawner.rs`,
`src-tauri/src/model_catalog.rs`

1. `infer_provider_kind`: `grok` prefix in the qualified branch; `lower.contains("grok")`
   in the bare branch. Add inference test.
2. `process_spawner::extract_session_id`: add `sessionId` (camelCase) fallback after
   `session_id` (grok `end` event). Add test.
3. `model_catalog::normalize_provider_label`: `"grok" | "xai" => "xAI"`.
   Run: `cargo test -q --manifest-path src-tauri/Cargo.toml` → all pass. Commit.

### Task 4 — Frontend wiring

Files: `src/renderer/src/types.ts`, `lib/providerResolution.ts`,
`lib/modelResolution.ts`, `lib/modelCatalogGrouping.ts`, `lib/providerSetup.ts`

1. `types.ts`: add `'grok'` to `ProviderKind`.
2. `providerResolution.ts`: `grok` in prefix list; `includes('grok')` keyword branch.
3. `modelResolution.ts`: add `'grok'` to `stripProviderPrefix` list (bare ids stay
   self-identifying).
4. `modelCatalogGrouping.ts`: `grok: 8` in `PROVIDER_PRIORITY`.
5. `providerSetup.ts`: `grok: 'grok login'` + `grok: 'https://github.com/xai-org/grok-build'`.
6. Tests: extend `tests/providerResolution.test.ts` with prefix + keyword + buildAgentConfig
   bare-id storage cases for grok.
   Run: `npm run test:js` → pass; `npm run typecheck` → pass. Commit.

### Task 5 — Docs

1. `AGENTS.md`: nine provider kinds; add `grok.rs` to the providers module table; add
   `grok` to the Provider Support list.
2. `CHANGELOG.md`: new `## [Unreleased]` section with an Added entry.
   Commit.

### Task 6 — Full verification

1. `npm test` (JS + Rust suites) → green.
2. `npm run typecheck` → green.
3. `npm run lint` → green.

## Merge & cleanup

Merge `feature/grok-build-cli` into `main` (`--no-ff`), run full suite on main,
delete branch, remove worktree.

## Risks / notes

- Mentor read-only relies on grok's documented internal tool IDs
  (`read_file,grep,list_dir`). If a future grok build renames them, mentor turns will
  surface the CLI error via ErrorDetailPanel; the IDs are stamped with the
  verification date in `grok.rs`.
- Grok is not installed on this machine, so all CLI-shape knowledge comes from the
  official docs (2026-09-19); tests pin the built command and parsed event shapes.
