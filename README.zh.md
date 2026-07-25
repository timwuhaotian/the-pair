<!-- prettier-ignore -->
<div align="center">

<img src="./resources/logo-the-pair.png" alt="The Pair" width="128" />

# The Pair

**自动化 AI 结对编程 —— 两个 AI Agent 互相交叉校验代码，让你喝杯咖啡的工夫就能拿到经过审查、交叉验证的成果。_（没错，The Pair 正是由 The Pair 自己构建的。）_**

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![GitHub release](https://img.shields.io/github/v/release/timwuhaotian/the-pair?include_prereleases&logo=github)](https://github.com/timwuhaotian/the-pair/releases)
[![Build Status](https://github.com/timwuhaotian/the-pair/actions/workflows/build.yml/badge.svg)](https://github.com/timwuhaotian/the-pair/actions)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.9-3178c6.svg?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-24c8db.svg?logo=tauri&logoColor=white)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-19-61dafb.svg?logo=react&logoColor=black)](https://react.dev/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Changelog](https://img.shields.io/badge/Changelog-CHANGELOG.md-informational)](CHANGELOG.md)

🌐 [English](README.md) • **简体中文** • [한국어](README.ko.md) • [日本語](README.ja.md)

**macOS** • **Windows** • **Linux** &nbsp;|&nbsp; [**⬇ 下载**](https://github.com/timwuhaotian/the-pair/releases) &nbsp;•&nbsp; [**CLI**](https://github.com/timwuhaotian/pair-code) &nbsp;•&nbsp; [**🌐 官网**](https://apps.timwuhaotian.dev/)

![The Pair 桌面应用 — Mentor 与 Executor 两个 AI Agent 实时协作完成编码任务，展示对话、工具调用与 Git 变更](https://github.com/user-attachments/assets/b9d0f06c-c167-45f1-9154-0c49187296ab)

_实时观察 Mentor 和 Executor Agent 的协作过程_

</div>

---

<details>
<summary><b>目录</b></summary>

- [概述](#概述) — The Pair 是什么，以及双 Agent 为何能减少 AI 幻觉
- [功能特性](#功能特性)
- [截图](#截图)
- [安装](#安装) — macOS、Windows、Linux
- [快速开始](#快速开始) — 安装 Provider CLI 并创建你的第一个 Pair
- [配置](#配置)
- [架构](#架构)
- [开发](#开发)
- [常见问题](#常见问题)

</details>

## 概述

**The Pair 是一款免费、开源的桌面应用，它运行两个 AI 编码 Agent —— 只读的 _Mentor_ 负责规划与审查，_Executor_ 负责编写代码与执行命令 —— 两者互相交叉校验，在 AI 幻觉进入你的代码库之前就将其拦截。** 应用完全在本地运行，支持 macOS、Windows 和 Linux，并且与模型无关：可任意组合 Claude Code、OpenAI Codex、Gemini CLI、Kimi Code 和 opencode（也支持通过 Ollama 使用本地模型）。

**担心 AI 代码幻觉？** The Pair 通过运行两个互相校验的 AI Agent 来解决这个问题：

- **Mentor Agent** — 负责规划、审查和验证（只读）
- **Executor Agent** — 负责编写代码和执行命令

它们工作时，你可以去喝杯咖啡。回来后即可获得经过交叉验证的代码。

### 核心优势

- **双模型交叉验证** — 两个模型互相检查对方的工作，大幅减少代码幻觉
- **自动化协作** — Agent 无需人工频繁干预即可协同工作
- **实时监控** — 实时查看每个 Agent 的 CPU/内存使用情况和活动状态
- **Git 集成** — 自动跟踪会话期间的所有文件变更
- **人工监督** — 随时介入暂停、调整或重新分配任务
- **会话恢复** — 中断的会话可完整恢复对话历史
- **引导式配置** — 首次使用引导，完成模型配置和目录选择
- **深色/浅色主题** — 自动检测系统主题并支持手动切换

### 使用场景

- 自主编码会话——让 AI Agent 迭代功能，你专注于审查
- 代码重构——自动分析和实施改进
- 缺陷修复——Agent 协作诊断和解决问题
- 学习工具——观察 AI Agent 如何拆解和解决问题
- 中断工作恢复——应用重启或崩溃后恢复会话状态

---

## 功能特性

- **双 Agent 架构** — 规划（Mentor）与执行（Executor）分离
- **全自动模式** — Agent 在工作区权限范围内自主工作
- **实时活动追踪** — 实时显示 Agent 状态（思考中、执行中、等待中）
- **资源监控** — 每秒更新每个 Agent 的 CPU 和内存使用情况
- **Git 变更追踪** — 自动检测修改、新增或删除的文件
- **对话历史** — 完整的 Agent 交互记录
- **本地编排** — 应用和 Agent 协调均在本地运行；模型调用取决于所选 Provider 或本地模型
- **多 Provider 支持** — 兼容 opencode、Claude Code、Codex、Gemini CLI 和 Kimi Code CLI
- **推理控制** — 按 Agent 角色调整推理强度（低/中/高）
- **Token 统计** — 实时显示每次交互的 Token 用量
- **技能系统** — 附加项目特定的技能文件来引导 Agent 行为
- **自动更新** — 应用内检查更新，一键安装

---

## 截图

审查结果 - 未通过（含证据）
<img width="2800" height="2000" alt="审查结果显示未通过证据" src="./docs/assets/intro-1.png" />

审查结果 - 通过（含证据）
<img width="2800" height="2000" alt="审查结果显示通过证据" src="./docs/assets/intro-3.png" />

---

## 安装

从 [GitHub Releases](https://github.com/timwuhaotian/the-pair/releases) 下载最新版本：

| 平台        | 文件                           |
| ----------- | ------------------------------ |
| **macOS**   | `the-pair-{version}.zip`       |
| **Windows** | `the-pair-{version}-setup.exe` |
| **Linux**   | `the-pair-{version}.AppImage`  |

### 从源码构建

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run build:mac  # 或 build:win / build:linux
```

在 macOS 上，`build:mac` 生成本地 DMG，而 `build:mac:release` 生成用于 GitHub Releases 的 ZIP 格式发布包。构建脚本会在调用 Tauri 之前自动安装所需的 Rust 目标平台。如需手动安装，请运行：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

---

## 快速开始

> [!NOTE]
> The Pair 至少需要一个 AI Provider CLI：[opencode](https://opencode.ai)、[Claude Code](https://docs.anthropic.com/en/docs/claude-code)、[Codex](https://github.com/openai/codex)、[Antigravity](https://github.com/google-gemini/antigravity) 或 [Kimi Code](https://github.com/MoonshotAI/kimi-code)。

### 1. 安装 AI Provider

安装一个或多个受支持的 CLI：

- **opencode** — `curl -fsSL https://opencode.ai/install | bash` 或 `npm install -g opencode-ai`
- **Claude Code** — 参见 [Claude Code 配置指南](https://docs.anthropic.com/en/docs/claude-code/getting-started)，或运行 `npm install -g @anthropic-ai/claude-code`
- **Codex** — `npm install -g @openai/codex`
- **Antigravity** — `agy install`（参见 [Antigravity](https://github.com/google-gemini/antigravity) 安装说明）
- **Kimi Code** — `curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash`（参见 [Kimi Code](https://github.com/MoonshotAI/kimi-code) 安装说明）

### 2. 配置 AI 模型（可选）

对于 opencode 支持的模型，在 `~/.config/opencode/opencode.json` 中配置你的 AI Provider：

```json
{
  "provider": {
    "openai": { "options": { "apiKey": "your-api-key" } },
    "anthropic": { "options": { "apiKey": "your-api-key" } }
  }
}
```

> [!TIP]
> Codex、Claude Code 和 Antigravity (`agy`) 会通过已安装的 CLI 自动检测登录状态。你也可以使用 [Ollama](https://ollama.com) 搭配本地模型进行离线开发。

### 3. 启动 The Pair

从"应用程序"文件夹或开始菜单打开。

### 4. 创建你的第一个 Pair

1. 点击 **New Pair** 按钮
2. 配置：名称、目录、任务描述和 AI 模型
3. 观察 Agent 工作——Mentor 规划，Executor 实现，Mentor 审查
4. 通过实时活动追踪和文件变更监控进度

---

## 配置

### Provider 配置

基于 OpenCode 的模型使用你现有的 opencode 配置：

- **macOS/Linux**：`~/.config/opencode/opencode.json`
- **Windows**：`%APPDATA%/opencode/opencode.json`

Codex、Claude Code、Gemini CLI 和 Kimi Code CLI 会从本地 CLI 安装和账户状态自动检测。

### Pair 运行时

每个 Pair 在项目目录下的 `.pair/runtime/<pairId>/` 中维护自己的运行时配置，包括会话文件、运行时权限和对话历史。

> [!NOTE]
> The Pair 不会修改你的全局 opencode 权限。所有权限都与会话绑定。

---

## 架构

### 技术栈

| 层级         | 技术                  |
| ------------ | --------------------- |
| **框架**     | Tauri 2.x             |
| **后端**     | Rust                  |
| **前端**     | React 19 + TypeScript |
| **样式**     | Tailwind CSS v4       |
| **状态管理** | Zustand               |
| **动画**     | Framer Motion         |
| **图标**     | Lucide React          |

### 系统架构

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

### Agent 工作流

```
开始 → 初始化与基线 → 指导阶段 → 执行阶段 → 审查阶段
                                              ↓
                                        完成？──是→ 已完成
                                           │
                                           否
                                           ↓
                                   （返回指导阶段循环）
```

---

## 开发

### 前置条件

- **Node.js** 22.22+
- **npm** 或 **pnpm**
- **Git**
- **Rustup**（用于桌面端构建）

> [!NOTE]
> 发布版本需要在 GitHub Actions 中配置更新签名密钥。

构建前运行环境检查：

```bash
npm run preflight
```

### 安装依赖

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run dev
```

### 项目结构

```
the-pair/
├── src/
│   └── renderer/          # React 前端
│       └── src/
│           ├── App.tsx
│           ├── components/
│           └── store/
├── src-tauri/             # Rust 后端
│   ├── src/
│   │   ├── lib.rs
│   │   ├── pair_manager.rs
│   │   ├── message_broker.rs
│   │   └── ...
│   └── Cargo.toml
├── build/                 # 构建资源
└── package.json
```

### 脚本命令

| 命令                        | 说明                             |
| --------------------------- | -------------------------------- |
| `npm run dev`               | 启动热重载开发服务器             |
| `npm run preflight`         | 检查本地构建前置条件             |
| `npm run preflight:mac`     | 检查 macOS 构建前置条件          |
| `npm run preflight:win`     | 检查 Windows 构建前置条件        |
| `npm run preflight:linux`   | 检查 Linux 构建前置条件          |
| `npm test`                  | 运行 JavaScript 和 Rust 单元测试 |
| `npm run test:js`           | 运行 Node/TypeScript 单元测试    |
| `npm run test:rust`         | 运行 Rust 单元测试               |
| `npm run typecheck`         | 检查 TypeScript 类型             |
| `npm run lint`              | 运行 ESLint                      |
| `npm run format`            | 使用 Prettier 格式化             |
| `npm run e2e:setup`         | 安装 Appium macOS 驱动           |
| `npm run e2e`               | 运行端到端测试（模拟模式）       |
| `npm run dev:mock`          | 以模拟 Agent 模式启动应用        |
| `npm run dev:smoke`         | 以冒烟测试模式启动应用           |
| `npm run clean`             | 清除生成的构建产物               |
| `npm run build:mac`         | 构建本地 macOS DMG               |
| `npm run build:mac:release` | 构建 macOS 发布版 ZIP 包         |
| `npm run build:win`         | 构建 Windows 版本                |
| `npm run build:linux`       | 构建 Linux 版本                  |

---

## 常见问题

**Q: The Pair 是免费和开源的吗？**

A: 是的。The Pair 在 Apache 2.0 许可证下完全开源，并且可免费下载用于 macOS、Windows 和 Linux。你只需为自己的 AI Provider 用量付费——或使用 [Ollama](https://ollama.com) 运行本地模型，零 API 成本。

**Q: The Pair 是 Cursor、GitHub Copilot 或 Aider 的替代品吗？**

A: 是的，但思路不同。Cursor、Copilot 和 Aider 都由单个 Agent 驱动。The Pair 运行两个独立 Agent——Mentor（只读审查者）和 Executor（代码编写者）——互相交叉校验，让错误被第二个模型拦截，而不是直接交付。它是本地优先、开源的替代方案，且与模型无关：可任意组合 Claude Code、Codex、Gemini、Kimi Code 或 opencode。

**Q: The Pair 支持哪些操作系统？**

A: The Pair 是面向 **macOS、Windows 和 Linux** 的原生桌面应用，基于 Tauri 2（Rust 后端 + React 前端）构建。

**Q: The Pair 与单 Agent AI 编码工具有什么区别？**

A: 单 Agent 工具依赖一个模型来编写和自我审查代码，容易遗漏自己的错误。The Pair 使用两个独立的 Agent，由 Mentor 审查 Executor 的工作，在代码提交前捕获错误。

**Q: The Pair 需要联网吗？**

A: The Pair 完全在本地运行。只有 AI 模型 API 调用需要网络（或通过 Ollama 使用本地模型）。

**Q: 支持哪些 AI Provider？**

A: The Pair 开箱支持五种 Provider：**opencode**（任意兼容模型）、**Claude Code CLI**、**OpenAI Codex CLI**、**Gemini CLI** 和 **Kimi Code CLI**。Codex、Claude、Gemini 和 Kimi 会从已安装的 CLI 自动检测。你可以混合使用 Provider——例如 Claude 作为 Mentor，Codex 作为 Executor。

**Q: 可以使用自己的 AI 模型吗？**

A: 可以，The Pair 与模型无关。opencode 支持的模型可与任意兼容的 Provider（OpenAI、Anthropic、Ollama 等）配合使用。对于 Claude、Codex、Gemini 和 Kimi Code，只需安装其 CLI 并登录。

**Q: 能控制 Agent 的"思考深度"吗？**

A: 可以。The Pair 支持**推理强度控制**，适用于提供该功能的模型（Claude、Codex o 系列、Gemini 2.5）。你可以为每个角色（Mentor 和 Executor）独立设置低/中/高推理强度，在创建 Pair 或设置中均可调整。

**Q: 如何追踪 Token 用量和费用？**

A: Token 用量在每个 Agent 轮次中实时追踪。实时输出的 Token 计数会内联显示在 Agent 控制台中，方便你随时监控消耗。

**Q: Agent 陷入死循环怎么办？**

A: The Pair 实现了迭代次数限制。达到配置的迭代次数后，Agent 会暂停并等待人工干预。

**Q: 应用崩溃或中途关闭怎么办？**

A: 会话快照会自动保存。重新启动时，The Pair 会检测到中断的会话并提供恢复选项，完整保留对话历史，使 Agent 可以从断点继续工作。

**Q: The Pair 支持自动更新吗？**

A: 支持。The Pair 在启动时检查新版本，并通过一键式更新流程通知你。无需手动下载。

---

<div align="center">

[⬇ 下载](https://github.com/timwuhaotian/the-pair/releases) &nbsp;•&nbsp; [🌐 官网](https://apps.timwuhaotian.dev/) &nbsp;•&nbsp; [💬 讨论区](https://github.com/timwuhaotian/the-pair/discussions) &nbsp;•&nbsp; [🐛 反馈问题](https://github.com/timwuhaotian/the-pair/issues)

由 [timwuhaotian](https://github.com/timwuhaotian) 用 ❤️ 构建

**[⭐ 点个 Star](https://github.com/timwuhaotian/the-pair)** 如果觉得这个项目对你有帮助！

<sub>The Pair — 开源 AI 结对编程 · 双 Agent AI 代码审查 · 多 Agent 编码助手 · Cursor / Copilot 替代方案 · 支持 Claude Code、Codex、Gemini、Kimi Code、opencode，适配 macOS、Windows、Linux。</sub>

</div>
