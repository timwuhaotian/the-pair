/**
 * Keeps the keyboard-active option of a scrolling listbox visible. jsdom (and
 * any host without layout) lacks `scrollIntoView`, so the call is guarded.
 */
export function scrollActiveOptionIntoView(list: HTMLElement | null): void {
  const active = list?.querySelector<HTMLElement>('[role="option"][aria-selected="true"]')
  if (active && typeof active.scrollIntoView === 'function') {
    active.scrollIntoView({ block: 'nearest' })
  }
}
