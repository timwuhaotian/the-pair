#!/usr/bin/env node
// Cross-platform replacement for the POSIX `VAR=value command args...` prefix
// in npm scripts (cmd.exe on Windows does not understand it):
//
//   node scripts/with-env.mjs THE_PAIR_E2E_MOCK=true npm run dev
//   node scripts/with-env.mjs A=1 B=2 -- some-command --flag
//
// Leading NAME=value arguments are added to the environment; the rest is the
// command. On macOS/Linux the command is spawned directly (no shell), exactly
// like `VAR=value command` would. On Windows it goes through cmd.exe so that
// `.cmd` shims such as npm.cmd or node_modules/.bin/wdio.cmd resolve.
import { spawn } from 'node:child_process'
import { realpathSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import process from 'node:process'

const ASSIGNMENT = /^([A-Za-z_][A-Za-z0-9_]*)=(.*)$/s

export function parseWithEnvArgs(argv) {
  const env = {}
  let index = 0

  for (; index < argv.length; index += 1) {
    const match = argv[index].match(ASSIGNMENT)
    if (!match) {
      break
    }
    env[match[1]] = match[2]
  }

  if (argv[index] === '--') {
    index += 1
  }

  const [command, ...args] = argv.slice(index)
  if (!command) {
    throw new Error(
      'Usage: node scripts/with-env.mjs NAME=value [NAME=value...] [--] <command> [args...]'
    )
  }

  return { env, command, args }
}

/** Quotes one argument for a cmd.exe command line. */
export function quoteWindowsArg(arg) {
  if (arg !== '' && /^[A-Za-z0-9_\-+=.,:/\\@]+$/.test(arg)) {
    return arg
  }
  return `"${arg.replace(/(\\*)"/g, '$1$1\\"').replace(/(\\+)$/, '$1$1')}"`
}

export function buildWindowsCommandLine(command, args) {
  return [command, ...args].map(quoteWindowsArg).join(' ')
}

function main() {
  const { env, command, args } = parseWithEnvArgs(process.argv.slice(2))
  const childEnv = { ...process.env, ...env }

  const child =
    process.platform === 'win32'
      ? spawn(buildWindowsCommandLine(command, args), {
          env: childEnv,
          stdio: 'inherit',
          shell: true
        })
      : spawn(command, args, { env: childEnv, stdio: 'inherit', shell: false })

  // Ctrl+C / Ctrl+Break reach the whole foreground process group (the child
  // included), so the wrapper only has to stay alive until the child exits.
  // Signals aimed at the wrapper alone (e.g. `kill <pid>`) are forwarded.
  const handlers = {
    SIGINT: () => {},
    SIGBREAK: () => {},
    SIGTERM: () => forward('SIGTERM'),
    SIGHUP: () => forward('SIGHUP')
  }
  function forward(signal) {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill(signal)
    }
  }
  for (const [signal, handler] of Object.entries(handlers)) {
    process.on(signal, handler)
  }

  child.on('error', (error) => {
    console.error(`with-env: failed to start "${command}": ${error.message}`)
    process.exit(1)
  })

  child.on('exit', (code, signal) => {
    if (signal) {
      // Exit the same way the child did, like `VAR=value command` would.
      for (const [name, handler] of Object.entries(handlers)) {
        process.off(name, handler)
      }
      process.kill(process.pid, signal)
      return
    }
    process.exit(code ?? 1)
  })
}

function isMainModule() {
  try {
    return (
      Boolean(process.argv[1]) && realpathSync(process.argv[1]) === fileURLToPath(import.meta.url)
    )
  } catch {
    return false
  }
}

if (isMainModule()) {
  try {
    main()
  } catch (error) {
    console.error(error instanceof Error ? error.message : String(error))
    process.exit(1)
  }
}
