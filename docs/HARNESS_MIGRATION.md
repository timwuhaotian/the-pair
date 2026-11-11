# Harness Migration Plan（CLI 派生 → 直连模型）

The Pair 从「spawn 各 agent 的 CLI」改造为「自建 harness，按 model-to-model 直连模型供应商」。
参照实现：`../killi`（同作者的 Rust CLI harness，含 LLM 传输层、配置导入、OAuth 登录、工具循环）。

## 0. 目标与非目标

### 目标（可验证的验收）

1. 不 spawn 任何 agent CLI；每个 turn 由 the-pair 自己的 HTTP 客户端直连模型供应商。
2. 供应商发现覆盖：kimi / pi / omp / opencode / codex / claude / gemini 的配置文件 + the-pair 现有 agent 配置 + `~/.killi/config.toml` + `~/.killi/chatgpt.json` + 环境变量；按 (协议, base_url) 去重。
3. OAuth 全部可在 app 内完成：ChatGPT（PKCE）、CommandCode（反向代理回传）、opencode-go（自定义 header）、muse code（key + 工具名映射）。
4. executor 自带工具循环（read / grep / glob / write / edit / bash + todo_write），mentor 只读。
5. pair 编排层语义不变：acceptance、quality_gate、context_bridge、smart_pause、session_snapshot/recovery、intelligence_store、iteration cap、stall 检测。
6. 旧 pair 快照尽力自动迁移；无法映射的标「需配置」并暂停等人类。

### 非目标

- 不保留 CLI 兜底轨道，不做 hybrid 模式。
- 不做工具调用人工审批（全自动执行）。
- 不做 web 工具、subagent / swarm、MCP。
- 不做 aider、kiro（v1 从供应商清单删除）。
- 不移植 Claude Code / Antigravity 的订阅登录态。
- 不做跨 app 的 OAuth 刷新协调（接受与 killi 的竞争）。

## 1. 已定决策

| #   | 决策                                                                                                                                            |
| --- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 自建工具循环，不 spawn 任何 CLI，无兜底轨道                                                                                                     |
| 2   | 供应商全集 = killi 特殊供应商（chatgpt OAuth / commandcode / muse code / opencode go / kimi / pi / omp）+ the-pair 现有 agent；删除 aider、kiro |
| 3   | 供应商实体唯一键 = (协议, base_url)；其下挂多账号；不同 agent 发现同一底层供应商则合并                                                          |
| 4   | 凭据复制进 app data dir（0600），可单独编辑/删除；OAuth 连 refresh token 一起复制、自己刷新                                                     |
| 5   | 协议 5 种：OpenAI Chat / Anthropic Messages / Responses / Gemini（OpenAI 兼容端点）/ CommandCode NDJSON                                         |
| 6   | 认证：ChatGPT PKCE、CommandCode 反向代理、opencode-go 自定义 header、muse 工具名映射、普通 API key、env 变量                                    |
| 7   | executor 工具 = read/grep/glob/write/edit/bash + todo_write；mentor 只读；无人工审批                                                            |
| 8   | 历史照抄 killi：每 (pair, role) 一份、全量重放、超限压缩；并入现有 pair snapshot                                                                |
| 9   | system prompt = killi 骨架 + pair 角色指令（必须带 plan checklist / verdict JSON / review evidence 契约）                                       |
| 10  | mentor ↔ executor 只桥接最终产出，不桥接工具轨迹                                                                                                |
| 11  | 模型目录 = 静态预设 + 导入源列表 + `GET /models` + 允许手输 model id；允许手加自定义供应商                                                      |
| 12  | 每角色可设 backup 模型 + 指数退避重试                                                                                                           |
| 13  | 首跑导入向导（逐源勾选、明示将复制哪些 key）→ 启动静默增量刷新 + 手动重扫                                                                       |
| 14  | 左侧 rail 新增第 4 section「模型」                                                                                                              |
| 15  | 旧数据尽力自动映射，无账号的标「需配置」并暂停                                                                                                  |
| 16  | 资源区块：删 CPU/内存 → token/延迟/费用估算/请求数；stall 120s/600s 检测保留                                                                    |
| 17  | 价格用内置静态表，无匹配显示「—」                                                                                                               |
| 18  | 四期交付                                                                                                                                        |
| 19  | Claude 只支持 API key（订阅态不可移植）                                                                                                         |

## 2. 后端模块地图

### 2.1 新增（照抄 killi，适配 Tauri）

