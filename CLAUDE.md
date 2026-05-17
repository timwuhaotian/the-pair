# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`AGENTS.md` contains the longer architectural reference (module table, component table, state machine, release process). Read it when you need that depth; this file covers the essentials for getting productive fast.

## What this app is

The Pair is a Tauri 2 desktop app (Rust backend, React 19 + TypeScript frontend) that orchestrates two AI agents working together on a coding task:

- **Mentor** — read-only planner/reviewer
- **Executor** — code writer / command runner

The Rust backend spawns local provider CLIs (`opencode`, `claude`, `codex`, `gemini`), parses their JSON event streams, runs a turn-based state machine, monitors process resources, and tracks git changes. The React frontend renders the conversation, status, and controls.

## Commands

```bash
# Setup
npm install
npm run preflight              # Verify rust/node/CLI tools are present

# Dev (runs Tauri shell + Vite renderer on :5173)
npm run dev                    # Uses scripts/run-with-rustup.mjs to inject PATH
npm run dev:mock               # Same, with THE_PAIR_E2E_MOCK=true (no real CLI spawns)
npm run dev:renderer           # Renderer-only (browser at :5173, no Tauri APIs)

# Quality gates (run before committing)
npm run lint                   # eslint --cache
npm run typecheck              # Runs both typecheck:node and typecheck:web
npm test                       # JS tests then `cargo test` for src-tauri
npm run test:js                # node --test against tests/*.test.ts (tsx loader)
npm run test:rust              # cargo test in src-tauri

# Run a single JS test file
node --import tsx --test tests/handoffGuard.test.ts

# Run a single Rust test
cargo test -q --manifest-path src-tauri/Cargo.toml <test_name>

# Builds (each runs preflight + ensures rust targets first)
npm run build:mac              # universal-apple-darwin DMG
npm run build:mac:release      # Release ZIP bundle used in GitHub Releases
npm run build:win              # x86_64-pc-windows-msvc
npm run build:linux            # x86_64-unknown-linux-gnu

# E2E (Appium + WebdriverIO, macOS only, mock mode)
npm run e2e:setup              # One-time: appium driver install mac2
npm run e2e

# Release
npm run bump <version>         # Updates package.json + Cargo.toml + tauri.conf.json
npm run validate:changelog
```

`npm test` does **not** invoke the renderer or run e2e — `tests/*.test.ts` are pure-Node unit tests covering lib utilities, store logic, and build scripts. Renderer components are tested by reasoning about pure helpers extracted out of them (see `tests/dashboardPairs.test.ts`, `tests/pairListSection.test.ts`).

## Architecture in 60 seconds

```
src/renderer/src/          React 19 frontend
  components/              UI (PascalCase.tsx); primitives live in components/ui/
  store/                   Zustand stores: usePairStore, useUpdateStore, useThemeStore, useLocaleStore
  lib/                     Pure helpers — testable without React. tauri-api.ts wraps invoke()
  hooks/                   Cross-component React hooks
  locales/  i18n.ts        i18next setup (en/zh/ja/ko)
  assets/main.css          Tailwind v4 + theme tokens (light/dark)

src-tauri/src/             Rust backend (see AGENTS.md for the full module table)
  lib.rs                   Registers every Tauri command in invoke_handler![]
  pair_manager.rs          Pair CRUD + task assignment
  message_broker.rs        Turn-coordination state machine
  process_spawner.rs       Spawns provider CLIs, parses JSON event streams
  provider_adapter.rs      Per-provider Input/Output/Session/Permission/Cwd strategies
  provider_registry.rs     Detect installed CLIs + their model catalogs
  session_snapshot.rs      Persist/restore full pair state
  resource_monitor.rs      Per-agent CPU/memory polling
  git_tracker.rs           Diff vs baseline commit
  recent_activity.rs       Recent activity feed shown on the dashboard

tests/                     Node --test unit tests (TypeScript via tsx)
e2e/                       Appium + WDIO end-to-end specs (macOS, mock mode)
scripts/                   Node-based build/release tooling
```

### How frontend talks to backend

Always go through `src/renderer/src/lib/tauri-api.ts`, which wraps `invoke()` from `@tauri-apps/api/core` with typed helpers. The renderer never imports Node built-ins; the only escape hatch is `tauri-shim.ts`, which provides safe fallbacks when running under `npm run dev:renderer` (no Tauri host).

### Adding a Tauri command

1. Implement `#[tauri::command]` in the appropriate `src-tauri/src/<module>.rs`.
2. Add the function to the `tauri::generate_handler![]` list in `src-tauri/src/lib.rs`.
3. Add a typed wrapper in `src/renderer/src/lib/tauri-api.ts`.
4. Cover with a unit test in `tests/` (pure logic) and/or a Rust test in the module file.

### State machine

`Idle → Mentoring → Executing → Reviewing → (loop | Finished | Paused | Awaiting Human Review | Error)`

Pairs hit an `iteration_limit` and pause for human review rather than running unbounded. `src/renderer/src/lib/handoffGuard.ts` prevents duplicate handoff events firing after a pair finishes — keep using it when adding new transition logic.

### Provider support

Four kinds: `opencode`, `codex` (OpenAI), `claude` (Claude Code), `gemini`. Each has its own transport / session / permission / cwd strategy in `provider_adapter.rs`. Detection logic and model catalogs live in `provider_registry.rs` and `model_catalog.rs`. When adding provider features, update both.

## Conventions

- **TypeScript only** in the renderer — no `.js` files, no `any`. Use `interface` for object shapes, export shared types from a local `types.ts`.
- **Tailwind v4 only** for styling — no inline styles, no CSS modules. Use `cn()` from `src/renderer/src/lib/utils.ts` to merge classes. Theme tokens are defined in `src/renderer/src/assets/main.css`.
- **Zustand** for global state; component-level state via hooks. Keep stores minimal — derive computed values rather than caching them.
- **Path aliases**: `@renderer/*` and `@/*` both map to `src/renderer/src/*` (see `vite.config.ts`).
- **Conventional Commits** for messages (`feat`, `fix`, `chore`, `docs`, `refactor`, `test`, `style`). PRs are squash-merged.
- **Husky + lint-staged** run `eslint --fix` and `prettier --write` on staged files on commit. Do not use `--no-verify` unless explicitly asked.

## Mock mode

Set `THE_PAIR_E2E_MOCK=true` (e.g. via `npm run dev:mock`) to short-circuit real CLI process spawning. `THE_PAIR_E2E_MOCK_SCENARIO=dev-smoke` seeds a richer mock dataset. This is the path used by e2e specs and is useful when iterating on UI without provider CLIs installed.

## Releases — DO NOT push tags manually

GitHub Actions (`build-signed-mac.yml`) auto-tags and publishes when a version bump lands on `main`. The full flow:

1. `npm run bump <version>` (updates `package.json`, `Cargo.toml`, `tauri.conf.json` in lock-step)
2. Update `CHANGELOG.md`
3. Run `npm test && npm run typecheck && npm run lint && npm run build`
4. `git commit -m "chore: bump version to X.Y.Z"` and `git push` — that's it

Never run `git tag` or `git push --tags`. Fallback if the workflow misses the bump: `gh workflow run build-signed-mac.yml`. See `docs/RELEASE_CHECKLIST.md`.
