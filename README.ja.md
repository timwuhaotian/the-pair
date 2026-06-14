<!-- prettier-ignore -->
<div align="center">

<img src="./resources/logo-the-pair.png" alt="The Pair" width="128" />

# The Pair

**自動化AIペアプログラミング — 2つのAIエージェントが互いのコードをクロスチェック。コーヒーを飲んでいる間に、レビュー済み・検証済みのコードが手に入ります。_（そう、The PairはThe Pair自身によって作られました。）_**

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://www.apache.org/licenses/LICENSE-2.0)
[![GitHub release](https://img.shields.io/github/v/release/timwuhaotian/the-pair?include_prereleases&logo=github)](https://github.com/timwuhaotian/the-pair/releases)
[![Build Status](https://github.com/timwuhaotian/the-pair/actions/workflows/build.yml/badge.svg)](https://github.com/timwuhaotian/the-pair/actions)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.9-3178c6.svg?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-2.0-24c8db.svg?logo=tauri&logoColor=white)](https://tauri.app/)
[![React](https://img.shields.io/badge/React-19-61dafb.svg?logo=react&logoColor=black)](https://react.dev/)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)
[![Changelog](https://img.shields.io/badge/Changelog-CHANGELOG.md-informational)](CHANGELOG.md)

🌐 [English](README.md) • [简体中文](README.zh.md) • [한국어](README.ko.md) • [日本語](README.ja.md)

**macOS** • **Windows** • **Linux** &nbsp;|&nbsp; [**⬇ ダウンロード**](https://github.com/timwuhaotian/the-pair/releases) &nbsp;•&nbsp; [**🌐 ウェブサイト**](https://apps.timwuhaotian.dev/)

![The Pair デスクトップアプリ — MentorとExecutorの2つのAIエージェントがコーディングタスクをリアルタイムで協働し、会話・ツール呼び出し・Gitの変更を表示](https://github.com/user-attachments/assets/b9d0f06c-c167-45f1-9154-0c49187296ab)

_MentorとExecutorエージェントのリアルタイムコラボレーションを観察_

</div>

---

<details>
<summary><b>目次</b></summary>

- [概要](#概要) — The Pairとは何か、なぜ2つのエージェントがAIの幻覚を減らすのか
- [機能](#機能)
- [スクリーンショット](#スクリーンショット)
- [インストール](#インストール) — macOS、Windows、Linux
- [クイックスタート](#クイックスタート) — プロバイダーCLIをインストールして最初のPairを作成
- [設定](#設定)
- [アーキテクチャ](#アーキテクチャ)
- [開発](#開発)
- [FAQ](#faq)

</details>

## 概要

**The Pairは、2つのAIコーディングエージェント — 計画とレビューを担う読み取り専用の _Mentor_ と、コードを書きコマンドを実行する _Executor_ — を実行する無料・オープンソースのデスクトップアプリです。両エージェントが互いの作業をクロスチェックし、AIの幻覚がコードベースに入り込む前に検出します。** ローカルで動作し、macOS・Windows・Linuxに対応。モデル非依存で、Claude Code、OpenAI Codex、Gemini CLI、opencodeを自由に組み合わせられます（Ollama経由のローカルモデルも利用可能）。

**AIコードの幻覚が心配ですか？** The Pairは、互いにクロスチェックする2つのAIエージェントを実行することでこの問題を解決します：

- **Mentorエージェント** — 計画、レビュー、検証（読み取り専用）
- **Executorエージェント** — コードの記述とコマンドの実行

エージェントが作業している間にコーヒーを飲みに行きましょう。戻ってきたら、クロス検証されたコードが準備できています。

### 主な利点

- **デュアルモデルクロス検証** — 2つのモデルが互いの作業をチェックし、コードの幻覚を大幅に削減
- **自動化されたコラボレーション** — エージェントが頻繁な人間の介入なしに連携
- **リアルタイムモニタリング** — エージェントごとのCPU/メモリ使用量とアクティビティをライブ追跡
- **Git統合** — セッション中のすべてのファイル変更を自動追跡
- **人間の監督** — いつでも介入して一時停止、調整、タスクの再割り当てが可能
- **セッション復元** — 中断されたセッションを完全な会話履歴付きで再開
- **オンボーディングウィザード** — モデル設定とディレクトリ選択の初回ガイド
- **ダーク/ライトテーマ** — 自動システムテーマ検出と手動切り替え

### ユースケース

- 自律コーディングセッション — AIエージェントが機能を反復している間、あなたはレビューに集中
- コードリファクタリング — 改善の自動分析と実装
- バグ修正 — エージェントが協力して問題を診断・解決
- 学習ツール — AIエージェントが問題を分解・解決する様子を観察
- 中断した作業の復元 — アプリの再起動またはクラッシュ後にセッション状態を復元

---

## 機能

- **デュアルエージェントアーキテクチャ** — 計画（Mentor）と実行（Executor）の分離
- **フルオートメーションモード** — エージェントがワークスペーススコープの権限内で自律的に作業
- **リアルタイムアクティビティ追跡** — エージェントのステータスをライブ表示（考え中、実行中、待機中）
- **リソースモニタリング** — エージェントごとのCPUとメモリ使用量を毎秒更新
- **Git変更追跡** — 変更、追加、削除されたファイルを自動検出
- **会話履歴** — すべてのエージェントインタラクションの完全なトランスクリプト
- **ローカルオーケストレーション** — アプリとエージェントの調整はすべてローカルで実行。モデル呼び出しは選択したプロバイダーまたはローカルモデルに依存
- **マルチプロバイダー** — opencode、Claude Code、Codex、Gemini CLIに対応
- **推論コントロール** — エージェントロールごとに思考の努力度を調整（低/中/高）
- **トークントラッキング** — ターンごとのリアルタイムトークン使用量をインライン表示
- **スキルシステム** — エージェントの行動をガイドするプロジェクト固有のスキルファイルを添付
- **自動更新** — アプリ内で更新をチェック、ワンクリックでインストール

---

## スクリーンショット

レビュー結果 - 失敗（証拠付き）
<img width="2800" height="2000" alt="失敗の証拠を示すレビュー結果" src="./docs/assets/intro-1.png" />

レビュー結果 - 合格（証拠付き）
<img width="2800" height="2000" alt="合格の証拠を示すレビュー結果" src="./docs/assets/intro-3.png" />

---

## インストール

[GitHub Releases](https://github.com/timwuhaotian/the-pair/releases)から最新リリースをダウンロード：

| プラットフォーム | ファイル                       |
| ---------------- | ------------------------------ |
| **macOS**        | `the-pair-{version}.zip`       |
| **Windows**      | `the-pair-{version}-setup.exe` |
| **Linux**        | `the-pair-{version}.AppImage`  |

### ソースからビルド

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run build:mac  # または build:win / build:linux
```

macOSでは、`build:mac`はローカルDMGを生成し、`build:mac:release`はGitHub Releasesで使用されるZIP形式のリリースバンドルを生成します。ビルドスクリプトはTauriを呼び出す前に必要なRustターゲットがインストールされるようにします。手動で設定する場合は以下を実行してください：

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

---

## クイックスタート

> [!NOTE]
> The PairにはAIプロバイダーCLIが少なくとも1つ必要です：[opencode](https://opencode.ai)、[Claude Code](https://docs.anthropic.com/en/docs/claude-code)、[Codex](https://github.com/openai/codex)、または [Gemini CLI](https://github.com/google-gemini/gemini-cli)。

### 1. AIプロバイダーをインストール

サポートされているCLIを1つ以上インストール：

- **opencode** — `curl -fsSL https://opencode.ai/install | bash` または `npm install -g opencode-ai`
- **Claude Code** — [Claude Codeセットアップ](https://docs.anthropic.com/en/docs/claude-code/getting-started)を参照、または `npm install -g @anthropic-ai/claude-code`
- **Codex** — `npm install -g @openai/codex`
- **Gemini CLI** — `npm install -g @google/gemini-cli` または `npx @google/gemini-cli`

### 2. AIモデルを設定（オプション）

opencodeベースのモデルの場合、`~/.config/opencode/opencode.json`でAIプロバイダーを設定：

```json
{
  "provider": {
    "openai": { "options": { "apiKey": "your-api-key" } },
    "anthropic": { "options": { "apiKey": "your-api-key" } }
  }
}
```

> [!TIP]
> Codex、Claude Code、Gemini CLIはインストール済みCLIからログイン状態を自動検出します。[Ollama](https://ollama.com)でローカルモデルを使用してオフライン開発も可能です。

### 3. The Pairを起動

アプリケーションフォルダまたはスタートメニューから開きます。

### 4. 最初のPairを作成

1. **New Pair** ボタンをクリック
2. 設定：名前、ディレクトリ、タスク説明、AIモデル
3. エージェントの作業を観察 — Mentorが計画、Executorが実装、Mentorがレビュー
4. リアルタイムアクティビティ追跡とファイル変更で進行状況をモニタリング

---

## 設定

### プロバイダー設定

OpenCodeベースのモデルは既存のopencode設定を使用：

- **macOS/Linux**: `~/.config/opencode/opencode.json`
- **Windows**: `%APPDATA%/opencode/opencode.json`

Codex、Claude Code、Gemini CLIはローカルCLIインストールとアカウント状態から自動検出されます。

### Pairランタイム

各Pairはプロジェクトディレクトリ内の `.pair/runtime/<pairId>/` で独自のランタイム設定を維持し、セッションファイル、ランタイム権限、会話履歴を含みます。

> [!NOTE]
> The Pairはグローバルなopencode権限を変更しません。すべての権限はセッション固有です。

---

## アーキテクチャ

### 技術スタック

| 層                 | テクノロジー          |
| ------------------ | --------------------- |
| **フレームワーク** | Tauri 2.x             |
| **バックエンド**   | Rust                  |
| **フロントエンド** | React 19 + TypeScript |
| **スタイリング**   | Tailwind CSS v4       |
| **状態管理**       | Zustand               |
| **アニメーション** | Framer Motion         |
| **アイコン**       | Lucide React          |

### システムアーキテクチャ

```
┌─────────────────────────────────────────────────────────┐
│                    The Pair App                         │
├─────────────────────────────────────────────────────────┤
│  フロントエンド (React UI)                              │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │  ダッシュボード│ Pair詳細     │    設定          │    │
│  └──────────────┴──────────────┴──────────────────┘    │
│                          ↕ Tauri IPC                    │
├─────────────────────────────────────────────────────────┤
│  バックエンド (Rust)                                    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │ PairManager  │MessageBroker │ ProcessSpawner   │    │
│  │ (ライフサイクル)│(ステートマシン)│(マルチプロバイダー)│    │
│  └──────────────┴──────────────┴──────────────────┘    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │ Git Tracker  │ Worktrees    │ セッションスナップショット│
│  └──────────────┴──────────────┴──────────────────┘    │
│  ┌──────────────┬──────────────┬──────────────────┐    │
│  │リソースモニ. │ Acceptance   │ レポートジェネレーター│
│  └──────────────┴──────────────┴──────────────────┘    │
└─────────────────────────────────────────────────────────┘
                            ↕
              ┌─────────────┴─────────────┐
              ↙                           ↘
     ┌─────────────────┐          ┌─────────────────┐
     │  AIプロバイダーCLI│          │   Gitリポジトリ  │
     │ opencode/Claude/ │          │  (ワークスペース)│
     │ Codex/Gemini     │          └─────────────────┘
     └─────────────────┘
```

### エージェントワークフロー

```
開始 → 初期化とベースライン → メンタリングフェーズ → 実行フェーズ → レビューフェーズ
                                                         ↓
                                               完了？ ──はい→ 完了
                                                  │
                                                  いいえ
                                                  ↓
                                          （メンタリングフェーズへループバック）
```

---

## 開発

### 前提条件

- **Node.js** 22.22+
- **npm** または **pnpm**
- **Git**
- **Rustup**（デスクトップビルド用）

> [!NOTE]
> リリースビルドにはGitHub Actionsでアップデータ署名シークレットの設定が必要です。

ビルド前に環境チェックを実行：

```bash
npm run preflight
```

### セットアップ

```bash
git clone https://github.com/timwuhaotian/the-pair.git
cd the-pair
npm install
npm run dev
```

### プロジェクト構造

```
the-pair/
├── src/
│   └── renderer/          # React フロントエンド
│       └── src/
│           ├── App.tsx
│           ├── components/
│           └── store/
├── src-tauri/             # Rust バックエンド
│   ├── src/
│   │   ├── lib.rs
│   │   ├── pair_manager.rs
│   │   ├── message_broker.rs
│   │   └── ...
│   └── Cargo.toml
├── build/                 # ビルドリソース
└── package.json
```

### スクリプト

| コマンド                    | 説明                                   |
| --------------------------- | -------------------------------------- |
| `npm run dev`               | ホットリロード開発サーバーを起動       |
| `npm run preflight`         | ローカルビルドの前提条件をチェック     |
| `npm run preflight:mac`     | macOSビルドの前提条件をチェック        |
| `npm run preflight:win`     | Windowsビルドの前提条件をチェック      |
| `npm run preflight:linux`   | Linuxビルドの前提条件をチェック        |
| `npm test`                  | JavaScriptおよびRustユニットテスト     |
| `npm run test:js`           | Node/TypeScriptユニットテスト          |
| `npm run test:rust`         | Rustユニットテスト                     |
| `npm run typecheck`         | TypeScript型チェック                   |
| `npm run lint`              | ESLintを実行                           |
| `npm run format`            | Prettierでフォーマット                 |
| `npm run e2e:setup`         | Appium macOSドライバーをインストール   |
| `npm run e2e`               | モックE2Eテストを実行                  |
| `npm run dev:mock`          | モックエージェントでアプリを起動       |
| `npm run dev:smoke`         | スモークテストモードでアプリを起動     |
| `npm run clean`             | 生成されたビルドアーティファクトを削除 |
| `npm run build:mac`         | ローカルmacOS DMGをビルド              |
| `npm run build:mac:release` | macOSリリースZIPバンドルをビルド       |
| `npm run build:win`         | Windows用ビルド                        |
| `npm run build:linux`       | Linux用ビルド                          |

---

## FAQ

**Q: The Pairは無料でオープンソースですか？**

A: はい。The PairはApache 2.0ライセンスの下で完全にオープンソースであり、macOS・Windows・Linux向けに無料でダウンロードできます。費用はご自身のAIプロバイダー利用分のみ——または[Ollama](https://ollama.com)でローカルモデルを実行すればAPIコストはゼロです。

**Q: The PairはCursor、GitHub Copilot、Aiderの代替になりますか？**

A: はい、ただしアプローチが異なります。Cursor、Copilot、Aiderは単一エージェントで動作します。The Pairは2つの独立したエージェント——Mentor（読み取り専用レビュアー）とExecutor（コード作成者）——を実行し、互いにクロスチェックすることで、ミスが出荷される前に2つ目のモデルが捕捉します。ローカルファーストでオープンソースの代替であり、モデル非依存です：Claude Code、Codex、Gemini、opencodeを自由に組み合わせられます。

**Q: The Pairはどのオペレーティングシステムに対応していますか？**

A: The Pairは **macOS・Windows・Linux** 向けのネイティブデスクトップアプリで、Tauri 2（Rustバックエンド + Reactフロントエンド）で構築されています。

**Q: The PairはシングルエージェントAIコーディングツールとどう違うのですか？**

A: シングルエージェントツールは1つのモデルがコードを書いて自己レビューするため、自分のミスを見逃すことがあります。The Pairは2つの独立したエージェントを使用し、MentorがExecutorの作業をレビューすることで、コードが適用される前にエラーを検出します。

**Q: The Pairにはインターネット接続が必要ですか？**

A: The Pairは完全にローカルで実行されます。AIモデルAPI呼び出しのみインターネットが必要です（またはOllamaでローカルモデルを設定）。

**Q: どのAIプロバイダーがサポートされていますか？**

A: The Pairは4つのプロバイダーをそのままサポートしています：**opencode**（互換性のある任意のモデル）、**Claude Code CLI**、**OpenAI Codex CLI**、**Gemini CLI**。Codex、Claude、Geminiはインストール済みCLIから自動検出されます。プロバイダーをミックスできます — 例えばMentorにClaude、ExecutorにCodex。

**Q: 自分のAIモデルを使えますか？**

A: はい、The Pairはモデルに依存しません。opencodeベースのモデルは互換性のある任意のプロバイダー（OpenAI、Anthropic、Ollamaなど）で動作します。Claude、Codex、Geminiの場合はCLIをインストールしてサインインするだけです。

**Q: エージェントの「思考の深さ」を制御できますか？**

A: はい。The Pairはこれを提供するモデルに対して**推論努力制御**をサポートしています（Claude、Codex oシリーズ、Gemini 2.5）。各ロールごとに低/中/高を設定でき、MentorとExecutorは独立して設定可能で、Pair作成時または設定から調整できます。

**Q: トークン使用量とコストを追跡するには？**

A: トークン使用量はエージェントの各ターンでリアルタイムに追跡されます。リアルタイム出力トークン数はエージェントコンソールにインライン表示され、エージェントの作業中に消費を監視できます。

**Q: エージェントがループに陥ったら？**

A: The Pairは反復回数制限を実装しています。設定された反復回数に達すると、エージェントは人間の介入のために一時停止します。

**Q: アプリがクラッシュしたりセッション中に閉じたら？**

A: セッションスナップショットは自動保存されます。再起動時、The Pairは中断されたセッションを検出し、完全な会話履歴付きで復元オプションを提供し、エージェントが中断した場所から再開できるようにします。

**Q: The Pairは自動更新されますか？**

A: はい。The Pairは起動時に新しいバージョンをチェックし、ワンクリック更新フローで通知します。手動ダウンロードは不要です。

---

<div align="center">

[⬇ ダウンロード](https://github.com/timwuhaotian/the-pair/releases) &nbsp;•&nbsp; [🌐 ウェブサイト](https://apps.timwuhaotian.dev/) &nbsp;•&nbsp; [💬 ディスカッション](https://github.com/timwuhaotian/the-pair/discussions) &nbsp;•&nbsp; [🐛 バグ報告](https://github.com/timwuhaotian/the-pair/issues)

[timwuhaotian](https://github.com/timwuhaotian) が ❤️ で構築

**[⭐ スターをつける](https://github.com/timwuhaotian/the-pair)** — 役に立ったら応援してください！

<sub>The Pair — オープンソースのAIペアプログラミング · デュアルエージェントAIコードレビュー · マルチエージェントコーディングアシスタント · Cursor / Copilotの代替 · Claude Code、Codex、Gemini、opencodeに対応、macOS・Windows・Linuxで動作。</sub>

</div>