| 模块                                                  | 抄自                    | 参考行数 | 职责                                                                            |
| ----------------------------------------------------- | ----------------------- | -------- | ------------------------------------------------------------------------------- |
| `src-tauri/src/llm/mod.rs`                            | killi `llm/mod.rs`      | 589      | `Client::complete`、重试/退避/429 cooldown、SSE 行缓冲、`Reply`/`Usage`/`Delta` |
| `src-tauri/src/llm/openai.rs`                         | 同名                    | 466      | Chat Completions prepare + SSE Parser                                           |
| `src-tauri/src/llm/anthropic.rs`                      | 同名                    | 525      | Messages prepare + SSE Parser                                                   |
| `src-tauri/src/llm/responses.rs`                      | 同名                    | 717      | Responses prepare + Parser（codex / chatgpt）                                   |
| `src-tauri/src/llm/commandcode.rs`                    | 同名                    | 723      | CommandCode NDJSON 线格式伪装                                                   |
| `src-tauri/src/llm/error.rs` `history.rs` `tokens.rs` | 同名                    | 313      | ErrorKind 分类、历史读取、token 估算                                            |
| `src-tauri/src/accounts/`                             | 新写                    | ~900     | 供应商实体/账号/模型路由 + 凭据库 + Tauri 命令（价格表并入 `presets.rs`）       |
| `src-tauri/src/sources/`                              | killi `config/import/*` | ~1400    | 7 个 importer + killi 自身 config 导入 + 去重合并                               |
| `src-tauri/src/login/`                                | killi `login/*`         | ~1900    | chatgpt PKCE / commandcode / opencode-go / muse                                 |
| `src-tauri/src/harness/`                              | killi `agent/*`         | ~4000    | turn 循环、工具 dispatch、每角色对话历史与截断                                  |
| `src-tauri/src/tools/`                                | killi `tools/*`         | ~2600    | read / write / edit / bash / grep / glob + todo_write（无 regex/globset 依赖）  |

（原计划的独立 `pricing.rs` 没建：价格作为 `ModelRoute::price` 挂在 `accounts/presets.rs` 的预设上，费用估算在前端算。）

### 2.2 删除

| 文件                               | 行数 | 处理                                                                         |
| ---------------------------------- | ---- | ---------------------------------------------------------------------------- |
| `providers/`（10 个模块 + mod.rs） | 7454 | 删；元数据（display name / billing / 角色推荐）并入 `accounts/` 的模型预设   |
| `provider_registry.rs`             | 4988 | 删；模型 id 发现并入 `sources/`，`which_binary` 与 CLI 能力探测删除          |
| `provider_adapter.rs`              | 578  | 删；`infer_provider_kind` 没有迁走，改由 `harness/resolve.rs` 按模型 id 解析 |
| `model_catalog.rs`                 | 1098 | 删；元数据并入 `accounts/presets.rs`                                         |
| `process_spawner.rs`               | 4683 | 删；turn 执行移入 `harness/`（编排在 `harness/driver.rs` 的 `TurnDriver`）   |
| `resource_monitor.rs`              | —    | 删                                                                           |

### 2.3 保留不动

`message_broker.rs`、`pair_manager.rs`、`acceptance.rs`、`quality_gate.rs`、`context_bridge.rs`、`smart_pause.rs`、`session_snapshot.rs`、`intelligence_store.rs`、`git_tracker.rs`、`worktree_manager.rs`、`skill_discovery.rs`、`report_generator.rs`、`recent_activity.rs`、`file_cache.rs`、`util.rs`、`types.rs`（扩展）。
`config_paths.rs` 缩编为只服务 `sources/`（opencode config/auth 路径 + `home_dir`）；`path_env.rs` 原样保留——进程 `$PATH` 仍要按 login shell 刷新，executor 的 bash 工具靠它找到 `npm`/`cargo`。

### 2.4 新增依赖

- `reqwest 0.12`（features: rustls-tls, stream, http2）—— killi 同款
- `toml 1` —— 读 `~/.codex/config.toml`、`~/.kimi-code/config.toml`、`~/.killi/config.toml`
- `tauri-plugin-opener` 已有，用于打开 OAuth 浏览器

### 2.5 移植硬风险（动手前必须解决）

1. **killi 是 bin crate，无 `lib.rs`**：搬模块时要重建模块树，`crate::` 路径全部重写。
2. **`Agent` 用 `Rc`/`RefCell`（`!Send`）**：Tauri async command 与 `tokio::spawn` 要求 `Send`。要么把 `Agent` 钉在单个 long-running task 内通过 channel 通信，要么把 `Rc` 换成 `Arc`/`Mutex`。建议前者（改动面小）。
3. **killi 的 `Event` 被 TUI exhaustive match（16 个文件）**：只搬 `agent/` `llm/` `tools/` `config/` `login/`，**不搬 `tui/`**。
4. **killi 无 CancellationToken**，靠进程信号取消；Tauri 侧需要显式取消通道（kill/pause 命令）。

