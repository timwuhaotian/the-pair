import assert from 'node:assert/strict'
import test from 'node:test'

import { toExternalUrl } from '../src/renderer/src/lib/externalLinks.ts'

test('toExternalUrl accepts absolute http, https and mailto links', () => {
  assert.equal(toExternalUrl('https://example.com/a?b=1'), 'https://example.com/a?b=1')
  assert.equal(toExternalUrl('  http://example.com  '), 'http://example.com/')
  assert.equal(toExternalUrl('mailto:dev@example.com'), 'mailto:dev@example.com')
})

test('toExternalUrl rejects scripts, local files, relative links and junk', () => {
  for (const href of [
    undefined,
    null,
    '',
    'javascript:alert(1)',
    'JAVASCRIPT:alert(1)',
    'java\tscript:alert(1)',
    'file:///etc/passwd',
    'data:text/html,<script>alert(1)</script>',
    'vscode://file/x',
    '/relative/path',
    '#anchor',
    'not a url'
  ]) {
    assert.equal(toExternalUrl(href), null, String(href))
  }
})
