# AI Agent Guidelines for "The Pair"

This file contains instructions and context for any AI agents (like yourself) working on the `the-pair` repository.

## Project Identity

"The Pair" is a Desktop application built with Tauri 2.x, React 19, and TypeScript. It orchestrates two local AI agents — a **Mentor** (planner/reviewer, read-only) and an **Executor** (code writer/command runner) — that cross-check each other's work. The app manages their lifecycle, monitors resources, tracks git changes, and provides a polished UI for humans to observe and intervene.

## Tech Stack

- **Framework:** Tauri 2.0
- **Backend:** Rust
- **Frontend:** React 19, TypeScript
- **Styling:** Tailwind CSS v4, `clsx`, `tailwind-merge`
- **Icons:** `lucide-react`
- **State Management:** `zustand` (`usePairStore`, `useUpdateStore`, `useThemeStore`, `useLocaleStore`)
- **Animations:** Framer Motion

## Architecture Rules

1. **Rust Backend vs. React Frontend:**
   - Never import Node.js built-ins directly into the Renderer (`src/renderer/src/`).
   - Use Tauri commands in `src-tauri/src/` to expose strict APIs to the frontend.
   - Frontend calls backend via `invoke()` from `@tauri-apps/api/core`.
2. **Styling:**
   - Always use Tailwind CSS classes. Custom dark/light mode palette is defined in `src/renderer/src/assets/main.css`.
   - Shell chrome (sidebar, hub, inspector, modals, palette) uses sans (`font-sans`, Geist Sans/SF); the conversation core (pair stream, diffs, code) uses mono (`font-mono`, Geist Mono). Never re-mono shell labels or re-sans the stream — `tests/shellTypography.test.ts` pins the split.
   - Shell surfaces use `.shell-card` (+ `.shell-card-hover`); the sidebar rail uses `.app-source-list`. The stream stays flat with hairline borders.
   - Use the `cn()` utility from `src/renderer/src/lib/utils.ts` for conditional class merging.
3. **Agent Interactions:**
   - The application spawns processes. Runs are unbounded by default (`maxIterations = 0`); an optional per-pair iteration cap pauses the pair into human review when reached — keep that cap semantics intact when touching turn logic.
   - Use the handoff guard (`src/renderer/src/lib/handoffGuard.ts`) to prevent race conditions when pairs finish.
4. **Error Handling:**
   - Handle permissions and locked files defensively in the Rust backend.
   - Propagate errors cleanly to the frontend via Tauri commands to display in the `ErrorDetailPanel`.

## Rust Backend Modules (`src-tauri/src/`)

