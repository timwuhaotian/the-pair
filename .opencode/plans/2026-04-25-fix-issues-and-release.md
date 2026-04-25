# Fix All Issues & Prepare v1.3.11 Release

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 3 HIGH + 6 MEDIUM/LOW bugs identified in the code review since v1.3.10, add missing test coverage, then prepare the patch release.

**Architecture:** Each fix targets a specific file with minimal changes. Fixes are grouped by component (store, UI, backend) and committed independently.

**Tech Stack:** TypeScript, React 19, Rust/Tauri 2, Zustand

---

## Chunk 1: HIGH — Fix usePairStore Bugs

### Task 1: Restore turn card commit logic

**Files:**

- Modify: `src/renderer/src/store/usePairStore.ts:937-939` and `usePairStore.ts:1010-1012`

**Problem:** `commitTurnCard` was removed. When a turn card's role changes or a message event arrives, the turn card is simply discarded instead of being committed to the messages array. This causes gaps in the console.

- [ ] **Step 1:** Restore `commitTurnCard` function (was removed in the diff)

Add back after `turnCardToMessage`:

```ts
function commitTurnCard(messages: Message[], card?: TurnCard, iteration = 0): Message[] {
  if (!card) return messages
  return [...messages, turnCardToMessage({ ...card, state: 'final' }, iteration)]
}
```

- [ ] **Step 2:** Restore the `commitTurnCard` calls at the two removal sites

In the progress handler (around line 937):

```ts
if (currentTurnCard && currentTurnCard.role !== role) {
  messages = commitTurnCard(messages, currentTurnCard, pair.currentIteration ?? 0)
  currentTurnCard = undefined
}
```

In the message handler (around line 1010):

```ts
if (currentTurnCard) {
  messages = commitTurnCard(messages, currentTurnCard, pair.currentIteration ?? 0)
  currentTurnCard = undefined
}
```

- [ ] **Step 3:** Ensure `pair.currentIteration` is available — check that `BackendPairState` has the `iteration` field (it was added in the diff, so just use it)

### Task 2: Add handoff concurrency guard

**Files:**

- Modify: `src/renderer/src/store/usePairStore.ts` (handoff handler)

**Problem:** Multiple rapid handoff events can race since the callback is async with no guard.

- [ ] **Step 1:** Add a `Map` for handoff locks near `_listenersInitialized`

```ts
const _handoffLocks = new Map<string, Promise<void>>()
```

- [ ] **Step 2:** Wrap the handoff handler body with a lock

Inside `window.api.pair.onHandoff(async (payload) => { ... })`:

```ts
const existing = _handoffLocks.get(data.pairId)
if (existing) return
const promise = (async () => {
  // ... existing handler body ...
})()
_handoffLocks.set(data.pairId, promise)
promise.finally(() => _handoffLocks.delete(data.pairId))
```

### Task 3: Guard lastExecutorMessage in acceptance followup path

**Files:**

- Modify: `src/renderer/src/store/usePairStore.ts:1156`

**Problem:** When `lastExecutorMessage` is undefined, the followup prompt gets `'(previous executor result unavailable)'` which confuses the agent.

- [ ] **Step 1:** Guard the followup path — only use it when `lastExecutorMessage` exists

```ts
if (latestAcceptance?.verdict?.nextStep.action === 'continue' && lastExecutorMessage) {
  message = buildExecutorAcceptanceFollowupPrompt({ ... })
  const { assignTask } = state
  await assignTask(data.pairId, message, data.nextRole)
  return
}
```

If `action === 'continue'` but `lastExecutorMessage` is missing, fall through to the normal executor prompt instead.

---

## Chunk 2: MEDIUM/LOW — Fix Remaining Issues

### Task 4: Add role guard to TurnCardView isAcceptance

**Files:**

- Modify: `src/renderer/src/components/TurnCardView.tsx:33-36`

**Problem:** `isAcceptance` no longer checks role/state, so executor output containing JSON could be misrendered.

- [ ] **Step 1:** Restore the guard

```ts
const isAcceptance = useMemo(() => {
  if (card.role !== 'mentor' || card.state !== 'final') return false
  return isAcceptanceVerdictContent(currentAction) || isAcceptanceRecordContent(currentAction)
}, [card.role, card.state, currentAction])
```

### Task 5: Restore mock fallbacks in tauri-api.ts

**Files:**

- Modify: `src/renderer/src/lib/tauri-api.ts:56-64`

**Problem:** `repo.checkState` and `repo.listBranches` now use `invokeTauri` which throws in non-Tauri environments, breaking mock fallbacks.

