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

// Keyed on the Message object: the store never mutates messages in place
// (it replaces them), so entries live exactly as long as the message and GC
// bounds the cache — no size cap, no clear() that wipes the hot set mid-pass
// on long transcripts.
const renderedTextsByMessage = new WeakMap<Message, string[]>()

// Footnote definitions only reach the DOM when a footnoteReference points at
// them (mdast→hast renders referenced ones in the footer section, drops the
// rest), so the walk needs the referenced identifier set up front.
function collectFootnoteIdentifiers(node: MdastNode, out: Set<string>): void {
  if (node.type === 'footnoteReference' && node.identifier) {
    out.add(node.identifier.toUpperCase())
    return
  }
  for (const child of node.children ?? []) collectFootnoteIdentifiers(child, out)
}

// text/inlineCode/code/html are the mdast nodes whose values reach the DOM as
// literal text — react-markdown (without skipHtml) turns inline HTML into text
// in its post step. Everything else (link URLs, fence info strings, image alt)
// renders as markup or attributes, never as highlightable text.
function collectTextNodes(node: MdastNode, out: string[], footnoteIds: Set<string>): void {
  if (
    node.type === 'text' ||
    node.type === 'inlineCode' ||
    node.type === 'code' ||
    node.type === 'html'
  ) {
    if (node.value) out.push(node.value)
    return
  }
  // An unreferenced footnote definition never renders — its text must not
  // count, or a card would list with nothing highlightable inside it.
  if (node.type === 'footnoteDefinition' && !footnoteIds.has(String(node.identifier).toUpperCase()))
    return
  // Body rows past the header width never render either: the hast table-row
  // handler caps every row at the header row's cell count.
  if (node.type === 'table') {
    const width = node.children?.[0]?.children?.length ?? 0
    for (const row of node.children ?? []) {
      for (const cell of row.children?.slice(0, width) ?? []) {
        collectTextNodes(cell, out, footnoteIds)
      }
    }
    return
  }
  for (const child of node.children ?? []) collectTextNodes(child, out, footnoteIds)
}

/** The text values this markdown renders — the only places a match can highlight. */
function renderedSearchTexts(msg: Message): string[] {
  const cached = renderedTextsByMessage.get(msg)
  if (cached) return cached
  const tree = markdownParser.parse(displayContentForMessage(msg)) as unknown as MdastNode
  const footnoteIds = new Set<string>()
  collectFootnoteIdentifiers(tree, footnoteIds)
  const out: string[] = []
  collectTextNodes(tree, out, footnoteIds)
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
  return renderedSearchTexts(msg).reduce((n, text) => n + countOccurrences(text, query), 0)
}
