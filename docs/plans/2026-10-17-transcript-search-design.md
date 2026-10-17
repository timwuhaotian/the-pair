# Transcript Search — Design

Date: 2026-10-17
Status: approved (auto-mode decision; user may amend — see "Open decision")

## Goal

Find text inside a pair's conversation transcript without scrolling.
Observed problem: `PairConsole` offers only role filter (`MessageFilterBar`:
All/Mentor/Executor). No text search exists anywhere in the console; long
runs (tens of iterations, long reports) cannot be searched for an error
string, a file name, or a phrase the user remembers.

## Acceptance (observable)

1. `Cmd+F` (or `Ctrl+F`) while a pair console is open focuses a search input
   in the console header. `Esc` closes it and clears the query.
2. Typing a query filters the visible transcript to messages whose content
   contains the query (case-insensitive substring), composing with the
   existing role filter (search applies after role filter).
3. Header shows match count as `N matches` (localized; `0 matches` when none).
4. Matched substrings are highlighted with `<mark>` in message prose,
   including inside markdown-rendered text nodes; no raw markdown source
   is ever shown differently than today.
5. Enter / Shift+Enter cycle to next/previous matching message: the target
   message scrolls into view and its card gets a temporary outline.
6. Searching an archived run (`viewingRun`) works the same as live.
7. Empty query restores the unfiltered transcript exactly.

## Non-goals

- No regex, no fuzzy matching, no whole-word toggle.
- No cross-pair / global search (hub, activity).
- No search persistence, history, or saved queries.
- No Rust/tauri changes. No new packages land in the tree — `remark-parse`
  and `unified` are promoted from transitive (via react-markdown) to direct
  dependencies for AST-based counting in `searchableCard.ts`.
- No highlight inside code blocks beyond text-node marking (text-node
  marking covers them automatically where react-markdown emits text).

## Architecture

Entry point: `PairConsole` header (next to `MessageFilterBar`).
Layers: pure match logic in a new lib module; rendering via props threaded
through existing components; no store change.

### New unit: `src/renderer/src/lib/consoleSearch.ts`

Pure, framework-free, unit-testable. As shipped:

- `normalizeQuery(query)` — trimmed, lowercased query.
- `findMatchRanges(text, query, options?)` — all non-overlapping
  `[start, end)` ranges, case-insensitive; whitespace collapses by
  default, `literal: true` matches it exact (code under `<pre>`);
  empty query returns `[]`.
- `countOccurrences(text, query, options?)` — `findMatchRanges(...).length`.
- `nextMatchIndex(current, total, dir)` — wrap-around index arithmetic
  for Enter/Shift+Enter cycling; `total <= 0` returns `-1`.

This module works on plain text only — it does not filter or count
messages. Per-message counting over the rendered markdown is AST-based
(`remark-parse`) in `searchableCard.ts` (`countSearchHighlights`), and
the filter that keeps messages with a nonzero count is applied in
`PairConsole.tsx` over `deduplicatedConsoleMessages`.

Invariants owned here: matching is substring + case-insensitive only;
ranges are sorted, non-overlapping, within bounds.

### Modified: `PairConsole.tsx`

- State: `searchOpen`, `searchQuery`, `activeMatchOrdinal` (index into the
  flat list of matching message ids).
- Derived: `searchMessages = filterMessagesByQuery(roleFiltered, query)`,
  then existing `collapseWithDropCounts` pipeline unchanged.
- Renders search input row in header when open: input, `N matches` label,
  prev/next buttons, close button. `data-testid`s:
  `console-search-input`, `console-search-count`, `console-search-prev`,
  `console-search-next`, `console-search-close`, `console-search-toggle`.
- Keyboard: `Cmd/Ctrl+F` opens (handled locally in the console; the app
  already forwards cmd-key shortcuts from text fields — see 2.11.1 note),
  `Esc` closes+clears, Enter/Shift+Enter in input cycle.
- Cycling: computes matching message ids in display order; on cycle,
  `document.getElementById('console-msg-' + id)?.scrollIntoView({ block: 'center' })`
  and sets `activeMessageId`, passed to `MessageCard` for outline.
- Live turn card (`TurnCardView`) is not searched/filtered (in-flight text;
  out of scope, matches "transcript" = finalized messages).

### Modified: `MessageCard.tsx` + `MarkdownContent.tsx`

- `MessageCard` accepts optional `searchQuery?: string` and
  `isActiveMatch?: boolean`; forwards query to `MarkdownContent`, adds
  `id={'console-msg-' + msg.id}` anchor and active outline class.
- `MarkdownContent` accepts optional `searchQuery`. When non-empty, a
  `text` renderer in the components map splits text nodes on
  `findMatchRanges` and wraps matches in `<mark>`. Components map becomes
  a factory keyed by query (memoized per query) — default export behavior
  without query is byte-identical to today.

### i18n

New keys under `console`: `searchPlaceholder`, `searchMatches_one/other`
(or count interpolation per existing pattern), `searchNoMatches`,
`searchPrev`, `searchNext`, `searchClose`, `searchOpen` — in `en`, `zh`,
`ja`, `ko`.

### Styling rule (AGENTS.md)

Console stream stays `font-mono`, flat hairlines. `<mark>` uses existing
state color token background, no new palette.

## Error handling

- No matches: keep list empty, show `0 matches`; prev/next disabled.
- Query with regex-special chars: plain substring, no escaping needed.
- Clipboard/DOM absence in tests: logic lives in pure lib; component tests
  use existing jsdom render harness.

## Testing

- `tests/consoleSearch.test.ts` (node:test, mirrors `consoleMessages.test.ts`):
  filter, ranges (overlap, case, unicode, empty), occurrences, cycling wrap.
- `tests/render/` component test if harness permits: query filters rendered
  cards, `<mark>` present.
- Gates: `npm run test:js`, `npm run test:ui`, `npm run typecheck`,
  `npm run lint`, full `npm test` before push. New lib target: full line
  coverage from its unit tests (project policy 95%+ on changed lines).

## Open decision

Auto-mode proceeded without explicit user pick between candidates A
(this), B (system notifications), C (hub filter), D (per-message copy).
This spec implements A. User amendment replaces it before merge; branch is
isolated and PR is never merged by the agent.
