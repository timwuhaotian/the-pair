/**
 * View rules for the updater UI (contract C8). Pure so they can be unit-tested.
 */

/** Phases in which the update modal has something to show (release notes, progress, failure). */
const MODAL_PHASES = new Set(['available', 'installing', 'error'])

export function shouldShowUpdateModal(showModal: boolean, phase: string): boolean {
  return showModal && MODAL_PHASES.has(phase)
}

/**
 * A new check (menu "Check for Updates", toolbar button) must not run while an
 * update is installing: it would close the Update being installed and could
 * surface the Install button again mid-install.
 */
export function canStartUpdateCheck(phase: string): boolean {
  return phase !== 'installing'
}

/** The modal can't be dismissed while the update is downloading/installing. */
export function canDismissUpdateModal(phase: string): boolean {
  return phase !== 'installing'
}
