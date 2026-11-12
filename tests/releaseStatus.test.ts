import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  inspectRelease,
  missingAssets,
  requiredAssets,
  requireStatus,
  verifyDraftAssets
} from '../scripts/release-status.mjs'

const version = '1.2.3'
const tag = `v${version}`
const digest = `sha256:${'0'.repeat(64)}`
const assets = requiredAssets(version).map((name: string) => ({
  name,
  state: 'uploaded',
  size: 1,
  digest
}))
const release = { id: 42, tag_name: tag, draft: false, prerelease: false }
const ref = { object: { type: 'commit', sha: 'abc' } }

function api(
  options: {
    releases?: object[]
    ref?: unknown
    assets?: object[]
    fail?: string
  } = {}
) {
  return async (url: string) => {
    const path = new URL(url).pathname.replace('/repos/owner/repo', '')
    if (path === options.fail) return new Response('failure', { status: 503 })
    let data: unknown
    if (path === '/releases') data = options.releases ?? []
    else if (path === `/git/ref/tags/${tag}`) {
      if (options.ref === null || options.ref === undefined)
        return new Response('', { status: 404 })
      data = options.ref
    } else if (path === '/releases/42/assets') data = options.assets ?? assets
    else throw new Error(`Unexpected path ${path}`)
    return Response.json(data)
  }
}

async function inspect(options: Parameters<typeof api>[0] = {}) {
  return inspectRelease({
    repo: 'owner/repo',
    tag,
    version,
    token: 'test',
    fetchImpl: api(options)
  })
}

test('new, orphan tag and complete public release are distinct', async () => {
  assert.equal((await inspect()).status, 'new')
  assert.equal((await inspect({ ref })).status, 'orphan-tag')
  assert.equal((await inspect({ ref, releases: [release] })).status, 'complete')
})

test('draft, prerelease, missing tag and incomplete release cannot be skipped', async () => {
  assert.equal((await inspect({ releases: [{ ...release, draft: true }] })).status, 'draft')
  assert.equal(
    (await inspect({ ref, releases: [{ ...release, prerelease: true }] })).status,
    'prerelease'
  )
  assert.equal((await inspect({ releases: [release] })).status, 'missing-tag')
  const incomplete = await inspect({ ref, releases: [release], assets: assets.slice(1) })
  assert.equal(incomplete.status, 'incomplete')
  assert.throws(
    () => requireStatus(incomplete, 'new', tag),
    /Missing or invalid assets.*No data was deleted/
  )
  assert.throws(
    () => requireStatus({ status: 'draft', missing: ['latest.json'] }, 'draft', tag),
    /latest.json/
  )
  requireStatus({ status: 'draft', missing: [] }, 'draft', tag)
})

test('checks all required uploaded, nonempty assets', () => {
  assert.deepEqual(missingAssets(assets, version), [])
  assert.deepEqual(
    missingAssets(
      [
        { ...assets[0], size: 0 },
        { ...assets[1], state: 'starter' },
        { ...assets[2], digest: 'sha256:xyz' },
        ...assets.slice(3)
      ],
      version
    ),
    assets.slice(0, 3).map((a: { name: string }) => a.name)
  )
  assert.deepEqual(
    missingAssets([null, ...assets.map((asset) => ({ ...asset, size: '1' }))], version),
    requiredAssets(version)
  )
  assert.throws(() => missingAssets(null, version), /Invalid release assets/)
})

test('draft asset verification independently rejects invalid upload state and size', async () => {
  for (const invalid of [
    { ...assets[0], state: 'starter' },
    { ...assets[0], size: 0 },
    { ...assets[0], size: '1' }
  ]) {
    await assert.rejects(verifyDraftAssets([invalid, ...assets.slice(1)], version), /state or size/)
  }
  await assert.rejects(verifyDraftAssets(assets.slice(1), version), /exactly one release asset/)
})

test('draft asset verification rejects unexpected and malformed entries before reading files', async () => {
  for (const invalid of [null, undefined, false, 1, 'asset', [], {}, { name: null }, { name: 1 }]) {
    await assert.rejects(
      verifyDraftAssets([...assets, invalid], version),
      /Invalid release asset object/
    )
  }
  for (const invalid of [null, {}, 'assets']) {
    await assert.rejects(verifyDraftAssets(invalid, version), /Invalid release assets response/)
  }
  for (const name of ['', 'extra.zip', 'latest.json.sig', `the-pair-1.2.4.zip`]) {
    await assert.rejects(
      verifyDraftAssets([...assets, { name }], version),
      /Unexpected release asset/
    )
  }
  await assert.rejects(
    verifyDraftAssets([...assets, { ...assets[0], state: 'starter' }], version),
    /exactly one release asset/
  )
})

