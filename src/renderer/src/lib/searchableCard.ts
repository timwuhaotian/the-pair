import { unified } from 'unified'
import remarkParse from 'remark-parse'
import remarkGfm from 'remark-gfm'
import { isAcceptanceRecordContent, isAcceptanceVerdictContent } from './acceptance'
import { countOccurrences } from './consoleSearch'
import { isTechnicalHandoff } from './timeline'
import { stripSystemPrompt } from './utils'
import type { Message } from '../store/usePairStore'

/** The body MessageCard renders for a message (system prompt stripped where it does so). */
export function displayContentForMessage(msg: Message): string {
  const isHuman = msg.from === 'human'
  return isHuman || isTechnicalHandoff(msg.content.trim())
    ? stripSystemPrompt(msg.content.trim())
    : msg.content.trim()
}

interface MdastNode {
  type: string
  value?: string
  identifier?: string
  children?: MdastNode[]
}

// Same parser config as MarkdownContent, so the text nodes collected here are
// exactly the ones rehypeHighlight wraps in <mark>.
const markdownParser = unified().use(remarkParse).use(remarkGfm)

/** One mdast text value plus whether it renders with whitespace exact (`<pre>`). */
interface RenderedText {
  value: string
  literal: boolean
}

// Keyed on the Message object: the store never mutates messages in place
// (it replaces them), so entries live exactly as long as the message and GC
// bounds the cache — no size cap, no clear() that wipes the hot set mid-pass
// on long transcripts.
const renderedTextsByMessage = new WeakMap<Message, RenderedText[]>()

// Footnote definitions only reach the DOM through the footer, which the
// mdast→hast walk fills from references that themselves render: root-level
// definitions are dropped from the flow, table rows cap at the header width
// (so references in excess cells never convert), and the footer loop
// re-scans as it converts each rendered definition (so chains resolve
// transitively). Mirror that with a walk over render-reachable nodes plus a
// worklist over the definitions it pulls in.
function collectRenderedFootnoteIds(tree: MdastNode): Set<string> {
  // First definition per label wins — footnoteById keeps the first.
  const firstDefById = new Map<string, MdastNode>()
  const indexDefinitions = (node: MdastNode): void => {
    if (node.type === 'footnoteDefinition' && node.identifier) {
      const id = node.identifier.toUpperCase()
      if (!firstDefById.has(id)) firstDefById.set(id, node)
    }
    for (const child of node.children ?? []) indexDefinitions(child)
  }
  indexDefinitions(tree)

  const rendered = new Set<string>()
  // Definition subtrees are skipped here: their content only converts when
  // the footer renders them, which the worklist below drives.
  const walkReferences = (node: MdastNode): void => {
    if (node.type === 'footnoteReference' && node.identifier) {
      rendered.add(node.identifier.toUpperCase())
      return
    }
    if (node.type === 'footnoteDefinition') return
    if (node.type === 'table') {
      const width = node.children?.[0]?.children?.length ?? 0
      for (const row of node.children ?? []) {
        for (const cell of row.children?.slice(0, width) ?? []) walkReferences(cell)
      }
      return
    }
    for (const child of node.children ?? []) walkReferences(child)
  }
  walkReferences(tree)

  const expanded = new Set<string>()
  const queue = [...rendered]
  while (queue.length > 0) {
    const id = queue.shift()
    if (id === undefined || expanded.has(id)) continue
    expanded.add(id)
    const def = firstDefById.get(id)
    if (!def) continue
    for (const child of def.children ?? []) walkReferences(child)
    for (const discovered of rendered) if (!expanded.has(discovered)) queue.push(discovered)
  }
  return rendered
}

// text/inlineCode/code/html are the mdast nodes whose values reach the DOM as
// literal text — react-markdown (without skipHtml) turns inline HTML into text
// in its post step. Everything else (link URLs, fence info strings, image alt)
// renders as markup or attributes, never as highlightable text.
//
// Whitespace mode mirrors rendering: `code` (fenced/indented blocks) lands in
// `<pre>`, where newlines and space runs stay exact, so it matches literally.
// text/inlineCode/html render in normal flow (inline `<code>` gets no
// white-space override), so they keep collapsed matching.
function collectTextNodes(
  node: MdastNode,
  out: RenderedText[],
  footnoteIds: Set<string>,
  emittedFootnoteDefs: Set<string>
): void {
  if (node.type === 'code') {
    if (node.value) out.push({ value: node.value, literal: true })
    return
  }
  if (node.type === 'text' || node.type === 'inlineCode' || node.type === 'html') {
    if (node.value) out.push({ value: node.value, literal: false })
    return
  }
  // A footnote definition renders only when referenced, and only the first
  // definition per label does (footnoteById keeps the first) — later
  // duplicates never reach the footer, so their text must not count.
  if (node.type === 'footnoteDefinition') {
    const id = String(node.identifier).toUpperCase()
    if (!footnoteIds.has(id) || emittedFootnoteDefs.has(id)) return
    emittedFootnoteDefs.add(id)
  }
  // Body rows past the header width never render either: the hast table-row
  // handler caps every row at the header row's cell count.
  if (node.type === 'table') {
    const width = node.children?.[0]?.children?.length ?? 0
    for (const row of node.children ?? []) {
      for (const cell of row.children?.slice(0, width) ?? []) {
        collectTextNodes(cell, out, footnoteIds, emittedFootnoteDefs)
      }
    }
    return
  }
  for (const child of node.children ?? [])
    collectTextNodes(child, out, footnoteIds, emittedFootnoteDefs)
}

/** The text values this markdown renders — the only places a match can highlight. */
function renderedSearchTexts(msg: Message): RenderedText[] {
  const cached = renderedTextsByMessage.get(msg)
  if (cached) return cached
  const tree = markdownParser.parse(displayContentForMessage(msg)) as unknown as MdastNode
  const footnoteIds = collectRenderedFootnoteIds(tree)
  const out: RenderedText[] = []
  collectTextNodes(tree, out, footnoteIds, new Set())
  renderedTextsByMessage.set(msg, out)
  return out
}

/**
 * Number of search highlights this message will actually render as <mark>.
 * Mirrors MessageCard's render branches: only agent messages rendered as a
 * TerminalBlock through MarkdownContent highlight (and carry the
 * `console-msg-<id>` anchor). Human banners, handoffs, technical handoffs,
 * empty bodies, and structured acceptance bodies render none. Matches are
 * counted per rendered text node — <mark> wraps single text nodes, so a
 * query spanning two nodes never highlights and must not count.
 */
export function countSearchHighlights(msg: Message, query: string): number {
  if (!query.trim() || msg.from === 'human' || msg.type === 'handoff') return 0
  const displayContent = displayContentForMessage(msg)
  if (!displayContent || displayContent === '{}' || displayContent === '[]') return 0
  if (isTechnicalHandoff(displayContent) && msg.from !== 'executor') return 0
  if (
    msg.type === 'acceptance' ||
    isAcceptanceVerdictContent(displayContent) ||
    isAcceptanceRecordContent(displayContent)
  ) {
    return 0
  }
  return renderedSearchTexts(msg).reduce(
    (n, text) => n + countOccurrences(text.value, query, { literal: text.literal }),
    0
  )
}
