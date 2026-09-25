import assert from 'node:assert/strict'
import test from 'node:test'

import {
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
  // A path like ` /tmp` must submit instead of becoming a fuzzy-matched skill.
  assert.equal(shouldAcceptSkillOnEnter('tmp', 'template-maker', false), false)
  // Explicit arrow-key navigation always wins.
  assert.equal(shouldAcceptSkillOnEnter('tmp', 'template-maker', true), true)
})
