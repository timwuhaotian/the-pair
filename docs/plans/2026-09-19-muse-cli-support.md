# Plan — Muse Code CLI provider support

Adds Meta's Muse Code CLI (`muse`) as the 9th provider in The Pair.

## Verified CLI surface (muse 1.0.3, 2026-09-19)

Captured from the installed binary, not the public docs — `dev.meta.ai/docs/muse-code`
still documents `muse-spark-1.2` as default while the shipped binary uses
`muse-spark-1.3`.

    muse exec --json [--session-id <uuid>] --model <id>
             [--reasoning-effort <none|minimal|low|medium|high|xhigh|max|ultra>]
             --approval-mode <untrusted|on-request|never>
             [--disable-write --disable-shell]
             <PROMPT>

`--json` emits JSONL envelopes: `{schema_version, id, stream{kind,id}, sequence,
payload_type, payload}`.

| Need            | Source                                                                                                     |
| --------------- | ---------------------------------------------------------------------------------------------------------- |
| Final turn text | `run.terminal.completed` -> `payload.text`                                                                 |
| Session id      | `stream.id` where `stream.kind == "session"`                                                               |
| Error detail    | `payload.reason` when `payload.terminal != "completed"`; `task.lifecycle.failed` -> `payload.event.reason` |
| Token usage     | not emitted -> `extract_token_usage` returns `None`                                                        |

Session semantics: `--session-id` is create-or-resume. Verified by running the
same id twice — the second run continued the stream (sequence 28 -> 29) instead
of starting a new one. So `SessionStrategy::NewFirst` works: omit on turn 1,
capture `stream.id`, pass it on turn 2+.

Models: no `muse models list` subcommand, and unknown ids are accepted silently
(a bogus `--model` still completed a run), so the catalog cannot be probed.
Seed the two known ids and union the user's configured model from
`~/.config/muse/settings.json`.

Auth: `~/.config/muse/auth.json` -> `providers.meta`, or `META_API_KEY`
(which `muse login --help` states takes priority over account login).

## Tasks

1. **`provider_registry.rs`** — add `ProviderKind::Muse`; `detect_muse()`;
   `discover_muse_models()` reading `~/.config/muse/settings.json` unioned with
   the static seed; `muse_authenticated()` checking `META_API_KEY` then
   `auth.json`. Tests: seed-only catalog, settings.json union, dedupe.
2. **`providers/muse.rs`** — new `MuseProvider` implementing `Provider`.
   `build_turn_command` emits the verified flags, adds `--disable-write
--disable-shell` when `role == "mentor"`. `collect_json_candidates` returns
   ONLY `run.terminal.*` text (bypasses the generic walker so
   `turn.input.user.prompt` never echoes back and deltas don't double-count).
   Tests: mentor vs executor flags, session id passthrough, candidate isolation.
3. **`providers/mod.rs`** — `pub mod muse;` + register in `all_providers()`.
4. **`provider_adapter.rs`** — `infer_provider_kind`: `muse/` prefix and bare
   `muse` keyword. Tests.
5. **`process_spawner.rs`** — `extract_session_id`: read `stream.id` when
   `stream.kind == "session"`. Guarded so no other provider can match. Tests
   including a full muse stream pipeline test.
6. **`model_catalog.rs`** — `normalize_provider_label`: `"muse"` -> `"Muse"`.
7. **Frontend** — `types.ts` union; `providerResolution.ts` inference;
   `modelResolution.ts` `stripProviderPrefix` (muse ids are bare and
   unambiguous, so strip like claude/codex/gemini); `modelCatalogGrouping.ts`
   `PROVIDER_PRIORITY`; `providerSetup.ts` login command + install URL.
8. **`tests/museProvider.test.ts`** — frontend inference/stripping/priority.
9. **Docs** — AGENTS.md + CLAUDE.md provider lists, CHANGELOG.

## Gate

`npm test && npm run typecheck && npm run lint` green in the worktree.