## 3. 核心数据模型

```rust
enum Protocol { OpenAiChat, Anthropic, Responses, CommandCode }

struct Provider {              // 唯一键 = protocol + normalized base_url
    id: String,
    protocol: Protocol,
    base_url: String,          // 去掉尾部 /，用于去重
    vendor: String,
    display_name: String,
    accounts: Vec<Account>,
    models: Vec<ModelRoute>,
}

struct Account {
    id: String,
    kind: AccountKind,         // ApiKey | ChatGptOAuth | CommandCodeOAuth | OpencodeGo | Env
    status: AccountStatus,     // ready | needs_key | auth_expired | rate_limited
    secret_ref: String,        // 凭据库内的引用
    custom_headers: Vec<(String, String)>,
    tool_name_map: Option<ToolNameMap>,   // muse: bash -> run_command
    source: String,            // 导入来源（kimi / opencode / manual / oauth ...）
}

struct ModelRoute {
    id: String,
    display_name: String,
    context_window: u64,
    max_output: u64,
    efforts: Vec<String>,
    default_effort: Option<String>,
    price: Option<Price>,      // 输入/输出 每百万 token
    source: ModelSource,       // Preset | Imported | Endpoint | Manual
}

struct PairSelection {         // mentor / executor 各一份
    provider_id: String,
    account_id: String,
    model: String,
    effort: Option<String>,
    backup: Option<BackupSelection>,
}
```

持久化：

- `app_data_dir/credentials.json`（0600）—— 供应商实体 + 账号元数据 + 模型路由；密钥本身加密或单独文件
- `app_data_dir/oauth/<service>.json`（0600，目录 0700）—— OAuth token，用 killi 同款 temp 文件 + rename + flock 写
- pair snapshot 扩展：新增 `harness_history`（每 role 的完整消息历史，含工具调用）

## 4. 四期交付

每期结束都必须 `npm test && npm run typecheck && npm run lint && cargo test` 全绿，且 app 可启动运行。

### Phase 1 —— 凭据注册表 + 导入 + OAuth + 「模型」section

**后端**

- 新增 `accounts/` `sources/` `login/` `pricing.rs`
- `sources/` 的 importer 清单（每个都要带测试）：

  | 源       | 读取路径                                         | 取什么                                                                 |
  | -------- | ------------------------------------------------ | ---------------------------------------------------------------------- |
  | kimi     | `~/.kimi-code/config.toml`                       | providers / models / default_model                                     |
  | pi       | `~/.pi/agent/models.json`、`settings.json`       | provider + model + base_url + key                                      |
  | omp      | `~/.omp/agent/models.yml`（回退 .json）          | 同上                                                                   |
  | opencode | `~/.config/opencode/opencode.json` + `auth.json` | provider.models / options.baseURL / options.apiKey                     |
  | codex    | `~/.codex/config.toml`                           | `[model_providers.*]` base_url / env_key / wire_api                    |
  | claude   | `~/.claude/settings.json`                        | `env.ANTHROPIC_BASE_URL` / `ANTHROPIC_API_KEY`；订阅态 bail            |
  | gemini   | `~/.gemini/settings.json` + `.env`               | base_url / api_key；无 OpenAI 兼容端点则 bail                          |
  | killi    | `~/.killi/config.toml` + `chatgpt.json`          | providers / models / OAuth token                                       |
  | env      | 进程环境                                         | `ANTHROPIC_API_KEY` `OPENAI_API_KEY` `GEMINI_API_KEY` `XAI_API_KEY` 等 |
  | manual   | UI                                               | 协议 + base_url + key + model id                                       |

- 去重合并：按 (protocol, normalized base_url) 归并；同实体多账号保留各自 source 标签
- 新 Tauri 命令：`accounts_list` `accounts_add_key` `accounts_update_account` `accounts_delete_account` `accounts_import_scan` `accounts_refresh_models` `accounts_launch_oauth` `accounts_cancel_oauth` `accounts_oauth_status`
- OAuth 服务：`chatgpt`（PKCE + 127.0.0.1 回调，端口 1455 失败回落 :0）、`commandcode`（studio 页反向回传）、`opencode-go`（header + key）、`muse`（key + 工具名映射）

**前端**

- `AppShell.tsx`：rail 加「模型」section，badge 显示缺可用凭据的供应商数（`credentialGapCount`：有模型但无 ready 账号的端点）
- 新增 `components/ModelsView.tsx`：供应商卡 → 账号列表 → 模型列表；OAuth 登录按钮 + 进度；导入向导（逐源勾选，明示将复制哪些 key）；手加供应商表单
- `tauri-shim.ts` / `env.d.ts`：补新命令面
- i18n：en / zh / ja / ko 补串