test('API failures fail closed', async () => {
  for (const fail of ['/releases', `/git/ref/tags/${tag}`, '/releases/42/assets']) {
    await assert.rejects(inspect({ fail, ref, releases: [release] }), /GitHub API.*503/)
  }
  await assert.rejects(inspectRelease({ repo: '', tag, version, token: 'test' }), /required/)
  await assert.rejects(
    inspectRelease({
      repo: 'owner/repo',
      tag,
      version,
      token: 'test',
      fetchImpl: async () => {
        throw new Error('network failure')
      }
    }),
    /network failure/
  )
})

test('rejects malformed and duplicate release responses', async () => {
  await assert.rejects(inspect({ releases: [release, release] }), /Multiple releases/)
  await assert.rejects(inspect({ releases: [{}] }), /Invalid GitHub API response/)
  await assert.rejects(inspect({ releases: [{ ...release, draft: 'false' }] }), /Invalid release/)
  await assert.rejects(inspect({ releases: [{ ...release, id: 0 }] }), /Invalid release/)
  for (const badRef of [
    {},
    false,
    0,
    '',
    { object: { type: 'tree', sha: 'abc' } },
    { object: { type: 'commit', sha: '' } }
  ]) {
    await assert.rejects(inspect({ ref: badRef }), /Invalid tag ref/)
    await assert.rejects(inspect({ ref: badRef, releases: [release] }), /Invalid tag ref/)
  }
  await assert.rejects(
    inspectRelease({
      repo: 'owner/repo',
      tag,
      version,
      token: 'test',
      fetchImpl: async (url: string) =>
        Response.json(
          url.endsWith('/repo')
            ? { permissions: { push: true } }
            : url.includes('/releases?')
              ? []
              : null
        )
    }),
    /Invalid GitHub API response/
  )
})

test('paginates releases and rejects malformed list responses', async () => {
  const many = Array.from({ length: 100 }, (_, i) => ({ tag_name: `other-${i}` }))
  const fetchImpl = async (url: string) => {
    const parsed = new URL(url)
    if (parsed.pathname.endsWith('/repo')) return Response.json({ permissions: { push: true } })
    if (parsed.pathname.endsWith('/releases'))
      return Response.json(parsed.searchParams.get('page') === '1' ? many : [release])
    if (parsed.pathname.endsWith('/assets')) return Response.json(assets)
    return Response.json(ref)
  }
  assert.equal(
    (await inspectRelease({ repo: 'owner/repo', tag, version, token: 'test', fetchImpl })).status,
    'complete'
  )
  await assert.rejects(
    inspectRelease({
      repo: 'owner/repo',
      tag,
      version,
      token: 'test',
      fetchImpl: async (url: string) =>
        Response.json(url.endsWith('/repo') ? { permissions: { push: true } } : {})
    }),
    /Invalid GitHub API response/
  )
})

test('paginates assets and fails closed if a later page fails', async () => {
  let failSecondPage = false
  const fetchImpl = async (url: string) => {
    const parsed = new URL(url)
    if (parsed.pathname.endsWith('/repo')) return Response.json({ permissions: { push: true } })
    if (parsed.pathname.endsWith('/releases')) return Response.json([release])
    if (parsed.pathname.endsWith('/assets')) {
      if (parsed.searchParams.get('page') === '1')
        return Response.json(Array.from({ length: 100 }, (_, i) => ({ name: `extra-${i}` })))
      return failSecondPage ? new Response('', { status: 503 }) : Response.json(assets)
    }
    return Response.json(ref)
  }
  assert.equal(
    (await inspectRelease({ repo: 'owner/repo', tag, version, token: 'test', fetchImpl })).status,
    'complete'
  )
  failSecondPage = true
  await assert.rejects(
    inspectRelease({ repo: 'owner/repo', tag, version, token: 'test', fetchImpl }),
    /HTTP 503/
  )
})

test('workflow creates an atomic tag and verifies draft before publishing', () => {
  const workflow = readFileSync('.github/workflows/build-signed-mac.yml', 'utf8')
  assert.match(
    workflow,
    /Build updater verifier[\s\S]*Upload updater verifier[\s\S]*Download updater verifier[\s\S]*Verify updater signatures[\s\S]*Generate updater manifest[\s\S]*git\/refs[\s\S]*draft=true[\s\S]*Upload draft assets[\s\S]*verify-draft[\s\S]*draft=false/
  )
  for (const asset of ['.app.tar.gz', '-setup.exe', '.AppImage.tar.gz']) {
    assert.ok(workflow.includes(`dist/the-pair-\${VERSION}${asset}.sig`))
  }
  // The CI-only verifier bin is gated behind required-features (tauri-apps/tauri#16460),
  // so every cargo invocation that needs it must enable the feature explicitly.
  assert.match(workflow, /cargo test -q --features verifier --manifest-path src-tauri\/Cargo\.toml/)
  assert.match(
    workflow,
    /cargo build --locked --features verifier --manifest-path src-tauri\/Cargo\.toml --bin verify-updater/
  )
  assert.doesNotMatch(workflow, /--clobber|gh release delete/)
})
