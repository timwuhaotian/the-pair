#!/usr/bin/env node
// Runs the Rust test suite under coverage (cargo-llvm-cov).
//
// cargo-llvm-cov needs llvm-cov / llvm-profdata. Where they live depends on how
// Rust was installed, so try the usual places in order and pin the pair that
// actually exists: the rustup sysroot, then anything on PATH, then Homebrew's
// llvm formula. Handing cargo-llvm-cov a path that cannot be executed makes it
// fail with "never executed", so it is only given one that works.
import { execFileSync, spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { delimiter, join } from 'node:path'

const run = (cmd, args) => execFileSync(cmd, args, { encoding: 'utf8' }).trim()

const host = run('rustc', ['-vV']).match(/^host: (\S+)/m)?.[1]
if (!host) {
  console.error('coverage-rust: could not determine the host triple')
  process.exit(1)
}

const searchDirs = [
  join(run('rustc', ['--print', 'sysroot']), 'lib', 'rustlib', host, 'bin'),
  ...(process.env.PATH ?? '').split(delimiter).filter(Boolean),
  '/opt/homebrew/opt/llvm/bin'
]

const findTool = (name) => {
  for (const dir of searchDirs) {
    const path = join(dir, name)
    if (existsSync(path)) return path
  }
  return undefined
}

const llvmCov = findTool('llvm-cov')
const llvmProfdata = findTool('llvm-profdata')

if (!llvmCov || !llvmProfdata) {
  console.error(
    'coverage-rust: could not find llvm-cov / llvm-profdata.\n' +
      '  Install the rustup component (`rustup component add llvm-tools-preview`)\n' +
      '  or Homebrew\'s llvm (`brew install llvm`), then re-run.'
  )
  process.exit(1)
}

const result = spawnSync('cargo', ['llvm-cov', '--lib', '--summary-only'], {
  cwd: 'src-tauri',
  stdio: 'inherit',
  env: { ...process.env, LLVM_COV: llvmCov, LLVM_PROFDATA: llvmProfdata }
})

process.exit(result.status ?? 1)
