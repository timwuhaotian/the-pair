# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

A Tauri 2 desktop app (macOS, Windows, Linux) whose UI is a React web view. The design language is the app's own, not native platform chrome. Minimum window width is 1200px (`src-tauri/tauri.conf.json`).

## Users

Developers who already use AI coding CLIs (Claude Code, Codex, opencode, Gemini/Antigravity, Kimi Code, Pi, Kiro, Aider, Grok Build, Muse Code). The primary scene is **watching and steering live**: The Pair sits open beside the editor, the developer reads the Mentor and Executor as they work, and steps in by stopping a turn, sending feedback, approving or sending back a plan, or reassigning models.

Hand-off-and-return ("grab a coffee") is a supported secondary workflow and part of the marketing story, but design decisions favor the live-steering scene first.

## Product Purpose

Run two AI coding agents against one task so they cross-check each other: a read-only **Mentor** plans and reviews, and an **Executor** writes code and runs commands. One model writing and judging its own work misses its own mistakes; an independent reviewer catches them before they reach the codebase. Success means the developer gets reviewed, validated changes with less babysitting and keeps control at every decision point.

## Positioning

- Two independent agents with separated roles (read-only reviewer plus writer), not one assistant reviewing itself.
- Model-agnostic and local: it orchestrates the provider CLIs the user already has installed, and any agent can fill either role (for example Claude as Mentor and Codex as Executor).
- Open source (Apache 2.0) and free. A terminal companion, Pair Code (`pair-code` on npm), covers terminal, script, and SSH workflows.

## Operating Context

- Lives next to the code editor during a working session, often for long-running pairs.
- Works in a local git repository: tracks modified files against a baseline commit, shows inline diffs, commits and pushes, and can run each task in a fresh worktree on an auto-named branch.
- Turn loop: Idle → Mentoring → Executing → Reviewing → (loop | Finished | Paused | Awaiting Human Review | Error). An optional plan gate pauses after the Mentor's first plan for human approval. An optional per-pair iteration cap pauses for human review when reached; runs are unbounded by default.
- Monitors per-agent CPU and memory, detects stalled turns, and restores full session state after a restart or crash.

## Capabilities and Constraints

- Ten provider kinds, each with its own transport, session, permission, and working-directory strategy. Models are detected from installed CLIs; users pick an agent and then one of its models for each role.
- Pair console: one transcript of task, plans, results, acceptance checks, human feedback, and live turn cards with tool-call and reasoning steps. Includes search, role filters, task history, and archived runs.
- Acceptance: the Mentor runs checks (tests, lint, and so on) and returns a structured verdict (pass/fail, risk, evidence, next step).
- Presets, skills (`/skill`), and file mentions (`@path`) attach context to tasks.
- Fixed terminology, never renamed or paraphrased in any locale: **Pair**, **Mentor**, **Executor**. A "task" is what the user gives a pair; "acceptance" is the Mentor's check of the Executor's work; "plan review" is the human approval gate.
- Mock mode (`THE_PAIR_E2E_MOCK=true`) runs the UI without real CLI processes.

## Brand Commitments

- **Terminal/TTY voice:** mono, terse, ASCII rules (`── checks ──`), glyph markers, compact uppercase tags. This is the product's identity, not decoration to remove.
- **Light humor stays:** lines such as "Mentor plans. Executor ships." and "grab a coffee" belong to the voice. Humor never appears in errors, destructive confirmations, or blocked work.
- **Four equal locales:** English, 简体中文, 日本語, and 한국어 are all first-class. Every new user-facing string ships in all four.
- Name "The Pair", the app icon (`docs/assets/app-icon-light.svg`, `docs/assets/app-icon-dark.svg`), and the logo (`resources/logo-the-pair.png`).

## Evidence on Hand

- Screenshots: `docs/assets/hub.png`, `docs/assets/pair-detail.png`, `docs/assets/pair-running.png`, `docs/assets/palette.png`, `docs/assets/social-preview.png`.
- Launch film: `docs/assets/launch-10s.mp4` (GIF: `docs/assets/launch-10s.gif`).
- The Pair was built using The Pair (stated in the README).
- Comparison table against Cursor, Copilot, and solo Claude Code in `README.md`.
- None of the following exists, and none may be invented: user testimonials, customer logos, usage or download figures beyond the live GitHub badges, benchmark results, or pricing (the app is free).

## Product Principles

1. **The human steers.** Every automated step leaves a clear point to pause, stop, approve, or redirect, and the current state is always legible at a glance.
2. **Show the work.** Agent reasoning, tool calls, checks, and verdicts are surfaced plainly so the developer can trust or challenge them.
3. **Roles stay distinct.** Mentor and Executor are always visually and verbally distinguishable; the separation is the product.
4. **Bring your own models.** No provider is privileged; any installed agent can fill either role.
5. **Local and recoverable.** Work stays on the user's machine and in their git history, and sessions survive interruption.

## Accessibility & Inclusion

WCAG 2.2 AA is the floor for every surface: text contrast, full keyboard operation, visible focus, and screen-reader names that match visible labels. Role and state are never conveyed by color alone. All four locales must survive text expansion at the minimum window width.
