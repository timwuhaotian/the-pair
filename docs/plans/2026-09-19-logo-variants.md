# Plan — Logo variants (3 candidates)

**Goal:** Produce 3 candidate logo marks as SVG + PNG for review. Nothing existing is replaced;
the winner is swapped in later on request.

**Palette** (derived from `src/renderer/src/assets/main.css` role tokens):

| Role     | Light token                | Dark token                 | Mark mid-tone (used in PNG) |
| -------- | -------------------------- | -------------------------- | --------------------------- |
| Mentor   | `hsl(220,70%,42%)` #2052B6 | `hsl(199,89%,65%)` #56C3F5 | `#2E7DE8`                   |
| Executor | `hsl(270,55%,48%)` #7A37BE | `hsl(275,80%,78%)` #CE9AF4 | `#9B5DE5`                   |

Mid-tones are chosen to clear contrast on both `#FAFAF7` (light bg) and `#0A0E14` (dark bg).
Each SVG additionally carries a `prefers-color-scheme` block so it adapts when embedded live.

## Tasks

### Task 1 — Variant A: "Evolved Pair"

- Create `docs/assets/logo-variants/variant-a.svg` (512x512, paths only, no fonts/rasters).
- Two agent heads leaning toward each other; inner edges taper to form a coffee-cup
  negative space over a saucer bar; eyes are `<` and `>` chevrons.
- Render, inspect, iterate until it reads at 512 and still holds silhouette at 32.

### Task 2 — Variant B: "Handoff Loop"

- Create `variant-b.svg`. Two ~160deg arcs (blue + purple) in a cycle with arrowhead ends,
  encoding Mentor -> Executor -> Review. Pure geometry, best small-size legibility.

### Task 3 — Variant C: "Prompt"

- Create `variant-c.svg`. Terminal pane frame + `>>` double caret (blue, purple) + block cursor.
- Frame stroke uses a neutral mid-tone that survives both themes.

### Task 4 — Raster pipeline

- Add `scripts/build-logo-variants.mjs`: rasterizes each SVG to 128/512/1024 PNG via
  `rsvg-convert`, transparent background. Idempotent, re-runnable.
- Run it; confirm 9 PNGs land with correct dimensions.

### Task 5 — Contact sheet

- `docs/assets/logo-variants/preview.html`: all 3 at 16/32/128/512 on light AND dark panels.
- Render `contact-sheet.png` from it for quick viewing in the repo.

### Task 6 — Test

- `tests/logoVariants.test.ts` (node --test, tsx loader), asserting for each variant:
  - SVG parses, has square `viewBox`, has `width`/`height`
  - no `<image>`, no `@font-face`, no `font-family`, no external `href`
  - declared brand hexes present
  - all 3 PNG sizes exist and report the expected pixel dimensions
- Red -> green -> commit per task.

### Task 7 — Docs

- `docs/assets/logo-variants/README.md`: what each variant is, and the exact list of files a
  future swap must touch (`resources/logo-the-pair.png`, `docs/assets/logo-the-pair.png`,
  `src/renderer/src/assets/app-icon.png`, `src-tauri/icons/*`).

## Gate

`npm test && npm run lint && npm run typecheck` green in the worktree before merge.
