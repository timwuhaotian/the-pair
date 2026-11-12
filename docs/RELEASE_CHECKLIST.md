# Release Checklist

Use this checklist before cutting a public release of The Pair.

**⚠️ CRITICAL: The release workflow is fully automated. Never manually create or push tags.**

The GitHub Actions workflow (`build-signed-mac.yml`) automatically:

- Detects version bumps in package.json
- Creates tags
- Builds all platform binaries
- Publishes GitHub releases
- Uploads signed artifacts

## 1. Repo Hygiene

- [ ] Working tree is clean except for the intended release commit
- [ ] Version bumped with `npm run bump <version>` (updates `package.json`, the root entries of `package-lock.json`, the `[package]` version in `src-tauri/Cargo.toml` and the app entry in `src-tauri/Cargo.lock`; `tauri.conf.json` reads `package.json`)
- [ ] `CHANGELOG.md` has a non-empty `## [X.Y.Z] - YYYY-MM-DD` section, heading at the start of a line (`npm run validate:changelog` passes)
- [ ] `README.md` still matches the current install and test flow
- [ ] `LICENSE`, `README.md`, `CONTRIBUTING.md`, and `SECURITY.md` are present

## 2. Quality Gates

Run these commands locally before pushing the release commit:

```bash
npm test
npm run typecheck
npm run lint
npm run build
```

- [ ] `npm test` passes
- [ ] `npm run typecheck` passes
- [ ] `npm run lint` passes
- [ ] `npm run build` passes

## 3. Release Metadata

- [ ] `package.json` still points to the correct repository and bugs URLs
- [ ] `src-tauri/Cargo.toml` has the correct package metadata
- [ ] `docs/RELEASE_CHECKLIST.md` was reviewed for any stale release process notes
- [ ] Release notes mention any breaking changes or migration steps
- [ ] `TAURI_SIGNING_PRIVATE_KEY` and optional password secrets are configured in GitHub Actions
- [ ] The updater signing key validates in CI before the build stage
- [ ] The collected updater packages pass signature verification against `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`

## 4. Binary Checks

- [ ] App launches locally after the build
- [ ] Icons render correctly on macOS, Windows, and Linux bundles
- [ ] Release artifacts are generated for macOS, Windows, and Linux
- [ ] `npm run preflight`, `npm run build:mac`, `npm run build:win`, or `npm run build:linux` work for the target platform

## 5. GitHub Release (Automated)

**⚠️ IMPORTANT: Do NOT create or push tags manually!**

The release workflow is fully automated:

- [ ] Push the version bump commit to main branch (`git push`)
- [ ] Wait for GitHub Actions workflow to detect version bump
- [ ] Workflow auto-creates tag and publishes release with all artifacts
- [ ] Monitor workflow at: https://github.com/timwuhaotian/the-pair/actions
- [ ] Verify the release page downloads without authentication
- [ ] Verify the release notes are readable and complete

**Workflow flow:**

1. `detect-version-bump` checks the version's tag, release and assets. It skips only a complete public release; a draft, orphan tag or incomplete release fails for manual inspection.
2. For a new version, the workflow validates the changelog, runs frontend checks and Rust tests in parallel, then builds all platforms.
3. `publish-release` verifies all three updater signatures against the app's configured public key, then rechecks the state and creates the tag and draft. It compares the uploaded assets' GitHub SHA-256 digests with the local files before publishing. Existing releases and assets are never overwritten.
4. Release runs share a concurrency group and are not cancelled mid-run. GitHub may replace an already pending run with a newer one; intermediate versions are not guaranteed to publish.

`detect-version-bump` and `publish-release` have `contents: write` (the former needs draft visibility); other jobs are read-only, and checkouts don't persist the token. The updater key and macOS certificate are removed from `$RUNNER_TEMP` by a cleanup step even on failure, although abrupt runner termination can prevent cleanup. Third-party actions (`dtolnay/rust-toolchain`, `Swatinem/rust-cache`) are pinned to commit SHAs — update the SHA and its `# version` comment together when upgrading.

If a release fails after tag creation, inspect the tag commit, draft, run artifacts and signatures. For a draft, upload only verified missing assets and publish only after checking the complete set. For an orphan tag, inspect it and choose a new version or resolve it manually. Never overwrite a public release's assets; publish fixes under a new version. The workflow does not delete tags or releases automatically.

**Manual trigger (fallback):** The selected commit must be reachable from `main`.

```bash
gh workflow run build-signed-mac.yml
```

## 6. Post-Release Verification

- [ ] Download the published artifact from GitHub Releases
- [ ] Install and launch the app from the packaged artifact
- [ ] Confirm `npm test` still passes on the release commit
- [ ] Confirm the new version is visible in the app and release page

## Notes

- Keep the checklist updated whenever the release workflow changes.
