import { mkdirSync, readFileSync, writeFileSync, existsSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import process from 'node:process'

const platformTargets = {
  macos: {
    targets: ['darwin-x86_64', 'darwin-aarch64'],
    assetName: (version) => `the-pair-${version}.app.tar.gz`,
    signatureName: (version) => `the-pair-${version}.app.tar.gz.sig`
  },
  windows: {
    target: 'windows-x86_64',
    assetName: (version) => `the-pair-${version}-setup.exe`,
    signatureName: (version) => `the-pair-${version}-setup.exe.sig`
  },
  linux: {
    target: 'linux-x86_64',
    assetName: (version) => `the-pair-${version}.AppImage.tar.gz`,
    signatureName: (version) => `the-pair-${version}.AppImage.tar.gz.sig`
  }
}

function parseArgs(argv) {
  const options = {}

  for (let index = 0; index < argv.length; index += 1) {
    const token = argv[index]
    if (!token.startsWith('--')) {
      continue
    }

    const next = argv[index + 1]
    if (!next || next.startsWith('--')) {
      throw new Error(`Missing value for ${token}`)
    }

    options[token.slice(2)] = next
    index += 1
  }

  return options
}

function readNotes(notesPath) {
  if (!notesPath || !existsSync(notesPath)) {
    return ''
  }

  return readFileSync(notesPath, 'utf8').trim()
}

export function buildManifest({ version, baseUrl, distDir = 'dist', notesPath, pubDate }) {
  const notes = readNotes(notesPath)
  const platforms = {}

  for (const config of Object.values(platformTargets)) {
    const assetName = config.assetName(version)
    const assetPath = join(distDir, assetName)
    const assetStat = statSync(assetPath, { throwIfNoEntry: false })
    if (!assetStat?.isFile() || assetStat.size === 0) {
      throw new Error(`Missing or empty updater asset: ${assetPath}`)
    }
    const assetUrl = `${baseUrl}/${assetName}`
    const signaturePath = join(distDir, config.signatureName(version))
    const signatureStat = statSync(signaturePath, { throwIfNoEntry: false })
    if (!signatureStat?.isFile()) {
      throw new Error(`Missing or empty updater signature: ${signaturePath}`)
    }
    const signature = readFileSync(signaturePath, 'utf8').trim()
    if (!signature) {
      throw new Error(`Missing or empty updater signature: ${signaturePath}`)
    }

    const targets = config.targets ?? [config.target]
    for (const target of targets) {
      platforms[target] = {
        url: assetUrl,
        signature
      }
    }
  }

  return {
    version,
    pub_date: pubDate ?? new Date().toISOString(),
    notes,
    platforms
  }
}

function main() {
  const options = parseArgs(process.argv.slice(2))
  const version = options.version
  const baseUrl = options['base-url']
  const distDir = options['dist-dir'] ?? 'dist'
  const notesPath = options['notes-file']
  const output = options.output ?? 'dist/latest.json'
  const pubDate = options['pub-date']

  if (!version || !baseUrl) {
    throw new Error(
      'Usage: node scripts/generate-updater-manifest.mjs --version <version> --base-url <url> [--notes-file <path>] [--output <path>] [--pub-date <iso8601>]'
    )
  }

  const manifest = buildManifest({ version, baseUrl, distDir, notesPath, pubDate })
  mkdirSync(dirname(output), { recursive: true })
  writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`)
  console.log(`Wrote updater manifest to ${output}`)
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  try {
    main()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  }
}
