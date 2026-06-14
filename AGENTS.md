# AI Agent Guidelines for "The Pair"

This file contains instructions and context for any AI agents (like yourself) working on the `the-pair` repository.

## Project Identity

"The Pair" is a Desktop application (v1.2.3) built with Tauri 2.x, React 19, and TypeScript. It orchestrates two local AI agents — a **Mentor** (planner/reviewer, read-only) and an **Executor** (code writer/command runner) — that cross-check each other's work. The app manages their lifecycle, monitors resources, tracks git changes, and provides a polished UI for humans to observe and intervene.

## Tech Stack

- **Framework:** Tauri 2.0
- **Backend:** Rust
- **Frontend:** React 19, TypeScript
- **Styling:** Tailwind CSS v4, `clsx`, `tailwind-merge`
- **Icons:** `lucide-react`
- **State Management:** `zustand` (`usePairStore`, `useUpdateStore`, `useThemeStore`)
- **Animations:** Framer Motion

## Architecture Rules

1. **Rust Backend vs. React Frontend:**
   - Never import Node.js built-ins directly into the Renderer (`src/renderer/src/`).
   - Use Tauri commands in `src-tauri/src/` to expose strict APIs to the frontend.
   - Frontend calls backend via `invoke()` from `@tauri-apps/api/core`.
2. **Styling:**
   - Always use Tailwind CSS classes. Custom dark/light mode palette is defined in `src/renderer/src/assets/main.css`.
   - Use the `cn()` utility from `src/renderer/src/lib/utils.ts` for conditional class merging.
3. **Agent Interactions:**
   - The application spawns processes. Always implement an iteration limit (`maxIterations`) before pausing for human intervention.
   - Use the handoff guard (`src/renderer/src/lib/handoffGuard.ts`) to prevent race conditions when pairs finish.
4. **Error Handling:**
   - Handle permissions and locked files defensively in the Rust backend.
   - Propagate errors cleanly to the frontend via Tauri commands to display in the `ErrorDetailPanel`.

## Rust Backend Modules (`src-tauri/src/`)

