# Transcript Search — Implementation Plan

Spec: `docs/plans/2026-10-17-transcript-search-design.md`
Branch: `feature/transcript-search` (worktree, off `origin/main` @ 2.11.3)

## Task 1 — Pure match logic + tests (TDD)

- Create `tests/consoleSearch.test.ts`: filter (case, trim, empty=same ref),
  ranges (multiple, overlap-adjacent, case-insensitive, no-match, empty),
  count, cycling wrap (fwd/back/empty/single).
- Run: `npm run test:js 2>&1 | grep -A5 consoleSearch` → fails (no module).
- Create `src/renderer/src/lib/consoleSearch.ts` implementing
  `filterMessagesByQuery`, `findMatchRanges`, `countOccurrences`,
  `nextMatchIndex`.
- Run test file green. Commit: `feat(console): transcript search match logic`.

## Task 2 — Highlight rendering

- Modify `MarkdownContent.tsx`: optional `searchQuery` prop; components
  factory with `text` renderer splitting on `findMatchRanges`, `<mark>`
  wrap. No query → current static map.
- Modify `MessageCard.tsx`: optional `searchQuery`, `isActiveMatch`,
  `id` anchor `console-msg-<id>`, outline class when active.
- Render test in `tests/render/` for `<mark>` presence (skip if harness
  blocks; then assert via unit-level react render in existing style).
- Gates: typecheck + test:ui. Commit: `feat(console): highlight search matches in transcript`.

## Task 3 — Console search UI + keyboard + cycling

- Modify `PairConsole.tsx`: state, header toggle/input/count/prev/next/close,
  Cmd/Ctrl+F + Esc + Enter cycling, scrollIntoView, compose after role filter.
- i18n keys in en/zh/ja/ko `console` section.
- Gates: test:js + test:ui + typecheck + lint.
- Commit: `feat(console): Cmd+F transcript search with match cycling`.

## Task 4 — Verify + deliver

- Full `npm test` in worktree (includes cargo). Fix until green.
- `git push -u origin feature/transcript-search`; `gh pr create` (check
  `gh pr view` first). Report URL. Cleanup worktree, back on `main`.
