# UI/UX consistency & polish pass

**Goal:** make every pair surface (hub, console, inspector, modals, pickers) read as one
system without changing the visual language (flat terminal shell, linen/navy themes).

**Non-goals:** new palette, motion design, layout redesign, backend changes.

## Acceptance

1. Every `bg-state-*/N`, `border-state-*/N`, `hover:text-state-*`, `outline-state-*/N` and
   `*-role-*/N` utility in the renderer emits CSS (verified against `vite build` output).
2. No nested interactive elements; every icon-only button has an accessible name.
3. Disabled buttons still show their `title` tooltip.
4. Mention popovers keep the keyboard-selected row in view and expose listbox semantics.
5. One button scale, one input recipe, one label recipe, one popover surface, one chip
   primitive — call sites migrated.
6. Rendered font sizes stay within 10–14px in chrome (hero/titles excepted).
7. Layering: modal 50 < popover 60 < palette 70 < toast 80 < boot splash 100.
8. `npm run lint`, `npm run typecheck`, `npm test`, `npm run e2e:web` green.

## Tasks

### 1. Theme tokens — `src/renderer/src/assets/main.css`

- Register `--color-state-{running,done,error}`, `--color-role-{mentor,executor,human}`,
  `--color-surface-{base,raised,sunken}`, `--color-rule`, `--color-rule-strong` in `@theme`.
- Keep the legacy `.bg-state-*` / `.border-role-*` mix classes (unlayered, they still win).
- Test: `tests/shellTypography.test.ts` — assert the tokens are registered.
- Verify: `vite build` CSS contains `bg-state-running\/15`.

### 2. Shared recipes — `src/renderer/src/lib/ui.ts` (new)

- `fieldInput`, `fieldLabel`, `sectionLabel`, `kbd`, `popoverSurface`, `menuItem(active)`,
  `hoverTint`, `Z` layer constants.
- Test: `tests/uiRecipes.test.ts` — menu item active/idle differ, z layers ordered.

### 3. Primitives — `src/renderer/src/components/ui/`

- `GlassButton`: add `xs` size; drop `pointer-events-none` on disabled; primary focus ring
  uses `--ring`.
- `IconButton.tsx` (new): square, `aria-label` required, `sm|md`.
- `Chip.tsx` (new): `tone` neutral|running|done|error|mentor|executor|human, `size` sm|md.
- Test: `tests/render/uiPrimitives.test.tsx`.

### 4. Behaviour fixes

- PairsHub / PresetPicker: card is a `div` + sibling action cluster (no nested buttons).
- PairListGroup: `span role=button` → real buttons, `group-focus-within` reveal.
- PairOperationsPanel: single Retry when status is Error.
- FileMention / SkillMention: `scrollIntoView({block:'nearest'})` on selection, `role=listbox`,
  `aria-selected`, i18n "more files" footer.
- ModelPicker: `text-blue-*` → `text-primary`.
- TaskHistoryPanel back button = PairConsole back button (shared `xs` GlassButton).
- UpdateNotification on GlassModal; CommandPalette gets a dim backdrop.
- Composer prompt glyph neutral (amber reserved for running); caret blink only on focus.
- Running ✻ glyph always role-colored.
- Test: extend `tests/render/*` where a component already has a render harness.

### 5. Call-site migration

- Hand-rolled buttons → GlassButton/IconButton; inputs → `fieldInput`; labels →
  `sectionLabel`/`fieldLabel`; pickers → `popoverSurface`/`menuItem`; badges → `Chip`.
- 8px/9px text → 10px.

### 6. Verify

- Full suite + `e2e:web`; light/dark before/after screenshots of hub, console, error pair,
  create-pair modal (mock IPC, playwright).
