import assert from 'node:assert/strict'
import test from 'node:test'

import {
  placeMentionPopup,
  replaceFileMentionToken,
  shouldAcceptSkillOnEnter
} from '../src/renderer/src/components/mentionText.ts'

test('replaceFileMentionToken swaps the @query in front of the cursor for the picked path', () => {
  const text = 'look at @src/ap and fix'
  const cursor = 'look at @src/ap'.length
  assert.deepEqual(replaceFileMentionToken(text, cursor, 'src/app.ts'), {
    value: 'look at @src/app.ts and fix',
    cursor: 'look at @src/app.ts'.length
  })
})

test('replaceFileMentionToken refuses to splice when the @ token is gone', () => {
  // The user deleted the `@` while the file content was being read.
  assert.equal(replaceFileMentionToken('look at src', 11, 'src/app.ts'), null)
  // The cursor moved past whitespace — the token is no longer being typed.
  assert.equal(replaceFileMentionToken('@src done', 9, 'src/app.ts'), null)
})

test('Enter only picks a skill when the match is unambiguous', () => {
  assert.equal(shouldAcceptSkillOnEnter('', 'review', false), true)
  assert.equal(shouldAcceptSkillOnEnter('rev', 'code-review', false), true)
  assert.equal(shouldAcceptSkillOnEnter('REV', 'code-review', false), true)
  // Fuzzy and description matches pick the highlighted skill, like Tab does.
  assert.equal(shouldAcceptSkillOnEnter('codereview', 'code-review', false), true)
  assert.equal(shouldAcceptSkillOnEnter('bugs', 'code-review', false), true)
  // Paths like ` /tmp/out`, ` /~/notes` or ` /./x` submit instead of becoming a skill.
  assert.equal(shouldAcceptSkillOnEnter('tmp/out', 'template-maker', false), false)
  assert.equal(shouldAcceptSkillOnEnter('~/notes', 'note-taker', false), false)
  assert.equal(shouldAcceptSkillOnEnter('./x', 'xray', false), false)
  // Explicit arrow-key navigation always wins.
  assert.equal(shouldAcceptSkillOnEnter('tmp/out', 'template-maker', true), true)
})

test('placeMentionPopup opens below the caret when the popover fits there', () => {
  const caret = { top: 100, bottom: 118, left: 40 }
  assert.deepEqual(placeMentionPopup(caret, 800, 256), { top: 96, left: 40 })
})

test('placeMentionPopup opens above the caret when the bottom-docked input leaves no room below', () => {
  // Caret line sits near the bottom of an 800px window (the pinned composer).
  const caret = { top: 760, bottom: 778, left: 40 }
  assert.deepEqual(placeMentionPopup(caret, 800, 256), { bottom: 18, left: 40 })
})

test('placeMentionPopup stays below when neither side fits but below has more room', () => {
  const caret = { top: 100, bottom: 118, left: 40 }
  assert.deepEqual(placeMentionPopup(caret, 300, 256), { top: 96, left: 40 })
})
