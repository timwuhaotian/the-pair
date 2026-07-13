<!-- prettier-ignore -->
<div align="center">

<img src="./resources/logo-the-pair.png" alt="The Pair" width="128" />

# The Pair

**자동화된 AI 페어 프로그래밍 — 두 AI 에이전트가 서로의 코드를 교차 검증합니다. 커피 한 잔 하는 사이에 검토·검증을 마친 코드를 받아보세요. _(맞습니다, The Pair는 The Pair가 직접 만들었습니다.)_**

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![GitHub release](https://img.shields.io/github/v/release/timwuhaotian/the-pair?include_prereleases&logo=github)](https://github.com/timwuhaotian/the-pair/releases)
[![Build Status](https://github.com/timwuhaotian/the-pair/actions/workflows/build.yml/badge.svg)](https://github.com/timwuhaotian/the-pair/actions)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.9-3178c6.svg?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-24c8db.svg?logo=tauri&logoColor=white)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-19-61dafb.svg?logo=react&logoColor=black)](https://react.dev/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Changelog](https://img.shields.io/badge/Changelog-CHANGELOG.md-informational)](CHANGELOG.md)

🌐 [English](README.md) • [简体中文](README.zh.md) • [한국어](README.ko.md) • [日本語](README.ja.md)

**macOS** • **Windows** • **Linux** &nbsp;|&nbsp; [**⬇ 다운로드**](https://github.com/timwuhaotian/the-pair/releases) &nbsp;•&nbsp; [**CLI**](https://github.com/timwuhaotian/pair-code) &nbsp;•&nbsp; [**🌐 웹사이트**](https://apps.timwuhaotian.dev/)

![The Pair 데스크톱 앱 — Mentor와 Executor 두 AI 에이전트가 코딩 작업을 실시간으로 협업하며 대화, 도구 호출, Git 변경 사항을 표시](https://github.com/user-attachments/assets/b9d0f06c-c167-45f1-9154-0c49187296ab)

_Mentor와 Executor 에이전트의 실시간 협업 과정을 지켜보세요_

</div>

---

<details>
<summary><b>목차</b></summary>

- [개요](#개요) — The Pair란 무엇이며 두 에이전트가 AI 환각을 줄이는 이유
- [기능](#기능)
- [스크린샷](#스크린샷)
- [설치](#설치) — macOS, Windows, Linux
- [빠른 시작](#빠른-시작) — 프로바이더 CLI를 설치하고 첫 Pair 만들기
- [구성](#구성)
- [아키텍처](#아키텍처)
- [개발](#개발)
- [FAQ](#faq)

</details>

## 개요

**The Pair는 두 개의 AI 코딩 에이전트 — 계획과 검토를 담당하는 읽기 전용 _Mentor_ 와 코드를 작성하고 명령을 실행하는 _Executor_ — 를 실행하는 무료 오픈소스 데스크톱 앱으로, 두 에이전트가 서로의 작업을 교차 검증하여 AI 환각이 코드베이스에 도달하기 전에 잡아냅니다.** 로컬에서 실행되며 macOS, Windows, Linux를 지원하고 모델에 구애받지 않습니다: Claude Code, OpenAI Codex, Gemini CLI, opencode를 자유롭게 조합할 수 있습니다(Ollama를 통한 로컬 모델도 가능).

**AI 코드 환각이 걱정되시나요?** The Pair는 서로를 교차 검증하는 두 AI 에이전트를 실행하여 이 문제를 해결합니다:

- **Mentor 에이전트** — 계획, 검토 및 검증 (읽기 전용)
- **Executor 에이전트** — 코드 작성 및 명령 실행

에이전트가 작업하는 동안 커피 한 잔 하세요. 돌아오면 교차 검증된 코드가 준비되어 있습니다.

### 핵심 장점

- **듀얼 모델 교차 검증** — 두 모델이 서로의 작업을 확인하여 코드 환각을 대폭 감소
- **자동화된 협업** — 에이전트가 빈번한 인간 개입 없이 협업
- **실시간 모니터링** — 각 에이전트의 CPU/메모리 사용량과 활동 상태 실시간 추적
- **Git 통합** — 세션 중 모든 파일 변경 사항 자동 추적
- **인간 감독** — 언제든지 개입하여 일시정지, 조정 또는 작업 재할당 가능
- **세션 복구** — 중단된 세션을 전체 대화 기록과 함께 복원
- **온보딩 마법사** — 모델 구성 및 디렉토리 선택을 위한 첫 실행 가이드
- **다크/라이트 테마** — 자동 시스템 테마 감지 및 수동 전환 지원

### 사용 사례

- 자율 코딩 세션 — AI 에이전트가 기능을 반복하는 동안 당신은 검토에 집중
- 코드 리팩토링 — 개선 사항 자동 분석 및 구현
- 버그 수정 — 에이전트가 협업하여 문제 진단 및 해결
- 학습 도구 — AI 에이전트가 문제를 분해하고 해결하는 과정 관찰
- 중단 작업 복구 — 앱 재시작 또는 충돌 후 세션 상태 복원

---

## 기능

- **듀얼 에이전트 아키텍처** — 계획(Mentor)과 실행(Executor) 분리
- **전체 자동화 모드** — 에이전트가 작업 공간 범위 권한 내에서 자율 작업
- **실시간 활동 추적** — 에이전트 활동 상태 실시간 표시 (사고 중, 실행 중, 대기 중)
- **리소스 모니터링** — 초당 각 에이전트의 CPU 및 메모리 사용량
- **Git 변경 추적** — 수정, 추가 또는 삭제된 파일 자동 감지
- **대화 기록** — 모든 에이전트 상호작용의 전체 기록
- **로컬 오케스트레이션** — 앱과 에이전트 조정은 모두 로컬에서 실행; 모델 호출은 선택한 프로바이더 또는 로컬 모델에 따름
- **멀티 프로바이더** — opencode, Claude Code, Codex, Gemini CLI 호환
- **추론 제어** — 에이전트 역할별 사고 노력 조정 (낮음/중간/높음)
- **토큰 추적** — 각 턴당 실시간 토큰 사용량 인라인 표시
- **스킬 시스템** — 에이전트 행동을 가이드하는 프로젝트별 스킬 파일 첨부
- **자동 업데이트** — 앱 내 업데이트 확인 및 원클릭 설치

---

## 스크린샷

검토 결과 - 실패 (증거 포함)
<img width="2800" height="2000" alt="실패 증거를 보여주는 검토 결과" src="./docs/assets/intro-1.png" />

검토 결과 - 통과 (증거 포함)
<img width="2800" height="2000" alt="통과 증거를 보여주는 검토 결과" src="./docs/assets/intro-3.png" />

---

## 설치

[GitHub Releases](https://github.com/timwuhaotian/the-pair/releases)에서 최신 릴리스를 다운로드하세요:

| 플랫폼      | 파일                           |
| ----------- | ------------------------------ |
| **macOS**   | `the-pair-{version}.zip`       |
| **Windows** | `the-pair-{version}-setup.exe` |
| **Linux**   | `the-pair-{version}.AppImage`  |

### 소스에서 빌드

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run build:mac  # 또는 build:win / build:linux
```

macOS에서 `build:mac`은 로컬 DMG를 생성하고, `build:mac:release`는 GitHub Releases에 사용되는 ZIP 스타일 릴리스 번들을 생성합니다. 빌드 스크립트는 Tauri를 호출하기 전에 필요한 Rust 타겟이 설치되도록 합니다. 수동으로 설정하려면 다음을 실행하세요:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

---

## 빠른 시작

> [!NOTE]
> The Pair에는 최소 하나의 AI 프로바이더 CLI가 필요합니다: [opencode](https://opencode.ai), [Claude Code](https://docs.anthropic.com/en/docs/claude-code), [Codex](https://github.com/openai/codex) 또는 [Antigravity](https://github.com/google-gemini/antigravity).

### 1. AI 프로바이더 설치

지원되는 CLI 중 하나 이상을 설치하세요:

- **opencode** — `curl -fsSL https://opencode.ai/install | bash` 또는 `npm install -g opencode-ai`
- **Claude Code** — [Claude Code 설정](https://docs.anthropic.com/en/docs/claude-code/getting-started) 참조 또는 `npm install -g @anthropic-ai/claude-code`
- **Codex** — `npm install -g @openai/codex`
- **Antigravity** — `agy install` ([Antigravity](https://github.com/google-gemini/antigravity) 설치 참조)

### 2. AI 모델 구성 (선택 사항)

opencode 기반 모델의 경우, `~/.config/opencode/opencode.json`에서 AI 프로바이더를 설정하세요:

```json
{
  "provider": {
    "openai": { "options": { "apiKey": "your-api-key" } },
    "anthropic": { "options": { "apiKey": "your-api-key" } }
  }
}
```

> [!TIP]
> Codex, Claude Code, Antigravity(`agy`)는 설치된 CLI와 로그인 상태에서 자동 감지됩니다. [Ollama](https://ollama.com)와 로컬 모델을 사용하여 오프라인 개발도 가능합니다.

### 3. The Pair 실행

응용 프로그램 폴더 또는 시작 메뉴에서 실행하세요.

### 4. 첫 Pair 만들기

1. **New Pair** 버튼 클릭
2. 구성: 이름, 디렉토리, 작업 설명 및 AI 모델
3. 에이전트 작업 관찰 — Mentor 계획, Executor 구현, Mentor 검토
4. 실시간 활동 추적 및 파일 변경으로 진행 상황 모니터링

---

## 구성

### 프로바이더 구성

OpenCode 기반 모델은 기존 opencode 구성을 사용합니다:

- **macOS/Linux**: `~/.config/opencode/opencode.json`
- **Windows**: `%APPDATA%/opencode/opencode.json`

Codex, Claude Code, Gemini CLI는 로컬 CLI 설치 및 계정 상태에서 자동 감지됩니다.

### Pair 런타임

각 Pair는 프로젝트 디렉토리 내 `.pair/runtime/<pairId>/`에서 자체 런타임 구성을 유지하며, 세션 파일, 런타임 권한 및 대화 기록을 포함합니다.

> [!NOTE]
> The Pair는 전역 opencode 권한을 수정하지 않습니다. 모든 권한은 세션별입니다.

---

## 아키텍처

### 기술 스택

| 계층           | 기술                  |
| -------------- | --------------------- |
| **프레임워크** | Tauri 2.x             |
| **백엔드**     | Rust                  |
| **프론트엔드** | React 19 + TypeScript |
| **스타일링**   | Tailwind CSS v4       |
| **상태 관리**  | Zustand               |
| **애니메이션** | Framer Motion         |
| **아이콘**     | Lucide React          |

### 시스템 아키텍처

```
┌─────────────────────────────────────────────────────────┐
│                    The Pair App                         │
├─────────────────────────────────────────────────────────┤
│  프론트엔드 (React UI)                                  │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │  대시보드     │ Pair 상세     │    설정          │    │
│  └──────────────┴──────────────┴──────────────────┘    │
│                          ↕ Tauri IPC                    │
├─────────────────────────────────────────────────────────┤
│  백엔드 (Rust)                                          │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │ PairManager  │MessageBroker │ ProcessSpawner   │    │
│  │ (라이프사이클) │(상태 기계)    │(멀티 프로바이더)  │    │
│  └──────────────┴──────────────┴──────────────────┘    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │ Git Tracker  │ Worktrees    │ 세션 스냅샷       │    │
│  └──────────────┴──────────────┴──────────────────┘    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │리소스 모니.  │ Acceptance   │ 보고서 생성기     │    │
│  └──────────────┴──────────────┴──────────────────┘    │
└─────────────────────────────────────────────────────────┘
                            ↕
              ┌─────────────┴─────────────┐
              ↙                           ↘
     ┌─────────────────┐          ┌─────────────────┐
     │  AI 프로바이더 CLI│          │   Git 저장소     │
     │ opencode/Claude/ │          │  (작업 공간)     │
     │ Codex/Gemini     │          └─────────────────┘
     └─────────────────┘
```

### 에이전트 워크플로우

```
시작 → 초기화 및 기준선 → 멘토링 단계 → 실행 단계 → 검토 단계
                                                   ↓
                                             완료? ──예→ 완료
                                                │
                                                아니오
                                                ↓
                                        (멘토링 단계로 루프백)
```

---

## 개발

### 사전 요구사항

- **Node.js** 22.22+
- **npm** 또는 **pnpm**
- **Git**
- **Rustup** (데스크톱 빌드용)

> [!NOTE]
> 릴리스 빌드는 GitHub Actions에서 업데이트 서명 비밀 키 구성이 필요합니다.

빌드 전 환경 확인을 실행하세요:

```bash
npm run preflight
```

### 설정

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run dev
```

### 프로젝트 구조

```
the-pair/
├── src/
│   └── renderer/          # React 프론트엔드
│       └── src/
│           ├── App.tsx
│           ├── components/
│           └── store/
├── src-tauri/             # Rust 백엔드
│   ├── src/
│   │   ├── lib.rs
│   │   ├── pair_manager.rs
│   │   ├── message_broker.rs
│   │   └── ...
│   └── Cargo.toml
├── build/                 # 빌드 리소스
└── package.json
```

### 스크립트

| 명령                        | 설명                            |
| --------------------------- | ------------------------------- |
| `npm run dev`               | 핫 리로드 개발 서버 시작        |
| `npm run preflight`         | 로컬 빌드 사전 요구사항 확인    |
| `npm run preflight:mac`     | macOS 빌드 사전 요구사항 확인   |
| `npm run preflight:win`     | Windows 빌드 사전 요구사항 확인 |
| `npm run preflight:linux`   | Linux 빌드 사전 요구사항 확인   |
| `npm test`                  | JavaScript 및 Rust 단위 테스트  |
| `npm run test:js`           | Node/TypeScript 단위 테스트     |
| `npm run test:rust`         | Rust 단위 테스트                |
| `npm run typecheck`         | TypeScript 타입 확인            |
| `npm run lint`              | ESLint 실행                     |
| `npm run format`            | Prettier로 포맷팅               |
| `npm run e2e:setup`         | Appium macOS 드라이버 설치      |
| `npm run e2e`               | 모의 E2E 테스트 실행            |
| `npm run dev:mock`          | 모의 에이전트로 앱 시작         |
| `npm run dev:smoke`         | 스모크 테스트 모드로 앱 시작    |
| `npm run clean`             | 생성된 빌드 아티팩트 제거       |
| `npm run build:mac`         | 로컬 macOS DMG 빌드             |
| `npm run build:mac:release` | macOS 릴리스 ZIP 번들 빌드      |
| `npm run build:win`         | Windows 빌드                    |
| `npm run build:linux`       | Linux 빌드                      |

---

## FAQ

**Q: The Pair는 무료이며 오픈소스인가요?**

A: 네. The Pair는 Apache 2.0 라이선스로 완전한 오픈소스이며 macOS, Windows, Linux용으로 무료로 다운로드할 수 있습니다. 비용은 본인의 AI 프로바이더 사용분만 발생하며——[Ollama](https://ollama.com)로 로컬 모델을 실행하면 API 비용이 전혀 들지 않습니다.

**Q: The Pair는 Cursor, GitHub Copilot 또는 Aider의 대안인가요?**

A: 네, 다만 접근 방식이 다릅니다. Cursor, Copilot, Aider는 단일 에이전트로 작동합니다. The Pair는 두 개의 독립 에이전트——Mentor(읽기 전용 리뷰어)와 Executor(코드 작성자)——를 실행하여 서로 교차 검증하므로, 실수가 출시되기 전에 두 번째 모델이 잡아냅니다. 로컬 우선의 오픈소스 대안이며 모델에 구애받지 않습니다: Claude Code, Codex, Gemini, opencode를 자유롭게 조합할 수 있습니다.

**Q: The Pair는 어떤 운영 체제를 지원하나요?**

A: The Pair는 **macOS, Windows, Linux**용 네이티브 데스크톱 앱으로, Tauri 2(Rust 백엔드 + React 프론트엔드)로 구축되었습니다.

**Q: The Pair는 단일 에이전트 AI 코딩 도구와 어떻게 다른가요?**

A: 단일 에이전트 도구는 하나의 모델이 코드를 작성하고 자체 검토하는 데 의존하므로 자신의 실수를 놓칠 수 있습니다. The Pair는 두 개의 별도 에이전트를 사용하여 Mentor가 Executor의 작업을 검토하여 코드가 적용되기 전에 오류를 잡아냅니다.

**Q: The Pair에 인터넷 연결이 필요한가요?**

A: The Pair는 완전히 로컬에서 실행됩니다. AI 모델 API 호출만 인터넷이 필요합니다 (또는 Ollama를 통한 로컬 모델 설정).

**Q: 어떤 AI 프로바이더를 지원하나요?**

A: The Pair는 기본적으로 네 가지 프로바이더를 지원합니다: **opencode** (호환 가능한 모든 모델), **Claude Code CLI**, **OpenAI Codex CLI**, **Gemini CLI**. Codex, Claude, Gemini는 설치된 CLI에서 자동 감지됩니다. 프로바이더를 혼합할 수 있습니다 — 예를 들어 Claude를 Mentor로, Codex를 Executor로.

**Q: 자체 AI 모델을 사용할 수 있나요?**

A: 네, The Pair는 모델에 구애받지 않습니다. opencode 기반 모델은 호환 가능한 모든 프로바이더(OpenAI, Anthropic, Ollama 등)와 작동합니다. Claude, Codex, Gemini의 경우 CLI를 설치하고 로그인하기만 하면 됩니다.

**Q: 에이전트의 "사고 깊이"를 제어할 수 있나요?**

A: 네. The Pair는 이를 제공하는 모델에 대해 **추론 노력 제어**를 지원합니다 (Claude, Codex o 시리즈, Gemini 2.5). 각 역할별로 낮음/중간/높음 노력을 설정할 수 있으며, Mentor와 Executor는 독립적으로 설정 가능하며 Pair 생성 또는 설정에서 조정할 수 있습니다.

**Q: 토큰 사용량과 비용을 어떻게 추적하나요?**

A: 토큰 사용량은 각 에이전트 턴별로 실시간 추적됩니다. 실시간 출력 토큰 수는 에이전트 콘솔에 인라인으로 표시되어 에이전트 작업 중 소비를 모니터링할 수 있습니다.

**Q: 에이전트가 무한 루프에 빠지면 어떻게 하나요?**

A: The Pair는 반복 횟수 제한을 구현합니다. 구성된 반복 횟수 후에 에이전트는 인간 개입을 위해 일시정지됩니다.

**Q: 앱이 충돌하거나 세션 중에 닫으면 어떻게 하나요?**

A: 세션 스냅샷이 자동으로 저장됩니다. 재실행 시 The Pair는 중단된 세션을 감지하고 전체 대화 기록과 함께 복원 옵션을 제공하여 에이전트가 중단된 지점부터 계속할 수 있습니다.

**Q: The Pair는 자동 업데이트를 지원하나요?**

A: 네. The Pair는 실행 시 새 버전을 확인하고 원클릭 업데이트 플로우로 알림을 보냅니다. 수동 다운로드가 필요하지 않습니다.

---

<div align="center">

[⬇ 다운로드](https://github.com/timwuhaotian/the-pair/releases) &nbsp;•&nbsp; [🌐 웹사이트](https://apps.timwuhaotian.dev/) &nbsp;•&nbsp; [💬 토론](https://github.com/timwuhaotian/the-pair/discussions) &nbsp;•&nbsp; [🐛 버그 신고](https://github.com/timwuhaotian/the-pair/issues)

[timwuhaotian](https://github.com/timwuhaotian)이 ❤️로 제작

**[⭐ Star 누르기](https://github.com/timwuhaotian/the-pair)** — 도움이 되셨다면 응원해 주세요!

<sub>The Pair — 오픈소스 AI 페어 프로그래밍 · 듀얼 에이전트 AI 코드 리뷰 · 멀티 에이전트 코딩 어시스턴트 · Cursor / Copilot 대안 · Claude Code, Codex, Gemini, opencode 지원, macOS, Windows, Linux에서 작동.</sub>

</div>
