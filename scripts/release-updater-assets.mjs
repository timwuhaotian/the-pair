import { copyFileSync, mkdirSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join, resolve } from 'node:path'
import process from 'node:process'

const platformConfigs = {
  macos: {
    subdir: 'macos',
    assets: [
      {
        patterns: [/\.app\.tar\.gz$/i],
        outputName: (version) => `the-pair-${version}.app.tar.gz`
      }
    ]
  },
  windows: {
    subdir: 'nsis',
    assets: [
      {
        patterns: [/-setup\.exe$/i],
        outputName: (version) => `the-pair-${version}-setup.exe`
      }
    ]
  },
  linux: {
    subdir: 'appimage',
    assets: [
      {
        patterns: [/\.AppImage\.tar\.gz$/i],
        outputName: (version) => `the-pair-${version}.AppImage.tar.gz`
      }
    ]
  }
}

function walkFiles(rootDir) {
  const entries = []

  for (const entry of readdirSync(rootDir, { withFileTypes: true }).sort((left, right) =>
    left.name.localeCompare(right.name)
  )) {
    const absolutePath = join(rootDir, entry.name)
    if (entry.isDirectory()) {
      entries.push(...walkFiles(absolutePath))
      continue
    }

    if (entry.isFile()) {
      entries.push(absolutePath)
    }
  }

  return entries
}

function findMatchingFile(rootDir, patterns) {
  const candidates = walkFiles(rootDir).filter((filePath) =>
    patterns.some((pattern) => pattern.test(filePath))
  )

  if (candidates.length !== 1) {
    throw new Error(`Expected exactly one updater asset in ${rootDir}, found ${candidates.length}`)
  }

  return candidates[0]
}

export function collectUpdaterAssets({ platform, bundleRoot, outDir, version }) {
  const config = platformConfigs[platform]
  if (!config) {
    throw new Error(`Unsupported platform: ${platform}`)
  }

  const searchRoot = resolve(bundleRoot, config.subdir)
  if (!statSync(searchRoot, { throwIfNoEntry: false })) {
    throw new Error(`Expected bundle directory at ${searchRoot}`)
  }

  const files = config.assets.flatMap((asset) => {
    const source = findMatchingFile(searchRoot, asset.patterns)
    if (statSync(source).size === 0) {
      throw new Error(`Empty updater asset: ${source}`)
    }

    const signature = `${source}.sig`
    const signatureStat = statSync(signature, { throwIfNoEntry: false })
    if (!signatureStat?.isFile() || !readFileSync(signature, 'utf8').trim()) {
      throw new Error(`Missing or empty updater signature: ${signature}`)
    }

    const destination = join(outDir, asset.outputName(version))
    return [
      { source, destination },
      { source: signature, destination: `${destination}.sig` }
    ]
  })

  mkdirSync(outDir, { recursive: true })
  for (const { source, destination } of files) {
    copyFileSync(source, destination)
  }
  return files
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

function main() {
  const options = parseArgs(process.argv.slice(2))
  const platform = options.platform
  const bundleRoot = options['bundle-root']
  const outDir = options['out-dir']
  const version = options.version

  if (!platform || !bundleRoot || !outDir || !version) {
    throw new Error(
      'Usage: node scripts/release-updater-assets.mjs --platform <macos|windows|linux> --bundle-root <path> --out-dir <path> --version <version>'
    )
  }

  const result = collectUpdaterAssets({ platform, bundleRoot, outDir, version })
  for (const item of result) {
    console.log(`Copied ${item.source} -> ${item.destination}`)
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
