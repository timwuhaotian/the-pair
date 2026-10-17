import { findMatchRanges } from './consoleSearch'

interface HastNode {
  type: string
  tagName?: string
  value?: string
  properties?: Record<string, unknown>
  children?: HastNode[]
}

const SKIP_TAGS = new Set(['script', 'style'])

// mdast-util-to-hast synthesizes footnote chrome with no mdast text node
// behind it: the `sup>a` counter, the `↩` backref, and the footer's sr-only
// "Footnotes" heading. searchableCard counts mdast text only, so wrapping a
// match in chrome would render a <mark> that never counts — leave it plain.
function isGeneratedFootnoteChrome(node: HastNode): boolean {
  if (node.type !== 'element' || !node.properties) return false
  if (node.properties.dataFootnoteRef === true) return true
  if ('dataFootnoteBackref' in node.properties) return true
  return node.tagName === 'h2' && node.properties.id === 'footnote-label'
}

/** Split a hast text value into text/element nodes wrapping matches in <mark>. */
function splitText(value: string, query: string, literal: boolean): HastNode[] {
  const ranges = findMatchRanges(value, query, { literal })
  if (ranges.length === 0) return [{ type: 'text', value }]
  const out: HastNode[] = []
  let cursor = 0
  for (const [start, end] of ranges) {
    if (start > cursor) out.push({ type: 'text', value: value.slice(cursor, start) })
    out.push({
      type: 'element',
      tagName: 'mark',
      properties: {},
      children: [{ type: 'text', value: value.slice(start, end) }]
    })
    cursor = end
  }
  if (cursor < value.length) out.push({ type: 'text', value: value.slice(cursor) })
  return out
}

function walk(node: HastNode, query: string, inPre: boolean): void {
  if (!node.children || (node.tagName && SKIP_TAGS.has(node.tagName))) return
  // Inside <pre> whitespace renders literally (fenced/indented code), so
  // matches must be literal too. Inline <code> renders with white-space:
  // normal and keeps collapsed matching, same as searchableCard's count.
  const childInPre = inPre || node.tagName === 'pre'
  const next: HastNode[] = []
  for (const child of node.children) {
    // `raw` nodes are inline HTML; react-markdown's post step (which runs
    // after rehype plugins) renders them as literal text, so match them here.
    if ((child.type === 'text' || child.type === 'raw') && typeof child.value === 'string') {
      next.push(...splitText(child.value, query, childInPre))
    } else {
      if (!isGeneratedFootnoteChrome(child)) walk(child, query, childInPre)
      next.push(child)
    }
  }
  node.children = next
}

/**
 * Rehype plugin factory: wraps search matches in hast <mark> elements.
 * Runs on the hast tree react-markdown builds, so it covers every rendered
 * text node uniformly — including inline code, fenced blocks, and raw inline
 * HTML, where file names, error strings, and unbackticked tag names usually
 * live. Text under <pre> matches whitespace literally; everything else
 * collapses it the way it renders. Script/style subtrees and generated
 * footnote chrome are skipped (see isGeneratedFootnoteChrome).
 */
export function rehypeHighlight(options: { query: string }): () => (tree: HastNode) => void {
  return () => (tree) => {
    if (options.query.trim()) walk(tree, options.query, false)
  }
}