| Module               | Responsibility                                                                                                                                                                                                                               |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pair_manager`       | Pair lifecycle: create, list, delete, pause, resume, assign task, update models; resolves each pair's workspace (in-place, or a fresh worktree on an auto-named `the-pair/…` branch — per task when requested)                               |
| `message_broker`     | State machine for agent turn coordination and event routing; also flags stalled turns (warning at 120s without output, stalled at 600s)                                                                                                      |
| `context_bridge`     | Parses the mentor's plan checklist and bridges handoff context between mentor/executor turns                                                                                                                                                 |
| `process_spawner`    | Spawns CLI processes, parses JSON event streams; delegates to provider trait for extraction; per-turn step-cycle runaway guard                                                                                                               |
| `acceptance`         | Mentor acceptance loop: parses verdict JSON, classifies change risk, runs verification checks (`git diff --check`, typecheck, tests) with timeouts, builds repair/follow-up prompts                                                          |
| `quality_gate`       | Validates mentor review evidence (files reviewed, checks performed) before a verdict is accepted                                                                                                                                             |
| `smart_pause`        | Pause reasons (iteration budget exhausted) and the plan-approval gate decision; human-readable pause messages                                                                                                                                |
| `worktree_manager`   | Git worktree lifecycle: repo state checks, branch listing, worktree creation with work-preservation contract on delete (stash + rescued/kept `the-pair/…` branches)                                                                          |
| `provider_adapter`   | Facade over the provider trait; legacy compatibility shim for `ProviderAdapter::build_turn_command()` etc.                                                                                                                                   |
| `provider_registry`  | `ProviderKind` enum, shared helpers (`which_binary`, `collect_*`), model discovery utilities                                                                                                                                                 |
| `providers`          | **Provider trait + per-provider modules** (`opencode.rs`, `codex.rs`, `claude.rs`, `gemini.rs`, `kimi.rs`, `pi.rs`, `kiro.rs`, `aider.rs`, `grok.rs`, `muse.rs`). Each implements CLI args, token extraction, detection, and model metadata. |
| `model_catalog`      | Static model metadata (display names, billing kind, recommended roles); delegates to provider trait for per-provider fields                                                                                                                  |
| `session_snapshot`   | Persists and restores full pair state; supports session recovery after crash/restart                                                                                                                                                         |
| `recent_activity`    | Derives the cross-pair recent-activity feed from session snapshots/messages                                                                                                                                                                  |
| `report_generator`   | Session run reports (markdown + file export), written to the app data dir — never into the user's repository                                                                                                                                 |
| `skill_discovery`    | Scans project dirs for `.md` skill files with YAML frontmatter                                                                                                                                                                               |
| `resource_monitor`   | Per-agent CPU/memory polling (1s interval)                                                                                                                                                                                                   |
| `git_tracker`        | Detects modified/added/deleted files relative to a baseline commit                                                                                                                                                                           |
| `file_cache`         | Lists files and parses `@mention` references in task specs                                                                                                                                                                                   |
| `path_env`           | Refreshes `$PATH` from login shell so CLI tools are discoverable                                                                                                                                                                             |
| `config_paths`       | Resolves platform-specific config file locations                                                                                                                                                                                             |
| `intelligence_store` | Cross-run intelligence: local SQLite store (`intelligence.db`) of run outcomes + human interventions, heuristic task tagging, insights/recommendation queries, one-time snapshot backfill                                                    |
| `types`              | Shared serde types (pair state, messages, acceptance, token usage) exchanged over Tauri IPC                                                                                                                                                  |
| `util`               | Small shared helpers (`now_millis`, mock-mode check, handoff prompt builders)                                                                                                                                                                |
| `test_env`           | Test-only lock serializing process-wide env mutations across `cargo test` threads                                                                                                                                                            |
| `stubs`              | Config/model-cache/provider-login commands                                                                                                                                                                                                   |

## Frontend Components

Paths are relative to `src/renderer/src/components/` unless noted.

| Component                                                             | Purpose                                                                                                             |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `../App.tsx`                                                          | Root: app shell routing (hub sections vs pair detail), modals, shortcuts, updater                                   |
| `AppChrome.tsx`                                                       | Window chrome, title bar, theme toggle                                                                              |
| `AppShell.tsx`                                                        | Left section rail (Pairs / Activity / Insights) with running + attention badges                                     |
| `PairsHub.tsx`                                                        | Pairs home: stat cards + workspace-grouped pair cards with quick actions                                            |
| `StartupHero.tsx` / `BootSplash.tsx`                                  | Boot splash + first-run empty-state hero (model config & directory selection happen in the hub)                     |
| `ShellViews.tsx`                                                      | `ActivityView` (needs-attention + recent activity) and `InsightsView` (cross-run leaderboard)                       |
| `StatCards.tsx`                                                       | Hub stat cards (running/paused/finished/attention counts)                                                           |
| `RecentActivityPanel.tsx`                                             | Cross-pair recent activity feed (activity section)                                                                  |
| `CreatePairModal.tsx`                                                 | New pair creation form (task presets via `PresetPicker.tsx`)                                                        |
| `PairSettingsModal.tsx`                                               | Edit existing pair settings                                                                                         |
| `AssignTaskModal.tsx`                                                 | Assign a new task to an existing pair                                                                               |
| `ModelPicker.tsx`                                                     | Model selection dropdown with role headers, drop-up support, recent model tracking (role-specific)                  |
| `BranchPicker.tsx`                                                    | Fuzzy repo/worktree branch picker                                                                                   |
| `SkillPicker.tsx`                                                     | Browse and attach skill files to a task                                                                             |
| `TaskHistoryPanel.tsx`                                                | View past run history for a pair                                                                                    |
| `MessageFilterBar.tsx`                                                | Filter console messages by role (All/Mentor/Executor)                                                               |
| `ScrollToBottomButton.tsx`                                            | Auto-appears when new messages arrive                                                                               |
| `IterationProgress.tsx`                                               | Visual indicator with warning when approaching iteration limit                                                      |
| `Dashboard.tsx`                                                       | Pair detail view: conversation console + operations inspector (no list state)                                       |
| `PairConsole.tsx`                                                     | Unified per-pair console: turn cards, terminal events, plan review bar, composer, search                            |
| `MessageCard.tsx` / `TurnCardView.tsx`                                | Message and turn-card rendering (markdown, mentions, token chips)                                                   |
| `AcceptanceMessageBody.tsx`                                           | Acceptance verdict/record rendering with tone chips                                                                 |
| `PlanReviewBar.tsx`                                                   | Human approval bar for the plan gate (Awaiting Human Review)                                                        |
| `MarkdownContent.tsx`                                                 | Markdown rendering with syntax highlighting for agent messages                                                      |
| `FileMention.tsx` / `SkillMention.tsx`                                | Renders `@file` and skill mentions in messages                                                                      |
| `TokenChip.tsx`                                                       | Inline per-turn token-usage chip                                                                                    |
| `SystemBanner.tsx` / `ShortcutsModal.tsx` / `LanguageSwitcher.tsx`    | System banners, shortcuts help modal, i18n language switch                                                          |
| `ErrorBoundary.tsx` / `ErrorDetailPanel.tsx`                          | Renderer crash boundary; actionable error display with retry/discard options                                        |
| `PairOperationsPanel.tsx`                                             | Inspector: collapsible sections (models, run state, resources, files, history, timeline)                            |
| `TimelinePanel.tsx` (+ `TimelineEventItem`, `TimelineIterationGroup`) | Run timeline rendering shared by inspector + console                                                                |
| `inspector/InspectorSection.tsx`                                      | Collapsible section primitive with header badge + actions slots                                                     |
| `inspector/InlineDiff.tsx`                                            | Inline file-diff body (shared diff rendering, no modal)                                                             |
| `terminal/`                                                           | Terminal event primitives (`TerminalBlock`, `TerminalEventRow`, `TerminalDivider`)                                  |
| `ui/`                                                                 | Reusable primitives: `GlassButton`, `GlassModal`, `ConfirmModal`, `Chip`, `IconButton`, `ResourceMeter`, `StatCard` |
| `RecoveryBanner.tsx`                                                  | Hub banner listing restored pairs needing a human decision                                                          |
| `CommandPalette.tsx`                                                  | Global fuzzy command palette (Cmd+K) with grouped actions + pair jump                                               |
| `InsightsPanel.tsx`                                                   | Cross-run combo leaderboard on the insights section (hidden until ≥5 recorded runs)                                 |
| `StatusBadge.tsx`                                                     | Pair status pill (Idle/Mentoring/Executing/Reviewing/Paused/Error/Finished)                                         |
| `UpdateNotification.tsx` / `UpdateControls.tsx`                       | In-app updater UI                                                                                                   |

## Key Frontend Libraries (`src/renderer/src/lib/`)

| File                    | Purpose                                                        |
| ----------------------- | -------------------------------------------------------------- |
| `modelResolution.ts`    | Resolves display name and provider from a model ID             |
| `modelPreferences.ts`   | Persists per-role recent model selections (role-specific keys) |
| `providerResolution.ts` | Maps provider kind to label/config                             |
| `providerSetup.ts`      | Checks provider readiness                                      |
| `handoffGuard.ts`       | Prevents duplicate handoff events after pair finishes          |
| `workspace.ts`          | Workspace directory helpers                                    |
| `animations.ts`         | Shared Framer Motion variants                                  |
| `tauri-api.ts`          | Typed wrappers around `invoke()` calls                         |
| `paletteActions.ts`     | Command-palette action registry shared by palette + menus      |

## Pair Status State Machine

```
Idle → Mentoring → Executing → Reviewing → (loop or Finished)
                                         → Paused → (resume → Mentoring or Reviewing)
                                         → Awaiting Human Review
                                         → Error
