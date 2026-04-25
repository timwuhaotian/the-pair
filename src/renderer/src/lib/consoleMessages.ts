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
