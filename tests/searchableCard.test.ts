import assert from 'node:assert/strict'
import test from 'node:test'

import {
  countSearchHighlights,
  displayContentForMessage
} from '../src/renderer/src/lib/searchableCard.ts'
import type { Message } from '../src/renderer/src/store/usePairStore.ts'

function makeMessage(overrides: Partial<Message>): Message {
  return {
    id: 'm1',
    timestamp: 1000,
    from: 'mentor',
    to: 'executor',
    type: 'plan',
    content: 'parser plan',
    iteration: 1,
    ...overrides
  }
}

test('countSearchHighlights counts agent card matches on the rendered body', () => {
  assert.equal(countSearchHighlights(makeMessage({}), 'parser'), 1)
  assert.equal(countSearchHighlights(makeMessage({}), ' PARSER '), 1)
  assert.equal(countSearchHighlights(makeMessage({}), 'zzz'), 0)
})

test('countSearchHighlights returns 0 for an empty query', () => {
  assert.equal(countSearchHighlights(makeMessage({}), ''), 0)
  assert.equal(countSearchHighlights(makeMessage({}), '   '), 0)
})

test('countSearchHighlights excludes human banners (no anchor, no highlight)', () => {
  assert.equal(
    countSearchHighlights(makeMessage({ from: 'human', type: 'feedback', iteration: 0 }), 'parser'),
    0
  )
})

test('countSearchHighlights excludes handoff and empty bodies', () => {
  assert.equal(countSearchHighlights(makeMessage({ type: 'handoff' }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content: '  ' }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content: '{}' }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content: '[]' }), 'parser'), 0)
})

test('countSearchHighlights excludes technical handoffs except executor cards', () => {
  const content = '### ROLE: MENTOR\nparser instructions'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content, from: 'executor' }), 'parser'), 1)
})

test('countSearchHighlights excludes structured acceptance bodies', () => {
  const verdict = JSON.stringify({
    verdict: 'pass',
    risk: 'low',
    confidence: 0.9,
    issues: [],
    evidence: ['parser evidence'],
    summary: 'parser ok',
    nextStep: { action: 'finish', instructions: [] }
  })
  assert.equal(
    countSearchHighlights(makeMessage({ type: 'acceptance', content: verdict }), 'parser'),
    0
  )
  assert.equal(countSearchHighlights(makeMessage({ content: verdict }), 'parser'), 0)
})

test('countSearchHighlights counts link text but not link URLs', () => {
  const content = 'Edited [parser.ts](src/parser.ts:12) and see [docs](https://example.com/github)'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'github'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'src/parser'), 0)
})

test('countSearchHighlights counts fenced code but not the fence info string', () => {
  const content = '```python\nparser failed\n```'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'python'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser'), 1)
})

test('countSearchHighlights counts raw HTML because react-markdown renders it literally', () => {
  // Without skipHtml, react-markdown's post step turns the tag into literal
  // text on screen — so both the tag spelling and the text after it match.
  const content = 'see <span title="parser tag">x</span> parser now'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser tag'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser'), 2)
})

test('countSearchHighlights counts unbackticked generics and JSX tags in agent prose', () => {
  // CommonMark parses these as inline HTML; they render literally, so search
  // must find them there too. A query spanning the text/HTML node boundary
  // still never matches — <mark> wraps single nodes.
  const content = 'Wrapped the list in <MessageCard> and returned Array<string> now'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'MessageCard'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'string'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'Array<string>'), 0)
})

test('countSearchHighlights ignores table cells beyond the header width', () => {
  // mdast-util-to-hast renders only header-count cells per body row (the row
  // handler caps children at align.length), so excess cell text never reaches
  // the DOM — counting it would list a card with nothing highlightable.
  const content = '| file | status |\n| --- | --- |\n| `parser.ts` | ok | failed | extra |'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'ok'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'failed'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'extra'), 0)
})

test('countSearchHighlights ignores unreferenced footnote definitions', () => {
  // The hast footer only emits definitions a footnoteReference points at, so
  // an unreferenced definition's paragraph never renders.
  const content = 'Note: the parser needs an options bag.\n\n[^1]: parser detail here'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'detail'), 0)
})

test('countSearchHighlights counts referenced footnote definitions (rendered in the footer)', () => {
  const content = 'See the note[^1] about options.\n\n[^1]: parser detail here'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parser detail'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'See the note'), 1)
})

test('countSearchHighlights counts no footnote chrome (counter digit, backref, heading)', () => {
  // Those texts are generated by mdast-util-to-hast with no mdast node
  // behind them, and rehypeHighlight leaves them unmarked to keep the
  // count === <mark> invariant — so the count side stays 0 for them too.
  const content = 'see[^1] here\n\n[^1]: note'
  assert.equal(countSearchHighlights(makeMessage({ content }), '1'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content }), '↩'), 0)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'Footnotes'), 0)
})

test('countSearchHighlights ignores footnotes referenced only inside unreferenced definitions', () => {
  // The [^2] reference lives inside the [^1] definition, which itself is
  // unreferenced and never renders — so [^2] never converts and its
  // definition never lands in the footer.
  const content = 'foo bar\n\n[^1]: defines one[^2]\n\n[^2]: the parsernote'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parsernote'), 0)
})

