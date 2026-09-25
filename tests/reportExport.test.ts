import assert from 'node:assert/strict'
import test from 'node:test'

import {
  buildReportFileName,
  escapeHtml,
  generateHtmlReport,
  generateMarkdownReport,
  isUnsafeHref,
  isUnsafeImageSrc,
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

// ── XSS via entity-encoded / obfuscated schemes ────────

test('isUnsafeHref decodes entities and strips whitespace before the scheme check', () => {
  for (const href of [
    'javascript&#58;alert(document.domain)',
    'javascript&#x3A;alert(1)',
    'javascript&colon;alert(1)',
    'JAVASCRIPT&COLON;alert(1)',
    'java&#x09;script:alert(1)',
    'java&Tab;script:alert(1)',
    'java\tscript:alert(1)',
    'java\nscript:alert(1)',
    '\u0001javascript:alert(1)',
    '&#106;avascript:alert(1)',
    'vbscript&#58;msgbox(1)',
    'data:image/svg+xml,<svg onload=alert(1)>',
    'file:///etc/passwd'
  ]) {
    assert.equal(isUnsafeHref(href), true, href)
  }
})

test('isUnsafeHref allows http(s), mailto, anchors and relative links', () => {
  for (const href of [
    'https://example.com/a?b=c#d',
    'HTTP://example.com',
    'mailto:dev@example.com',
    '#section-2',
    'docs/readme.md',
    './a/b',
    '../up',
    '/abs/path',
    '?q=1'
  ]) {
    assert.equal(isUnsafeHref(href), false, href)
  }
})

function timelineWithContent(content: string): TimelineData {
  const timeline = makeTimeline()
  timeline.iterations[0].events = [
    {
      id: 'e1',
      type: 'mentor-plan',
      iteration: 1,
      from: 'mentor',
      timestamp: 1700000000000,
      title: 'Mentor Plan',
      summary: 'summary',
      // Long enough to render the markdown detail section.
      content: `${'x'.repeat(130)}\n\n${content}`
    }
  ]
  return timeline
}

test('generateHtmlReport neutralizes encoded javascript: links, incl. reference-style ones', () => {
  const html = generateHtmlReport(
    timelineWithContent(
      [
        '[inline](javascript&#58;alert(document.domain))',
        '[tab](java&#x09;script:alert(1))',
        '[named](javascript&colon;alert(1))',
        '[ref-style][evil]',
        '[ok](https://example.com)',
        '',
        '[evil]: javascript&#58;alert(2)'
      ].join('\n')
    )
  )
  const hrefs = [...html.matchAll(/<a href="([^"]*)"/g)].map((match) => match[1])
  assert.deepEqual(hrefs, ['#', '#', '#', '#', 'https://example.com'])
  assert.ok(!/javascript/i.test(hrefs.join(' ')))
})

// ── Export file name ───────────────────────────────────

test('buildReportFileName uses the local date, not the UTC date', () => {
  // 00:30 local time on Jan 2 — in any timezone east of UTC this is still Jan 1 in UTC.
  const now = new Date(2026, 0, 2, 0, 30)
  assert.equal(buildReportFileName('My Pair', now), 'pair-report-my-pair-2026-01-02.html')
})

test('buildReportFileName strips path separators and reserved characters from the pair name', () => {
  const now = new Date(2026, 8, 25, 12, 0)
  assert.equal(
    buildReportFileName('feat/login: fix <auth>?', now),
    'pair-report-feat-login-fix-auth-2026-09-25.html'
  )
  assert.equal(buildReportFileName('..\\..\\etc', now), 'pair-report-etc-2026-09-25.html')
  assert.equal(buildReportFileName('///', now), 'pair-report-pair-2026-09-25.html')
})

test('isUnsafeImageSrc keeps raster data and file images but blocks scriptable sources', () => {
  for (const src of [
    'data:image/png;base64,iVBORw0KGgo=',
    'DATA:image/jpeg;base64,/9j/4AAQ',
    'data:image/webp;base64,UklGR',
    'file:///Users/me/screenshot.png',
    'https://example.com/a.png',
    'img/local.gif'
  ]) {
    assert.equal(isUnsafeImageSrc(src), false, src)
  }
  for (const src of [
    'data:image/svg+xml,<svg onload=alert(1)>',
    'data:text/html,<script>alert(1)</script>',
    'javascript&#58;alert(1)',
    'java&#x09;script:alert(1)'
  ]) {
    assert.equal(isUnsafeImageSrc(src), true, src)
  }
})