| Module              | Responsibility                                                                                                                                                              |
| ------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pair_manager`      | Pair lifecycle: create, list, delete, pause, resume, assign task, update models                                                                                             |
| `message_broker`    | State machine for agent turn coordination and event routing                                                                                                                 |
| `process_spawner`   | Spawns CLI processes, parses JSON event streams; delegates to provider trait for extraction                                                                                 |
| `provider_adapter`  | Facade over the provider trait; legacy compatibility shim for `ProviderAdapter::build_turn_command()` etc.                                                                  |
| `provider_registry` | `ProviderKind` enum, shared helpers (`which_binary`, `collect_*`), model discovery utilities                                                                                |
| `providers`         | **Provider trait + per-provider modules** (`opencode.rs`, `codex.rs`, `claude.rs`, `gemini.rs`). Each implements CLI args, token extraction, detection, and model metadata. |
| `model_catalog`     | Static model metadata (display names, billing kind, recommended roles); delegates to provider trait for per-provider fields                                                 |
| `session_snapshot`  | Persists and restores full pair state; supports session recovery after crash/restart                                                                                        |
| `skill_discovery`   | Scans project dirs for `.md` skill files with YAML frontmatter                                                                                                              |
| `resource_monitor`  | Per-agent CPU/memory polling (1s interval)                                                                                                                                  |
| `git_tracker`       | Detects modified/added/deleted files relative to a baseline commit                                                                                                          |
| `file_cache`        | Lists files and parses `@mention` references in task specs                                                                                                                  |
| `path_env`          | Refreshes `$PATH` from login shell so CLI tools are discoverable                                                                                                            |
| `config_paths`      | Resolves platform-specific config file locations                                                                                                                            |
| `stubs`             | Placeholder Tauri commands not yet fully implemented                                                                                                                        |

## Frontend Components (`src/renderer/src/components/`)

| Component                                       | Purpose                                                                                            |
| ----------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `App.tsx`                                       | Root: routing between dashboard and pair detail views                                              |
| `AppChrome.tsx`                                 | Window chrome, title bar, theme toggle                                                             |
| `OnboardingWizard.tsx`                          | First-run guided setup (model config, directory selection)                                         |
| `CreatePairModal.tsx`                           | New pair creation form                                                                             |
| `PairSettingsModal.tsx`                         | Edit existing pair settings                                                                        |
| `AssignTaskModal.tsx`                           | Assign a new task to an existing pair                                                              |
| `ModelPicker.tsx`                               | Model selection dropdown with role headers, drop-up support, recent model tracking (role-specific) |
| `SkillPicker.tsx`                               | Browse and attach skill files to a task                                                            |
| `SessionRecoveryModal.tsx`                      | Restore interrupted sessions                                                                       |
| `TaskHistoryPanel.tsx`                          | View past run history for a pair                                                                   |
| `MessageFilterBar.tsx`                          | Filter console messages by role (All/Mentor/Executor)                                              |
| `ScrollToBottomButton.tsx`                      | Auto-appears when new messages arrive                                                              |
| `IterationProgress.tsx`                         | Visual indicator with warning when approaching iteration limit                                     |
| `ErrorDetailPanel.tsx`                          | Actionable error display with retry/discard options                                                |
| `DashboardEmptyState.tsx`                       | Empty state with onboarding explanation                                                            |
| `FileMention.tsx`                               | Renders `@file` mentions in messages                                                               |
| `StatusBadge.tsx`                               | Pair status pill (Idle/Mentoring/Executing/Reviewing/Paused/Error/Finished)                        |
| `UpdateNotification.tsx` / `UpdateControls.tsx` | In-app updater UI                                                                                  |

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

Four provider kinds are supported: `opencode`, `codex` (OpenAI Codex CLI), `claude` (Claude Code CLI), `gemini` (Gemini CLI). Each is implemented as a `Provider` trait in its own module under `src-tauri/src/providers/`.

### Adding a New Provider

1. Create `src-tauri/src/providers/new_provider.rs` implementing the `Provider` trait
2. Add `pub mod new_provider;` to `src-tauri/src/providers/mod.rs`
3. Add `Arc::new(new_provider::NewProvider)` to `all_providers()` in `mod.rs`
4. Add the variant to `ProviderKind` in `provider_registry.rs`
5. Add the variant to the TypeScript `ProviderKind` union in `src/renderer/src/types.ts`
6. Update `inferProviderFromModel()` in `providerResolution.ts` (frontend)
7. Update `PROVIDER_PRIORITY` in `modelCatalogGrouping.ts` (frontend)

## Adding Features

- **UI Components:** Keep components modular in `src/renderer/src/components/`. Reusable primitives go in `components/ui/`.
- **State:** Use `usePairStore` for pair-related global state. Add new stores in `src/renderer/src/store/` only if the concern is orthogonal.
- **Tauri Commands:** Add new commands to the appropriate Rust module and register them in `lib.rs`'s `invoke_handler`.
- **Models:** Update `model_catalog.rs` when adding new model entries; implement the `Provider` trait in `providers/` for new CLI tools.

## Release Process (Automated)

**⚠️ CRITICAL: Never manually create or push git tags!**

The release workflow (`build-signed-mac.yml`) is fully automated:

1. **Prepare release:**
   - Update version in `package.json` using `npm run bump <version>`
   - Update `CHANGELOG.md` with release notes
   - Run quality gates locally: `npm test && npm run typecheck && npm run lint`

2. **Publish:**
   - Commit changes: `git commit -m "chore: bump version to X.Y.Z"`
   - Push to main: `git push`
   - **Do NOT run `git tag` or `git push --tags`**

3. **Workflow automation:**
   - GitHub Actions detects version bump
   - Auto-creates tag `vX.Y.Z`
   - Builds macOS, Windows, Linux binaries
   - Publishes GitHub release with changelog
   - Uploads signed artifacts

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
