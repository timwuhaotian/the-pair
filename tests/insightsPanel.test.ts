import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const insightsPanel = await readFile(
  new URL('../src/renderer/src/components/InsightsPanel.tsx', import.meta.url),
  'utf8'
)

const dashboard = await readFile(
  new URL('../src/renderer/src/components/Dashboard.tsx', import.meta.url),
  'utf8'
)

const tauriApi = await readFile(
  new URL('../src/renderer/src/lib/tauri-api.ts', import.meta.url),
  'utf8'
)

test('InsightsPanel exports InsightsPanel component', () => {
  assert.match(insightsPanel, /export function InsightsPanel\(/)
})

test('InsightsPanel fetches the get_insights_summary Tauri command', () => {
  assert.match(insightsPanel, /tauriApi\.insights\.getSummary/)
  assert.match(tauriApi, /invokeTauri\('get_insights_summary'\)/)
})

test('InsightsPanel hides during cold start until at least 5 runs', () => {
  assert.match(insightsPanel, /COLD_START_MIN_RUNS = 5/)
  assert.match(insightsPanel, /summary\.totalRuns < COLD_START_MIN_RUNS/)
  assert.match(insightsPanel, /return null/)
})

test('InsightsPanel renders a combo leaderboard when populated', () => {
  assert.match(insightsPanel, /summary\.combos/)
  assert.match(insightsPanel, /crossRun\.combo/)
  assert.match(insightsPanel, /crossRun\.success/)
  assert.match(insightsPanel, /crossRun\.avgIter/)
  assert.match(insightsPanel, /crossRun\.avgTokens/)
  assert.match(insightsPanel, /successRate/)
})

test('InsightsPanel renders the overall success rate', () => {
  assert.match(insightsPanel, /crossRun\.successRate/)
  assert.match(insightsPanel, /summary\.successfulRuns/)
})

test('Dashboard mounts InsightsPanel when no pair is selected', () => {
  assert.match(dashboard, /import \{ InsightsPanel \} from '\.\/InsightsPanel'/)
  assert.match(dashboard, /<InsightsPanel \/>/)
})
