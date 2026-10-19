# Bottom-pinned composer — implementation plan

**Goal:** Pin the pair console's task input to the bottom of the console, like
most chat / agent apps, instead of letting it flow at the end of the transcript.

**Acceptance**

- The task input (`[data-testid="pair-task-input"]`) renders outside the
  transcript scroll container, in a `shrink-0` dock below it.
- When the pair is "Awaiting Human Review", the plan review bar (or, while an
  archived run is on screen, the "return to review" link) docks directly above
  the input instead of scrolling with the transcript.
- Viewing an archived run shows no input (unchanged).
- Long input scrolls inside the dock (capped at 40vh) rather than squeezing the
  transcript.
- While the reader is following the bottom, the dock growing or shrinking
  (multi-line input, review bar appearing) keeps the newest output in view.
- `@file` / `/skill` popovers open upward when there is no room below the
  caret, so they never spill off-screen from the bottom dock. Modal usages
  with room below keep opening downward.

**Non-goals:** no new send button, no visual restyle of the `>` prompt, no new
i18n strings, no extracted `Composer` component.

## Task 1 — `placeMentionPopup` helper

- Modify `src/renderer/src/components/mentionText.ts`: add
  `placeMentionPopup(caret, viewportHeight, popupMaxHeight)` returning
  `{ top, left }` (open below) or `{ bottom, left }` (open above).
- Test `tests/uiMentionText.test.ts`: fits below → below; no room below and
  more room above → above; no room either side but more below → below.
- Run `node --import tsx --test tests/uiMentionText.test.ts` → fail, implement,
  pass, commit.

## Task 2 — Mention popovers use the helper

- Modify `FileMention.tsx` / `SkillMention.tsx`: `getCursorPosition` measures
  the caret line (`top`, `bottom`, `left`) and returns
  `placeMentionPopup(...)` with the popover's max height (256 / 288 px).
- Run `npm run typecheck`, commit.

## Task 3 — Dock the composer and review bar

- Test first `tests/render/pairConsoleComposer.test.tsx`:
  - live pair: input exists and is not inside `[data-testid="console-transcript"]`,
    and is inside `[data-testid="console-composer-dock"]`;
  - empty pair: same;
  - Awaiting Human Review: `plan-approve-btn` is inside the dock;
  - archived run on screen: no input textarea.
- Modify `PairConsole.tsx`:
  - tag the scroll container `data-testid="console-transcript"`;
  - remove `taskInputRow` and the review block from the transcript;
  - render a `shrink-0 border-t` dock after the message area holding the
    review block then the input;
  - wrap the input mirror/textarea in a `max-h-[40vh] overflow-y-auto` box;
  - `ResizeObserver` on the scroll container re-pins to the bottom while
    following (skipped when archived / searching / unsupported).
- Modify `PlanReviewBar.tsx`: drop the transcript indent wrapper (`pl-[3ch]`).
- Run `npm run test:ui` → pass, commit.

## Task 4 — Full gate

`npm test && npm run typecheck && npm run lint`
