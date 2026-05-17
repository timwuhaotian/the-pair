import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const dashboard = await readFile(
  new URL('../src/renderer/src/components/Dashboard.tsx', import.meta.url),
  'utf8'
)

const enLocale = await readFile(
  new URL('../src/renderer/src/locales/en.json', import.meta.url),
  'utf8'
)

test('Dashboard keeps the empty create action inside the pairs section', () => {
  assert.doesNotMatch(dashboard, /DashboardEmptyState/)
  assert.match(dashboard, /onCreatePair=\{onCreatePair\}/)
})

test('Dashboard renders a three-column workspace layout', () => {
  assert.match(dashboard, /<PairListSection/)
  assert.match(dashboard, /<PairConsole/)
  assert.match(dashboard, /<(?:PairOperationsPanel|DashboardInsightPanel)/)
})

test('empty state has a real localized description', () => {
  const locale = JSON.parse(enLocale)

  assert.equal(typeof locale.emptyState.description, 'string')
  assert.match(locale.emptyState.description, /mentor/i)
  assert.match(locale.emptyState.description, /executor/i)
})