**Phase 1 验收**

- [ ] 装有 kimi + opencode 的机器，扫描后得到去重后的供应商列表，同 base_url 不重复
- [ ] ChatGPT OAuth 在 app 内登录成功，拉到 plan models 并落凭据库
- [ ] 账号可编辑 key、可删除；删除后 pair 选不到该账号
- [ ] 首跑向导逐源勾选，未勾选的源不写入任何密钥
- [ ] 此期 pair 仍走 CLI，全流程不受影响

### Phase 2 —— 传输层替换

**后端**

- 新增 `llm/`（4 协议：OpenAI Chat / Anthropic / Responses / CommandCode NDJSON）+ `harness/`（turn + resolve）
- `process_spawner.trigger_turn` 内部改为调用 harness；事件映射到现有 `pair:message` / cognitive event / token usage
- `decide_turn_outcome` / `complete_turn` / `TurnOutputCollector` 等纯函数保留，输入从「stdout 事件」换成「harness 事件」
- 保留 mock 模式（`util.rs` 的 mock 检查）供 e2e 使用

**Phase 2 验收**

- [x] `llm/` 四协议 + `harness/` 落地，`trigger_turn` 有直连分支
- [x] token 用量与供应商返回的 usage 一致（按 turn 汇总）——wire 测试断言 prompt/completion tokens
- [x] 429 触发退避重试（wire 测试：同一次 turn 两次请求，第二次成功）
- [x] 主模型失败切 backup（wire 测试：主 401 → backup 出文本，`fell_back = true`）
- [x] kill / pause 能真正中断进行中的 HTTP 流（`turn_cancels` + `CancelResult`，wire 测试：流中途取消）
- [ ] 一个 pair 用直连模式跑完 mentor → executor → acceptance 全 loop —— **未验证**：`complete_turn` 需要 Tauri AppHandle，单测覆盖不到；与 mock 路径共用同一函数，编译期一致
- [ ] 120s 警告 / 600s 停滞检测在直连模式下仍生效 —— **未验证**：基于「无输出」判定，直连分支每收到 delta 都调 `update_output_progress`，逻辑上等价，无运行时证据

### Phase 3 —— 工具循环 + 角色分级

**后端**

- 移植 `tools/`：read / grep / glob / write / edit / bash + todo_write
- 工具白名单按角色下发：mentor 只有 read/grep/glob/todo_write；executor 全量
- system prompt = killi 骨架（system.md + Environment + AGENTS.md + skills）+ pair 角色指令，角色指令必须包含：plan checklist 格式、verdict JSON 格式、review evidence 要求（`context_bridge` / `acceptance` / `quality_gate` 的解析契约）
- 历史：每 (pair, role) 一份，全量重放，超过 context window 触发压缩；压缩策略照抄 killi（Full / Reduced）
- snapshot 扩展 `harness_history`，恢复时重放

**Phase 3 验收**

- [x] executor 能真实修改工作区文件并执行测试 —— wire 测试：模型 `write` 后 `bash cat`，文件真在磁盘上，两条工具结果以 `role:"tool"` 回灌
- [x] mentor 请求 write/edit/bash 被工具层拒绝，不影响 turn 继续 —— wire 测试：ReadOnly 策略下 bash 被拒，turn 仍以模型文本收尾
- [x] muse 供应商的 bash 工具名正确映射为 `run_command` —— wire 测试：请求体出现 `run_command`、不出现 `"bash"`，跑起来的仍是 bash
- [x] 超长历史被截断且 turn 仍正常 —— `harness/history.rs`：按消息数（400）+ 字节数（400KB）双上限，切点只落在非 tool 消息上，`tool` 结果不与它的 assistant call 分离
- [x] 崩溃恢复后 harness 历史完整，agent 能接着上下文继续 —— 历史与 selection 一起进 snapshot（`providerSessions.mentorHistory/executorHistory/mentorSelection/executorSelection`），`build_process_context` 恢复；恢复时也恢复 selection，否则重启后 pair 会掉回 CLI 且历史变成死重

**Phase 3 偏差（未按 killi 原样做）**

