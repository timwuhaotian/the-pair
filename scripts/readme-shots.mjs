#!/usr/bin/env node
/**
 * Regenerates the README screenshots in docs/assets from the renderer itself.
 *
 * The renderer runs in a headless browser against the mocked Tauri IPC the
 * web e2e suite uses (e2e-web/mock-init.js), seeded with the pairs below, so
 * the pictures show the current UI without a model call or a credential.
 *
 * It needs two things running, and starts neither:
 *   npm run dev:renderer -- --port 5199 --strictPort --host 127.0.0.1
 *   a browser speaking the DevTools protocol on port 9222
 *     (the committed pictures were taken with `moli serve -p 9222 --layout`)
 *
 * Usage: node scripts/readme-shots.mjs [name…]   (no name = every shot)
 */
/* global fetch, WebSocket -- Node 22 globals the shared ESLint list does not name */
import { readFile, writeFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'

const APP_URL = process.env.SHOTS_APP_URL ?? 'http://127.0.0.1:5199/'
const CDP_PORT = process.env.SHOTS_CDP_PORT ?? '9222'
const OUT_DIR = new URL('../docs/assets/', import.meta.url)
const MOCK_INIT = new URL('../e2e-web/mock-init.js', import.meta.url)

// 1400×1000 at 2x: the 2800×2000 the README has always shown.
const VIEWPORT = { width: 1400, height: 1000, deviceScaleFactor: 2, mobile: false }

// ── The scene ────────────────────────────────────────────────────────────────

const MENTOR_MODEL = 'claude-sonnet-4-5-20250929'
const EXECUTOR_MODEL = 'gpt-5.1-codex-max'

const now = Date.now()
const minutesAgo = (minutes) => now - Math.round(minutes * 60_000)

const usage = (inputTokens, outputTokens, at, provider) => ({
  inputTokens,
  outputTokens,
  lastUpdatedAt: at,
  source: 'final',
  provider
})

const activity = (phase, label, at, detail) => ({
  phase,
  label,
  detail,
  startedAt: at,
  updatedAt: at,
  lastOutputAt: at,
  outputLineCount: phase === 'idle' ? 0 : 14
})

function basePair(id, name, repo, spec, extra) {
  const worktree = `${repo}/.worktrees/${id}`
  return {
    pairId: id,
    name,
    directory: worktree,
    spec,
    iterations: 1,
    maxIterations: 0,
    turn: 'mentor',
    mentorModel: MENTOR_MODEL,
    executorModel: EXECUTOR_MODEL,
    executorReasoningEffort: 'medium',
    messages: [],
    mentorActivity: activity('idle', 'Mentor idle', minutesAgo(3)),
    executorActivity: activity('idle', 'Executor idle', minutesAgo(3)),
    modifiedFiles: [],
    gitTracking: { available: true, rootPath: repo, baseline: 'a41c9e7', gitReviewAvailable: true },
    automationMode: 'full-auto',
    runCount: 1,
    runHistory: [],
    currentRunStartedAt: minutesAgo(20),
    createdAt: minutesAgo(20),
    repoPath: repo,
    worktreePath: worktree,
    worktreeBranch: `the-pair/${id}`,
    snapshotVersion: 1,
    savedAt: now,
    providerSessions: {},
    ...extra
  }
}

const SPAM_SPEC =
  'Add spam filtering to the contact form backend — reject submissions containing SEO or crypto keywords, case-insensitively, and return typed results.'

const SPAM_PLAN = `Read \`src/app/api/contact/route.ts\` and the two sign-up forms that post to it. Nothing filters the message today, and the route returns an untyped \`{ ok }\`.

**Plan for the Executor**

1. **Keyword lists** — add \`src/utils/spamFilter.ts\` with the SEO and crypto lists as lowercase \`Set\`s, so a lookup is O(1).
2. **Matching** — normalise \`name + email + phone + message\` with \`toLowerCase()\` before checking, and collect every keyword that matched.
3. **Typed result** — the route returns \`{ success: false, error: "spam_detected", matchedKeywords }\` on a hit and \`{ success: true }\` otherwise. Update both forms to read it.

Run \`npm test -- spamFilter\` and \`npm run typecheck\` before you hand back.`

const SPAM_RESULT = `Implemented as planned. Three files changed, one added.

| Requirement | Status |
| --- | --- |
| SEO keyword filtering | 12 keywords covered |
| Crypto keyword filtering | 18 keywords covered |
| Case-insensitive matching | \`toLowerCase()\` on the combined fields |
| Early return on spam | logs the hit, returns \`{ success: false }\` |
| Typed result | \`Promise<ContactResult>\`, both forms updated |

\`npm test -- spamFilter\` → 9 passed. \`npm run typecheck\` → clean.`

const SPAM_REVIEW = `Read the diff against \`a41c9e7\` and re-ran both commands myself: 9 tests pass and the types are clean.

The filter matches whole words only, so "cryptography" in a real message is not flagged — I checked that case by hand since no test covered it. The implementation follows the plan and meets every requirement.

TASK_COMPLETE`

const spamFilter = basePair(
  'contact-spam-filter',
  'contact-spam-filter',
  '/Users/dev/code/storefront',
  SPAM_SPEC,
  {
    status: 'Finished',
    iterations: 2,
    currentRunStartedAt: minutesAgo(42),
    currentRunFinishedAt: minutesAgo(31),
    createdAt: minutesAgo(42),
    messages: [
      {
        id: 'spam-1',
        timestamp: minutesAgo(41),
        from: 'mentor',
        to: 'executor',
        type: 'plan',
        content: SPAM_PLAN,
        iteration: 1,
        tokenUsage: usage(18_420, 612, minutesAgo(41), 'Anthropic')
      },
      {
        id: 'spam-2',
        timestamp: minutesAgo(35),
        from: 'executor',
        to: 'mentor',
        type: 'result',
        content: SPAM_RESULT,
        iteration: 1,
        tokenUsage: usage(41_870, 3_954, minutesAgo(35), 'OpenAI')
      },
      {
        id: 'spam-3',
        timestamp: minutesAgo(31),
        from: 'mentor',
        to: 'human',
        type: 'feedback',
        content: SPAM_REVIEW,
        iteration: 2,
        tokenUsage: usage(27_310, 488, minutesAgo(31), 'Anthropic')
      }
    ],
    modifiedFiles: [
      { path: 'src/utils/spamFilter.ts', status: 'A', displayPath: 'src/utils/spamFilter.ts' },
      {
        path: 'src/utils/spamFilter.test.ts',
        status: 'A',
        displayPath: 'src/utils/spamFilter.test.ts'
      },
      {
        path: 'src/app/api/contact/route.ts',
        status: 'M',
        displayPath: 'src/app/api/contact/route.ts'
      },
      {
        path: 'src/components/Auth/SignIn/index.tsx',
        status: 'M',
        displayPath: 'src/components/Auth/SignIn/index.tsx'
      },
      {
        path: 'src/components/Auth/SignUp/index.tsx',
        status: 'M',
        displayPath: 'src/components/Auth/SignUp/index.tsx'
      }
    ]
  }
)

const RATE_SPEC =
  'Add sliding-window rate limiting to the public API routes: 60 requests a minute per API key, a 429 with Retry-After when exceeded.'

const RATE_PLAN = `The public routes live under \`src/app/api/v1/\` and share \`withApiKey()\`, so the limiter belongs there rather than in each handler.

**Plan for the Executor**

1. **Limiter** — add \`src/lib/rateLimit.ts\`: a sliding window of timestamps per API key, 60 per 60 s, in memory behind a small \`RateLimitStore\` interface so Redis can replace it later.
2. **Wiring** — call it from \`withApiKey()\`; on a miss return 429 with \`Retry-After\` in whole seconds.
3. **Tests** — cover the boundary (60th passes, 61st fails) and the window sliding forward, with a fake clock.

Run \`npm test -- rateLimit\` before you hand back.`

const rateTools = [
  ['read', 'Read src/lib/withApiKey.ts', 'completed', 4.2],
  ['grep', 'Search "withApiKey(" in src/app/api/v1', 'completed', 3.9],
  ['write', 'Write src/lib/rateLimit.ts', 'completed', 2.6],
  ['edit', 'Edit src/lib/withApiKey.ts', 'completed', 1.4],
  ['bash', 'npm test -- rateLimit', 'running', 0.4]
].map(([toolName, description, status, ago], index) => ({
  id: `rate-tool-${index}`,
  timestamp: minutesAgo(ago),
  role: 'executor',
  eventType: 'tool_call',
  toolName,
  description,
  status
}))

const rateLimit = basePair(
  'rate-limit-middleware',
  'rate-limit-middleware',
  '/Users/dev/code/storefront',
  RATE_SPEC,
  {
    status: 'Executing',
    turn: 'executor',
    iterations: 1,
    currentRunStartedAt: minutesAgo(9),
    createdAt: minutesAgo(9),
    messages: [
      {
        id: 'rate-1',
        timestamp: minutesAgo(8),
        from: 'mentor',
        to: 'executor',
        type: 'plan',
        content: RATE_PLAN,
        iteration: 1,
        tokenUsage: usage(16_240, 574, minutesAgo(8), 'Anthropic')
      }
    ],
    executorActivity: activity(
      'using_tools',
      'Running tests',
      minutesAgo(0.4),
      'npm test -- rateLimit'
    ),
    currentTurnCard: {
      id: 'rate-turn',
      role: 'executor',
      state: 'live',
      content:
        'The limiter is in and wired into `withApiKey()`. Running the boundary tests against a fake clock now.',
      activity: activity('using_tools', 'Running tests', minutesAgo(0.4), 'npm test -- rateLimit'),
      startedAt: minutesAgo(5),
      updatedAt: minutesAgo(0.4),
      tokenUsage: {
        inputTokens: 22_380,
        outputTokens: 1_746,
        lastUpdatedAt: minutesAgo(0.4),
        source: 'live',
        provider: 'OpenAI'
      },
      cognitiveEvents: rateTools
    },
    cognitiveEvents: rateTools,
    modifiedFiles: [
      { path: 'src/lib/rateLimit.ts', status: 'A', displayPath: 'src/lib/rateLimit.ts' },
      { path: 'src/lib/rateLimit.test.ts', status: 'A', displayPath: 'src/lib/rateLimit.test.ts' },
      { path: 'src/lib/withApiKey.ts', status: 'M', displayPath: 'src/lib/withApiKey.ts' }
    ]
  }
)

const checkoutRetry = basePair(
  'checkout-retry',
  'checkout-retry',
  '/Users/dev/code/storefront',
  'Retry failed payment-provider calls with exponential backoff and jitter, and never retry a request that is not idempotent.',
  {
    status: 'Awaiting Human Review',
    iterations: 3,
    maxIterations: 3,
    currentRunStartedAt: minutesAgo(95),
    createdAt: minutesAgo(95),
    modifiedFiles: [
      { path: 'src/lib/payments/retry.ts', status: 'A', displayPath: 'src/lib/payments/retry.ts' },
      { path: 'src/lib/payments/client.ts', status: 'M', displayPath: 'src/lib/payments/client.ts' }
    ]
  }
)

const invoiceExport = basePair(
  'invoice-pdf-export',
  'invoice-pdf-export',
  '/Users/dev/code/billing-api',
  'Export an invoice as a PDF from the invoices endpoint, streamed, with the line items paginated across pages.',
  {
    status: 'Finished',
    iterations: 2,
    currentRunStartedAt: minutesAgo(260),
    currentRunFinishedAt: minutesAgo(228),
    createdAt: minutesAgo(260),
    modifiedFiles: [
      { path: 'internal/invoice/pdf.go', status: 'A', displayPath: 'internal/invoice/pdf.go' },
      {
        path: 'internal/invoice/handler.go',
        status: 'M',
        displayPath: 'internal/invoice/handler.go'
      }
    ]
  }
)

const account = (id, label, source, maskedSecret) => ({
  id,
  kind: 'apiKey',
  label,
  source,
  status: 'ready',
  headers: [],
  hasSecret: true,
  maskedSecret
})

const model = (id, displayName, contextWindow, maxOutput, efforts, input, output) => ({
  id,
  displayName,
  contextWindow,
  maxOutput,
  efforts,
  defaultEffort: efforts.length > 0 ? 'medium' : undefined,
  source: 'preset',
  price: { inputPerMillion: input, outputPerMillion: output }
})

// Ids, names and prices as in src-tauri/src/accounts/presets.rs.
const providers = [
  {
    id: 'anthropic-1',
    protocol: 'anthropic',
    baseUrl: 'https://api.anthropic.com',
    vendor: 'anthropic',
    displayName: 'Anthropic',
    readyAccountCount: 1,
    accounts: [account('anthropic-1-a', 'ANTHROPIC_API_KEY', 'claude', 'sk-ant…mnop')],
    models: [
      model('claude-sonnet-4-5-20250929', 'Claude Sonnet 4.5', 200_000, 64_000, [], 3, 15),
      model('claude-opus-4-1-20250805', 'Claude Opus 4.1', 200_000, 32_000, [], 15, 75)
    ]
  },
  {
    id: 'openai-responses-1',
    protocol: 'responses',
    baseUrl: 'https://api.openai.com/v1',
    vendor: 'openai',
    displayName: 'OpenAI',
    readyAccountCount: 1,
    accounts: [account('openai-responses-1-a', 'OPENAI_API_KEY', 'codex', 'sk-pro…x7Qd')],
    models: [
      model(
        'gpt-5.1-codex-max',
        'GPT-5.1 Codex Max',
        400_000,
        128_000,
        ['low', 'medium', 'high', 'xhigh'],
        1.25,
        10
      ),
      model('gpt-5.1', 'GPT-5.1', 400_000, 128_000, ['low', 'medium', 'high'], 1.25, 10)
    ]
  },
  {
    id: 'gemini-1',
    protocol: 'openaiChat',
    baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai',
    vendor: 'google',
    displayName: 'Google Gemini',
    readyAccountCount: 0,
    accounts: [
      {
        id: 'gemini-1-a',
        kind: 'apiKey',
        label: 'GEMINI_API_KEY',
        source: 'gemini',
        status: 'needsKey',
        headers: [],
        hasSecret: false
      }
    ],
    models: [model('gemini-2.5-pro', 'Gemini 2.5 Pro', 1_048_576, 65_536, [], 1.25, 10)]
  }
]

const MOCK_STATE = {
  providers,
  snapshots: [spamFilter, rateLimit, checkoutRetry, invoiceExport]
}

// ── The shots ────────────────────────────────────────────────────────────────

/** Each shot starts from a fresh load of the hub. */
const SHOTS = {
  hub: async () => {},
  'pair-detail': async (page) => {
    await page.click('[data-testid="hub-pair-card-contact-spam-filter"] button')
    await page.waitFor('document.body.textContent.includes("TASK_COMPLETE")')
  },
  'pair-running': async (page) => {
    await page.click('[data-testid="hub-pair-card-rate-limit-middleware"] button')
    await page.waitFor('document.body.textContent.includes("fake clock")')
  },
  palette: async (page) => {
    await page.click('[data-testid="hub-pair-card-contact-spam-filter"] button')
    await page.waitFor('document.body.textContent.includes("TASK_COMPLETE")')
    await page.key('k', { meta: true })
    await page.waitFor('!!document.querySelector("[role=dialog] input, [cmdk-input]")')
    await page.type('pair')
  },
  models: async (page) => {
    await page.click('[data-testid="shell-nav-models"]')
    await page.click('[data-testid="provider-card-anthropic-1"] button')
    await page.waitFor('document.body.textContent.includes("ANTHROPIC_API_KEY")')
  }
}

// ── Driving the browser ──────────────────────────────────────────────────────

async function connect() {
  const targets = await (await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`)).json()
  const target = targets.find((entry) => entry.type === 'page')
  if (!target) throw new Error(`no page target on DevTools port ${CDP_PORT}`)
  const socket = new WebSocket(target.webSocketDebuggerUrl)
  await new Promise((resolve, reject) => {
    socket.onopen = resolve
    socket.onerror = () => reject(new Error(`cannot reach the browser on port ${CDP_PORT}`))
  })
  let nextId = 0
  const pending = new Map()
  socket.onmessage = (message) => {
    const reply = JSON.parse(message.data)
    const waiter = pending.get(reply.id)
    if (!waiter) return
    pending.delete(reply.id)
    if (reply.error) waiter.reject(new Error(`${waiter.method}: ${reply.error.message}`))
    else waiter.resolve(reply.result)
  }
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      pending.set(++nextId, { resolve, reject, method })
      socket.send(JSON.stringify({ id: nextId, method, params }))
    })
  return { send, close: () => socket.close() }
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))

function pageFor(send) {
  const evaluate = async (expression) => {
    const { result, exceptionDetails } = await send('Runtime.evaluate', {
      expression,
      returnByValue: true,
      awaitPromise: true
    })
    if (exceptionDetails) throw new Error(`page threw: ${exceptionDetails.text ?? expression}`)
    return result.value
  }
  const waitFor = async (condition, timeoutMs = 10_000) => {
    const deadline = Date.now() + timeoutMs
    while (Date.now() < deadline) {
      if (await evaluate(`Boolean(${condition})`)) return
      await sleep(100)
    }
    throw new Error(`timed out waiting for: ${condition}`)
  }
  return {
    evaluate,
    waitFor,
    click: async (selector) => {
      await waitFor(`document.querySelector(${JSON.stringify(selector)})`)
      await evaluate(`document.querySelector(${JSON.stringify(selector)}).click()`)
    },
    key: async (key, { meta = false } = {}) => {
      const event = {
        key,
        code: `Key${key.toUpperCase()}`,
        windowsVirtualKeyCode: key.toUpperCase().charCodeAt(0),
        modifiers: meta ? 4 : 0
      }
      await send('Input.dispatchKeyEvent', { type: 'keyDown', ...event })
      await send('Input.dispatchKeyEvent', { type: 'keyUp', ...event })
    },
    type: (text) => send('Input.insertText', { text })
  }
}

async function main() {
  const wanted = process.argv.slice(2)
  const unknown = wanted.filter((name) => !(name in SHOTS))
  if (unknown.length > 0) {
    throw new Error(`unknown shot: ${unknown.join(', ')} (have: ${Object.keys(SHOTS).join(', ')})`)
  }
  const names = wanted.length > 0 ? wanted : Object.keys(SHOTS)

  const mockInit = await readFile(MOCK_INIT, 'utf8')
  // Before any app code: a Mac (so shortcuts read ⌘), the dark theme, English,
  // the scene, then the mocked IPC that serves it.
  const seed = `
    Object.defineProperty(navigator, 'platform', { get: () => 'MacIntel' })
    localStorage.setItem('theme-storage', JSON.stringify({ state: { theme: 'dark' }, version: 0 }))
    localStorage.setItem('locale-storage', JSON.stringify({ state: { locale: 'en' }, version: 0 }))
    window.__MOCK_STATE__ = ${JSON.stringify(MOCK_STATE)}
  `

  const { send, close } = await connect()
  try {
    await send('Page.enable')
    await send('Runtime.enable')
    await send('Emulation.setDeviceMetricsOverride', VIEWPORT)
    await send('Emulation.setEmulatedMedia', {
      features: [{ name: 'prefers-color-scheme', value: 'dark' }]
    })
    await send('Page.addScriptToEvaluateOnNewDocument', { source: seed })
    await send('Page.addScriptToEvaluateOnNewDocument', { source: mockInit })

    const page = pageFor(send)
    for (const name of names) {
      await send('Page.navigate', { url: APP_URL })
      await page.waitFor('document.querySelector(\'[data-testid="pairs-hub"]\')')
      // The boot splash holds for 700 ms and fades for 460 more.
      await page.waitFor('!document.querySelector(".boot-splash")')
      await SHOTS[name](page)
      await sleep(600)
      const { data } = await send('Page.captureScreenshot', { format: 'png' })
      const file = new URL(`${name}.png`, OUT_DIR)
      await writeFile(file, Buffer.from(data, 'base64'))
      console.log(`wrote ${fileURLToPath(file)}`)
    }
  } finally {
    close()
  }
}

await main()
