import {
  isAcceptanceRecordContent,
  parseAcceptanceRecordForDisplay,
  parseAcceptanceVerdictForDisplay
} from './acceptance'
import type { Tone } from './ui'

/**
 * State tone for an acceptance verdict: pass reads as done, fail (rework
 * needed) as running. A verdict that hasn't landed yet stays running too.
 */
export function verdictTone(verdict: string | null | undefined): Tone {
  return verdict === 'pass' ? 'done' : 'running'
}

/**
 * Tone for an acceptance message body. A record without a verdict falls back
 * to its checks: any failed check is an error, all passing is done.
 */
export function acceptanceContentTone(content: string): Tone {
  try {
    if (isAcceptanceRecordContent(content)) {
      const record = parseAcceptanceRecordForDisplay(content)
      if (record.verdict) return verdictTone(record.verdict.verdict)
      return record.checks.some((c) => c.status === 'failed') ? 'error' : 'done'
    }
    return verdictTone(parseAcceptanceVerdictForDisplay(content).verdict)
  } catch {
    return 'running'
  }
}
