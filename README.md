<!-- prettier-ignore -->
<div align="center">

<img src="./build/icon.png" alt="The Pair" width="128" />

# The Pair

**Automated pair programming — grab a coffee while two AI agents cross-check each other's work, built by itself**

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![GitHub release](https://img.shields.io/github/v/release/timwuhaotian/the-pair?include_prereleases&logo=github)](https://github.com/timwuhaotian/the-pair/releases)
[![Build Status](https://github.com/timwuhaotian/the-pair/actions/workflows/build.yml/badge.svg)](https://github.com/timwuhaotian/the-pair/actions)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.9-3178c6.svg?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-24c8db.svg?logo=tauri&logoColor=white)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-19-61dafb.svg?logo=react&logoColor=black)](https://react.dev/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Changelog](https://img.shields.io/badge/Changelog-CHANGELOG.md-informational)](CHANGELOG.md)

**macOS** • **Windows** • **Linux**

![CleanShot2026-03-27at03 02 42-ezgif com-optimize](https://github.com/user-attachments/assets/b9d0f06c-c167-45f1-9154-0c49187296ab)

_Watch Mentor and Executor agents collaborate in real-time_

</div>

---

## Overview

**Worried about AI code hallucinations?** The Pair solves this by running two AI agents that cross-check each other:

- **Mentor Agent** — Plans, reviews, and validates (read-only)
- **Executor Agent** — Writes code and runs commands

While they work, go grab a coffee. Come back to reviewed, cross-validated code.

### Key Benefits

- **Dual-Model Cross-Validation** — Two models check each other's work, dramatically reducing code hallucinations
- **Automated Collaboration** — Agents work together without constant human intervention
- **Real-Time Monitoring** — Watch CPU/memory usage per agent with live activity tracking
- **Git Integration** — Automatic tracking of all file changes made during a session
- **Human Oversight** — Step in at any time to pause, adjust, or reassign tasks
- **Session Recovery** — Resume interrupted sessions with full conversation history restoration
- **Onboarding Wizard** — Guided first-time setup with model configuration and directory selection
- **Dark/Light Themes** — Automatic system theme detection with manual toggle

### Use Cases

- Autonomous coding sessions — Let AI agents iterate on features while you focus on review
- Code refactoring — Automated analysis and implementation of improvements
- Bug fixing — Agents collaborate to diagnose and resolve issues
- Learning tool — Observe how AI agents break down and solve problems
- Interrupted work recovery — Restore session state after app restart or crash

---

## Features

- **Dual-Agent Architecture** — Separation of planning (Mentor) and execution (Executor)
- **Full Automation Mode** — Agents work autonomously with workspace-scoped permissions
- **Real-Time Activity Tracking** — Live status showing agent activity (thinking, doing, waiting)
- **Resource Monitoring** — CPU and memory usage per agent, updated every second
- **Git Change Tracking** — Automatic detection of modified, added, or deleted files
- **Conversation History** — Full transcript of all agent interactions
- **Local Orchestration** — Runs the app and agent coordination locally; model calls depend on your selected provider or local model
- **Multi-Provider** — Works with opencode, Claude Code, Codex, and Gemini CLI
- **Reasoning Controls** — Adjust thinking effort per agent role (low/medium/high)
- **Token Tracking** — Real-time per-turn token usage displayed inline
- **Skill System** — Attach project-specific skill files to guide agent behavior
- **Auto-Update** — In-app update checking with one-click install

---

## Screenshots

Review Result - Fail With Evidence
<img width="2800" height="2000" alt="Review result showing failed evidence" src="./docs/assets/intro-1.png" />

Review Result - Pass With Evidence
<img width="2800" height="2000" alt="Review result showing passing evidence" src="./docs/assets/intro-3.png" />

---

## Installation

Download the latest release from [GitHub Releases](https://github.com/timwuhaotian/the-pair/releases):

| Platform    | File                           |
| ----------- | ------------------------------ |
| **macOS**   | `the-pair-{version}.zip`       |
| **Windows** | `the-pair-{version}-setup.exe` |
| **Linux**   | `the-pair-{version}.AppImage`  |

### From Source

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run build:mac  # or build:win / build:linux
```

On macOS, `build:mac` produces a local DMG, while `build:mac:release` produces the ZIP-style release bundle used in GitHub Releases. The build script will ensure the required Rust targets are installed before invoking Tauri. If you prefer to set them up manually, run:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

---

## Quick Start

> [!NOTE]
> The Pair requires at least one AI provider CLI: [opencode](https://opencode.ai), [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://github.com/openai/codex), or [Gemini CLI](https://github.com/google-gemini/gemini-cli).

### 1. Install an AI Provider

Install one or more of the supported CLIs:

- **opencode** — `curl -fsSL https://opencode.ai/install | bash` or `npm install -g opencode-ai`
- **Claude Code** — see [Claude Code setup](https://docs.anthropic.com/en/docs/claude-code/getting-started), or use `npm install -g @anthropic-ai/claude-code`
- **Codex** — `npm install -g @openai/codex`
- **Gemini CLI** — `npm install -g @google/gemini-cli` or `npx @google/gemini-cli`

### 2. Configure AI Models (Optional)

For opencode-backed models, set up your AI providers in `~/.config/opencode/opencode.json`:

```json
{
  "provider": {
    "openai": { "options": { "apiKey": "your-api-key" } },
    "anthropic": { "options": { "apiKey": "your-api-key" } }
  }
}
```

> [!TIP]
> Codex, Claude Code, and Gemini CLI are detected from their installed CLIs and sign-in state. You can also use local models with [Ollama](https://ollama.com) for offline development.

### 3. Launch The Pair

Open from Applications folder or start menu.

### 4. Create Your First Pair

1. Click **New Pair** button
2. Configure: name, directory, task description, and AI models
3. Watch the agents work — Mentor plans, Executor implements, Mentor reviews
4. Monitor progress with real-time activity tracking and file changes

---

## Configuration

### Provider Configuration

OpenCode-backed models use your existing opencode configuration:

- **macOS/Linux**: `~/.config/opencode/opencode.json`
- **Windows**: `%APPDATA%/opencode/opencode.json`

Codex, Claude Code, and Gemini CLI are detected from their local CLI install and account state.

### Pair Runtime

Each pair maintains its own runtime configuration in `.pair/runtime/<pairId>/` within your project directory, including session files, runtime permissions, and conversation history.

> [!NOTE]
> The Pair does not modify your global opencode permissions. All permissions are session-specific.

---

## Architecture

### Tech Stack

| Layer          | Technology            |
| -------------- | --------------------- |
| **Framework**  | Tauri 2.x             |
| **Backend**    | Rust                  |
| **Frontend**   | React 19 + TypeScript |
| **Styling**    | Tailwind CSS v4       |
| **State**      | Zustand               |
| **Animations** | Framer Motion         |
| **Icons**      | Lucide React          |

### System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    The Pair App                         │
├─────────────────────────────────────────────────────────┤
│  Frontend (React UI)                                    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │  Dashboard   │ Pair Detail  │    Settings      │    │
│  └──────────────┴──────────────┴──────────────────┘    │
│                          ↕ Tauri IPC                    │
├─────────────────────────────────────────────────────────┤
│  Backend (Rust)                                         │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │ PairManager  │MessageBroker │ ProcessSpawner   │    │
│  │ (Lifecycle)  │(State Machine)│(Multi-Provider) │    │
│  └──────────────┴──────────────┴──────────────────┘    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │ Git Tracker  │ Worktrees    │ Session Snapshot │    │
│  └──────────────┴──────────────┴──────────────────┘    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │Resource Mon. │ Acceptance   │ Report Generator │    │
│  └──────────────┴──────────────┴──────────────────┘    │
└─────────────────────────────────────────────────────────┘
                            ↕
              ┌─────────────┴─────────────┐
              ↙                           ↘
     ┌─────────────────┐          ┌─────────────────┐
     │  AI Provider CLIs│          │   Git Repo      │
     │ opencode/Claude/ │          │  (Workspace)    │
     │ Codex/Gemini     │          └─────────────────┘
     └─────────────────┘
```

### Agent Workflow

```
Start → Initialize & Baseline → Mentoring Phase → Executing Phase → Reviewing Phase
                                                        ↓
                                              Done? ──Yes→ Finished
                                                 │
                                                 No
                                                 ↓
                                         (loop back to Mentoring)
```

---

## Development

### Prerequisites

- **Node.js** 22.22+
- **npm** or **pnpm**
- **Git**
- **Rustup** for desktop builds

> [!NOTE]
> Release builds require the updater signing secret to be configured in GitHub Actions.

Run a quick environment check before building:

```bash
npm run preflight
```

### Setup

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run dev
```

### Project Structure

```
the-pair/
├── src/
│   └── renderer/          # React frontend
│       └── src/
│           ├── App.tsx
│           ├── components/
│           └── store/
├── src-tauri/             # Rust backend
│   ├── src/
│   │   ├── lib.rs
│   │   ├── pair_manager.rs
│   │   ├── message_broker.rs
│   │   └── ...
│   └── Cargo.toml
├── build/                 # Build resources
└── package.json
```

### Scripts

| Command                     | Description                         |
| --------------------------- | ----------------------------------- |
| `npm run dev`               | Start hot-reload development server |
| `npm run preflight`         | Check local build prerequisites     |
| `npm run preflight:mac`     | Check macOS build prerequisites     |
| `npm run preflight:win`     | Check Windows build prerequisites   |
| `npm run preflight:linux`   | Check Linux build prerequisites     |
| `npm test`                  | Run JavaScript and Rust unit tests  |
| `npm run test:js`           | Run Node/TypeScript unit tests      |
| `npm run test:rust`         | Run Rust unit tests                 |
| `npm run typecheck`         | Check TypeScript types              |
| `npm run lint`              | Run ESLint                          |
| `npm run format`            | Format with Prettier                |
| `npm run e2e:setup`         | Install the Appium macOS driver     |
| `npm run e2e`               | Run mocked end-to-end tests         |
| `npm run dev:mock`          | Start the app with mocked agents    |
| `npm run dev:smoke`         | Start the app in smoke-test mode    |
| `npm run clean`             | Remove generated build artifacts    |
| `npm run build:mac`         | Build local macOS DMG               |
| `npm run build:mac:release` | Build macOS release ZIP bundle      |
| `npm run build:win`         | Build for Windows                   |
| `npm run build:linux`       | Build for Linux                     |

---

## FAQ

**Q: How does The Pair differ from single-agent AI coding tools?**

A: Single-agent tools rely on one model to write and self-review code, which can miss its own mistakes. The Pair uses two separate agents where the Mentor reviews the Executor's work, catching errors before they land.

**Q: Does The Pair require internet connectivity?**

A: The Pair runs entirely locally. Only the AI model API calls require internet (or local model setup via Ollama).

**Q: Which AI providers are supported?**

A: The Pair supports four providers out of the box: **opencode** (any compatible model), **Claude Code CLI**, **OpenAI Codex CLI**, and **Gemini CLI**. Codex, Claude, and Gemini are detected automatically from their installed CLIs. You can mix providers — e.g., Claude as Mentor and Codex as Executor.

**Q: Can I use my own AI models?**

A: Yes, The Pair is model-agnostic. Opencode-backed models work with any compatible provider (OpenAI, Anthropic, Ollama, etc.). For Claude, Codex, and Gemini, simply install their CLI and sign in.

**Q: Can I control how much the agents "think"?**

A: Yes. The Pair supports **reasoning effort controls** for models that offer it (Claude, Codex o-series, Gemini 2.5). You can set low/medium/high effort per role — Mentor and Executor independently — from pair creation or settings.

**Q: How do I track token usage and costs?**

A: Token usage is tracked in real-time per agent turn. Live output token counts appear inline in the agent console so you can monitor spend as agents work.

**Q: What happens if an agent gets stuck in a loop?**

A: The Pair implements iteration limits. After a configured number of iterations, agents pause for human intervention.

**Q: What if the app crashes or I close it mid-session?**

A: Session snapshots are saved automatically. On relaunch, The Pair detects interrupted sessions and offers to restore them with full conversation history, so agents can continue from where they left off.

**Q: Does The Pair auto-update?**

A: Yes. The Pair checks for new versions on launch and notifies you with a one-click update flow. No manual download needed.

---

<div align="center">

Built with ❤️ by [timwuhaotian](https://github.com/timwuhaotian)

**⭐ Star this repo if you find it helpful!**

</div>