```

## Session Snapshot & Recovery

Snapshots are persisted to Tauri's app data directory. Each snapshot includes full conversation history, agent activity, resource usage, git tracking state, and provider session IDs. On restore, the appropriate resume prompt is injected so agents can continue from where they left off.

## Provider Support

Ten provider kinds are supported: `opencode`, `codex` (OpenAI Codex CLI), `claude` (Claude Code CLI), `gemini` (Antigravity CLI), `kimi` (Kimi Code CLI), `pi` (Pi Agent CLI), `kiro` (AWS Kiro CLI), `aider` (Aider CLI), `grok` (xAI Grok Build CLI), `muse` (Meta Muse Code CLI). Each is implemented as a `Provider` trait in its own module under `src-tauri/src/providers/`.

### Adding a New Provider

1. Create `src-tauri/src/providers/new_provider.rs` implementing the `Provider` trait
2. Add `pub mod new_provider;` to `src-tauri/src/providers/mod.rs`
3. Add `Arc::new(new_provider::NewProvider)` to `all_providers()` in `mod.rs`
4. Add the variant to `ProviderKind` in `provider_registry.rs`, plus a `detect_*` fn there, and extend `infer_provider_kind()` in `provider_adapter.rs`
5. Add the variant to the TypeScript `ProviderKind` union in `src/renderer/src/types.ts`
6. Update `inferProviderFromModel()` in `providerResolution.ts` (frontend, keep in lockstep with the Rust heuristic) and decide whether `stripProviderPrefix()` in `modelResolution.ts` strips the new prefix (strip it only when bare model ids remain self-identifying — see the kimi comments there)
7. Update `PROVIDER_PRIORITY` in `modelCatalogGrouping.ts` (frontend)
8. Add login command + install URL to the maps in `providerSetup.ts` (frontend)

### Updating Provider CLI Interfaces (`update agents`)

When the user says **"update agents"**, audit every supported provider's CLI interface to make sure The Pair's `build_turn_command()`, `extract_token_usage()`, `collect_json_candidates()`, and `detect_*` functions still match the real CLI behavior. CLIs evolve fast — flags get renamed, output formats change, session/resume semantics shift.

**Procedure:**

1. **For each provider** (`opencode`, `codex`, `claude`, `gemini`/`agy`, `kimi`, `pi`, `kiro`, `aider`, `grok`, `muse`):
   - **Web search** the latest CLI documentation and changelog/release notes for the tool. Look for changes to:
     - Headless/non-interactive invocation flags (e.g. `-p`, `--message`, `exec`)
     - Output format flags (e.g. `--json`, `--stream`, `--output-format stream-json`, `--output-last-message`)
     - Session resume flags (e.g. `--resume`, `--session`, `resume <id>`)
     - Permission/sandbox flags (e.g. `--sandbox`, `--permission-mode`, `--yes-always`)
     - Model selection flags (e.g. `--model`)
     - Reasoning effort flags (e.g. `-c model_reasoning_effort=`)
     - Token usage / cost reporting in JSON event schema
   - **If the tool can be installed** (pip, npm, brew, curl), install or update it in an isolated environment and run `--help` / `--json` sample invocations to examine the actual CLI surface and event output firsthand. This is more reliable than docs alone.
   - Compare findings against the Rust `Provider` trait implementation in `src-tauri/src/providers/<name>.rs` and the detection logic in `provider_registry.rs`.

2. **Update any mismatched code:**
   - `build_turn_command()` — flags, arg order, flag syntax
   - `runtime_spec()` — transport/session/permission strategy if changed
   - `extract_token_usage()` / `collect_json_candidates()` / `extract_error_detail()` — JSON event schema
   - `usage_scope()` — whether a usage event covers one step, the turn, or the whole resumed session (agy's `result` and Codex ≥ 0.154's `turn.completed` are session totals; check with two resumed turns, not one)
   - `split_plain_output()` — for plain-text CLIs that print failures to stdout and still exit 0 (aider)
   - `detect_*()` — auth detection, model discovery commands
   - Frontend maps in `providerSetup.ts`, `providerResolution.ts`, `modelCatalogGrouping.ts`

3. **Run tests** after every provider update: `cargo test` + `npm test` + `npm run typecheck`

4. **Report** a summary of what changed per provider (flag renames, new features, deprecations).

## Adding Features

- **UI Components:** Keep components modular in `src/renderer/src/components/`. Reusable primitives go in `components/ui/`.
- **State:** Use `usePairStore` for pair-related global state. Add new stores in `src/renderer/src/store/` only if the concern is orthogonal.
- **Tauri Commands:** Add new commands to the appropriate Rust module and register them in `lib.rs`'s `invoke_handler`.
- **Models:** Update `model_catalog.rs` when adding new model entries; implement the `Provider` trait in `providers/` for new CLI tools.

## Testing & Coverage

**Policy: new or changed code must have 95%+ line coverage from unit tests.** Measured as _changed lines_ (`git diff`) against the coverage report, not whole-repo averages.

Three layers, all headless except the optional desktop smoke:

| Layer               | Command                                                                                                                   | Covers                                                                                  |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------- |
| Rust unit tests     | `npm run test:rust` (in `cargo test`)                                                                                     | `PairManager`/broker/spawner logic, git worktree operations against real temp git repos |
| Renderer unit tests | `npm run test:js` (store/logic, `tests/*.test.ts`) + `npm run test:ui` (jsdom component tests, `tests/render/*.test.tsx`) | Store contracts, modal/branch-chip UI                                                   |
| Headless e2e        | `npm run e2e:web` (Playwright + a scripted Tauri IPC mock, `e2e-web/`)                                                    | Real app UI against a mocked backend                                                    |
| Desktop e2e         | `npm run e2e` (Appium mac2, needs a GUI machine)                                                                          | Real packaged app, on-demand only                                                       |

Coverage commands:

- `npm run coverage:ts` — c8 report for the renderer (runs both unit suites).
- `npm run coverage:rust` — `cargo llvm-cov` for the Rust lib.

Harness notes:

- **Renderer component tests** (`tests/render/`) run React under jsdom via `node:test` + `@testing-library/react`. They use `TSX_TSCONFIG_PATH=tsconfig.web.json` (through `scripts/with-env.mjs`) so tsx applies the automatic JSX transform — component modules rely on it. `tests/render/setup-dom.ts` installs the jsdom globals and the `__APP_VERSION__` vite define; `tests/render/mock-ipc.ts` installs the real `tauri-shim` plus a scripted `mockIPC` handler covering `repo_check_state` and friends.
- **Headless e2e** (`e2e-web/`) starts the vite dev server (`npm run dev:renderer -- --port 5199 --host 127.0.0.1`) and injects `e2e-web/mock-init.js` before app code, defining `window.__TAURI_INTERNALS__`/`__TAURI_EVENT_PLUGIN_INTERNALS__`. Every IPC call lands on `window.__MOCK_CALLS__` so specs assert exact payloads. Test state is seeded through `openApp(page, state)` in `e2e-web/fixtures/app.ts`.
- Keep tauri command bodies thin: extract the testable logic into plain functions (see `prepare_new_run_workspace`, `prepare_run_context`, `validate_assign_task` in `pair_manager.rs`) — command shells cannot be unit-covered without an app runtime.

## Release Process (Automated)

**⚠️ CRITICAL: Never manually create or push git tags!**

The release workflow (`build-signed-mac.yml`) is fully automated:

1. **Prepare release:**
   - Bump the version with `npm run bump <version>` — it updates `package.json`, the root entries of `package-lock.json`, the `[package]` version in `src-tauri/Cargo.toml` and the app entry in `src-tauri/Cargo.lock` (`tauri.conf.json` reads its version from `package.json`)
   - Update `CHANGELOG.md` with release notes under a `## [X.Y.Z] - YYYY-MM-DD` heading at the start of a line (`npm run validate:changelog` checks it; the release workflow uses the same matcher, `scripts/changelog.mjs`, for its gate and for the release notes)
   - Run quality gates locally: `npm test && npm run typecheck && npm run lint`

2. **Publish:**
   - Commit changes: `git commit -m "chore: bump version to X.Y.Z"`
   - Push to main: `git push`
   - **Do NOT run `git tag` or `git push --tags`**

3. **Workflow automation:**
   - GitHub Actions detects version bump (publishes only when tag `vX.Y.Z` doesn't exist yet)
   - Release runs are serialized (`concurrency`, never cancelled); a run queued behind a release sees the new tag and skips
   - Builds macOS, Windows, Linux binaries
   - Creates the GitHub release and tag `vX.Y.Z` on the pushed commit (`--target $GITHUB_SHA`) with the changelog section as notes
   - Uploads signed artifacts and `latest.json`; if the release already exists it fails instead of overwriting assets

4. **Verify:**
   - Monitor at: https://github.com/timwuhaotian/the-pair/actions
   - Check release page for correct artifacts and notes

### Version Bump Guidance

Follow Semantic Versioning (`MAJOR.MINOR.PATCH`):

| Keyword           | Bump   | When to use                                                                   |
| ----------------- | ------ | ----------------------------------------------------------------------------- |
| **patch release** | `Z` +1 | Bug fixes, docs updates, typo corrections, internal refactors (no API change) |
| **minor release** | `Y` +1 | New features, new components, new Tauri commands (backward-compatible)        |
| **major release** | `X` +1 | Breaking changes, UI redesigns, removed APIs, state machine changes           |

**Quick decision tree:**

- `fix:`, `docs:`, `chore:`, `style:` → **patch** (e.g. 2.0.0 → 2.0.1)
- `feat:` without breaking changes → **minor** (e.g. 2.0.0 → 2.1.0)
- `feat:` with breaking changes, or removing public APIs → **major** (e.g. 2.0.0 → 3.0.0)

**Fallback:** If workflow fails to trigger, run:

```bash
gh workflow run build-signed-mac.yml
```

See `docs/RELEASE_CHECKLIST.md` for detailed checklist.
