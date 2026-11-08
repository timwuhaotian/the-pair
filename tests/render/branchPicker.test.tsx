/**
 * Component unit tests for BranchPicker: the localized placeholder labels, the
 * repository states (loading / unreadable / not a git repo) and the branch
 * list (groups, current chip, tracking hint, relative dates, dirty warning).
 */
import './setup-dom.ts'
import './mock-ipc.ts'
import React from 'react'
import assert from 'node:assert/strict'
import test, { afterEach } from 'node:test'
import { mockIPC } from '@tauri-apps/api/mocks'
import { I18nextProvider } from 'react-i18next'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import i18n from '../../src/renderer/src/i18n.ts'
import { BranchPicker } from '../../src/renderer/src/components/BranchPicker.tsx'
import { mockRepoState, state, type MockRepoState } from './mock-ipc.ts'
import { assertAbsent } from './assert-dom.ts'

type RepoCheckMode = 'ok' | 'fail' | 'pending'
let repoCheckMode: RepoCheckMode = 'ok'

// The shared mock resolves the repo check instantly; this file needs to park
// it to observe the loading state.
mockIPC((cmd) => {
  if (cmd === 'repo_check_state') {
    if (repoCheckMode === 'pending') return new Promise<never>(() => undefined)
    if (repoCheckMode === 'fail') throw new Error('repo state unreadable')
    return state.repo
  }
  return undefined
})

const DAY = 86_400

function setLocale(): void {
  void i18n.changeLanguage('en')
}

function wrap(node: React.ReactNode): React.ReactNode {
  return <I18nextProvider i18n={i18n}>{node}</I18nextProvider>
}

function daysAgo(days: number): number {
  return Math.floor(Date.now() / 1000) - days * DAY
}

/** Local branch plus one remote branch per relative-date bucket. */
function repoWithRemoteDates(): MockRepoState {
  return {
    isGitRepo: true,
    isDirty: false,
    currentBranch: 'main',
    branches: [
      {
        name: 'main',
        isLocal: true,
        isRemote: false,
        lastCommitMessage: 'init',
        lastCommitDate: daysAgo(0),
        isCheckedOutLocally: true
      },
      { name: 'origin/today', isLocal: false, isRemote: true, lastCommitDate: daysAgo(0) },
      { name: 'origin/yesterday', isLocal: false, isRemote: true, lastCommitDate: daysAgo(1) },
      { name: 'origin/days', isLocal: false, isRemote: true, lastCommitDate: daysAgo(3) },
      { name: 'origin/weeks', isLocal: false, isRemote: true, lastCommitDate: daysAgo(14) },
      { name: 'origin/old', isLocal: false, isRemote: true, lastCommitDate: daysAgo(60) }
    ]
  }
}

afterEach(() => {
  cleanup()
  repoCheckMode = 'ok'
  state.repo = mockRepoState.committedRepo
})

test('BranchPicker: shows the loading label while the repo state is fetched', () => {
  setLocale()
  repoCheckMode = 'pending'
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  assert.ok(screen.getByText('Checking repository...'))
})

test('BranchPicker: reports an unreadable repository state', async () => {
  setLocale()
  repoCheckMode = 'fail'
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  await screen.findByText('✗ Could not check repository state')
  assert.ok(screen.getByText('Error: repo state unreadable'))
})

test('BranchPicker: reports a directory that is not a git repository', async () => {
  setLocale()
  state.repo = mockRepoState.emptyRepo
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  await screen.findByText('Not a git repository — worktrees unavailable')
})

test('BranchPicker: the base variant and the plain variant label the trigger', async () => {
  setLocale()
  state.repo = mockRepoState.committedRepo

  const base = render(
    wrap(<BranchPicker directory="/repo" onChange={() => undefined} variant="base" />)
  )
  await screen.findByText('base branch (defaults to current)')
  base.unmount()
  cleanup()

  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))
  await screen.findByText('select branch (optional)')
})

test('BranchPicker: groups the branches and marks the current one', async () => {
  setLocale()
  state.repo = repoWithRemoteDates()
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  fireEvent.click(await screen.findByText('select branch (optional)'))

  await screen.findByText('── Local branches ──')
  assert.ok(screen.getByText('── Remote branches ──'))
  assert.ok(screen.getByRole('group', { name: 'Local branches' }))
  assert.ok(screen.getByRole('group', { name: 'Remote branches' }))
  // The current branch carries a chip.
  assert.ok(screen.getByText('current'))
  // Every remote branch without a local checkout announces the tracking branch it creates.
  assert.equal(screen.getAllByText('→ Will create local tracking branch').length, 5)
})

test('BranchPicker: filters the list and clears the filter again', async () => {
  setLocale()
  state.repo = repoWithRemoteDates()
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  fireEvent.click(await screen.findByText('select branch (optional)'))
  const input = await screen.findByPlaceholderText('Filter branches...')

  // Fuzzy matching: a query must be distinctive enough to keep only one branch.
  fireEvent.change(input, { target: { value: 'weeks' } })
  await screen.findByText('origin/weeks')
  assertAbsent(screen.queryByText('origin/today'))

  fireEvent.click(screen.getByRole('button', { name: 'Clear filter' }))
  await waitFor(() => assert.equal((input as HTMLInputElement).value, ''))
  assert.ok(screen.getByText('origin/today'))
})

test('BranchPicker: reports that no branch matches the filter', async () => {
  setLocale()
  state.repo = repoWithRemoteDates()
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  fireEvent.click(await screen.findByText('select branch (optional)'))
  fireEvent.change(await screen.findByPlaceholderText('Filter branches...'), {
    target: { value: 'zzzzz' }
  })

  await screen.findByText('— No branches match "zzzzz" —')
})

test('BranchPicker: warns about uncommitted changes in the base variant', async () => {
  setLocale()
  state.repo = { ...mockRepoState.committedRepo, isDirty: true }
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} variant="base" />))

  fireEvent.click(await screen.findByText('base branch (defaults to current)'))
  const warning = await screen.findByText(/only the current branch can be the base/)
  assert.match(warning.textContent ?? '', /uncommitted changes/)
})

test('BranchPicker: warns about uncommitted changes in the plain variant', async () => {
  setLocale()
  state.repo = { ...mockRepoState.committedRepo, isDirty: true }
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  fireEvent.click(await screen.findByText('select branch (optional)'))
  const warning = await screen.findByText(/commit or stash before selecting a branch/)
  assert.match(warning.textContent ?? '', /uncommitted changes/)
})

test('BranchPicker: renders the relative commit dates of remote branches', async () => {
  setLocale()
  state.repo = repoWithRemoteDates()
  render(wrap(<BranchPicker directory="/repo" onChange={() => undefined} />))

  fireEvent.click(await screen.findByText('select branch (optional)'))

  await screen.findByText('Today')
  assert.ok(screen.getByText('Yesterday'))
  assert.ok(screen.getByText('3 days ago'))
  assert.ok(screen.getByText('2 weeks ago'))
  // Anything older than a month falls back to the absolute date.
  assert.ok(screen.getByText(new Date(daysAgo(60) * 1000).toLocaleDateString()))
})