1. **压缩是截断 + 提示，不是 killi 的 Full / Reduced 摘要压缩**。超限时保留最新窗口，提示走 system prompt（不是往历史里插一条 user 消息——有的网关拒绝连续 user，也会让 tool 结果落单）。完整摘要压缩留作后续。
2. **system prompt 只搬了 Environment + AGENTS.md**（OS/arch/cwd/日期 + 工作区 `AGENTS.md`，16KB 截断）。killi 的 skills 注入没搬：the-pair 的技能已由前端 SkillPicker 按任务附带。日期与 AGENTS.md 会每日/每次编辑失效一次 prompt cache 前缀，接受。
3. **mentor 的 `CHECKS:` 契约因此改成「你通过阅读验证了什么」**：mentor 是 ReadOnly（无 bash），写 "none" 会被 `quality_gate` 拒绝，所以提示词明确禁止 `none` 并要求列出读过的错误路径/类型/计划步骤。
4. **`pair_kill` 对直连 pair 不再要求有子进程**：直连 turn 没有 child，原先会报 "No active mentor process found" 且不 bump generation，pair 会卡在 Mentoring 直到 600s 停滞检测。现在只 raise cancel flag + bump generation。
5. **token 用量按工具循环累计**：一个 turn 跑了 N 次请求就报 N 次之和（此前只报最后一次）。`cached_tokens`/`reasoning_tokens` 仍不进 `TurnTokenUsage`，费用估算只按 input/output 算。
6. **mock 模式先于直连分支判定**：否则 desktop e2e 在开发机有真实凭据时会真的发 HTTP 请求。
7. **已知遗留**：直连 pair 的 CPU/内存监控仍是 0（决策 #16 换成 token/费用/请求数，Phase 4 已把 `resource_monitor` 整个删掉，见 Phase 4 偏差 6）；`turn_cancels` 在 pair 删除时不清理（每条目几十字节，暂不处理）。

### Phase 4 —— 清理 + 前端重整 + 迁移

**后端**

- 删除 `providers/` `provider_registry.rs` `provider_adapter.rs` `process_spawner.rs` `resource_monitor.rs`
- `stubs.rs` 的 `config_get_models` 等命令改由 `accounts/` 提供
- 迁移模块：旧 `mentorModel`/`executorModel` 字符串 → (provider, account, model)；用 `infer_provider_kind` + 静态 catalog 匹配；匹配不到账号 → 「需配置」+ 暂停
- localStorage 迁移：`the-pair-preferred-mentor-model` 等旧 key 迁到新结构

**前端**

- `ModelPicker.tsx` 重写：agent CLI 分组 → 供应商分组；步骤 agent→model→route→effort 改为 供应商→账号→模型→effort；「CLI 未安装」提示全部改为凭据状态（无 key / 未登录 / 已过期）
- `lib/providerSetup.ts` 删除 install/login 命令表，改账号状态展示
- `lib/providerResolution.ts` `modelResolution.ts` `modelCatalogGrouping.ts` 适配三元组
- `PairOperationsPanel.tsx` 资源区块重写为 token / 延迟 / 费用估算 / 请求数
- `usePairStore.ts` 模型选择结构与迁移加载
- e2e-web `mock-init.js` 重写，覆盖新命令面

**Phase 4 验收**

- [x] `npm test && npm run typecheck && npm run lint && cargo test` 全绿 —— 本机实跑：`test:js` 518、`test:ui` 179、`cargo test` 810 全过，typecheck 与 lint 无输出，`cargo build --lib` 与 `cargo fmt --check` 干净
- [x] 旧快照（含无账号可映射的）迁移后状态正确，pair 不丢消息历史 —— `session_snapshot.rs` 的 `legacy_snapshot_without_newer_fields_still_loads` 直接反序列化一份带 `mentorProvider` 的 v1 JSON；`prepare_loaded_snapshot_*` 三个测试断言消息与 run 历史不丢；解析不到账号时 `harness/resolve.rs` 返回 `None`，turn 报错而不是伪造选择
- [x] 全部 4 语言 UI 无缺失 key —— en/zh/ja/ko 四份 locale 各 613 个 key，集合与 `{{placeholder}}` 完全一致（脚本逐 key 比对，不是测试）
- [ ] e2e-web 全绿 —— **未验证**：本机无 chromium 且禁止安装。本期只把 `mock-init.js` 与 4 个 spec 对齐到新命令面：`accounts_list` 种子改成 `ProviderView[]`（一个 ready + 一个 `needsKey` 端点）、删掉 `config_*` / `provider_launch_login` 分支、`worktree.spec.ts` 的 `pair_create` 断言从 `useWorktree` 扩到 `mentor`/`executor` 的 `{ role, model }`
- [ ] 变更行覆盖率 ≥95% —— **未测量**：迁移横跨 src-tauri 几乎所有模块，没有按 `git diff` 逐行对过覆盖率报告

**Phase 4 偏差（未按 Phase 4 计划做）**

