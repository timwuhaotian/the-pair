import { isAcceptanceRecordContent, isAcceptanceVerdictContent } from './acceptance'
import { countOccurrences } from './consoleSearch'
import { isTechnicalHandoff } from './timeline'
import { stripSystemPrompt } from './utils'
import type { Message } from '../store/usePairStore'

/** The body MessageCard renders for a message (system prompt stripped where it does so). */
export function displayContentForMessage(msg: Message): string {
  const isHuman = msg.from === 'human'
  return isHuman || isTechnicalHandoff(msg.content)
    ? stripSystemPrompt(msg.content.trim())
    : msg.content.trim()
}

/**
 * Number of search highlights this message will actually render as <mark>.
 * Mirrors MessageCard's render branches: only agent messages rendered as a
 * TerminalBlock through MarkdownContent highlight (and carry the
 * `console-msg-<id>` anchor). Human banners, handoffs, technical handoffs,
 * empty bodies, and structured acceptance bodies render none.
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
  return countOccurrences(displayContent, query)
}
