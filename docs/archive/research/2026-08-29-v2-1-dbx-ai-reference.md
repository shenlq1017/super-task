# v2.1 前置调研：dbx（t8y2/dbx）AI 配置实现调研与参考快照

- 日期：2026-08-29
- 目的：为 v2.1（AI 三场景：日志解释 / 配置建议 / 草稿增强 + README 确定性导入）的 AI 配置部分选取可借鉴实现。调研对象 https://github.com/t8y2/dbx （MongoDB 桌面客户端，Rust + Vue + Tauri/Axum 双通道，自带完整 AI 助手体系）。
- 结论先行：**借鉴其传输层与配置层（类型、校验、endpoint/header/proxy、错误脱敏、provider 预设、normalize、模板限额、模型目录缓存），拒绝其持久化安全姿态（key 明文入库）与 CLI/agent 体系。** 参考代码已快照至 `references/dbx/2026-08-29-8f54385/`（只读、不编译），逐文件分级见该目录 `README.md`。

## 1. 快照基线与许可证

| 项 | 值 |
|---|---|
| 上游 | https://github.com/t8y2/dbx |
| commit | `8f54385b97d8bf8bb6504161b9b7cbfe7b8acce1`（2026-08-28，`chore(packages): release 0.4.75`，tag `packages-v0.4.75`） |
| 版本口径 | 根应用 `0.5.98`（package.json），包发布 tag `0.4.75`，两者并存，引用时不要混用 |
| 许可证 | Apache-2.0（根 LICENSE，已随快照保留）。无根 NOTICE |
| 快照位置 | `references/dbx/2026-08-29-8f54385/`（25 个文件，逐文件行数与上游核对一致，未改动） |

Apache-2.0 允许复制与衍生；本仓库做法是"只读快照 + 自行改写"，不直接编译其代码，因此无 NOTICE/修改标注义务，但快照保留原 LICENSE 与来源说明（`SOURCE.md`）。

## 2. dbx AI 配置体系综述

dbx 的 AI 体系分四层（行号均为快照 commit 处）：

1. **传输层** `crates/dbx-core/src/ai.rs`（8067 行）
   - 类型：`AiProvider`（claude/openai/gemini/deepseek/qwen/minimax/ollama + `openai-compatible`/`anthropic-compatible`/`custom` + 8 个 CLI provider）、`AiApiStyle`（completions/responses/anthropic-messages）、`AiAuthMethod`（api-key/bearer）、`AiConfig`（api_key/auth_method/endpoint/model/models/api_style/proxy_enabled/proxy_url/enable_thinking/reasoning_level/context_window/max_retries + 每 CLI path/env）。
   - 关键函数：`resolve_endpoint`（ai.rs:761，按 api style 与 `/v1` 归一）、`validate_config`（:1556，官方 provider 强制 key/endpoint/model，兼容/本地 provider 允许空 key）、`maybe_bearer_headers`（:1587）/`claude_headers`（:1597，Bearer vs x-api-key）、`build_ai_http_client`（:1647，reqwest + proxy：裸 `host:port` 自动补 `http://`，**loopback endpoint 强制绕过 proxy**）、`categorized_http_error`（:2462，**错误响应文本中替换 API key、清理 URL 凭据**后再展示/记录）。
   - 其余：模型发现（:1661+）、各 provider 请求分支（Claude Messages / OpenAI completions / Responses / Gemini SSE / Ollama）、连接测试（:2599+）、complete/stream（:2975+）、reasoning effort 映射（`ai_effort.rs`）、模型过滤（`ai_model_filter.rs`）。
2. **持久化层** `crates/dbx-core/src/storage.rs`（7625 行，SQLite）：三代 AI 配置表（单条 → per-provider → 命名多配置 `ai_configs(id,name UNIQUE,config_json,is_default)`）、唯一 default 约束、重名映射、旧 JSON 迁移（:1243–1464，:4356+）；模板表 + 限额（:2500–2598）；global instructions trim + 8000 字符（:1919+）。
3. **壳层**：Tauri commands（`src-tauri/src/commands/ai.rs` 499 行：complete/stream/cancel/test/CRUD + defense-in-depth 校验；`ai_multi_config.rs` 30 行、`prompt_template.rs` 36 行薄壳）与 Axum 路由（`dbx-web/src/routes/ai.rs`，Web 版拒绝 CLI provider，SSE broadcast）——本次未快照 dbx-web 路由，接口形状可按需补。
4. **前端**（Vue + Pinia）：`AI_PROVIDER_PRESETS` 预设表（settingsStore.ts:136 起）、`normalizeAiConfig`、prompt 构建纯函数（`lib/ai/ai.ts`）、action skill 注册表（`aiSkills.ts`）、模型目录缓存（`useAiModelCatalog.ts`：5 分钟 TTL、请求去重、config signature 指纹）、AI 设置 UI（`EditorSettingsDialog.vue` AI tab）。

安全姿态现状：**AI key 以明文存于 SQLite `ai_configs.config_json`**（UI 仅 PasswordInput 显示层）；导出配置才有 WebCrypto 加密（`configCrypto.ts`，PBKDF2-SHA256 100k + AES-256-GCM）；错误链路有 key 脱敏；深链/剪贴板导入拒绝 query 传 key（`aiConfigDeepLink.ts`）。macOS keychain 仅只读探测，与 AI key 无关。

## 3. adopt / adapt / reject

### Adopt（逻辑可直接移植进 SuperTask）