1. **旧 CLI 模块是整体删除，没有「拆」。** `provider_registry.rs` 的 `homedir()` 原样搬进 `config_paths::home_dir()`（env 优先 + `dirs::home_dir` 回落，Windows 由 `APPDATA`/`LOCALAPPDATA` 反推 home 的逻辑一起搬）；`config_paths` 只剩 `opencode_config_path` / `opencode_auth_path` 两个公开函数，muse、grok、opencode dirs 三个 helper 全删。`model_catalog.rs` 也是整文件删——元数据在 Phase 1 就并进了 `accounts/presets.rs`，Phase 4 只是把壳删掉。`infer_provider_kind` 没有迁往任何迁移模块，直接消失。
2. **`ProviderKind` 从 Rust IPC 面彻底消失。** `AgentConfig.provider`、`Pair.mentor_provider`/`executor_provider`、`SessionSnapshotRecord`/`Draft` 的 `mentor_provider`/`executor_provider` 全部删除。旧快照照常加载：serde 默认忽略未知字段（全仓没有 `deny_unknown_fields`），`legacy_snapshot_without_newer_fields_still_loads` 用一份带 `mentorProvider` 的 v1 JSON 钉住这一点。
3. **旧模型限定符在前后端各归一一次，没有独立迁移模块。** Rust 侧 `harness::resolve::model_id` 把 `vendor:id` 与 `vendor/id` 都削成裸 id，但网关路由（`anthropic/claude-…`）不算限定符、保留整条；前端 `lib/providerResolution::stripModelQualifier` 同理，`lib/modelResolution::resolveEffectiveModels` 在出口统一削一次，pair 镜像、更新载荷与后端因此看到同一个裸 id。计划里的「匹配不到账号 → 『需配置』+ 暂停」没有落地：解析失败时 pair 照常创建，turn 直接报错（`The mentor has no model selected. Pick one in the pair's settings.`），让人去 Models section 补凭据。
4. **`stubs.rs` 只剩两个 broker 命令。** `pair_get_messages` / `pair_get_state`；`config_get_models`、`config_get_cached_models`、`config_refresh_models`、`config_get_providers`、`config_read`、`config_open_file`、`provider_launch_login` 全部删除，没有「改由 `accounts/` 提供」的替身——`accounts_list` 是唯一的模型面，前端经 `window.api.models.list()` 读。
5. **每角色 backup 模型（决策 #12）不在 UI 里。** `harness::ModelSelection::backup` 存在，`harness/turn.rs` 真的会在主模型被拒时切过去（wire 测试 `the_backup_answers_when_the_primary_is_refused`），但 `harness/resolve.rs` 与 `pair_manager.rs` 两处构造都写死 `backup: None`，前端也没有任何控件设置它——决策 #12 只兑现了「harness 支持」这一半。
6. **CPU/内存监控删除。** `resource_monitor.rs` 整文件删，`PairResources` 结构体连同 `Pair`/快照里的 `cpuUsage`/`memUsage`（及 `mentorCpu`/`mentorMemMb`/`executorCpu`/`executorMemMb`）字段一起从 Rust 与 TypeScript 类型里彻底删除，没有留空占位。仅剩的 `cpuUsage`/`memUsage` 字样在 `session_snapshot.rs` / `intelligence_store.rs` 的旧快照测试夹具里，用来钉住「带这些字段的旧快照照常加载（serde 忽略未知字段）」。inspector 的 `resources` 段改成 `usage`：token（turn / run）、费用估算、请求数（= 上报过 usage 的 turn 数）、最近输出。
7. **前端 lib 是改写不是「适配三元组」。** `providerSetup.ts` 整文件删除（账号状态在 ModelsView / picker 各自渲染）；`providerResolution.ts` 只做模型 id 归一与注册表查找；`modelCatalogGrouping.ts` 换成 canonical model + endpoint route 两级；`modelPreferences.ts` 的 localStorage key 沿用旧名（`the-pair-preferred-{mentor,executor}-model`），没有迁移——旧值里的限定 id 在读取时被归一。

## 4.5 交付后自查修复（Phase 4 收尾）

Phase 4 落完后做了一轮全仓自查（10 个并行 review，覆盖 `llm/` `accounts/` `login/` `tools/` `harness/` `sources/` 与前端），共修掉 100+ 个确认缺陷。按性质归四类：

**会让功能整个不工作的（必修）**

