/**
 * View rules for the updater UI (contract C8). Pure so they can be unit-tested.
 */

/**
 * Phases in which the update modal has something to show. `idle` never does;
 * every other phase is shown once the modal is open (a manual check opens it
 * up front, a background check only when an update is found).
 */
const MODAL_PHASES = new Set([
  'checking',
  'up-to-date',
  'available',
  'installing',
  'ready',
  'error'
])

export function shouldShowUpdateModal(showModal: boolean, phase: string): boolean {
  return showModal && MODAL_PHASES.has(phase)
}

/**
 * A new check (menu "Check for Updates", toolbar button) must not run while one
 * is in flight, while an update is installing (it would close the Update being
 * installed and could surface the Install button again mid-install), or once an
 * update is installed and only waits for a restart.
 */
export function canStartUpdateCheck(phase: string): boolean {
  return phase !== 'checking' && phase !== 'installing' && phase !== 'ready'
}

/** The modal can't be dismissed while the update is downloading/installing. */
export function canDismissUpdateModal(phase: string): boolean {
  return phase !== 'installing'
}

/**
 * Dismissing the modal mid-check or with an installed update only hides it:
 * the check still lands, and the installed update still waits for a restart
 * (or the next launch). Any other phase is done, so dismissing resets.
 */
export function keepsStateOnDismiss(phase: string): boolean {
  return phase === 'checking' || phase === 'ready'
}

const MB = 1024 * 1024

/** "12.4 MB" style size for the download caption. */
export function formatMegabytes(bytes: number): string {
  return `${(bytes / MB).toFixed(1)} MB`
}
