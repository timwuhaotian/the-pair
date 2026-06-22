/**
 * Check if an agent is in an executing phase (thinking, using tools, or responding)
 */
export function isAgentExecuting(phase: string): boolean {
  return phase === 'thinking' || phase === 'using_tools' || phase === 'responding'
}
