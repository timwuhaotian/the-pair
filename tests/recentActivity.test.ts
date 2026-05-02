import assert from 'node:assert/strict'
import test from 'node:test'
import { readFile } from 'node:fs/promises'

const activityItem = await readFile(
  new URL('../src/renderer/src/components/ActivityItem.tsx', import.meta.url),
  'utf8'
)

const recentActivityPanel = await readFile(
  new URL('../src/renderer/src/components/RecentActivityPanel.tsx', import.meta.url),
  'utf8'
)

test('ActivityItem exports ActivityItem component', () => {
  assert.match(activityItem, /export function ActivityItem\(/)
})

test('ActivityItem accepts activity prop with type/description/timestamp', () => {
  assert.match(activityItem, /activity/)
  assert.match(activityItem, /type/)
  assert.match(activityItem, /description/)
  assert.match(activityItem, /timestamp/)
})

test('ActivityItem uses line-clamp for text truncation', () => {
  assert.match(activityItem, /line-clamp/)
})

test('RecentActivityPanel exports RecentActivityPanel component', () => {
  assert.match(recentActivityPanel, /export function RecentActivityPanel\(/)
})

test('RecentActivityPanel invokes get_recent_activities Tauri command', () => {
  assert.match(recentActivityPanel, /invoke|get_recent_activities/)
})

test('RecentActivityPanel handles empty state', () => {
  assert.match(recentActivityPanel, /empty|暂无/i)
})