- `llm/mod.rs` 少了 CommandCode 的 NDJSON 分支：该网关回 `application/x-ndjson`、一行一个裸 JSON，被 content-type 检查直接判死，每个 turn 未发先败。killi 原有 `ndjson` 标志位，移植时丢了。
- `harness/resolve.rs` 的 `vendor_hint` 同时过滤了两轮候选，非该 vendor 命名的端点（`ANTHROPIC_BASE_URL` 代理、OpenRouter、手加端点）永远服务不了 `claude-*` / `gpt-*`，`pair_create` 直接回滚。现在 hint 只排序，「明确列出该 id 的端点」这轮不设门槛。
- `turn.rs` 从未读 `ProcessContext.{mentor,executor}_reasoning_effort`：picker 第三步与 inspector 的 effort 选择全是装饰，四层传输都实现了这个旋钮却没接到。现在 pair 自己的 effort > 路由默认 > 不传。
- `login/chatgpt.rs` 刷新成功不落盘：OpenAI 轮换 refresh token，磁盘上留的是用掉的那张，第三次登录必然 `invalid_grant` 并删缓存。
- `accounts/registry.rs` OAuth 账号永远离不开 `authExpired`（守卫读的是已被覆盖后的 secret），贴 key 到 OAuth 账号也会被判过期，`expires_at` 完全没人看。
- `sources/codex.rs` 的 `wire_api` 映射反了：现在的 Codex 只有 Responses 一种 wire，缺省被映射成 `OpenAiChat`，每个网关导入都 404。
- `sources/kimi.rs` 把 `type = "kimi"` 整类跳过，而那是 Moonshot 自己的 OpenAI 兼容接口。
- GUI 启动不继承 profile，`Secret::Env` 全读不到——`lib.rs` 现在连 `$PATH` 一起把登录 shell 的整个环境搬进来。

**资源与 DoS**

- `tools/search.rs` 的重复计数只受 `usize` 限制：`a{100000000}` 编译出 1 亿条指令（实测 ~4 GB / 253 ms），`a{4294967295}` 直接 abort 整个 app。编译期拒绝超过 1000 的计数。
- `match_parts` 递归没有自己的预算，`**/`×N 的 glob 在深树上不可恢复地挂死。
- `acceptance.rs` 的 verdict 扫描是 O(n²) 且对每个平衡 `{…}` 都 `from_str`，长回复会卡住 review turn。
- `driver.rs` 每收到一个 delta 就 `notify_state_update` 一次全量 `PairState`：4k token 的回复 = 4000 次全状态 IPC 序列化。activity / reasoning 现在按 ~1s 合流，turn 结束时补一次终态。
- `edit.rs` 读文件没有大小上限（兄弟模块都有 10 MB），100 MB 文件 >1 GB RSS。

**正确性 / 数据安全**

- `llm/*` 断了 `repair_call_arguments`：流被切断的 tool call 参数原样存进历史，之后每次请求都 400 且永不好。新增 `Delta::Discard` 让失败尝试的流式输出整体作废（driver 侧配 `MessageBroker::retract_cognitive_events` 回滚已发布的心智事件）。
- `accounts/types.rs::mask_secret` 按字节切片，非 ASCII key 直接 panic；`registry.rs::upsert_manual` 对归一成空的 base URL `.expect()` panic。
- `accounts/store.rs` 临时文件先 0644 再收紧，有个世界可读的窗口；`load` 从不重新收紧权限。
- `tools/files.rs` / `edit.rs` 的写入是原地截断，ENOSPC/崩溃会留下半个文件；改成同目录临时文件 + rename。
- `files.rs` / `edit.rs` 在 `async fn` 里做阻塞 I/O，FIFO 能永久钉住一个 tokio worker 且无法取消；改 `spawn_blocking`。
- `session_snapshot.rs::load_all_pairs` 会用磁盘快照盖掉活着的 pair（前端在手抖路径里会调），live pair 的 turn 立刻自杀、pair 挂在 Mentoring 永不返回。
- `pair_manager.rs` 的 `start_turn` 失败发生在 `begin_new_run` 清空消息之后且无回滚，transcript 直接没了。
- `pair_manager.rs::apply_model_update` 只改字符串不清 selection，改模型对进行中的 run 无效且跨重启保留。
- `usePairStore` 的 usage 汇总把同一个 turn 数了两遍（console 用 `collapseWithDropCounts` 藏了重复，inspector 没藏）。
- `session_snapshot.rs::restore_session` 直接调 `trigger_turn`，selection 解析失败时 pair 已复活成 Executing 但是死的。
- `harness/turn.rs` 主模型失败切 backup 时，主模型已经写出去的工具调用从历史里消失、用量不计费；现在一旦有工具真的跑过就拒绝 fallback。
- `message_broker.rs` 的心智事件 id 在同一毫秒同一角色下会撞，产生重复 React key。
- `login/mod.rs` 漏了 `CommandCode` 的 `needs_key()`，粘贴 key 的兜底路径永远走不到。
- `login/*` 用 `open`/`xdg-open` 开浏览器，Windows 上没有这两个命令——改用已注册但从未被调用的 `tauri-plugin-opener`。

