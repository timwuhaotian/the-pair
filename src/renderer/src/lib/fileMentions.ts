/**
 * Helpers for `@`-mentioned workspace files attached to a task spec.
 *
 * The renderer collects file contents as the user picks them in `FileMention`,
 * then prepends them as a context block before the user's spec when the task
 * is sent to the agent.
 */

export type FileContexts = Map<string, string>

/**
 * Returns only the mentions that still appear as `@path` substrings in the spec.
 * Users may have deleted some mentions after selecting them; we drop those.
 */
export function selectReferencedFiles(
  spec: string,
  fileContexts: FileContexts
): Array<[string, string]> {
  return Array.from(fileContexts.entries()).filter(([path]) => spec.includes(`@${path}`))
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
