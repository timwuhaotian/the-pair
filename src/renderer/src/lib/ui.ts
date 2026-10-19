/**
 * Shared class recipes for shell + core surfaces. Components compose these
 * with `cn()` so one edit here restyles every input, menu, label and chip.
 */

/** Stacking layers. Portaled popovers sit above modals so pickers opened from a
 *  dialog are never clipped behind it. */
export const Z = {
  modal: 'z-50',
  popover: 'z-[60]',
  palette: 'z-[70]',
  toast: 'z-[80]',
  splash: 'z-[100]'
} as const

/** Pointer hover / keyboard-active tints. Two levels only. */
export const hoverTint = 'hover:bg-foreground/[0.05]'
export const activeTint = 'bg-foreground/[0.08]'

/** Single-line text field / textarea used in forms and inline editors. */
export const fieldInput =
  'w-full rounded-sm border border-border bg-background px-2 py-1.5 text-[12px] text-foreground placeholder:text-muted-foreground-faint transition-colors focus:outline-none focus:border-foreground/50 disabled:cursor-not-allowed disabled:opacity-40'

/** Search field inside a picker or popover (leading icon slot at left-2). */
export const searchInput =
  'w-full rounded-sm border border-border bg-background py-1 pl-7 pr-2 text-[12px] text-foreground placeholder:text-muted-foreground-faint transition-colors focus:outline-none focus:border-foreground/50'

/** Form field caption above an input. */
export const fieldLabel = 'block text-[10px] uppercase tracking-[0.16em] text-muted-foreground'

/** Shell section heading (hub sections, activity, inspector). */
export const sectionLabel =
  'text-[11px] font-medium uppercase tracking-[0.14em] text-muted-foreground'

/** Keyboard shortcut hint. */
export const kbd =
  'inline-flex min-w-[1.6em] items-center justify-center rounded-sm border border-border px-1 py-px font-mono text-[10px] leading-none text-muted-foreground'

/** Dropdown / popover / menu surface. */
export const popoverSurface =
  'rounded-md border border-rule-strong bg-popover text-popover-foreground'

/** Row inside a menu or listbox. */
export function menuItem(active: boolean): string {
  return [
    'flex w-full cursor-pointer items-center gap-2 rounded-sm px-2 py-1 text-left text-[12px] transition-colors',
    active ? `${activeTint} text-foreground` : `text-foreground/85 ${hoverTint}`
  ].join(' ')
}

/** Group header inside a picker list (`── label ──` rows sit on `tty-rule`). */
export const groupHeader = 'px-2 py-1 text-[10px] uppercase tracking-[0.16em] text-muted-foreground'

/** Inline notice blocks in modals and pickers. */
export const warningCallout =
  'border-l-2 border-state-running bg-state-running/10 px-2.5 py-2 text-[12px] state-running'
export const errorCallout =
  'border-l-2 border-state-error bg-state-error/10 px-2.5 py-2 text-[12px] state-error'

/** Placeholder line for empty pickers and lists. */
export const emptyHint = 'px-3 py-3 text-center text-[11px] text-muted-foreground-faint'

export type Tone = 'neutral' | 'mentor' | 'executor' | 'human' | 'running' | 'done' | 'error'

export function toneClasses(tone: Tone): { fg: string; bg: string; border: string } {
  switch (tone) {
    case 'mentor':
      return { fg: 'role-mentor', bg: 'bg-role-mentor', border: 'border-role-mentor' }
    case 'executor':
      return { fg: 'role-executor', bg: 'bg-role-executor', border: 'border-role-executor' }
    case 'human':
      return { fg: 'role-human', bg: 'bg-role-human', border: 'border-role-human' }
    case 'running':
      return { fg: 'state-running', bg: 'bg-state-running', border: 'border-state-running' }
    case 'done':
      return { fg: 'state-done', bg: 'bg-state-done', border: 'border-state-done' }
    case 'error':
      return { fg: 'state-error', bg: 'bg-state-error', border: 'border-state-error' }
    default:
      return { fg: 'text-muted-foreground', bg: 'bg-muted', border: 'border-border' }
  }
}

export type ChipShape = 'pill' | 'tag'

/** `pill` = shell status badge; `tag` = compact uppercase metadata in the stream. */
export function chipClass(shape: ChipShape): string {
  return shape === 'pill'
    ? 'inline-flex items-baseline gap-1 rounded-full border px-2 py-px text-[11px] font-medium whitespace-nowrap'
    : 'inline-flex items-center gap-1 rounded-sm border px-1.5 py-px text-[10px] uppercase leading-[1.4] tracking-[0.12em] whitespace-nowrap'
}
