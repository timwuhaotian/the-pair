# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.2.6] - 2026-06-22

### Fixed

- **`now_millis()` no longer panics on pre-epoch clocks.** A misconfigured system clock (VM resume, NTP correction) could crash the entire backend. Now uses `unwrap_or_default()`.
- **Handoff events are no longer processed on errored pairs.** The handoff guard now ignores events when a pair is in `Error` status, preventing unwanted agent turns on already-errored pairs.
- **`retryTurn` no longer causes unhandled promise rejections.** Added proper try/catch with error state, matching all other store actions.
- **`FileMention` and `SkillMention` no longer leak `setTimeout` callbacks.** Timer cleanup prevents React state updates on unmounted components.
- **`SkillPicker` no longer crashes in dev (non-Tauri) mode.** Switched from raw `invoke()` to the `window.api` wrapper with runtime guards.
- **Mock API no longer returns mutable references.** `checkState` and `listBranches` now return shallow copies, preventing callers from corrupting mock data.
- **`Versions` component handles `getVersion()` failures gracefully** instead of leaving an unhandled rejection.
- **`handleOpenConfig` in OnboardingWizard catches errors** instead of propagating unhandled rejections.
- **Provider inference matches future OpenAI model prefixes** (`o4`, `o5`, …) via regex instead of hardcoded `o1`/`o3` strings.
- **Eliminated 3 duplicate timestamp code paths** in `process_spawner.rs` that each carried an independent `.unwrap()` panic risk; now all use the shared `now_millis()` utility.

### Changed

- Removed dead `getRoleColors()` helper (leftover from earlier design iteration).
- Moved `isMac`/`cmdKey` to module-level constants in `App.tsx` (were recomputed on every render).
- Removed unused `isCompactLayout` prop chain and associated viewport-height resize listener from `OnboardingWizard`.

## [2.2.5] - 2026-06-22

### Fixed

- **Renamed files now show their diff.** Git reports a rename as `old -> new`; the file tracker stored that entire string as the path, so clicking a renamed file showed "No changes to display". It now records the destination path and the diff resolves correctly.
- **Exported HTML reports sanitize agent output.** The session-report export rendered model/agent markdown through an unsanitized converter, so raw `<script>` / `<img onerror=…>` markup or `javascript:` links in agent output could execute when the saved report was opened in a browser. Raw HTML is now escaped and script-bearing URL protocols are neutralized.
- **Activity stall monitor survives a backward clock jump.** A wall-clock step backward (NTP correction, VM resume) could underflow the stall timer — reporting a bogus multi-century "stalled" warning, or panicking in debug builds and poisoning the shared state lock. Elapsed time is now computed with saturating subtraction.
- **Deleting a pair always clears its session snapshot.** A repeated delete could return early and leave the snapshot orphaned on disk; cleanup now always runs.
- **Viewing task history no longer crashes when the pair was just deleted**, and model loading no longer throws if the cached-models response is empty or not an array.
- **Acceptance cards no longer show "NaN%."** A non-numeric confidence value is now rejected by the display fallback instead of rendering as `NaN`.
- **`@file` mentions no longer match path prefixes.** Mentioning `@lib/api-v2` no longer also attaches an unrelated `@lib/api` file whose path is a prefix of the mention.
- **Fixed stray React state updates and a leaked update listener** when the file-mention, skill-mention, and recent-activity panels (or the app shell) unmount while an async load is still in flight.

### Changed

- Internal cleanups (no user-facing behavior change): extracted a unit-tested `git status --porcelain` parser, de-duplicated the model-resolution helpers, removed a dead diff branch, and hoisted a render-defined component to module scope. Adds 8 regression tests.

## [2.2.4] - 2026-06-17

### Added

- **Plan-approval gate (human-in-the-loop review).** A new per-pair toggle in the Create Pair modal pauses the workflow after the mentor's plan — before the executor starts — at an "Awaiting Human Review" checkpoint. A plan-review bar lets you **Approve & start** (releases the plan to the executor) or **Send back** with optional notes (the mentor re-plans, threading your feedback into the prompt). Your decision is recorded as a visible message in the conversation, a rejected plan re-plans rather than being mistaken for a review verdict, and the setting persists across restarts via session snapshots. If you navigate to an archived run while a plan is waiting, a banner offers a one-click return. Localized for English, Chinese, Japanese, and Korean.

## [2.2.3] - 2026-06-15

### Added

- **Keyboard shortcuts cheat-sheet.** Press `Shift+?` (or click the keyboard icon in the title bar) to open a lightweight overlay listing every active shortcut with its platform-aware key combo. Shortcut descriptions are localized (English, Japanese, Korean, Chinese). A new `formatShortcutParts` helper renders each modifier and key as a separate `<kbd>` token, and symbol keys like `?` no longer show a redundant `⇧` prefix.

## [2.2.2] - 2026-06-14

### Added