1. **错误脱敏**：`categorized_http_error` 的"错误文本中替换 key、清理 URL 凭据"策略——v2.1 规格已有 sanitize 硬要求（feature-spec:85），dbx 是现成参照实现。
2. **endpoint 归一与 header 组装**：`resolve_endpoint` 按 api style 补 `/v1`；api-key vs bearer 的双 header 处理。
3. **proxy 判定**：裸 `host:port` 补 `http://`、loopback endpoint 绕过 proxy——与 AGENTS「健康检查不走代理」精神一致。
4. **provider 预设表结构**：`AI_PROVIDER_PRESETS`（label/endpoint/model/apiStyle/authMethod/requiresApiKey）比逐 provider if 分支干净；v2.1 虽只做 OpenAI 兼容端点，表结构留扩展位。
5. **normalize 策略**：`normalizeAiConfig` 的补默认/trim/env key 校验。
6. **模板限额**：name 50 / content 8000 / global 8000 / active 总量 16000 + `Array.from` Unicode 计数；Rust 侧 `PromptTemplate` serde 结构。
7. **模型目录缓存**：TTL + 请求去重 + config signature 指纹（避免 key/endpoint 变更后用旧缓存）。
8. **配置可用性判定**：`aiConfigCandidates.ts` 的小函数思路。
9. **prompt 构建纯函数 + skill 注册表**：`aiSkills.ts`（riskPolicy/contextNeeds/systemRules/outputContract 数据驱动）与 `lib/ai/ai.ts` 的 `buildSystemPrompt`/`buildUserPrompt` 分离，附件按不可信内容注入——正好覆盖 v2.1 三场景的 prompt 组织。

### Adapt（思路采用，实现必须换成 SuperTask 基建）

1. **key 保存**：dbx 明文 SQLite → SuperTask **secrets 后端固定 id `supertask.ai`**（feature-spec:73–74：永不入云 vault、不回显、不进 `secrets.status` 返回值/日志）。现有 `secrets.rs` 只有 set/delete/validate/status，需新增"固定 id 内部读取"路径，仅供 AI 请求链路消费。
2. **配置本体**：dbx 多配置表 → SuperTask appdata 单配置 `{base_url, model, timeout_secs, max_tokens}`（feature-spec:72），未知字段走 appdata 既有 `extra` 保留机制。
3. **HTTP 客户端**：dbx 用 reqwest → SuperTask 沿用 `cloud/http.rs` 的 ureq/rustls。**注意缺口：`HttpExecutor` 目前未把 `ProviderConfig.timeout` 接进 ureq**（`crates/supertask-core/src/cloud/http.rs`），v2.1 实现 `timeout_secs` 时需补齐——dbx 的 `build_ai_http_client` timeout 写法可参照。
4. **壳层命令**：dbx 的 `ai_multi_config.rs`/`prompt_template.rs` 薄壳形态 → SuperTask `src-tauri` 适配层 + `cloud.*` 同款模式，IPC 契约落 `docs/spec/ipc.md` 新 §10.13（feature-spec:99 `ai.configure` 已定义入参，api_key 写 secrets 不回显）。
5. **模型发现/多配置/agent/CLI**：v2.1 范围只有 OpenAI 兼容 `POST {base_url}/chat/completions`（feature-spec:28 明确不做自研模型/内置代理/官方推理端点之外的东西），dbx 的多 provider 请求分支、模型发现、agent loop、8 个 CLI provider 均超出范围，仅留作后续版本参考。

### Reject（不采用）

1. **key 明文持久化姿态**（含把 apiKey 拼进可序列化 config key 的 `aiConfigList.ts` 写法）。
2. **CLI provider 全家**（codex/claude/pi/opencode/cursor/grok/codebuddy/qoder）：依赖 dbx 进程/MCP/agent 契约，且 v2.1 不做。
3. **Vue/Pinia 组件原样使用**（EditorSettingsDialog.vue / AiAssistant.vue）：栈不同，仅作交互清单参考（React + shadcn 重写，按钮变体按 AGENTS 约定）。
4. **SQLite 持久化**：SuperTask 原则是能文件存储就不上数据库，AI 配置走 appdata。

## 4. 与 v2.1 规格的落点对照

| v2.1 条目（feature-spec） | dbx 对应参考（快照内路径） |
|---|---|
| §appdata 配置 `{base_url,model,timeout_secs,max_tokens}`（:72） | `crates/dbx-core/src/ai.rs` `AiConfig` 字段裁剪 + `apps/desktop/src/stores/settingsStore.ts` normalize |
| key 固定 id `supertask.ai`、不入云、不回显（:73–74） | 反面参照：dbx 明文 SQLite；正面参照：`aiConfigDeepLink.ts` 拒绝 query 传 key |
| sanitize 掩码进 prompt（:85） | `categorized_http_error`（ai.rs:2462）脱敏策略 |
| `ai.configure` IPC（:99） | `src-tauri/src/commands/ai_multi_config.rs` 薄壳形态 |
| `/ai` 页配置卡 + 连接测试（:117） | `EditorSettingsDialog.vue` AI tab 交互清单；连接测试实现 `ai.rs:2599+` |
| 三场景 prompt 组织（§AI 场景） | `lib/ai/ai.ts` buildSystemPrompt/buildUserPrompt + `aiSkills.ts` 注册表 |

## 5. 遗留 / 后续

- 本调研只快照了上表 25 个文件；若实现期需要 dbx 的 Axum 路由（`dbx-web/src/routes/ai.rs`）或 `ai_effort` 之外的 provider 请求分支，再补快照到同目录，并更新该目录 `README.md` 与 `SOURCE.md`。
- v2.1 实现期需核对的三处缺口（已在本文记录）：`cloud/http.rs` timeout 未接线；`secrets.rs` 缺固定 id 读取路径；`src-tauri/src/cloud.rs` settings 云同步白名单需硬排除 `supertask.ai`。
- 快照维护规则见 `references/dbx/2026-08-29-8f54385/SOURCE.md`：禁止原地修改；更新快照开新目录。
