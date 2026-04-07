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
- [ ] Version in `package.json` is bumped
- [ ] `CHANGELOG.md` includes the release notes
- [ ] `README.md` still matches the current install and test flow
- [ ] `LICENSE`, `README.md`, `CONTRIBUTING.md`, and `SECURITY.md` are present

## 2. Quality Gates

Run these commands locally before tagging:

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
- [ ] `src-tauri/tauri.conf.json` `plugins.updater.pubkey` matches the current updater private key

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

1. `detect-version-bump` checks if version in package.json changed
2. If tag doesn't exist → auto-publishes (lints, builds, tags, releases)
3. If tag already exists → skips (prevents duplicate releases)

**Manual trigger (fallback):**

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
- If signing or notarization is enabled, add the signing validation steps here before release day.