- **Provider Sign In and Install guidance in onboarding.** When a CLI tool is installed but not authenticated — or not installed at all — the first-run onboarding screen now shows a per-provider card with a one-click **Sign In** button (opens a terminal running the provider's login command) or an **Install** button (opens the download page). The `Provider` trait gained `login_command()` and `install_url()` methods, each implemented for Claude Code, Codex, Gemini (agy / legacy aware), and OpenCode. A new `provider_launch_login` Tauri command opens the system terminal cross-platform (Terminal.app on macOS, gnome-terminal/konsole/xterm on Linux, cmd on Windows).
- **Auth-missing hint in the model picker.** When a provider has models locked behind authentication, the picker now shows a compact "N more models available after signing in" row with an inline Sign In link, so users discover hidden models without leaving the picker.

## [2.2.1] - 2026-06-14

### Changed

- **Conversation feed is now a single left-aligned column.** Mentor and executor turns previously sat on opposite sides as left/right chat bubbles; they now share one left-aligned column distinguished by a role-colored left accent border (mentor / executor / human). Long, mixed transcripts read more clearly and stay legible on narrow windows.
- **Provider backend refactored into a `Provider` trait (internal).** Per-provider behavior — CLI args, token extraction, detection, and model metadata — for opencode, Codex, Claude Code, and Gemini moved out of `process_spawner`/`provider_adapter` into dedicated modules under `src-tauri/src/providers/`, with `provider_adapter` kept as a thin compatibility facade. No user-facing behavior change; adding a provider is now a self-contained module.

### Fixed

- **Updating a pair's model now sends the bare model ID the CLI expects.** The picker uses qualified IDs such as `claude/claude-haiku-4-5`; applying a model change to an existing pair now strips the `claude`/`codex`/`gemini` prefix before it reaches the backend (OpenCode `provider/model` IDs are left intact), preventing a mismatched or unrecognized ID on model swaps.
- **Onboarding shows the current brand icon.** The first-run onboarding header displayed the retired icon; it now uses the current robots logo, matching the app icon and README.

### Removed

- Orphaned old-brand icon assets (`build/icon.*`, `resources/icon.png`, `resources/the-pair-icon-1.png`) and the unused `scripts/generate-ico.cjs` helper.

## [2.2.0] - 2026-06-14

### Added

- **Unified model picker that merges routes.** The same model reachable through more than one route — a native CLI _and_ OpenCode, or different plans/keys — now collapses into a single entry in the picker instead of appearing as near-duplicate rows. Selecting a multi-route model reveals a nested route sub-picker so you choose _how_ to run it (e.g. "Claude Code login" vs "OpenAI API key"), and the last route you used for each role is remembered. Merging is driven by a backend `canonicalKey` (`brand::normalized-model`, with provider prefixes and trailing date stamps stripped), so `claude-sonnet-4-5` and `claude-sonnet-4-5-20250929` collapse onto one entry while genuinely different versions (4-5 vs 4-6) stay apart. When there's no remembered route, plan-included native routes are preferred over pay-as-you-go OpenCode so you don't accidentally spend on an API key.
- **Keyboard focus rings.** A crisp accent ring now appears for keyboard and assistive-tech navigation (`:focus-visible`) on buttons, links, and list rows that previously had no focus affordance; pointer clicks stay ring-free.
- **Animated, accessible modals.** `GlassModal` and `ConfirmModal` now animate in (honoring `prefers-reduced-motion`) and expose proper `role="dialog"`, `aria-modal`, and `aria-label`.

### Changed

- **Reasoning effort is now inline and route-aware.** The effort control lives inside the model picker as a segmented selector sourced from the currently selected route, replacing the standalone picker. Antigravity bakes effort into the model name ("Gemini 3.5 Flash (Low/Medium/High)"), so those rows now collapse onto one model with the effort offered as a sub-control; Codex o-series still passes effort through as a runtime flag. Medium is the default when a route exposes the axis.
- **Acceptance verdict and next action are now independent axes.** A mentor's `verdict` ("pass"/"fail") judges the executor's latest output, while `nextStep.action` ("continue"/"finish") tracks whether the whole task is done. Previously a "pass" was forced to pair with "finish", which derailed multi-step tasks — a correct intermediate step had to be mislabeled "fail" just to keep the loop running. A good step with work remaining is now legitimately "pass" + "continue" with the next instructions; "fail" + "finish" is still rejected (it would leave the loop with no actionable next turn). The mentor prompt explains the two axes, and the console now renders whatever structured verdict the mentor produced instead of falling back to raw JSON on a contract mismatch.
- **OpenCode model list is de-noised.** OpenCode mirrors the entire models.dev catalog plus any provider you hold a key for; unauthenticated rows you can't actually run are now filtered out so they no longer bury the native providers. A model offered by both a native CLI and OpenCode is no longer dropped — both routes survive and merge into one picker entry (see above). Native CLIs still surface their unavailable rows so you can see _why_ an installed CLI isn't usable yet.

### Removed

- **Standalone `ReasoningEffortPicker` component.** Reasoning effort is now part of the model picker itself, sourced from the selected route.

## [2.1.0] - 2026-06-13

### Added

- **Antigravity CLI (`agy`) now backs the Gemini provider**, with automatic fallback to the legacy `gemini` CLI. The legacy Gemini CLI stops serving requests on 2026-06-18, so The Pair now prefers `agy` when installed (discovered via `agy models`), runs it with `--dangerously-skip-permissions` (no approval prompts), and only falls back to `gemini` when `agy` is absent. Antigravity model display names (e.g. "Gemini 3.5 Flash (Low)") are routed correctly alongside canonical lowercase ids.
- **Unlimited iteration budget.** A `maxIterations` of `0` now means no cap: the progress bar shows `∞`, the "nearing limit" banner is suppressed, and a `0` budget never auto-pauses. Pairs that should run unbounded no longer hit a false limit.
- **Context-aware "New Task" button in pair sessions.** The top-right chrome button was a duplicate "New Pair" that stayed a create action even inside a session. It now hides on the dashboard (the sidebar remains the single create entry) and, inside a session, becomes a prominent "New Task" button that opens the assign-task modal for the selected pair as a fresh task. It is disabled while the pair is busy to avoid losing in-flight work, and `Cmd/Ctrl+N` is context-aware too.

### Changed

- **Reasoning-effort control is now provider-aware.** Claude Code (2.1.x), the Gemini CLI, and `opencode` expose no CLI flag for reasoning/thinking effort — injecting one hard-crashes the Claude turn or is silently ignored — so the control is hidden for them. Codex configures reasoning via `-c model_reasoning_effort=` (the removed `--reasoning-effort` flag now errors out). Only Codex o-series models offer the control.
- **Codex sandbox is now per-role.** The mentor runs read-only (the CLI default) while the executor gets workspace-write, so the executor can actually apply edits in the worktree instead of silently failing.

### Fixed

- **Claude Code error results no longer look like success.** A `result` event with `subtype: "error"` / `is_error: true` (including permission denials) now surfaces as a hard Error with its detail, instead of leaking the (usually empty) result text into the conversation and never entering Error.
- **Codex token usage now flips live.** `turn.completed` (with `input_tokens`/`output_tokens`) is classified as the terminal/final usage event, so the UI token chip updates instead of staying on a spinner.

## [2.0.3] - 2026-06-05

### Fixed

- **`opencode` CLI no longer goes undetected on macOS/Linux:** `~/.opencode/bin` is now included in the fallback PATH directories used to locate provider CLIs. Previously, when `opencode` was installed to its default per-user location and not present on the inherited PATH, the app failed to find the binary (and its model catalog). Thanks @fxricky (#6).

## [2.0.2] - 2026-05-26

### Fixed

- **Acceptance checks no longer fight text-only turns:** `git diff --check`, `npm run typecheck`, and `npm run test` are now skipped when the executor produced no file modifications. Previously a smoke greeting or text-only diagnostic would still trigger `git diff --check`, and any environment-level failure (e.g. exit 129 from a signal kill) was reported to the mentor as a hard `failed`, costing ~1K tokens per iteration to explain away.
- **`git diff --check` env failures are no longer treated as bugs:** spawn errors, signal kills (exit > 128), and "not a git repository" stderr are now reported as `Skipped` instead of `Failed`. Real whitespace errors (exit 1/2) still surface as `Failed`.
- **Dev-smoke detection is more forgiving:** `is_dev_smoke_greeting_output` now matches paraphrased forms ("Send Greeting 1/3", "Greeting 1/3 received", "Acknowledged: Greeting 2/3") so a real executor CLI that echoes the mentor's instruction wording instead of the bare greeting still skips code-quality checks.
- **Executor re-anchored to original task on follow-up turns:** `buildExecutorAcceptanceFollowupPrompt` now appends an `ORIGINAL TASK` section before the adjustments list so multi-iteration runs don't drift into responding only to the latest instruction.

### Added

- **Hidden-message indicator in pair console:** When consecutive messages from the same role/iteration get collapsed, the console now shows "+ N earlier messages hidden" so users know content was suppressed rather than silently lost. New `collapseWithDropCounts` helper in `consoleMessages.ts` powers this.
- **Iteration-limit approaching warning:** New banner in `IterationProgress` warns when a pair is nearing its iteration budget, with hint to narrow scope or stop the turn before the auto-pause kicks in.
- **Stop turn button + running input hint:** Pair console now exposes a Stop Turn action and surfaces "Pair is running — pause first to queue a new task" when input is disabled mid-run.
- **Models-panel queue state:** Model changes mid-run now show as `queued`/`in use` badges with copy clarifying that updates apply to the next task, not the running turn (`saveActionRunning`, `savedQueued`, `modelsUpdateHintRunning`).
- **Status label localization:** All `PairStatus` values (Idle/Mentoring/Executing/Reviewing/Paused/Awaiting Human Review/Error/Finished) are now i18n keys with translations for en/zh/ja/ko.
- **Acceptance details toggle:** New show/hide controls on `TurnCardView` to reveal full acceptance reports and step lists on demand.
- **`.github/` polish:** Added `FUNDING.yml`, issue template `config.yml`, and social-preview image; refreshed `build-signed-mac.yml` workflow.
- **README and package.json keywords** expanded for discoverability (claude, codex, gemini, copilot-alternative, rust, react, etc.).

## [2.0.1] - 2026-05-18

### Added

- **Skill discovery:** Backend module scans project directories for `.md` skill files with YAML frontmatter, exposing them to task assignment flows.
- **Boot splash:** New `BootSplash` component with animated loading screen shown during app initialization.
- **Startup hero:** `StartupHero` component displayed after boot splash while the main UI hydrates.
- **Skill mentions:** `SkillMention` component and `skillMentions.ts` library for rendering skill references in messages.
- **File mentions:** Enhanced `FileMention` component and `fileMentions.ts` library with improved popover positioning and modal stacking.

### Changed

- **Dashboard:** Updated layout and styling with improved pair list rendering and dashboard state management.
- **PairConsole:** Refined console output rendering and message classification for better readability.
- **App shell:** Restructured `App.tsx` to integrate boot splash and startup hero into the initialization flow.
- **AssignTaskModal & CreatePairModal:** Updated to support skill file selection and improved form layouts.
- **CSS tokens:** Added 212 lines of new theme tokens and utility styles in `main.css` for boot splash and startup animations.
- **Locales:** Updated Chinese, Japanese, Korean, and English translations for new UI strings.
- **Documentation:** Updated project documentation in README.md.

## [2.0.0] - 2026-05-17

### Added

- **Unified Pair Console:** Replaced the split PairDetail view with a new `PairConsole` component that merges the agent console, timeline, and activity tracking into a single scrollable feed.
- **System Banner:** New `SystemBanner` component for surfacing app-wide notifications and alerts.
- **Dashboard Operations Panel:** Replaced the recent activity sidebar with a live operations summary showing attention items, running pairs, resource load, workspaces, and changed files.
- **Pair Cards:** Added compact per-pair metrics for turns, CPU, memory, and modified files directly in the pair list.
- **Dashboard Insight Panel:** New `DashboardInsightPanel` for contextual tips and guidance.
- **Empty State Guide:** New `EmptyStateGuide` with step-by-step onboarding for first-time users.
- **Terminal Components:** New `TerminalBlock`, `TerminalDivider`, and `TerminalEventRow` for structured terminal-like output rendering.
- **Handoff Prompts Library:** New `handoffPrompts.ts` module for structured agent handoff messages.
- **Sound Cue System:** New `pairSoundCue.ts` for contextual audio feedback during pair runs.

### Changed

- **Major UI Redesign:** Comprehensive visual overhaul of all components with improved glass-morphism effects, refined spacing, and consistent dark/light theme tokens.
- **App Chrome:** Updated window chrome with new logo, refined title bar, and streamlined controls.
- **Dashboard Layout:** Restructured from sidebar-heavy design to a card-based grid with responsive stat cards.
- **Activity Tracking:** `ActivityIndicator` and `ActivityItem` components redesigned for better clarity and visual hierarchy.
- **Message Cards:** `MessageCard` and `TurnCardView` simplified for better readability and reduced visual noise.
- **Timeline Panel:** `TimelinePanel`, `TimelineEventItem`, and `TimelineIterationGroup` refined for cleaner iteration grouping.
- **Status Badge:** `StatusBadge` updated with new color variants and improved accessibility.
- **Locale Persistence:** Language preference now persists across sessions via `useLocaleStore`.
- **Glass UI Components:** `GlassButton`, `GlassCard`, `GlassModal`, and `StatCard` updated with refined tokens and animation variants.
- **Resource Meter:** `ResourceMeter` improved with better compact mode and label hiding.
- **Acceptance Messages:** `AcceptanceMessageBody` updated for cleaner mentor verdict rendering.

### Removed

- **PairDetail Component:** Deleted the legacy `PairDetail.tsx` in favor of the unified `PairConsole`.
- **ToolCallSteps Component:** Removed `ToolCallSteps.tsx` as cognitive event visualization is now handled inline.

### Fixed

- **Dashboard Create Action:** Moved the new-pair call to action into the pair list area and removed the dead floating action menu entries.
- **Persisted Status Handling:** Normalized restored pair statuses before rendering so older snapshots do not create invisible dashboard groups.
- **Claude Output Capture:** Captured assistant text blocks when Claude events do not include a final result payload, preserving visible mentor output in the console.
- **Acceptance Parsing:** Improved robustness of mentor acceptance report parsing for edge cases in JSON extraction.
- **E2E Selectors:** Updated e2e helper selectors to match the new component structure.

## [1.4.5] - 2026-05-11

### Added

- **Dashboard operations panel:** Replaced the recent activity sidebar with a live operations summary showing attention items, running pairs, resource load, workspaces, and changed files.
- **Pair cards:** Added compact per-pair metrics for turns, CPU, memory, and modified files directly in the pair list.

### Fixed

- **Dashboard create action:** Moved the new-pair call to action into the pair list area and removed the dead floating action menu entries.
- **Persisted status handling:** Normalized restored pair statuses before rendering so older snapshots do not create invisible dashboard groups.
- **Claude output capture:** Captured assistant text blocks when Claude events do not include a final result payload, preserving visible mentor output in the console.

## [1.4.4] - 2026-05-02

### Fixed

- **CreatePairModal layout:** Added max-height with scrollable content area so the "Create Pair" button is always visible regardless of content length. Form content now scrolls independently of fixed footer buttons.
- **ModelPicker dropdown positioning:** Dropdown now consistently renders at the bottom of the card regardless of recent model count. Removed forced equal-height grid stretching that caused overlap when one card had more recent models than the other.
- **ModelPicker spacing:** Fixed spacing between recent model grid and dropdown selector when 3+ recent models create a third row.
- **PresetPicker border conflict:** Removed `ring-2 ring-primary/40` from selected preset card that created double-border visual conflict with the container.
- **BranchPicker colors:** Replaced hardcoded `text-slate-400/500` with theme-aware `text-muted-foreground` for proper light/dark mode visibility.
- **Pair card visibility:** Redesigned pair list cards from transparent (`border-transparent`) to glass-card style with visible border and background, making them clearly distinguishable on the dashboard.

## [1.4.3] - 2026-04-29

### Fixed

- **Complete CJK i18n coverage:** Replaced 60+ hardcoded English strings across 8 components (`PairDetail`, `OnboardingWizard`, `TaskHistoryPanel`, `TimelinePanel`, `ErrorDetailPanel`, `ActivityIndicator`, `TurnCardView`, `UpdateControls`) with `react-i18next` translations. All UI labels, buttons, status text, empty states, and error messages now fully support Chinese, Japanese, and Korean.

## [1.4.2] - 2026-04-29

### Fixed

- **Intent chips not showing:** Fixed two issues blocking cognitive event display. (1) Opencode's `"stream"` and Codex's `"message"` event types were not recognized by the pattern matcher in `process_spawner.rs` — added them to the content/reasoning branch. (2) The fallback `else` branch in the first-output handler did not create any cognitive event, so unrecognized event types left the turn with zero cognitive events — now always emits a generic "Processing" reasoning event as a safety net.
- **"— tok" placeholder:** Token chip no longer renders with a dash when no token data is available yet. The chip is hidden until the first token count arrives, eliminating visual noise on fresh turns.

## [1.4.1] - 2026-04-29

### Fixed

- **Token tracking:** Enhanced input token support and comprehensive reporting metrics across the UI and export modules. Token usage now shows complete input/output breakdown in timeline, turn cards, and run reports.

## [1.4.0] - 2026-04-29

### Added

- **Cognitive transparency:** Real-time intent chips and tool call visualization so you can see exactly what the AI agent is doing.
- **Intent chip:** Floating indicator in the live turn card showing the agent's current intent (分析中 / 编写中 / 验证中 / 审查中 / 等待中 / 处理错误) with animated icons.
- **Tool call steps:** Expandable step-by-step visualization of all tool calls (bash/read/write/search) with status indicators (pending/running/completed/error).
- **Reasoning events:** Agent reasoning steps captured from content blocks and thinking signals, rendered alongside tool calls.
- **Cognitive event tracking:** New `CognitiveEvent` data model flowing from Rust backend through Tauri events to the frontend, stored on `TurnCard` for turn-scoped lifecycle.

### Fixed

- **Task History layout:** Duration text and restore button no longer overlap — added spacing via `mr-6` and `marginRight` offset.
- **Console executor cards:** Executor messages containing handoff prompt markers are now correctly rendered — `isTechnicalHandoff` filter excludes executor messages, and `stripSystemPrompt` extracts actual response content from handoff-prompt-wrapped messages.
- **Infinite re-render loop:** Fixed React infinite re-render caused by Zustand selector returning new array references — cognitive events moved from `Pair` (pair-scoped) to `TurnCard` (turn-scoped), eliminating the selector entirely.

## [1.3.14] - 2026-04-28

### Changed

- **Flat iteration budget:** Replaced the adaptive file-count-based budgeting system with a flat default of 20 iterations per pair run. The complex tiered system (None=1, Simple=3, Medium=6, Complex=10) was over-engineered for a human-in-the-loop workflow where resume is the intended escalation path.
- **Iteration limit enforcement simplified:** Removed the `adaptive_budget` state field and the `.max(20)` floor. The budget now respects the user's configured `maxIterations` directly, allowing intentional low-budget runs (e.g., smoke tests with 3 iterations) without override.
- **Pause message wording:** Updated the budget exhaustion message from "Agent reached iteration budget" to "Reached iteration limit" with clearer next-step guidance (continue, assign new task, or finish).

### Removed

- **`adaptive_stop.rs` module:** Deleted the entire weighted file complexity scoring system (180 lines). It was never fully integrated — adaptive budgets were computed but the enforcement used a separate inline check.
- **`ScopeDrift` pause reason:** Removed the scope drift detection that paused pairs when executors modified files outside the mentor's plan. It produced false positives on legitimate refactors and was never a stable feature.
- **Dead code:** Removed unused `HandoffContext` and prompt formatting functions from `context_bridge.rs`, unused `check_review_quality` from `quality_gate.rs`, unused `set_pause_message` and `set_adaptive_budget` from `message_broker.rs`, and the `pause_message` field from `PairState`. Also deleted a stale `docs/ux-optimization-checklist.md`.

### Fixed

- **Resume iteration bug:** `restore_session` now calls `resume_run` instead of `prepare_run`, fixing incorrect iteration increments when restoring paused mentor review turns. Previously, restoring a review turn would bump the iteration counter, making the progress display inaccurate.
- **Resume stale handoff race:** `pair_resume` now increments `run_generation` before spawning the resumed turn. This ensures the stale handoff guard correctly rejects any late events from pre-pause turns that could trigger unwanted additional turns.
- **Frontend default budget:** Changed the frontend fallback from `9999` to `20` in `usePairStore` so new pairs display the correct default. (Existing pairs created before this change retain their original `maxIterations` value from their snapshot.)

## [1.3.13] - 2026-04-27

### Added

- **File diff viewer:** Click any changed file in the pair detail view to open a `FileDiffModal` showing a side-by-side git diff (staged + unstaged). Backend `git_get_file_diff` Tauri command walks the git_tracker to compute unified diffs.
- **Global keyboard shortcuts:** Registered platform-agnostic shortcuts for common actions (new pair, toggle pair settings, focus console, mute/unmute sounds). Visible shortcut labels added to toolbar buttons.
- **Sound feedback system:** Rewrote sound module with HTML Audio + Web Audio fallback. Added finish chime, error alert, and pause-confirm sounds. Mute toggle in app chrome, preloaded on app mount.
- **i18n READMEs:** Added Chinese (README.zh.md), Japanese (README.ja.md), and Korean (README.ko.md) translations with language switch links.

### Fixed

- **Step cycle detection:** Added per-turn step cycle counter (max 50 cycles) that terminates the agent process if it enters an infinite step loop, preventing CPU exhaustion.
- **Mentor iteration limit:** Extended the max-iterations check to mentor turns (previously executor-only) so both agents respect the iteration cap.
- **Onboarding wizard stall:** Reset `isCheckingProviders` loading state when models are already cached, preventing a perpetual spinner.
- **Audio context resume:** Resume suspended Web Audio context before playing the finish chime so sound works after browser autoplay policy blocks.
- **Diff modal accuracy:** Use `git diff HEAD` instead of `git diff` so staged changes are included alongside unstaged modifications.

## [1.3.12] - 2026-04-26

### Fixed

- **Startup responsiveness:** Rendered the app shell immediately during startup instead of holding a blank window while initial IPC calls complete.
- **Provider model loading:** Load cached model data first, then refresh provider/model detection in the background so startup is no longer gated on CLI scans.
- **Provider probe stalls:** Added bounded CLI probe timeouts and parallel provider detection to reduce startup stalls when provider commands are slow.

## [1.3.10] - 2026-04-23

### Fixed

- **OpenCode detection on Linux (.deb installs):** Added `~/go/bin` to the fallback PATH directory list so that `opencode` installed via `go install` is correctly detected even when the app is launched from a `.desktop` file with a minimal inherited PATH (#4).

## [1.3.9] - 2026-04-10

### Added

- **Stalled activity detection**: New `stalled` activity phase for agents that haven't updated in over 60 seconds, with visual indicator in StatusBadge and ActivityIndicator.
- **Process termination**: Backend support for gracefully terminating running pair processes with cleanup, preventing zombie processes when stopping or deleting pairs.
- **Turn card tracking**: Improved turn card state tracking with stall detection and proper phase transitions in the frontend store.

### Changed

- **Dashboard visual polish**: Refined spacing scale consistency (gap-2/3/4), standardized badge typography to text-[10px] with refined tracking, and improved card layout alignment with grid-aligned min-height.
- **Color palette migration**: Migrated from zinc to slate color palette across BranchPicker, TokenChip, and ResourceMeter for improved UI consistency with the rest of the app.
- **UI localization**: Localized remaining Chinese comments and labels to English in Dashboard, AppChrome, and ResourceMeter components.
- **Dark mode refinements**: Added proper dark mode variants for all status badge and model indicator colors for improved contrast.

### Fixed

- **Activity indicator positioning**: Fixed layout positioning for ActivityIndicator component to prevent overflow in constrained spaces.

## [1.3.8] - 2026-04-07

### Fixed

- **Update notification overflow**: Removed release body from update notification message to prevent UI overflow with long changelogs.

## [1.3.7] - 2026-04-07

### Added

- **Geist font integration**: Added Geist font family support and updated design system tokens and animations for a more polished visual identity.
- **Auto-generate default config**: Default configuration file is now auto-generated on first run, reducing manual setup.
- **Dashboard pair container UI**: New dashboard and message UI components for pair container management.
- **Project cleanup script**: Added a cleanup script to remove build artifacts and temporary files.

### Changed

- **CSS layer organization**: Wrapped base styles in a CSS layer and removed redundant box-sizing and font-family declarations.
- **UI spacing consistency**: Refined padding and component styling across the dashboard and app chrome for a more cohesive layout.
- **Default config simplification**: Simplified default configuration structure and implemented Claude model ID validation with proper display name formatting.
- **Onboarding wizard**: Updated button visibility logic to reflect auto-generated config availability.

### Fixed

- **Release tag detection**: Fixed version tag detection by using full git fetch instead of shallow fetch.
- **Duplicate release prevention**: Removed workflow_dispatch bypass in the release workflow to prevent duplicate GitHub releases.

## [1.3.6] - 2026-04-05

### Added

- **Preset picker**: New `PresetPicker` component with categorized preset cards (bug-fix, refactor, feature, hardening), popover details, and inline preset selection for pair creation, onboarding, and task assignment flows.
- **Preset utilities**: New `presetUtils` module with category/icon/color mapping and `usePresets` hook for loading and selecting pair configuration presets.
- **Token usage in session snapshots**: Session snapshots now capture and persist token usage data for each pair run.
- **Expandable markdown in report exports**: Report exports now render markdown content in expandable sections for cleaner long-form output.

### Changed

- **Create Pair modal**: Simplified the preset selection flow — removed the preset/custom toggle and now show the `PresetPicker` directly.
- **Assign Task modal**: Removed the "Reuse this pair container" info card to reduce visual clutter.
- **Spec initialization**: Simplified spec initialization logic when selecting presets by removing legacy template stripping.

### Fixed

- **Preset duplication**: Selecting a preset no longer duplicates model configuration when re-selecting or switching presets.
- **Model dropdown light theme visibility**: Fixed text contrast in the model dropdown picker under light theme.

### Added

- **Timeline panel**: New timeline view showing pair run events (mentor plans, executor results, reviews, handoffs) with timestamps, durations, and token usage in a vertical timeline layout.
- **Run report export**: New report generation module for exporting pair run summaries and timelines.
- **Run generation counter**: Monotonically increasing generation counter per pair run prevents stale spawned tasks from emitting handoff events after their process was killed.

### Changed

- **Acceptance risk classification**: Simplified backend-only risk classification — any backend file change now qualifies as medium risk regardless of frontend presence.
- **Task assignment flow**: New runs now stop existing pair processes and reset the message broker session before starting, ensuring a clean state.
- **Assign Task modal**: Improved modal layout with scrollable content area and fixed action bar for longer task specs.
- **Process event filtering**: `tool_result` events are no longer filtered as noise in final event processing.

### Fixed

- **Acceptance verdict parsing**: Mentor review now handles verdict parse failures gracefully — pauses after repeated failures instead of hanging or proceeding with an invalid state.

## [1.3.4] - 2026-04-02

### Changed

- **Create Pair model picker layout**: Widened the Create New Pair modal and made the mentor/executor model cards stack on narrower widths so the picker UI has enough room instead of appearing squeezed.
- **Model option readability**: Increased spacing, padding, and text sizing across recent-model chips, the selected-model trigger, and dropdown search/results rows for a cleaner, easier-to-scan selection flow.

### Fixed

- **Duplicate-looking model labels**: Simplified secondary metadata in model dropdown rows so options no longer appear to repeat the model name beneath the primary title.
- **Cramped recent models in cards**: Switched card-mode recent models to a single-column layout so longer model names and provider labels no longer collide.
- **Release test runner**: Updated the JavaScript test script to load TypeScript tests through `tsx`, so local release checks and GitHub Actions can execute the existing `.ts` test suite instead of failing on file extension errors.

## [1.3.3] - 2026-04-02

### Changed

- **Console message classification**: Mentor acceptance verdicts are now recognized even when JSON is embedded in surrounding text, so archived turn cards and the live console consistently render review output as acceptance messages instead of generic plans.
- **Interactive affordances**: Buttons, modal backdrops, and picker controls now consistently show pointer cursors across the app for clearer click targets.

### Fixed

- **Paused handoff guard**: Handoff events are now ignored when a pair is already paused, preventing accidental extra turns after manual pauses or iteration-limit pauses.
- **Resume review fallback**: Restored review sessions now fall back to the task spec when no prior executor output is available, avoiding empty mentor review prompts.

## [1.3.2] - 2026-03-31

### Fixed

- **Cross-platform provider detection**: Fixed CLI tool discovery on Windows and macOS when launched from GUI contexts (Dock, Finder, Explorer). Previously only Codex was detected; Claude and OpenCode are now correctly discovered.
- **PATH fallback directories**: Added Windows npm install locations (`%APPDATA%\npm`, `%LOCALAPPDATA%\npm`) and Unix user bin directories (`~/.local/bin`, `~/.npm-global/bin`, `~/.volta/bin`, NVM version dirs) to PATH fallback list.
- **Binary discovery**: Added `.cmd`/`.exe`/`.bat` extension checks for Windows npm shims, enabling detection of `claude.cmd`, `opencode.cmd`, etc.
- **OpenCode auth path**: Fixed OpenCode auth.json path resolution on Windows to use `%APPDATA%\opencode\auth.json`.
- **Environment variable passthrough**: All CLI child processes now explicitly receive `HOME`, `USERPROFILE`, `APPDATA`, and `LOCALAPPDATA` environment variables, ensuring config files are discoverable regardless of GUI app environment isolation.
- **Claude credential fallback**: Added file-based auth detection for Claude using `~/.claude/.credentials.json` and Windows AppData paths when `claude auth status` subprocess fails.

### Changed

- **Dynamic model discovery**: Replaced hardcoded model catalogs with dynamic discovery from config files, CLI help output, and session history for all providers (Claude, Codex, Gemini, OpenCode).
- **Gemini event parsing**: Added structured text extraction for Gemini's `candidates`, `serverContent`, and `modelTurn` event formats.
- **Noise filtering**: Improved plain output filtering to skip punctuation-only protocol artifacts.

## [1.3.1] - 2026-03-31

### Added

- **Branch conflict detection**: Prevents creating pairs on branches already used by another active pair, eliminating worktree conflicts.
- **Worktree cleanup on failure**: If broker initialization fails during pair creation, the newly created worktree is automatically deleted to prevent orphaned directories.
- **BranchPicker "current" badge**: Visual indicator showing which branch is currently checked out in the repository.
- **Remote branch filtering**: Remote branches that have a corresponding local branch are now hidden from the BranchPicker to reduce clutter.
- **`extractErrorMessage` utility**: Centralized error message extraction helper in `utils.ts` for consistent error handling across the app.

### Fixed

- **Worktree creation for current branch**: Fixed worktree creation logic when selecting the currently checked-out branch; now uses `--detach` flag to avoid "git worktree add" failures.
- **Worktree deletion**: Fixed `delete_worktree` to run git commands from the parent repository instead of the worktree itself, resolving remove failures.
- **Local branch detection**: Fixed `ensure_local_tracking_branch` to correctly identify local vs remote branches and handle edge cases with branch names containing slashes.
- **Session snapshot recovery**: Fixed `build_process_context` to gracefully fall back to the original directory if the worktree path no longer exists.
- **Store error handling**: Improved error message extraction across `usePairStore` and `useUpdateStore` to handle non-Error objects consistently.

### Changed

- **Gitignore strategy**: Worktrees are now excluded via `.git/info/exclude` instead of `.gitignore` to avoid polluting the repository's tracked files.

## [1.3.0] - 2026-03-29

### Added

- **Reasoning effort controls**: Per-role reasoning effort picker (`ReasoningEffortPicker` component) integrated into model selection for both pair creation (`CreatePairModal`) and pair settings (`PairSettingsModal`). Supports low/medium/high for Claude and Codex o-series models, none/low/medium/high for Gemini 2.5/2.0-flash models.
- **Token usage tracking**: Live per-turn token usage (`TurnTokenUsage` type) extracted from provider JSON event streams for Claude, Codex, Gemini, and OpenCode. Output tokens, input tokens, provider source, and live/final status are tracked per agent turn.
- **Token chip display**: New `TokenChip` component showing live output token counts inline in the agent console during execution.
- **Update notification modal**: Dedicated `UpdateNotification` component replacing the inline updater controls with a standalone modal for update announcements.
- **Centralized update store**: New `useUpdateStore` (Zustand) managing update check, download, and install state, replacing the previous component-local state management.
- **Error boundary**: `ErrorBoundary` component wrapping the app to catch and gracefully display React render errors.
- **Sound feedback**: `playFinishChime()` utility that plays an audio chime when a pair reaches the `Finished` status.
- **Snapshot diff utility**: `shouldSaveSnapshot()` moved to dedicated `snapshotDiff.ts` module with unit tests, comparing status, turn, iteration, models, reasoning effort, and token usage changes.
- **Token usage utilities**: `tokenUsage.ts` module with `resolveCurrentTurnTokenUsage`, `syncTokenUsage`, and `turnCardToMessage` helpers, plus 275 lines of unit tests covering edge cases.
- **Compact ResourceMeter**: `ResourceMeter` component now supports `compact` and `hideLabels` props for use in space-constrained layouts.
- **usePrevious hook**: Generic `usePrevious<T>` utility for detecting value changes across renders.
- **Model catalog reasoning levels**: `AvailableModel` entries now include `reasoningEffortLevels` metadata so the frontend knows which models support reasoning effort configuration.
- **Provider reasoning effort passthrough**: Provider adapter passes `--reasoning-effort` flag to Claude and Codex CLI, `--thinking-budget` to Gemini CLI (mapped from effort levels), with full test coverage.

### Changed

- **Removed verification gate**: Deleted the entire `verification_gate.rs` backend module (668 lines), `VerificationGatePanel` component (265 lines), `verificationGate.ts` frontend library (473 lines), `ReleaseNotesModal`, and associated tests. The verification gate workflow has been replaced by simpler mentor review logic.
- **Simplified pair status machine**: Removed `Awaiting Human Review` status. The flow is now `Idle → Mentoring → Executing → Reviewing → (loop or Finished)`, with no separate human review gate.
- **Simplified session recovery**: `SessionRecoveryModal` no longer gates resume behind `canResume` status checks; sessions can always resume with a new task.
- **Streamlined dashboard cards**: Reduced pair cards from `min-h-[220px]` to `min-h-[140px]` with tighter spacing (9–10px text, 1.5px gaps). Active pair count shown inline with a pulsing badge in the heading.
- **Improved updater architecture**: `UpdateControls` rewritten from 154 lines of component-local state to a thin 34-line wrapper delegating to `useUpdateStore`, with event-driven check/install flow via Tauri events.
- **Enhanced process spawning**: `ProcessSpawner` now extracts token usage from provider-specific JSON event formats (Claude `result`/`content_block_delta` events, Codex `usage` objects, Gemini `usageMetadata`, OpenCode generic usage). Token usage is pushed to `MessageBroker` via new `update_token_usage`/`reset_token_usage` methods.
- **Session snapshot schema**: Snapshots now persist `mentor_reasoning_effort`, `executor_reasoning_effort`, per-turn `token_usage`, and run-level `total_output_tokens`. The `verification` field has been removed from all snapshot types.
- **Pair state persistence**: `Pair` objects now track `mentorReasoningEffort`, `executorReasoningEffort`, `mentorTokenUsage`, and `executorTokenUsage` instead of `verification` state.
- **Agent state tracking**: `AgentState` now carries an optional `tokenUsage` field. `Message` records include optional per-message `tokenUsage`.
- **Message broker**: Removed all verification-related methods (`set_verification_report`, verification verdict parsing, verification review/retry prompts). Added `update_token_usage` and `reset_token_usage` methods. Agent activity labels changed from "Awaiting verification verdict" to "Executor standing by" / "Checking the work".

### Fixed

- Fixed CI `test:rust` failures caused by uncommitted struct field additions (`mentor_reasoning_effort`, `executor_reasoning_effort`) that were referenced by `pair_manager.rs` but missing from `CreatePairInput`, `ProcessContext`, and `UpdatePairModelsInput`.
- Fixed reasoning effort not being passed through to provider CLI commands during agent turns.
- Fixed Gemini reasoning effort mapping: `none` maps to `--thinking-budget 0`, `low` to 1024, `medium` to 8192, `high` to 32768.

## [1.2.3] - 2026-03-28

### Added

- Added role-specific recent model tracking (separate for Mentor and Executor).
- Added verbose flag to Claude provider for enhanced debugging output.
- Added card variant to ModelPicker with role headers and subtitles.
- Added drop-up support for ModelPicker in constrained layouts.

### Changed

- Refactored ModelPicker to use native dropdown instead of modal for improved UX.
- Enhanced onboarding wizard with streamlined model selection cards.
- Updated visual design with refined color variables for light/dark themes.
- Improved model selection UI with better visual hierarchy and role differentiation.
- Migrated legacy recent models storage to role-specific keys.

### Fixed

- Fixed recent models list not being role-specific.
- Fixed ModelPicker layout issues in modal contexts.

## [1.2.2] - 2026-03-28

### Added

- Added resume functionality for paused pairs with intelligent state restoration that distinguishes between planning and review phases.
- Added DashboardEmptyState component with clear onboarding and visual explanation of Mentor/Executor roles.
- Added ErrorDetailPanel with actionable retry/discard options and expandable error details.
- Added IterationProgress indicator with visual warnings when approaching iteration limits.
- Added MessageFilterBar to filter console messages by role (All/Mentor/Executor) with message counts.
- Added ScrollToBottomButton that auto-appears when new console messages arrive.
- Added handoff guard to prevent race conditions when pairs finish.
- Added comprehensive unit tests for resume scenarios and handoff guards.

### Changed

- Improved resource meters to hide progress bars when usage is below 0.5%.
- Enhanced dashboard cards to line-clamp long specs to 3 lines.
- Refactored monitor spawning logic for better reusability.
- Improved accessibility with aria-labels and titles on interactive elements.

### Fixed

- Fixed mentor finish signal not being prioritized over verification turns.
- Fixed handoff events being processed after pair already finished.
- Fixed iteration count being incorrectly incremented on resume.
- Fixed resource meter showing empty bars at 0% usage.

## [1.2.1] - 2026-03-27

### Added

- Added an automated verification gate that runs workspace-specific checks and routes mentor verdicts through a strict JSON review loop.
- Added verification status chips and a dedicated gate panel so the dashboard, recovery modal, and task history can surface review progress at a glance.

### Changed

- Preserved verification state across snapshots, recoverable sessions, and archived run history.
- Refined onboarding with a compact layout, automatic pair-name suggestions from the selected workspace, and role-specific model defaults.
- Expanded the mentor and executor handoff prompts so verification retries keep iterating autonomously without dropping context.

### Fixed

- Tightened provider-specific output handling so Claude final results and stderr logs are treated separately from streaming noise.
- Kept legacy snapshots and recoverable session records readable after the verification state schema change.

## [1.2.0] - 2026-03-27

### Changed

- Expanded provider detection and model cataloging across OpenCode, Codex, Claude Code, and Gemini CLI.
- Reworked onboarding and model selection to validate actual ready models instead of only checking for an OpenCode config file.
- Refreshed provider-facing copy and model guidance throughout the app to match the new multi-provider flow.

### Fixed

- Corrected OpenCode config path resolution on Windows so the app reads and opens the right file.
- Improved fallback CLI discovery and added coverage for provider readiness, model preference resolution, and override handling.

## [1.1.22] - 2026-03-26

### Fixed

- Reduced metal sheen overlay opacity to improve contrast and readability
- Adjusted gradient color stops for a more refined metallic finish

## [1.1.21] - 2026-03-26

### Fixed

- Fixed model and subscription detection on Apple Silicon Macs where CLI tools (`claude`, `opencode`, `codex`) were not found when the app was launched from the Dock or Finder
- Added fallback binary detection that checks known install paths directly when the `which` command fails
- Fixed PATH setup to always include common binary directories even when login shell PATH capture is blocked by corporate security software (e.g. CyberArk EPM)
- Fixed OpenCode zen-backed models (`opencode/*` provider) being filtered out from the model list even when opencode is installed and authenticated

## [1.1.20] - 2026-03-26

### Changed

- Improved release workflow with early changelog validation to prevent build failures

## [1.1.19] - 2026-03-26

### Fixed

- Fixed React ref access errors in UpdateControls component by moving ref data to state

## [1.1.18] - 2026-03-26

### Added

- Added an in-app release notes modal so updater patch notes are readable without leaving the main window

### Changed

- Reworked the updater controls so the install action and release-notes affordance sit together in a compact action row

### Fixed

- Fixed the release notes modal so long changelogs scroll inside the dialog instead of being clipped
- Preserved GitHub-flavored Markdown rendering in release notes, including lists and tables

## [1.1.17] - 2026-03-25

### Fixed

- Fixed the Linux updater artifact mode so release manifests can find the `.AppImage.tar.gz.sig` file

## [1.1.16] - 2026-03-25

### Fixed

- Restored the updater private key password secret so the signed release pipeline can publish again

## [1.1.15] - 2026-03-25

### Fixed

- Rotated the updater keypair again to match the newly generated GitHub secret
- Restored the release pipeline after the updater signing validation exposed a malformed secret
- Updated the embedded updater public key to the current trust root

## [1.1.14] - 2026-03-25

### Fixed

- Added updater signing key validation to the release workflow so missing or mismatched secrets fail fast
- Clarified the updater signing secret requirements in the release docs and checklist
- Bumped the release version after restoring the signing secret flow

## [1.1.13] - 2026-03-25

### Fixed

- Forced the updater signing key prep step to use Bash on every release runner so Windows no longer parses POSIX shell syntax as PowerShell
- Rotated the updater signing keypair and updated the embedded updater public key so future signed releases use the new trust root

## [1.1.12] - 2026-03-25

### Changed

- Redesigned the earliest Powering Up boot splash with a colder steel gradient and stronger mirror-like sweep
- Made destructive session actions more legible in dark theme

### Fixed

- Documented the boundary between the pre-React boot splash and the post-hydration skeleton
- Kept file mention popovers anchored to the caret and outside modal stacking contexts

## [1.1.11] - 2026-03-25

### Fixed

- Fixed Windows release builds so the rustup wrapper preserves the PATH separator on each platform
- Split Tauri build entrypoints by platform and updated CI and release workflows to call the explicit macOS, Windows, and Linux commands

## [1.1.10] - 2026-03-25

### Fixed

- Installed Linux desktop build dependencies in the release workflow before Tauri builds
- Forced the cross-platform Tauri build step to use Bash so Windows runners can execute the release script correctly

## [1.1.9] - 2026-03-25

### Fixed

- Installed Linux desktop build dependencies in CI before Rust tests and Tauri builds
- Added `libglib2.0-dev` so Ubuntu runners can resolve the `glib-2.0` pkg-config metadata required by Tauri

## [1.1.8] - 2026-03-25

### Changed

- Added an explicit `build:mac:release` command for release-style macOS ZIP bundles
- Kept `build:mac` focused on the local DMG experience for easier manual installs
- Split the docs so GitHub Releases and local macOS builds describe their own bundle formats

## [1.1.4] - 2026-03-25

### Changed

- Added a concrete release checklist for the GitHub Actions publish flow
- Updated package metadata and release hygiene for public-source distribution
- Wired the default test command to run both JavaScript and Rust unit tests

### Fixed

- Added real unit coverage for renderer helpers and Rust core modules
- Improved process output collapsing so repeated final snapshots are deduplicated

## [1.0.1] - 2025-03-23

### Changed

- Improved README with clearer value proposition
- Added "Why Two Agents?" section explaining dual-model cross-validation benefits
- Updated tagline to emphasize automated pair programming with coffee break messaging

## [1.0.0] - 2025-03-22

### Added

- Initial public release
- Dual-agent architecture with Mentor and Executor roles
- Real-time CPU/memory monitoring per agent
- Git change tracking for all session modifications
- Full automation mode with workspace-scoped permissions
- Human oversight with approval/rejection workflow
- Cross-platform support (macOS, Windows, Linux)
- Code signing and notarization for macOS builds
- Dark/light theme support
- Model picker with automatic provider detection
- Onboarding wizard for first-time users
- File mention support for context injection

### Security

- Session-specific permissions (no global opencode config modification)
- Workspace-scoped file system access
- Secure handling of API keys via opencode configuration

[2.2.0]: https://github.com/timwuhaotian/the-pair/compare/v2.1.0...v2.2.0
[2.1.0]: https://github.com/timwuhaotian/the-pair/compare/v2.0.3...v2.1.0
[2.0.3]: https://github.com/timwuhaotian/the-pair/compare/v2.0.2...v2.0.3
[2.0.2]: https://github.com/timwuhaotian/the-pair/compare/v2.0.1...v2.0.2
[2.0.1]: https://github.com/timwuhaotian/the-pair/compare/v2.0.0...v2.0.1
[2.0.0]: https://github.com/timwuhaotian/the-pair/compare/v1.4.5...v2.0.0
[1.4.5]: https://github.com/timwuhaotian/the-pair/compare/v1.4.4...v1.4.5
[1.4.4]: https://github.com/timwuhaotian/the-pair/compare/v1.4.3...v1.4.4
[1.4.3]: https://github.com/timwuhaotian/the-pair/compare/v1.4.2...v1.4.3
[1.4.2]: https://github.com/timwuhaotian/the-pair/compare/v1.4.1...v1.4.2
[1.4.1]: https://github.com/timwuhaotian/the-pair/compare/v1.4.0...v1.4.1
[1.4.0]: https://github.com/timwuhaotian/the-pair/compare/v1.3.14...v1.4.0
[1.3.14]: https://github.com/timwuhaotian/the-pair/compare/v1.3.13...v1.3.14
[1.3.13]: https://github.com/timwuhaotian/the-pair/compare/v1.3.12...v1.3.13
[1.3.12]: https://github.com/timwuhaotian/the-pair/compare/v1.3.10...v1.3.12
[1.3.10]: https://github.com/timwuhaotian/the-pair/compare/v1.3.9...v1.3.10
[1.3.9]: https://github.com/timwuhaotian/the-pair/compare/v1.3.8...v1.3.9
[1.3.8]: https://github.com/timwuhaotian/the-pair/compare/v1.3.7...v1.3.8
[1.3.7]: https://github.com/timwuhaotian/the-pair/compare/v1.3.6...v1.3.7
[1.3.0]: https://github.com/timwuhaotian/the-pair/compare/v1.2.3...v1.3.0
[1.2.3]: https://github.com/timwuhaotian/the-pair/compare/v1.2.2...v1.2.3
[1.2.2]: https://github.com/timwuhaotian/the-pair/compare/v1.2.1...v1.2.2
[1.2.1]: https://github.com/timwuhaotian/the-pair/compare/v1.2.0...v1.2.1
[1.2.0]: https://github.com/timwuhaotian/the-pair/compare/v1.1.22...v1.2.0
[1.1.17]: https://github.com/timwuhaotian/the-pair/compare/v1.1.16...v1.1.17
[1.1.10]: https://github.com/timwuhaotian/the-pair/compare/v1.1.9...v1.1.10
[1.1.15]: https://github.com/timwuhaotian/the-pair/compare/v1.1.14...v1.1.15
[1.1.14]: https://github.com/timwuhaotian/the-pair/compare/v1.1.13...v1.1.14
[1.1.12]: https://github.com/timwuhaotian/the-pair/compare/v1.1.11...v1.1.12
[1.1.11]: https://github.com/timwuhaotian/the-pair/compare/v1.1.10...v1.1.11
[1.1.13]: https://github.com/timwuhaotian/the-pair/compare/v1.1.12...v1.1.13
[1.1.9]: https://github.com/timwuhaotian/the-pair/compare/v1.1.8...v1.1.9
[1.1.8]: https://github.com/timwuhaotian/the-pair/compare/v1.1.7...v1.1.8
[1.1.7]: https://github.com/timwuhaotian/the-pair/compare/v1.1.6...v1.1.7
[1.1.4]: https://github.com/timwuhaotian/the-pair/compare/v1.0.1...v1.1.4
[1.0.1]: https://github.com/timwuhaotian/the-pair/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/timwuhaotian/the-pair/releases/tag/v1.0.0
