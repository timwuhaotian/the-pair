import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { join } from 'node:path'
import test from 'node:test'
import { promisify } from 'node:util'
import { requiredAssets } from '../scripts/release-status.mjs'

const exec = promisify(execFile)
const script = join(process.cwd(), 'scripts/release-status.mjs')
const names = requiredAssets('1.2.3') as string[]

function createAssets(dir: string) {
  mkdirSync(join(dir, 'dist'))
  return names.map((name) => {
    const content = name === names[0] ? Buffer.alloc(200_000, 42) : `content of ${name}`
    writeFileSync(join(dir, 'dist', name), content)
    return {
      name,
      state: 'uploaded',
      size: Buffer.byteLength(content),
      digest: `sha256:${createHash('sha256').update(content).digest('hex')}`
    }
  })
}

test('CLI detects releases and verifies the exact draft and tag', async () => {
  const dir = mkdtempSync(join(process.cwd(), '.release-status-test-'))
  const output = join(dir, 'output')
  let state = 'new'
  const assets = createAssets(dir)
  const server = createServer((request, response) => {
    const path = new URL(request.url!, 'http://localhost').pathname.replace('/repos/owner/repo', '')
    response.setHeader('Content-Type', 'application/json')
    if (path === '') response.end(JSON.stringify({ permissions: { push: true } }))
    else if (path === '/releases')
      response.end(
        JSON.stringify(
          state === 'new'
            ? []
            : [
                {
                  id: 42,
                  tag_name: 'v1.2.3',
                  draft: state !== 'complete',
                  prerelease: state === 'prerelease'
                }
              ]
        )
      )
    else if (path === '/git/ref/tags/v1.2.3') {
      if (state === 'new') {
        response.statusCode = 404
        response.end('{}')
      } else
        response.end(
          JSON.stringify({
            object: { type: 'commit', sha: state === 'wrong-sha' ? 'other' : 'abc' }
          })
        )
    } else if (path === '/releases/42/assets') response.end(JSON.stringify(assets))
    else {
      response.statusCode = 500
      response.end('{}')
    }
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('No server address')
  const env = {
    GITHUB_API_URL: `http://127.0.0.1:${address.port}`,
    GITHUB_REPOSITORY: 'owner/repo',
    GH_TOKEN: 'test',
    GITHUB_OUTPUT: output,
    VERSION: '1.2.3',
    TAG: 'v1.2.3',
    GITHUB_SHA: 'abc',
    EXPECTED_RELEASE_ID: '42'
  }
  const run = (mode: string, extra = {}) =>
    exec(process.execPath, [script, mode], { cwd: dir, env: { ...env, ...extra } })
  try {
    await run('detect')
    assert.match(readFileSync(output, 'utf8'), /should_publish=true/)
    await run('assert-new')
    state = 'complete'
    await run('detect')
    assert.match(readFileSync(output, 'utf8'), /should_publish=false/)
    await assert.rejects(run('assert-new'), /expected new/)
    state = 'draft'
    await assert.rejects(run('detect'), /is draft/)
    await run('verify-draft')
    assert.match(readFileSync(output, 'utf8'), /release_id=42/)
    await assert.rejects(
      run('verify-draft', { EXPECTED_RELEASE_ID: '43' }),
      /not the release created/
    )
    state = 'wrong-sha'
    await assert.rejects(run('verify-draft'), /expected commit/)
    state = 'prerelease'
    await assert.rejects(run('verify-draft'), /is a prerelease/)
    await assert.rejects(run('unknown'), /Usage/)
    await assert.rejects(run('detect', { VERSION: 'invalid' }), /Invalid release version/)
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    )
    rmSync(dir, { recursive: true, force: true })
  }
})

test('offline release lifecycle fails closed until every draft upload and tag are verified', async () => {
  const dir = mkdtempSync(join(process.cwd(), '.release-status-test-'))
  const output = join(dir, 'output')
  let ref: { object: { type: string; sha: string } } | null = null
  let draft: { id: number; tag_name: string; draft: boolean; prerelease: boolean } | null = null
  const allAssets = createAssets(dir)
  const assets: (Omit<(typeof allAssets)[number], 'digest'> & { digest?: string | null })[] = []
  let failAssets = false
  let extraAssets: unknown[] = []
  const requests: string[] = []
  const server = createServer((request, response) => {
    const url = new URL(request.url!, 'http://127.0.0.1')
    requests.push(`${request.method} ${url.pathname}${url.search}`)
    assert.equal(request.headers.authorization, 'Bearer test')
    assert.equal(request.headers.accept, 'application/vnd.github+json')
    assert.equal(request.headers['x-github-api-version'], '2022-11-28')
    assert.equal(request.method, 'GET')
    response.setHeader('Content-Type', 'application/json')
    const path = url.pathname.replace('/repos/owner/repo', '')
    if (path === '') response.end(JSON.stringify({ permissions: { push: true } }))
    else if (path === '/releases') response.end(JSON.stringify(draft ? [draft] : []))
    else if (path === '/git/ref/tags/v1.2.3') {
      if (!ref) response.statusCode = 404
      response.end(JSON.stringify(ref))
    } else if (path === '/releases/42/assets') {
      if (failAssets) response.statusCode = 503
      response.end(JSON.stringify([...assets, ...extraAssets]))
    } else {
      response.statusCode = 500
      response.end('{}')
    }
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const address = server.address()
  if (!address || typeof address === 'string') throw new Error('No server address')
  const env = {
    GITHUB_API_URL: `http://127.0.0.1:${address.port}`,
    GITHUB_REPOSITORY: 'owner/repo',
    GH_TOKEN: 'test',
    GITHUB_OUTPUT: output,
    VERSION: '1.2.3',
    TAG: 'v1.2.3',
    GITHUB_SHA: 'abc',
    EXPECTED_RELEASE_ID: '42'
  }
  const run = (mode: string) => exec(process.execPath, [script, mode], { cwd: dir, env })
  const rejectedWithoutOutput = async (mode: string, error: RegExp) => {
    writeFileSync(output, 'sentinel\n')
    await assert.rejects(run(mode), error)
    assert.equal(readFileSync(output, 'utf8'), 'sentinel\n')
  }
  try {
    await run('detect')
    await run('assert-new')
    ref = { object: { type: 'commit', sha: 'abc' } }
    await rejectedWithoutOutput('assert-new', /orphan-tag/)
    draft = { id: 42, tag_name: 'v1.2.3', draft: true, prerelease: false }
    await rejectedWithoutOutput('verify-draft', /Missing or invalid assets/)
    assets.push(...allAssets.map((asset) => ({ ...asset })))
    assets[0].state = 'starter'
    await rejectedWithoutOutput('verify-draft', /Missing or invalid assets/)
    assets[0].state = 'uploaded'
    assets[1].size = 0
    await rejectedWithoutOutput('verify-draft', /Missing or invalid assets/)
    assets[1].size = allAssets[1].size
    failAssets = true
    await rejectedWithoutOutput('verify-draft', /HTTP 503/)
    failAssets = false
    ref = null
    await rejectedWithoutOutput('verify-draft', /expected commit/)
    ref = { object: { type: 'tag', sha: 'abc' } }
    await rejectedWithoutOutput('verify-draft', /expected commit/)
    ref = { object: { type: 'commit', sha: 'abc' } }
    const digest = assets[0].digest
    for (const invalid of [undefined, null, '', 'sha512:abc', 'sha256:xyz']) {
      assets[0].digest = invalid
      await rejectedWithoutOutput('verify-draft', /Missing or invalid assets/)
    }
    assets[0].digest = `sha256:${'0'.repeat(64)}`
    await rejectedWithoutOutput('verify-draft', /digest mismatch/i)
    assets[0].digest = digest
    assets[0].size += 1
    await rejectedWithoutOutput('verify-draft', /size mismatch/i)
    assets[0].size -= 1
    assets.push({ ...assets[0] })
    await rejectedWithoutOutput('verify-draft', /exactly one release asset/i)
    assets.pop()
    extraAssets = [{ name: 'unexpected.zip', state: 'uploaded', size: 1 }]
    await rejectedWithoutOutput('verify-draft', /Unexpected release asset: unexpected\.zip/)
    extraAssets = [null]
    await rejectedWithoutOutput('verify-draft', /Invalid release asset object/)
    extraAssets = []
    const local = join(dir, 'dist', assets[0].name)
    const original = readFileSync(local)
    writeFileSync(local, Buffer.alloc(original.length, 0))
    await rejectedWithoutOutput('verify-draft', /digest mismatch/i)
    rmSync(local)
    await rejectedWithoutOutput('verify-draft', /Cannot read local asset/i)
    writeFileSync(local, original)
    await run('verify-draft')
    assert.match(readFileSync(output, 'utf8'), /release_id=42/)
    draft.draft = false
    rmSync(join(dir, 'dist', assets[0].name))
    await run('detect')
    assert.match(readFileSync(output, 'utf8'), /should_publish=false/)
    assert.ok(requests.includes('GET /repos/owner/repo/releases?per_page=100&page=1'))
    assert.ok(requests.includes('GET /repos/owner/repo/releases/42/assets?per_page=100&page=1'))
    assert.ok(requests.every((request) => !request.includes('/assets/')))
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve()))
    )
    rmSync(dir, { recursive: true, force: true })
  }
})
