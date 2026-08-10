import assert from 'node:assert/strict'
import test from 'node:test'

import {
  escapeHtml,
  generateHtmlReport,
  generateMarkdownReport,
  isUnsafeHref,
  shortModel
} from '../src/renderer/src/lib/reportExport.ts'
import type { TimelineData } from '../src/renderer/src/lib/timeline.ts'

// ── escapeHtml ─────────────────────────────────────────

test('escapeHtml escapes <script> tags and quotes', () => {
  const result = escapeHtml('<script>alert("xss")</script>')
  assert.ok(result.includes('&lt;script&gt;'))
  assert.ok(result.includes('&quot;'))
})

test('escapeHtml escapes all 5 special characters', () => {
  assert.ok(escapeHtml('&').includes('&amp;'))
  assert.ok(escapeHtml('<').includes('&lt;'))
  assert.ok(escapeHtml('>').includes('&gt;'))
  assert.ok(escapeHtml('"').includes('&quot;'))
  assert.ok(escapeHtml("'").includes('&#39;'))
})

// ── isUnsafeHref ───────────────────────────────────────

test('isUnsafeHref detects javascript: protocol', () => {
  assert.equal(isUnsafeHref('javascript:alert(1)'), true)
})

test('isUnsafeHref detects vbscript: protocol', () => {
  assert.equal(isUnsafeHref('vbscript:msgbox(1)'), true)
})

test('isUnsafeHref detects data:text/html protocol', () => {
  assert.equal(isUnsafeHref('data:text/html,<script>'), true)
})

test('isUnsafeHref returns false for https URL', () => {
  assert.equal(isUnsafeHref('https://example.com'), false)
})

test('isUnsafeHref is case/space insensitive', () => {
  assert.equal(isUnsafeHref('  JavaScript:alert(1)  '), true)
})

// ── shortModel ─────────────────────────────────────────

test('shortModel extracts part after slash', () => {
  assert.equal(shortModel('claude/sonnet-4'), 'sonnet-4')
})

test('shortModel returns full string when no slash', () => {
  assert.equal(shortModel('gpt-4o'), 'gpt-4o')
})

// ── generateMarkdownReport ─────────────────────────────

function makeTimeline(): TimelineData {
  return {
    pairName: 'Test',
    spec: 'Fix bug',
    mentorModel: 'claude/sonnet',
    executorModel: 'gpt-4o',
    startedAt: 1700000000000,
    finishedAt: 1700000005000,
    status: 'Finished',
    iterations: [
      {
        iteration: 1,
        events: [],
        startedAt: 1700000000000,
        endedAt: 1700000005000,
        durationMs: 5000,
        totalTokens: 100,
        totalInputTokens: 50
      }
    ],
    totalOutputTokens: 100,
    totalInputTokens: 50,
    mentorOutputTokens: 60,
    mentorInputTokens: 30,
    executorOutputTokens: 40,
    executorInputTokens: 20,
    acceptanceRecords: [],
    modifiedFiles: [],
    durationMs: 5000
  }
}

test('generateMarkdownReport produces report header and pair info', () => {
  const md = generateMarkdownReport(makeTimeline())
  assert.ok(md.includes('# Pair Session Report'))
  assert.ok(md.includes('Test'))
  assert.ok(md.includes('Fix bug'))
})

// ── generateHtmlReport ─────────────────────────────────

test('generateHtmlReport produces valid HTML with escaped pair name', () => {
  const html = generateHtmlReport(makeTimeline())
  assert.ok(html.startsWith('<!DOCTYPE html>'))
  assert.ok(html.includes('Test'))
})
