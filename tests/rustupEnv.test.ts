import assert from 'node:assert/strict'
import test from 'node:test'

import { prependPathEntry, resolveRustupBinDir } from '../scripts/rustup-env.mjs'

function fakeFs(files: string[], links: Record<string, string> = {}) {
  const present = new Set(files)
  return {
    exists: (path: string) => present.has(path),
    realpath: (path: string) => links[path] ?? (present.has(path) ? path : null)
  }
}

test('prependPathEntry joins PATH entries with the platform delimiter', () => {
  assert.equal(
    prependPathEntry('/Users/example/.cargo/bin', '/usr/local/bin:/usr/bin', ':'),
    '/Users/example/.cargo/bin:/usr/local/bin:/usr/bin'
  )

  assert.equal(
    prependPathEntry('C:\\Users\\example\\.cargo\\bin', 'C:\\Windows\\System32;C:\\Windows', ';'),
    'C:\\Users\\example\\.cargo\\bin;C:\\Windows\\System32;C:\\Windows'
  )
})

test('resolveRustupBinDir follows a Homebrew rustup symlink to its proxies (GitHub macOS runner)', () => {
  // Cargo home is set but empty: rust-cache missed and rustup came from Homebrew.
  const cellar = '/opt/homebrew/Cellar/rustup/1.29.0/bin'
  const fs = fakeFs([`/opt/homebrew/bin/rustup`, `${cellar}/rustup`, `${cellar}/cargo`], {
    '/opt/homebrew/bin/rustup': `${cellar}/rustup`
  })

  const dir = resolveRustupBinDir({
    env: { PATH: '/usr/bin:/opt/homebrew/bin', CARGO_HOME: '/Users/runner/.cargo' },
    platform: 'darwin',
    home: '/Users/runner',
    ...fs
  })

  assert.equal(dir, cellar)
})

test('resolveRustupBinDir uses the rustup-init proxy dir found on PATH', () => {
  const fs = fakeFs(['/home/u/.cargo/bin/rustup', '/home/u/.cargo/bin/cargo'])

  const dir = resolveRustupBinDir({
    env: { PATH: '/usr/bin:/home/u/.cargo/bin' },
    platform: 'linux',
    home: '/home/u',
    ...fs
  })

  assert.equal(dir, '/home/u/.cargo/bin')
})

test('resolveRustupBinDir falls back to Cargo home when rustup is not on PATH', () => {
  const fs = fakeFs(['/home/u/.cargo/bin/rustup', '/home/u/.cargo/bin/cargo'])

  assert.equal(
    resolveRustupBinDir({ env: { PATH: '/usr/bin' }, platform: 'linux', home: '/home/u', ...fs }),
    '/home/u/.cargo/bin'
  )
})

test('resolveRustupBinDir ignores a Cargo home that only holds cargo-installed tools', () => {
  // A restored cache can leave ~/.cargo/bin with no proxies in it.
  const fs = fakeFs(['/home/u/.cargo/bin/cargo-audit'])

  assert.equal(
    resolveRustupBinDir({ env: { PATH: '/usr/bin' }, platform: 'linux', home: '/home/u', ...fs }),
    null
  )
})

test('resolveRustupBinDir matches .exe proxies and a Windows Path key', () => {
  const fs = fakeFs(['C:/Users/u/.cargo/bin/rustup.exe', 'C:/Users/u/.cargo/bin/cargo.exe'])

  assert.equal(
    resolveRustupBinDir({
      env: { Path: 'C:/Windows;C:/Users/u/.cargo/bin' },
      platform: 'win32',
      home: 'C:/Users/u',
      ...fs
    }),
    'C:/Users/u/.cargo/bin'
  )
})
