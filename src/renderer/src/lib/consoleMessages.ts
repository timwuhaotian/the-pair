type ConsoleMessageLike = {
  id: string
  from: string
  type: string
  iteration?: number
}

/** Local turn cards the renderer commits (`turn-<role>-iter<N>`); the backend
 *  later reports the same turn as its own message. One turn must not bill twice. */
export const LOCAL_TURN_CARD_ID = /^turn-(?:mentor|executor)-iter\d+$/

function localTurnKey(message: ConsoleMessageLike): string {
  return `${message.from}:${message.iteration ?? 'unknown'}`
}

/**
 * Shared dedup for console / timeline / inspector: drop a local turn card when
 * the backend also reported that turn. The backend message wins — on the direct
 * connection it is the only one of the two that carries usage.
 */
export function dropDuplicatedLocalTurnCards<T extends ConsoleMessageLike>(
  messages: ReadonlyArray<T>
): T[] {
  const reportedTurns = new Set(
    messages.filter((message) => !LOCAL_TURN_CARD_ID.test(message.id)).map(localTurnKey)
  )
  return messages.filter(
    (message) => !LOCAL_TURN_CARD_ID.test(message.id) || !reportedTurns.has(localTurnKey(message))
  )
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
