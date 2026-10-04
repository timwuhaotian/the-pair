# 2026-10-04 — Pair Commit & Push

Approved design: Option A — Commit + Push buttons in the pair's Modified Files
panel. Push allowed on any branch; the button shows the branch name.

## Backend (`src-tauri/src/`)

Testable plain functions in `git_tracker.rs`; thin `#[tauri::command]` shells
in `lib.rs` registered in `invoke_handler` (shells run via `spawn_blocking`).

1. `commit_changes(directory, message) -> Result<CommitOutcome, String>`
   - `CommitOutcome { sha: String, files_committed: usize }`.
   - Validate: `message.trim()` non-empty (else `"Commit message is empty"`),
     cap at 10_000 chars.
   - Stage: `git add -A` with the same regenerable-dir excludes the tracker
     uses (`exclude_pathspec`), so untracked `node_modules/` etc. never get
     committed. Reuse `collect_modified_files` first: empty list →
     `Err("Nothing to commit")`.
   - `git commit -m <message>` with `--no-verify`? No — leave hooks enabled.
     On failure return stderr trimmed (covers bad identity → "author identity
     unknown", empty after excludes, locks).
   - Return short SHA via `git rev-parse --short HEAD`.
2. `push_changes(directory, remote = "origin") -> Result<PushOutcome, String>`
   - `PushOutcome { branch: String, remote: String, up_to_date: bool }`.
   - Resolve branch via `git branch --show-current`; empty (detached HEAD) →
     `Err("Cannot push a detached HEAD — ...")`.
   - `git push -u <remote> <branch>`; treat "Everything up-to-date" /
     "up to date" output as `up_to_date: true` (success).
   - Auth/no-remote failures surface stderr verbatim.
3. Commands `git_commit_changes { directory, message }` and
   `git_push_changes { directory }`, both `Result<JsonOutcome, String>`.
4. Rust tests (TempRepo pattern from `git_tracker.rs` tests):
   - commit stages tracked edits + new files, skips untracked regenerable
     dirs, returns a SHA; empty message / nothing-to-commit errors.
   - push to a local `--bare` remote sets upstream; second push reports
     up-to-date; detached HEAD errors; missing remote errors.

## Frontend (`src/renderer/src/`)

5. `lib/tauri-api.ts`: `repo.commitChanges(directory, message)` →
   `invoke('git_commit_changes', …)`, `repo.pushChanges(directory)` →
   `invoke('git_push_changes', …)`. No mock fallback (like `getFileDiff`).
   `env.d.ts`: add both to `window.api.repo`.
6. `PairOperationsPanel.tsx` Modified Files section:
   - Commit: message `<input>` (prefill `the-pair: <task summary>` — first
     60 chars of `pair.currentTurnCard?.content`? fall back to empty
     placeholder), Commit button (`data-testid="ops-commit-btn"`). Disabled
     when no modified files or message blank. Shows inline success (short
     SHA) / error, auto-clears like `saveStatus`.
   - Push button (`data-testid="ops-push-btn"`) labeled with branch
     (`pair.worktreeBranch ?? pair.branch`); hidden when neither is known.
     Inline up-to-date / pushed / error states.
   - After a successful commit, the file list refreshes on the next
     `pair:state` emit (tracker re-polls); no manual store mutation.
   - Tailwind + `cn()`, `GlassButton`, lucide icons (`GitCommitHorizontal`,
     `CloudUpload` — verify names exist in installed lucide-react).
7. i18n keys under `pair.*` in all four locales (`en/zh/ja/ko`):
   `commitMessage`, `commitPlaceholder`, `commit`, `push`, `pushTo`,
   `nothingToCommit`, `commitFailed`, `pushFailed`, `committed`,
   `pushedUpToDate`, `detachedHead`, `noBranchToPush`.
8. `tauri-shim` check: if the shim enumerates repo methods, wire the two new
   ones (check `src/renderer/src/tauri-shim.ts`).

## Tests

9. `tests/tauriApi.test.ts`-adjacent new test file asserting the new wrappers
   call the right commands (follow existing mock-IPC style).
10. New `tests/render/pairCommitPush.test.tsx`: render ops panel with modified
    files → commit button invokes `git_commit_changes` with directory+message;
    push button invokes `git_push_changes`; error renders inline. Extend
    `tests/render/mock-ipc.ts` with the two commands (scriptable results).
11. `e2e-web/mock-init.js`: add the two commands returning canned outcomes so
    headless e2e keeps passing; extend `e2e-web/specs/worktree.spec.ts`-style
    coverage only if a natural hook exists (no new spec file required).
12. Coverage: 95%+ of changed lines (`npm run coverage:ts`,
    `npm run coverage:rust`).

## Commands per step

- Rust: `npm run test:rust` (fast subset: `cargo test --manifest-path
src-tauri/Cargo.toml git_tracker`).
- JS: `npm run test:js`; UI: `npm run test:ui`; types: `npm run typecheck`.
- Full gate before push: `npm test && npm run typecheck && npm run lint`.
- Commit style: `feat:`, small frequent commits in the worktree.