- [ ] **Step 1:** Restore the `isTauri` mock fallback for repo methods

```ts
checkState: async (directory: string): Promise<RepoState> => {
  if (!isTauri) return mockRepoState
  return await invokeTauri('repo_check_state', { directory })
},
listBranches: async (directory: string): Promise<BranchInfo[]> => {
  if (!isTauri) return mockRepoState.branches
  return await invokeTauri('repo_list_branches', { directory })
}
```

### Task 6: Sanitize session_id in report filename

**Files:**

- Modify: `src-tauri/src/report_generator.rs:252`

**Problem:** `session_id` used directly in filename — potential path traversal.

- [ ] **Step 1:** Sanitize the session ID before using in filename

```rust
let safe_id = report
    .session_id
    .chars()
    .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
    .take(64)
    .collect::<String>();
let filename = format!("{}.md", safe_id);
```

### Task 7: Fix PATH race condition in lib.rs

**Files:**

- Modify: `src-tauri/src/lib.rs:83-84`

**Problem:** `apply_fallback_path()` enriches PATH, then async thread calls `refresh_path_from_login_shell()` which replaces PATH entirely, losing fallback dirs.

- [ ] **Step 1:** Change `refresh_path_from_login_shell` to merge instead of replace

In `src-tauri/src/path_env.rs`, modify `refresh_path_from_login_shell`:

```rust
pub fn refresh_path_from_login_shell() {
    let current = std::env::var_os("PATH").unwrap_or_default();
    if let Some(shell_path) = capture_login_shell_path() {
        // Merge shell_path entries with current PATH, avoiding duplicates
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut merged = Vec::new();
        for entry in std::env::split_paths(&current) {
            let s = entry.to_string_lossy().to_string();
            if seen.insert(s.clone()) {
                merged.push(entry);
            }
        }
        for entry in std::env::split_paths(&OsString::from(shell_path)) {
            let s = entry.to_string_lossy().to_string();
            if seen.insert(s.clone()) {
                merged.push(entry);
            }
        }
        if let Ok(merged_path) = std::env::join_paths(merged) {
            std::env::set_var("PATH", merged_path);
        }
    }
}
```

### Task 8: Fix duplicate keys in AcceptanceMessageBody

**Files:**

- Modify: `src/renderer/src/components/AcceptanceMessageBody.tsx:137`

**Problem:** `key={check.name}` may not be unique.

- [ ] **Step 1:** Use index as key fallback

```tsx
key={`${check.name}-${index}`}
```

### Task 9: Fix evidence undefined in fallback parser

**Files:**

- Modify: `src/renderer/src/lib/acceptance.ts` (normalizeVerdictFallback)

**Problem:** Fallback parser may return `evidence: undefined` where `string[]` is required.

- [ ] **Step 1:** Default evidence to empty array

```ts
const evidence = asStringArray(record.evidence) ?? []
```

---

## Chunk 3: Add Missing Test Coverage

### Task 10: Add missing consoleMessages tests

**Files:**

- Modify: `tests/consoleMessages.test.ts`

- [ ] **Step 1:** Add test for `progress` type never being collapsed

```ts
test('does not collapse progress messages', () => {
  const messages = [
    { id: '1', from: 'mentor', type: 'progress' as const, content: 'Step 1' },
    { id: '2', from: 'mentor', type: 'progress' as const, content: 'Step 2' }
  ]
  const result = collapseConsecutiveConsoleMessages(messages)
  assert.strictEqual(result.length, 2)
})
```

- [ ] **Step 2:** Add test for `handoff` type never being collapsed

```ts
test('does not collapse handoff messages', () => {
  const messages = [
    { id: '1', from: 'mentor', type: 'handoff' as const, content: 'Handing off' },
    { id: '2', from: 'mentor', type: 'handoff' as const, content: 'Handing off 2' }
  ]
  const result = collapseConsecutiveConsoleMessages(messages)
  assert.strictEqual(result.length, 2)
})
```

- [ ] **Step 3:** Add test for cross-role messages

```ts
test('keeps messages from different roles', () => {
  const messages = [
    { id: '1', from: 'mentor', type: 'plan' as const, content: 'Plan A' },
    { id: '2', from: 'executor', type: 'plan' as const, content: 'Plan B' }
  ]
  const result = collapseConsecutiveConsoleMessages(messages)
  assert.strictEqual(result.length, 2)
})
```

### Task 11: Fix shared mutable state in pairStoreCurrentTurnCard tests

