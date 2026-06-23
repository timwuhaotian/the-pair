import { homedir } from 'node:os'
import { delimiter, join, resolve } from 'node:path'

function resolveRustupHome() {
  if (process.env.CARGO_HOME) {
    return resolve(process.env.CARGO_HOME)
  }

  return join(homedir(), '.cargo')
}

export function getRustupBinDir() {
  return join(resolveRustupHome(), 'bin')
}

export function prependPathEntry(entry, currentPath = '', pathDelimiter = delimiter) {
  if (!currentPath) {
    return entry
  }

  return `${entry}${pathDelimiter}${currentPath}`
}
