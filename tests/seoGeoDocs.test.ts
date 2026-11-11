import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import test from 'node:test'

const root = new URL('../', import.meta.url)

const readText = (path: string) => readFile(new URL(path, root), 'utf8')

test('README targets high-intent AI coding search phrases', async () => {
  const readme = await readText('README.md')

  assert.match(readme, /open-source AI pair programming/i)
  assert.match(readme, /multi-agent coding assistant/i)
  assert.match(readme, /AI code review/i)
  assert.match(readme, /Cursor.*Copilot alternative/i)
})

test('package metadata includes discoverable developer-tool keywords', async () => {
  const pkg = JSON.parse(await readText('package.json')) as {
    keywords: string[]
  }

  assert.ok(pkg.keywords.includes('ai-pair-programming'))
  assert.ok(pkg.keywords.includes('multi-agent-coding'))
  assert.ok(pkg.keywords.includes('cursor-alternative'))
  assert.ok(pkg.keywords.includes('copilot-alternative'))
})

test('llms.txt gives AI crawlers an answer-first product summary', async () => {
  const llms = await readText('llms.txt')

  assert.match(llms, /^# The Pair/m)
  assert.match(llms, /open-source AI pair programming/i)
  assert.match(llms, /Mentor.*Executor/s)
  assert.match(llms, /https:\/\/github\.com\/timwuhaotian\/the-pair/)
  assert.match(llms, /https:\/\/apps\.timwuhaotian\.dev\/apps\/the-pair/)
})

test('software schema is valid JSON-LD for repo and website reuse', async () => {
  const schema = JSON.parse(await readText('docs/seo/software-application.schema.json')) as {
    '@context': string
    '@type': string
    name: string
    applicationCategory: string
    operatingSystem: string[]
    url: string
    sameAs: string[]
    featureList: string[]
    softwareVersion?: string
  }

  assert.equal(schema['@context'], 'https://schema.org')
  assert.equal(schema['@type'], 'SoftwareApplication')
  assert.equal(schema.name, 'The Pair')
  assert.equal(schema.applicationCategory, 'DeveloperApplication')
  assert.deepEqual(schema.operatingSystem, ['macOS', 'Windows', 'Linux'])
  assert.equal(schema.url, 'https://apps.timwuhaotian.dev/apps/the-pair')
  assert.ok(schema.sameAs.includes('https://github.com/timwuhaotian/the-pair'))
  assert.ok(schema.featureList.length >= 6)
  // Nothing bumps a version written here, so one would go stale on the next release.
  assert.equal(schema.softwareVersion, undefined)
})

test('FAQ schema exposes extractable AI-search answers', async () => {
  const schema = JSON.parse(await readText('docs/seo/faq.schema.json')) as {
    '@context': string
    '@type': string
    mainEntity: Array<{
      '@type': string
      name: string
      acceptedAnswer: {
        '@type': string
        text: string
      }
    }>
  }

  assert.equal(schema['@context'], 'https://schema.org')
  assert.equal(schema['@type'], 'FAQPage')
  assert.ok(schema.mainEntity.length >= 4)
  assert.ok(schema.mainEntity.some((entry) => /Cursor|Copilot/.test(entry.acceptedAnswer.text)))
  assert.ok(
    schema.mainEntity.some((entry) => /Claude|GPT|Gemini|Ollama/.test(entry.acceptedAnswer.text))
  )
})

test('search-facing docs describe the direct-endpoint app, with no dead links', async () => {
  const docs = await Promise.all(
    [
      'README.md',
      'README.zh.md',
      'README.ja.md',
      'README.ko.md',
      'llms.txt',
      'docs/seo/software-application.schema.json',
      'docs/seo/faq.schema.json'
    ].map(async (path) => ({ path, text: await readText(path) }))
  )

  for (const { path, text } of docs) {
    // The pair-code repository is gone; the npm page is the CLI's home.
    assert.doesNotMatch(text, /github\.com\/timwuhaotian\/pair-code/, path)
    // Wording from before 3.0, when the app drove other agents' CLIs.
    assert.doesNotMatch(text, /ten (provider )?CLIs|Antigravity|Aider, Grok Build/, path)
  }
})
