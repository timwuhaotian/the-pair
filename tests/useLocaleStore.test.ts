import assert from 'node:assert/strict'
import test from 'node:test'

import { useLocaleStore, LOCALE_LABELS } from '../src/renderer/src/store/useLocaleStore.ts'

test.afterEach(() => {
  useUpdateStoreReset()
})

function useUpdateStoreReset() {
  useLocaleStore.getState().setLocale('en')
}

test('LOCALE_LABELS has all four locales with native and flag', () => {
  const keys = Object.keys(LOCALE_LABELS)
  assert.ok(keys.includes('en'))
  assert.ok(keys.includes('zh'))
  assert.ok(keys.includes('ja'))
  assert.ok(keys.includes('ko'))

  for (const key of ['en', 'zh', 'ja', 'ko'] as const) {
    assert.equal(typeof LOCALE_LABELS[key].native, 'string')
    assert.ok(LOCALE_LABELS[key].native.length > 0)
    assert.equal(typeof LOCALE_LABELS[key].flag, 'string')
    assert.ok(LOCALE_LABELS[key].flag.length > 0)
  }
})

test('setLocale sets locale to zh', () => {
  useLocaleStore.getState().setLocale('zh')
  assert.equal(useLocaleStore.getState().locale, 'zh')
})

test('setLocale sets locale to ja', () => {
  useLocaleStore.getState().setLocale('ja')
  assert.equal(useLocaleStore.getState().locale, 'ja')
})
