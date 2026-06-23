import { clsx, type ClassValue } from 'clsx'
import { twMerge } from 'tailwind-merge'

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs))
}

export function extractErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as { message: unknown }).message)
  }
  return fallback
}

/**
 * Format an iteration count against its budget. A `max` of 0 (or absent) means
 * unlimited → renders as "∞" instead of a finite cap.
 */
export function formatIterations(current: number, max?: number): string {
  if (!max || max <= 0) return `${current}/∞`
  return `${current}/${max}`
}

export function stripSystemPrompt(content: string): string {
  let result = content

  // Only strip handoff prompts if they look like complete system prompts
  // (start with ROLE: and contain the full prompt structure)
  const isHandoffPrompt =
    content.includes('### ROLE:') ||
    content.includes('--- COMMAND TO EXECUTE ---') ||
    content.includes('--- REVIEW REQUEST ---') ||
    content.startsWith('ROLE: MENTOR.')

  if (isHandoffPrompt) {
    // Check if there's actual user content after the prompt markers
    const lines = result.split('\n')
    let userContentStart = 0
    for (let i = 0; i < lines.length; i++) {
      const trimmed = lines[i].trim()
      if (
        trimmed &&
        !trimmed.startsWith('###') &&
        !trimmed.startsWith('---') &&
        !trimmed.startsWith('- ') &&
        !trimmed.startsWith('ROLE:') &&
        !trimmed.startsWith('Your mission') &&
        !trimmed.startsWith('Your ONLY') &&
        !trimmed.startsWith('DO NOT') &&
        !trimmed.startsWith('JUST') &&
        !trimmed.startsWith('Never output') &&
        i > 3
      ) {
        userContentStart = i
        break
      }
    }
    if (userContentStart > 0) {
      result = lines.slice(userContentStart).join('\n').trim()
    } else {
      // Fall back to regex-based stripping
      const mentorTaskPattern = /^ROLE: MENTOR\..*?TASK:\s*/is
      if (mentorTaskPattern.test(result)) {
        result = result.replace(mentorTaskPattern, '')
      }

      const executorCmdPattern = /^### ROLE: EXECUTOR\n.*?--- COMMAND TO EXECUTE ---\n*/is
      if (executorCmdPattern.test(result)) {
        result = result.replace(executorCmdPattern, '')
      }

      const mentorReviewPattern = /^### ROLE: MENTOR\n.*?--- REVIEW REQUEST ---\n*/is
      if (mentorReviewPattern.test(result)) {
        result = result.replace(mentorReviewPattern, '')
      }

      const roleHeaderPattern = /^### ROLE: \w+\s*\n.*?\n\n/s
      if (roleHeaderPattern.test(result)) {
        result = result.replace(roleHeaderPattern, '')
      }

      result = result.replace(/^- DO NOT.*$/gm, '')
      result = result.replace(/^- You CANNOT.*$/gm, '')
      result = result.replace(/^- Never output.*$/gm, '')
      result = result.replace(/^- YOUR GOAL:.*$/gm, '')
      result = result.replace(/^- \w+ GOAL:.*$/gm, '')
    }
  }

  return result.replace(/\n{3,}/g, '\n\n').trim()
}
