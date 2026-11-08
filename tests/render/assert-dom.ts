/**
 * DOM-safe assertions for component tests.
 *
 * Never hand a jsdom node to `assert.equal` / `assert.deepEqual`: on failure
 * node:assert renders the actual value with `customInspect: false` and a deep
 * depth, and a jsdom node reaches the whole window graph. The diff string
 * grows to tens of GB within seconds and the test process takes the machine
 * down instead of failing. Assert on a primitive, or use these helpers.
 */
import assert from 'node:assert/strict'

const PREVIEW_LIMIT = 300

function preview(node: Element): string {
  const markup = node.outerHTML
  return markup.length > PREVIEW_LIMIT ? `${markup.slice(0, PREVIEW_LIMIT)}…` : markup
}

/** Fails when `node` is present, printing a short markup preview instead of the node. */
export function assertAbsent(node: Element | null, message = 'expected no element'): void {
  if (node === null) return
  assert.fail(`${message}, found: ${preview(node)}`)
}
