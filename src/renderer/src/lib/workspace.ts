function getLastPathSegment(path: string): string {
  const trimmed = path.replace(/[\\/]+$/u, '')
  if (trimmed === '') return ''

  const segments = trimmed.split(/[\\/]+/u).filter(Boolean)
  return segments.at(-1) ?? trimmed
}

export function derivePairNameFromDirectory(directory: string): string {
  return getLastPathSegment(directory)
}

/**
 * A path short enough for a one-line label that keeps its informative tail:
 * the home directory collapses to `~`, and anything deeper than `maxSegments`
 * keeps only its last segments behind `…`. The full path belongs in a tooltip.
 */
export function compactPath(path: string, maxSegments = 3): string {
  const separator = path.includes('\\') && !path.includes('/') ? '\\' : '/'
  const trimmed = path.replace(/[\\/]+$/u, '')
  if (trimmed === '') return path

  const home = trimmed.match(/^(?:\/Users\/[^\\/]+|\/home\/[^\\/]+|[A-Za-z]:\\Users\\[^\\/]+)/u)
  const rest = home ? trimmed.slice(home[0].length) : trimmed
  const segments = rest.split(/[\\/]+/u).filter(Boolean)
  const head = home ? `~${separator}` : ''

  if (segments.length <= maxSegments) {
    if (!home) return trimmed
    return segments.length === 0 ? '~' : `${head}${segments.join(separator)}`
  }
  return `${head}…${separator}${segments.slice(-maxSegments).join(separator)}`
}
