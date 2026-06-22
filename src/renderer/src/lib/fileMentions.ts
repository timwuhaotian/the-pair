/**
 * Helpers for `@`-mentioned workspace files attached to a task spec.
 *
 * The renderer collects file contents as the user picks them in `FileMention`,
 * then prepends them as a context block before the user's spec when the task
 * is sent to the agent.
 */

export type FileContexts = Map<string, string>

/** Builds a regex matching `@path` only when it is not immediately followed by a
 * path-continuation char, so a shorter path (`@lib/api`) is not falsely matched
 * inside a longer mention (`@lib/api-v2`). Mirrors the boundary handling in
 * `skillMentions.tokenMatcher`. */
function mentionMatcher(path: string): RegExp {
  const escaped = path.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  return new RegExp(`@${escaped}(?![A-Za-z0-9_./-])`)
}

/**
 * Returns only the mentions that still appear as `@path` in the spec.
 * Users may have deleted some mentions after selecting them; we drop those.
 */
export function selectReferencedFiles(
  spec: string,
  fileContexts: FileContexts
): Array<[string, string]> {
  return Array.from(fileContexts.entries()).filter(([path]) => mentionMatcher(path).test(spec))
}

/**
 * Prepends a `--- REFERENCED FILES ---` block with the picked file contents.
 * Returns the spec unchanged when there are no referenced files.
 */
export function prependFileContext(spec: string, fileContexts: FileContexts): string {
  const referenced = selectReferencedFiles(spec, fileContexts)
  if (referenced.length === 0) return spec

  const header =
    '--- REFERENCED FILES ---\n' +
    referenced.map(([path, content]) => `@${path}:\n${content}`).join('\n\n') +
    '\n\n--- TASK ---\n'
  return header + spec
}