test('countSearchHighlights ignores footnotes referenced only in dropped table cells', () => {
  // The third body cell (past the header width) never converts, so the
  // reference it holds cannot pull [^1] into the footer.
  const content = '| a | b |\n| --- | --- |\n| x | y | [^1] |\n\n[^1]: the parsernote'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parsernote'), 0)
})

test('countSearchHighlights counts footnotes referenced from rendered footnotes', () => {
  // The footer converts each rendered definition in reference order and
  // picks up references found inside them, so the chain renders end to end.
  const content = 'see[^1]\n\n[^1]: then[^2]\n\n[^2]: the parsernote'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parsernote'), 1)
})

test('countSearchHighlights expands the first definition per footnote label', () => {
  // footnoteById keeps the first definition of a duplicate label, so only
  // that one's references can pull further definitions into the footer.
  const content = 'see[^1]\n\n[^1]: then[^2]\n\n[^1]: dup body\n\n[^2]: the parsernote'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parsernote'), 1)
})

test('countSearchHighlights ignores duplicate footnote definitions past the first', () => {
  // footnoteById keeps the first definition per label; later duplicates
  // never render, so counting both would overstate the highlights.
  const content = 'ref[^1]\n\n[^1]: alpha parsernote\n\n[^1]: beta parsernote'
  assert.equal(countSearchHighlights(makeMessage({ content }), 'parsernote'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'alpha'), 1)
  assert.equal(countSearchHighlights(makeMessage({ content }), 'beta'), 0)
})

test('countSearchHighlights counts GFM autolinked URLs (rendered as link text)', () => {
  assert.equal(
    countSearchHighlights(makeMessage({ content: 'see https://example.com/github now' }), 'github'),
    1
  )
})

test('countSearchHighlights does not count a query spanning two text nodes', () => {
  // "Edited " and "parser.ts" render as separate text nodes; <mark> wraps one
  // node at a time, so a query crossing the boundary never highlights.
  assert.equal(
    countSearchHighlights(
      makeMessage({ content: 'Edited [parser.ts](src/parser.ts:12)' }),
      'd parser'
    ),
    0
  )
})

test('countSearchHighlights stays warm across passes over long transcripts', () => {
  // PairConsole walks every role-filtered message on each keystroke, twice
  // (pre-dedup filter + post-dedup count). The parse cache must keep that
  // walk warm no matter how many distinct bodies the run has — the old
  // >400-entries clear() wiped the hot set mid-pass, forcing a full remark
  // reparse on every keystroke (~450ms at 450 realistic bodies).
  const body = (i: number): string => {
    const para = `Paragraph ${i} about the parser with \`inline-${i}\` code and a zqneedle mention. `
    let out = ''
    for (let k = 0; k < 50; k++) out += para
    return `${out}\n\n\`\`\`ts\nconst v${i} = compute(${i}) // zqneedle in code\n\`\`\`\n`
  }
  const messages = Array.from({ length: 450 }, (_, i) =>
    makeMessage({ id: `long-${i}`, content: body(i) })
  )

  const pass = (): number[] => messages.map((m) => countSearchHighlights(m, 'zqneedle'))

  const coldStart = performance.now()
  const coldCounts = pass()
  const coldMs = performance.now() - coldStart
  const warmStart = performance.now()
  const warmCounts = pass()
  const warmMs = performance.now() - warmStart

  assert.deepEqual(coldCounts, warmCounts)
  assert.ok(coldCounts.every((c) => c === 51))
  // Cold pays the remark parses; warm must be cache hits, not a reparse.
  assert.ok(warmMs < coldMs / 2, `warm pass ${warmMs.toFixed(1)}ms vs cold ${coldMs.toFixed(1)}ms`)
})

test('displayContentForMessage strips system prompts like MessageCard', () => {
  assert.equal(displayContentForMessage(makeMessage({ content: '  parser plan  ' })), 'parser plan')
  assert.equal(
    displayContentForMessage(
      makeMessage({ from: 'human', type: 'feedback', content: '  parser  ' })
    ),
    'parser'
  )
})

test('countSearchHighlights matches code blocks with whitespace exact', () => {
  const codeBlock = 'Ran:\n```\nstepOne()\nstepTwo()\n```'
  // A space query is a phrase that visibly does not exist in the block.
  assert.equal(countSearchHighlights(makeMessage({ content: codeBlock }), 'stepOne() stepTwo()'), 0)
  // The literal newline query matches the block once.
  assert.equal(
    countSearchHighlights(makeMessage({ content: codeBlock }), 'stepOne()\nstepTwo()'),
    1
  )
})

test('countSearchHighlights keeps collapsed matching for prose and inline code', () => {
  // Inline <code> renders with white-space: normal — a space query typed off
  // the collapsed visible text still matches the raw double-space value.
  const inline = 'Ran `stepOne()  stepTwo()` twice'
  assert.equal(countSearchHighlights(makeMessage({ content: inline }), 'stepOne() stepTwo()'), 1)
  const prose = 'stepOne()\nstepTwo()'
  assert.equal(countSearchHighlights(makeMessage({ content: prose }), 'stepOne() stepTwo()'), 1)
})
