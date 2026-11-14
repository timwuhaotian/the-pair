import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import process from 'node:process'

import { resolveRustupBinDir } from './rustup-env.mjs'

function loadPackageJson() {
  const packageJsonUrl = new URL('../package.json', import.meta.url)
  return JSON.parse(readFileSync(packageJsonUrl, 'utf8'))
}

function parseVersion(version) {
  const match = String(version)
    .trim()
    .match(/^v?(\d+)(?:\.(\d+))?(?:\.(\d+))?/)
  if (!match) {
    return null
  }

  return {
    major: Number(match[1]),
    minor: Number(match[2] ?? '0'),
    patch: Number(match[3] ?? '0')
  }
}

function compareVersions(left, right) {
  if (left.major !== right.major) {
    return left.major - right.major
  }

  if (left.minor !== right.minor) {
    return left.minor - right.minor
  }

  return left.patch - right.patch
}

export function parseMinimumNodeRange(range) {
  const match = String(range)
    .trim()
    .match(/^>=\s*v?(\d+)(?:\.(\d+))?(?:\.(\d+))?$/)
  if (!match) {
    return null
  }

  return {
    major: Number(match[1]),
    minor: Number(match[2] ?? '0'),
    patch: Number(match[3] ?? '0')
  }
}

export function normalizePlatform(value) {
  const platform = (value ?? process.platform).toLowerCase()

  if (platform === 'darwin' || platform === 'mac' || platform === 'macos') {
    return 'macos'
  }

  if (platform === 'win32' || platform === 'windows' || platform === 'win') {
    return 'windows'
  }

  if (platform === 'linux') {
    return 'linux'
  }

  return platform
}

export function getCrossBuildNotes({ hostPlatform, targetPlatform }) {
  if (targetPlatform === 'macos' && hostPlatform !== 'macos') {
    return ['macOS bundles must be built on macOS.']
  }

  if (targetPlatform === 'windows' && hostPlatform === 'macos') {
    return ['Windows cross-builds from macOS need `llvm-rc` and the Windows Rust target toolchain.']
  }

  if (targetPlatform === 'linux' && hostPlatform === 'macos') {
    return [
      'Linux cross-builds from macOS can require `pkg-config`, a Linux sysroot, and extra system libraries.'
    ]
  }

  return []
}

function checkNodeVersion() {
  const packageJson = loadPackageJson()
  const requiredRange = packageJson.engines?.node
  const requiredMinimum = requiredRange ? parseMinimumNodeRange(requiredRange) : null
  const currentVersion = parseVersion(process.version)

  if (!requiredRange || !requiredMinimum || !currentVersion) {
    return { ok: true, message: null }
  }

  if (compareVersions(currentVersion, requiredMinimum) < 0) {
    return {
      ok: false,
      message: `Node ${process.version} is too old. The project requires ${requiredRange}. Install a newer Node version before building.`
    }
  }

  return { ok: true, message: null }
}

function checkRustup() {
  try {
    execFileSync('rustup', ['--version'], { stdio: 'ignore' })
  } catch (error) {
    const hint = 'Install Rust with rustup and make sure `rustup` is available on your PATH.'
    if (error?.code === 'ENOENT') {
      return { ok: false, message: `rustup was not found. ${hint}` }
    }

    return { ok: false, message: `Unable to run rustup. ${hint}` }
  }

  if (!resolveRustupBinDir()) {
    return {
      ok: false,
      message:
        'rustup is installed but its `cargo` proxy was not found next to any `rustup` on PATH or in Cargo home. Run `rustup default stable` or reinstall Rust with rustup.'
    }
  }

  return { ok: true, message: null }
}

export function runPreflight(platformArg = process.platform) {
  const targetPlatform = normalizePlatform(platformArg)
  const hostPlatform = normalizePlatform(process.platform)
  const notes = getCrossBuildNotes({ hostPlatform, targetPlatform })

  const nodeCheck = checkNodeVersion()
  const rustupCheck = checkRustup()
  const failures = [nodeCheck, rustupCheck].filter((item) => !item.ok)

  console.log(`Preflight target: ${targetPlatform}`)
  console.log(`Host platform: ${hostPlatform}`)
  console.log(`Node: ${process.version}`)

  if (notes.length > 0) {
    for (const note of notes) {
      console.log(`Note: ${note}`)
    }
  }

  if (nodeCheck.message) {
    console.error(nodeCheck.message)
  }

  if (rustupCheck.message) {
    console.error(rustupCheck.message)
  }

  return {
    ok: failures.length === 0,
    failures,
    notes
  }
}

function parseCliArgs(argv) {
  let platform = null

  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index]

    if (token === '--platform' || token === '-p') {
      platform = argv[index + 1] ?? null
      index += 1
      continue
    }

    if (token.startsWith('--')) {
      continue
    }

    if (!platform) {
      platform = token
    }
  }

  return { platform }
}

function main() {
  const { platform } = parseCliArgs(process.argv.slice(2))
  const result = runPreflight(platform ?? process.platform)

  if (!result.ok) {
    process.exit(1)
  }
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  }
}
