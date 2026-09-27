#!/usr/bin/env node
// Runs the Rust test suite under coverage (cargo-llvm-cov) with the llvm tools
// of the active toolchain exported, since cargo-llvm-cov does not always find
// them through rustup on its own.
import { execFileSync, spawnSync } from 'node:child_process'
import { join } from 'node:path'

const run = (cmd, args) =>
  execFileSync(cmd, args, { encoding: 'utf8' }).trim()

const sysroot = run('rustc', ['--print', 'sysroot'])
const host = run('rustc', ['-vH']).match(/^host: (\S+)/m)?.[1]
if (!host) {
  console.error('coverage-rust: could not determine the host triple')
  process.exit(1)
}

const bin = join(sysroot, 'lib', 'rustlib', host, 'bin')
const result = spawnSync('cargo', ['llvm-cov', '--lib', '--summary-only'], {
  cwd: 'src-tauri',
  stdio: 'inherit',
  env: {
    ...process.env,
    LLVM_COV: join(bin, 'llvm-cov'),
    LLVM_PROFDATA: join(bin, 'llvm-profdata')
  }
})

process.exit(result.status ?? 1)
