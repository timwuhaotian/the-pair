type ConsoleMessageLike = {
  id: string
  from: string
  type: string
  iteration?: number
}

function canCollapseMessage(type: string): boolean {
  return type !== 'handoff'
}

function isAgentMessage(message: ConsoleMessageLike): boolean {
  return message.from === 'mentor' || message.from === 'executor'
}

function roleIterationKey(message: ConsoleMessageLike): string {
  return `${message.from}:${message.iteration ?? 'unknown'}`
}

export function collapseConsecutiveConsoleMessages<T extends ConsoleMessageLike>(
  messages: T[]
): T[] {
  const finalMessageIdsByRoleIteration = new Map<string, string>()

  for (const message of messages) {
    if (isAgentMessage(message) && canCollapseMessage(message.type)) {
      finalMessageIdsByRoleIteration.set(roleIterationKey(message), message.id)
    }
  }

  return messages.filter((message) => {
    if (!isAgentMessage(message) || !canCollapseMessage(message.type)) return true
    return finalMessageIdsByRoleIteration.get(roleIterationKey(message)) === message.id
  })
}

/**
 * Same collapse as {@link collapseConsecutiveConsoleMessages}, but also reports
 * how many earlier same-role messages within the same iteration were dropped
 * in favor of each surviving message. The renderer uses this to surface a
 * "+ N earlier messages hidden" marker so users know content was suppressed
 * rather than silently lost.
 */
export function collapseWithDropCounts<T extends ConsoleMessageLike>(
  messages: T[]
): { kept: T[]; droppedBeforeId: Map<string, number> } {
  const finalMessageIdsByRoleIteration = new Map<string, string>()
  const dropCounts = new Map<string, number>()

  for (const message of messages) {
    if (isAgentMessage(message) && canCollapseMessage(message.type)) {
      finalMessageIdsByRoleIteration.set(roleIterationKey(message), message.id)
    }
  }

  const kept: T[] = []
  for (const message of messages) {
    const survives =
      !isAgentMessage(message) ||
      !canCollapseMessage(message.type) ||
      finalMessageIdsByRoleIteration.get(roleIterationKey(message)) === message.id

    if (survives) {
      kept.push(message)
    } else {
      const finalId = finalMessageIdsByRoleIteration.get(roleIterationKey(message))
      if (finalId) {
        dropCounts.set(finalId, (dropCounts.get(finalId) ?? 0) + 1)
      }
    }
  }

  return { kept, droppedBeforeId: dropCounts }
}