**顺手清掉的死代码 / 不一致**

- 前端 `providerResolution.ts` 的归一化与 Rust `QUALIFIERS` 不一致（大小写、首字符集），同一模型前后端判定相反；`modelCatalogGrouping` 的 canonical key 大小写敏感会把一个模型拆成两行。
- `useModelStore` 只在 Models section 挂载时才 load，rail 徽章在新机器上恒为 0。
- `ModelPicker` 的 recent 列表只读一次，刚选的模型切个端点回来就丢。
- `SignInModal` 关闭不重置状态，重开带着上次的错误和禁用按钮。
- `ModelsView` 的协议/账号类型标签是硬编码英文，绕过 i18n。
- 删除 `findProviderForModel` / `buildAgentConfig` / `getPreferredPairModelSelection` 三个死导出及其测试、`brandIcons` 里已下线的渠道图标、`tauri-api.ts` 的 `TauriPair.mentorProvider`。
- `PairPreset.pauseOnIteration` 之前根本没有接任何地方（UI 承诺「自动暂停」但不传 `maxIterations`），现在接上；`autoAttachGitBaseline` 没有对应后端字段，连文案一起删。
- 删除 `modals.loadSkill` 之前 CreatePairModal / AssignTaskModal 里硬编码的英文插入串。

**未修（记录在案）**

- `harness/driver.rs` 的 reasoning 合流靠计数回滚，`Delta::Discard` 之前已发布的事件由 `retract_cognitive_events` 收尾；若未来 broker 需要更细的撤销语义再扩。
- `tools::run` 仍不接收取消令牌，`pair_delete` / worktree 轮换只能等 10s 超时后继续；彻底取消要把 `Arc<AtomicBool>` 穿进 `bash::run`。
- `experimental_bearer_token`（codex）只记 note 不导入：不确定它是字面 token 还是变量名，猜错会把凭据放错 header。
- 93 个 4 语言共有的死 i18n key（`dashboard.*` / `onboarding.*` / `emptyState.*` 等）是迁移前就存在的，本轮未动。

## 5. 前端改动清单

| 文件                                                  | 改动                                                       |
| ----------------------------------------------------- | ---------------------------------------------------------- |
| `src/renderer/src/components/AppShell.tsx`            | rail 加「模型」section + badge                             |
| `src/renderer/src/components/ModelsView.tsx`          | 新增：供应商/账号/模型管理 + OAuth + 导入向导 + 手加供应商 |
| `src/renderer/src/components/ModelPicker.tsx`         | 重写为 端点 → 模型 → effort 三步；凭据状态文案             |
| `src/renderer/src/lib/providerSetup.ts`               | 整文件删除（账号状态在 ModelsView / picker 各自渲染）      |
| `src/renderer/src/lib/providerResolution.ts`          | 模型 id 归一 + 注册表查找                                  |
| `src/renderer/src/lib/modelResolution.ts`             | 有效/待定模型解析与 `pair_update_models` 载荷              |
| `src/renderer/src/lib/modelCatalogGrouping.ts`        | canonical model + endpoint route 两级分组                  |
| `src/renderer/src/lib/modelPreferences.ts`            | key 沿用旧名，读取时归一限定 id                            |
| `src/renderer/src/components/PairOperationsPanel.tsx` | resources 区块改写为 usage                                 |
| `src/renderer/src/store/usePairStore.ts`              | 模型选择结构、迁移加载                                     |
| `src/renderer/src/tauri-shim.ts` `env.d.ts`           | 新命令面                                                   |
| `src/renderer/src/locales/{en,zh,ja,ko}.json`         | 补串                                                       |

## 6. 必须接受的代价

- 包体 +2~3MB（reqwest + rustls）
- 密钥在磁盘存两份（源 agent 配置 + the-pair 凭据库）
- 与 killi 的 OAuth 刷新竞争 → 偶发需重新登录
- Claude Code / Antigravity 订阅用户失去通道（需自备 API key）
- aider / kiro 用户失去通道
- 失去 per-agent CPU / 内存监控（换取 token / 延迟 / 费用）

## 7. 自行假定、待确认

1. gemini / agy 与 Claude 同等处理：只支持 Google API key，Antigravity 的 Google 登录态不移植。
2. mentor 也下发 `todo_write`（只读 + 待办），用于时间线展示。
3. 环境变量是一等账号来源（killi 的 live env 优先语义），`ANTHROPIC_API_KEY` 等自动成为 ready 账号。
4. bash 工具直接在当前 pair 工作目录执行，不做沙箱（与现状 CLI agent 一致）。