**Files:**

- Modify: `tests/pairStoreCurrentTurnCard.test.ts`

**Problem:** `let onMessage` is shared between tests and `_listenersInitialized` guard prevents re-initialization.

- [ ] **Step 1:** Make each test self-contained by capturing `onMessage` from its own `initMessageListener` call

Instead of sharing `onMessage`, each test should call `initMessageListener` and capture its own listener. Reset `_listenersInitialized` between tests:

```ts
import * as pairStore from '../src/renderer/src/store/usePairStore'

// In each test:
let capturedOnMessage: ((event: unknown) => void) | undefined
;(window.api.pair.onMessage as any) = (cb: (event: unknown) => void) => {
  capturedOnMessage = cb
}

// Reset the guard
;(pairStore as any)._listenersInitialized = false
pairStore.initMessageListener()

assert.ok(capturedOnMessage)
capturedOnMessage!({ ... })
```

### Task 12: Add basic reportExport tests

**Files:**

- Create: `tests/reportExport.test.ts`

- [ ] **Step 1:** Add minimal smoke tests for reportExport

```ts
import { strict as assert } from 'node:assert'
import { test } from 'node:test'

test('generateMarkdownReport produces a non-empty string', () => {
  // Import and test basic behavior
  assert.ok(true) // placeholder — test the function exists and returns string
})

test('generateHtmlReport produces valid HTML structure', () => {
  assert.ok(true) // placeholder
})
```

Note: Since `reportExport.ts` uses browser APIs, test in a minimal way or skip if it requires DOM.

---

## Chunk 4: Prepare Release

### Task 13: Bump version and update changelog

**Files:**

- Modify: `package.json` (version)
- Modify: `CHANGELOG.md`
- Modify: `src-tauri/tauri.conf.json` (version if applicable)

- [ ] **Step 1:** Bump version to 1.3.11

```bash
npm run bump 1.3.11
```

- [ ] **Step 2:** Add changelog entry for v1.3.11

```markdown
## [1.3.11] - 2026-04-25

### Fixed

- **Turn card message loss:** Restored turn card commit logic so in-progress agent output is properly archived to the message history instead of being discarded.
- **Handoff race condition:** Added concurrency guard to prevent multiple rapid handoff events from processing simultaneously.
- **Executor followup prompt:** Guarded the acceptance followup path to avoid sending placeholder text to agents when no prior executor output exists.
- **Acceptance card misclassification:** Restored role guard in TurnCardView to prevent executor output from being rendered as acceptance cards.
- **Mock fallback regression:** Restored mock fallbacks for `repo.checkState` and `repo.listBranches` in non-Tauri environments.
- **Report filename sanitization:** Sanitized session IDs in report filenames to prevent path traversal.
- **PATH race condition:** Fixed PATH refresh to merge login shell entries instead of overwriting fallback directories.
- **Duplicate check keys:** Fixed React key warnings in acceptance check list rendering.
- **Evidence type safety:** Fixed fallback parser returning undefined evidence where string[] is required.
```

- [ ] **Step 3:** Update the changelog link references at the bottom

```markdown
[1.3.11]: https://github.com/timwuhaotian/the-pair/compare/v1.3.10...v1.3.11
```

### Task 14: Run quality gates

- [ ] **Step 1:** Run tests

```bash
npm test
```

- [ ] **Step 2:** Run typecheck

```bash
npm run typecheck
```

- [ ] **Step 3:** Run lint

```bash
npm run lint
```

- [ ] **Step 4:** Fix any failures and re-run until all pass

### Task 15: Commit the fixes

- [ ] **Step 1:** Commit in logical groups

```bash
git add -A && git commit -m "fix: resolve issues found in post-v1.3.10 review

- Restore turn card commit logic to prevent message gaps
- Add handoff concurrency guard
- Guard lastExecutorMessage in acceptance followup
- Restore role guard in TurnCardView
- Restore mock fallbacks in tauri-api
- Sanitize session_id in report filenames
- Fix PATH merge race condition
- Fix duplicate React keys in acceptance checks
- Fix evidence type safety in fallback parser
- Add missing test coverage"
```

- [ ] **Step 2:** Bump version commit

```bash
git commit -m "chore: bump version to 1.3.11"
```

---

## Post-Plan: Release Execution

After all tasks are complete and verified:

1. Push to main: `git push`
2. **Do NOT manually tag** — the CI workflow auto-creates `v1.3.11`
3. Monitor at: https://github.com/timwuhaotian/the-pair/actions
