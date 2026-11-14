import { existsSync, realpathSync } from 'node:fs'
import { homedir } from 'node:os'
import { delimiter, dirname, join, resolve } from 'node:path'

function executableName(name, platform) {
  return platform === 'win32' ? `${name}.exe` : name
}

function safeRealpath(path) {
  try {
    return realpathSync(path)
  } catch {
    return null
  }
}

/**
 * Locate the directory holding rustup's `cargo`/`rustc` proxies.
 *
 * rustup-init puts them in `$CARGO_HOME/bin`, but a package-manager rustup
 * (Homebrew, as on GitHub's macOS runners) keeps them next to its own binary,
 * often reached only through a symlink. So: follow every `rustup` on PATH first
 * (the one the shell would run), then fall back to Cargo home for shells that
 * never had it on PATH. A directory only counts if it actually has `cargo`.
 */
export function resolveRustupBinDir({
  env = process.env,
  platform = process.platform,
  home = homedir(),
  exists = existsSync,
  realpath = safeRealpath
} = {}) {
  const rustup = executableName('rustup', platform)
  const cargo = executableName('cargo', platform)
  const pathDelimiter = platform === 'win32' ? ';' : ':'
  const pathKey = Object.keys(env).find((key) => key.toLowerCase() === 'path') ?? 'PATH'

  const candidates = []
  for (const dir of (env[pathKey] ?? '').split(pathDelimiter).filter(Boolean)) {
    const rustupPath = join(dir, rustup)
    if (!exists(rustupPath)) {
      continue
    }
    candidates.push(dir)
    const target = realpath(rustupPath)
    if (target) {
      candidates.push(dirname(target))
    }
  }
  candidates.push(join(env.CARGO_HOME ? resolve(env.CARGO_HOME) : join(home, '.cargo'), 'bin'))

  return candidates.find((dir) => exists(join(dir, cargo))) ?? null
}

export function prependPathEntry(entry, currentPath = '', pathDelimiter = delimiter) {
  if (!currentPath) {
    return entry
  }

  return `${entry}${pathDelimiter}${currentPath}`
}
