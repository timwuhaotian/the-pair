import { findMatchRanges } from './consoleSearch'

interface HastNode {
  type: string
  tagName?: string
  value?: string
  properties?: Record<string, unknown>
  children?: HastNode[]
}

const SKIP_TAGS = new Set(['script', 'style'])

/** Split a hast text value into text/element nodes wrapping matches in <mark>. */
function splitText(value: string, query: string): HastNode[] {
  const ranges = findMatchRanges(value, query)
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

function walk(node: HastNode, query: string): void {
  if (!node.children || (node.tagName && SKIP_TAGS.has(node.tagName))) return
  const next: HastNode[] = []
  for (const child of node.children) {
    if (child.type === 'text' && typeof child.value === 'string') {
      next.push(...splitText(child.value, query))
    } else {
      walk(child, query)
      next.push(child)
    }
  }
  node.children = next
}

/**
 * Rehype plugin factory: wraps search matches in hast <mark> elements.
 * Runs on the hast tree react-markdown builds, so it covers every rendered
 * text node uniformly — including inline code and fenced blocks, where file
 * names and error strings usually live. Only script/style subtrees are skipped.
 */
export function rehypeHighlight(options: { query: string }): () => (tree: HastNode) => void {
  return () => (tree) => {
    if (options.query.trim()) walk(tree, options.query)
  }
}
