import { createHash } from 'node:crypto'
import { appendFileSync, createReadStream, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

export function requiredAssets(version) {
  return [
    `the-pair-${version}.zip`,
    `the-pair-${version}.app.tar.gz`,
    `the-pair-${version}.app.tar.gz.sig`,
    `the-pair-${version}-setup.exe`,
    `the-pair-${version}-setup.exe.sig`,
    `the-pair-${version}.AppImage.tar.gz`,
    `the-pair-${version}.AppImage.tar.gz.sig`,
    'latest.json'
  ]
}

export function missingAssets(assets, version) {
  if (!Array.isArray(assets)) throw new Error('Invalid release assets response')
  return requiredAssets(version).filter(
    (name) =>
      !assets.some(
        (asset) =>
          asset?.name === name &&
          asset.state === 'uploaded' &&
          Number.isSafeInteger(asset.size) &&
          asset.size > 0 &&
          typeof asset.digest === 'string' &&
          /^sha256:[0-9a-f]{64}$/.test(asset.digest)
      )
  )
}

export async function verifyDraftAssets(assets, version, distDir = 'dist') {
  if (!Array.isArray(assets)) throw new Error('Invalid release assets response')
  const required = requiredAssets(version)
  const allowed = new Set(required)
  for (const asset of assets) {
    if (
      !asset ||
      typeof asset !== 'object' ||
      Array.isArray(asset) ||
      typeof asset.name !== 'string'
    )
      throw new Error('Invalid release asset object')
    if (!allowed.has(asset.name)) throw new Error(`Unexpected release asset: ${asset.name}`)
  }
  for (const name of required) {
    const matches = assets.filter((asset) => asset?.name === name)
    if (matches.length !== 1) throw new Error(`Expected exactly one release asset: ${name}`)
    const asset = matches[0]
    if (asset.state !== 'uploaded' || !Number.isSafeInteger(asset.size) || asset.size <= 0)
      throw new Error(`Invalid upload state or size for ${name}`)
    if (typeof asset.digest !== 'string' || !/^sha256:[0-9a-f]{64}$/.test(asset.digest))
      throw new Error(`Missing or invalid SHA-256 digest for ${name}`)
    const hash = createHash('sha256')
    let size = 0
    try {
      for await (const chunk of createReadStream(join(distDir, name))) {
        size += chunk.length
        hash.update(chunk)
      }
    } catch (error) {
      throw new Error(`Cannot read local asset ${name}: ${error.message}`)
    }
    if (size !== asset.size) throw new Error(`Size mismatch for ${name}`)
    if (`sha256:${hash.digest('hex')}` !== asset.digest)
      throw new Error(`SHA-256 digest mismatch for ${name}`)
  }
}

export async function inspectRelease({
  repo,
  tag,
  version,
  token,
  apiUrl = 'https://api.github.com',
  fetchImpl = globalThis.fetch,
  includeAssets = false
}) {
  if (!repo || !tag || !version || !token)
    throw new Error('Repository, tag, version and GitHub token are required')
  const base = `${apiUrl}/repos/${repo}`
  async function request(path, allowNotFound = false) {
    const response = await fetchImpl(`${base}${path}`, {
      headers: {
        Accept: 'application/vnd.github+json',
        Authorization: `Bearer ${token}`,
        'X-GitHub-Api-Version': '2022-11-28'
      },
      signal: globalThis.AbortSignal.timeout(30000)
    })
    if (response.status === 404 && allowNotFound) return null
    if (!response.ok) throw new Error(`GitHub API ${path} failed (HTTP ${response.status})`)
    const data = await response.json()
    if (data === null) throw new Error(`Invalid GitHub API response: ${path}`)
    return data
  }
  async function list(path) {
    const all = []
    for (let page = 1; ; page += 1) {
      const items = await request(`${path}?per_page=100&page=${page}`)
      if (!Array.isArray(items)) throw new Error(`Invalid GitHub API response: ${path}`)
      all.push(...items)
      if (items.length < 100) return all
    }
  }

  // Drafts are only returned to callers with push access. If this token
  // cannot see drafts, later steps fail closed: assert-new refuses anything
  // but a clean new state, and tag creation is atomic.
  const allReleases = await list('/releases')
  if (allReleases.some((release) => typeof release?.tag_name !== 'string'))
    throw new Error('Invalid GitHub API response: /releases')
  const releases = allReleases.filter((release) => release.tag_name === tag)
  if (releases.length > 1) throw new Error(`Multiple releases found for ${tag}`)
  const ref = await request(`/git/ref/tags/${encodeURIComponent(tag)}`, true)
  if (
    ref !== null &&
    (!['commit', 'tag'].includes(ref.object?.type) ||
      typeof ref.object?.sha !== 'string' ||
      !ref.object.sha)
  ) {
    throw new Error(`Invalid tag ref response for ${tag}`)
  }
  const release = releases[0]
  if (!release) return { status: ref ? 'orphan-tag' : 'new' }
  if (
    !Number.isSafeInteger(release.id) ||
    release.id <= 0 ||
    typeof release.draft !== 'boolean' ||
    typeof release.prerelease !== 'boolean'
  ) {
    throw new Error(`Invalid release response for ${tag}`)
  }
  const assets = await list(`/releases/${release.id}/assets`)
  const missing = missingAssets(assets, version)
  if (release.draft)
    return {
      status: 'draft',
      id: release.id,
      missing,
      ref: ref?.object,
      prerelease: release.prerelease,
      ...(includeAssets ? { assets } : {})
    }
  if (!ref) return { status: 'missing-tag', id: release.id, missing }
  if (release.prerelease) return { status: 'prerelease', id: release.id, missing }
  return { status: missing.length ? 'incomplete' : 'complete', id: release.id, missing }
}

export function requireStatus(result, expected, tag) {
  if (result.status !== expected || (expected === 'draft' && result.missing.length)) {
    const missing = result.missing?.length
      ? ` Missing or invalid assets: ${result.missing.join(', ')}.`
      : ''
    throw new Error(
      `Release ${tag} is ${result.status}; expected ${expected}.${missing} No data was deleted or overwritten. Inspect the tag, release and assets in GitHub. If already public, publish fixes under a new version; otherwise, manually recover the draft or choose a new version after verifying its commit and artifacts.`
    )
  }
}

async function main() {
  const mode = process.argv[2]
  if (!['detect', 'assert-new', 'verify-draft'].includes(mode))
    throw new Error('Usage: release-status.mjs detect|assert-new|verify-draft')
  const version = process.env.VERSION || JSON.parse(readFileSync('package.json', 'utf8')).version
  const tag = process.env.TAG || `v${version}`
  if (
    !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(version) ||
    tag !== `v${version}`
  ) {
    throw new Error('Invalid release version or tag')
  }
  const result = await inspectRelease({
    repo: process.env.GITHUB_REPOSITORY,
    tag,
    version,
    token: process.env.GH_TOKEN,
    apiUrl: process.env.GITHUB_API_URL,
    includeAssets: mode === 'verify-draft'
  })
  if (mode === 'detect') {
    if (result.status !== 'complete') requireStatus(result, 'new', tag)
    const shouldPublish = result.status === 'new'
    appendFileSync(
      process.env.GITHUB_OUTPUT,
      `version=${version}\ntag=${tag}\nshould_publish=${shouldPublish}\n`
    )
    console.log(`${tag}: ${result.status}; should_publish=${shouldPublish}`)
  } else if (mode === 'assert-new') {
    requireStatus(result, 'new', tag)
  } else {
    requireStatus(result, 'draft', tag)
    if (result.prerelease) throw new Error(`Draft ${tag} is a prerelease; refusing to publish.`)
    if (String(result.id) !== process.env.EXPECTED_RELEASE_ID) {
      throw new Error(`Draft ${tag} is not the release created by this run; refusing to publish.`)
    }
    if (
      !process.env.GITHUB_SHA ||
      result.ref?.type !== 'commit' ||
      result.ref.sha !== process.env.GITHUB_SHA
    ) {
      throw new Error(
        `Tag ${tag} does not point directly to the expected commit; refusing to publish. Inspect the tag and draft; no data was deleted.`
      )
    }
    await verifyDraftAssets(result.assets, version)
    appendFileSync(process.env.GITHUB_OUTPUT, `release_id=${result.id}\n`)
    console.log(`${tag}: draft assets verified`)
  }
}

if (fileURLToPath(import.meta.url) === process.argv[1]) {
  main().catch((error) => {
    console.error(`::error::${error.message}`)
    process.exitCode = 1
  })
}
