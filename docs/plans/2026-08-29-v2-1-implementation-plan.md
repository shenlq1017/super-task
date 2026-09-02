# v2.1 实现计划（智能：README 导入 / AI 助手）

> 2026-08-29。依据：[2026-08-29-v2-1-feature-spec.md](2026-08-29-v2-1-feature-spec.md)（规划稿，随拍板同步修订）。
> 前置：v2.0 收尾基线（若 2.1 先行，ureq 由本期引入，Phase 2 任务不变）。
> 执行约定：先读 `project tooling/executing-plans-0.1.0\SKILL.md`；前端任务点名 skill。

## 基线与每期回归

- 参照基线（v2.0 收尾）：core ≈ 510+ / cli ≈ 28；parity ≈ 940。kickoff 实测后回填。
- 每 Phase 收尾必跑：core / cli 测试、`frontend/` 内 `npm run build`、i18n parity 脚本。
- 目标基线：core ≈ 570+（新增 ~60）、parity ≈ 985（ai 页 + discover/config/log 入口增量）。CLI 本期无新命令。
- 单测零真实网络：AiClient trait + fake；README 导入纯函数。

---

## Phase 1 · core：README 导入器（与 Phase 2 完全并行）

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 1.1 | `importer/mod.rs` + `importer/readme.rs`：README 发现（大小写不敏感 md/markdown 变体）、解码（UTF-8 → GBK 兜底，复用既有策略） | 新模块；解码复用 `log/` 或 launcher 既有工具 | 发现矩阵 + 无文件空草稿单测 |
| 1.2 | 命令抽取：fenced block（sh/bash/shell/console/无语言）+ 行内 code；`&&`/`;`/`&` 拆分；`VAR=v` 前缀剥离；`export PORT=` 端口提示 | importer/readme.rs | 抽取矩阵单测（含噪声行计数） |
| 1.3 | 分类规则表（spec §3.3）：service / script / 忽略 三类 + 章节加权（中英标题）+ 归一化去重 + confidence | importer/readme.rs | 分类矩阵单测（每模式正反例） |
| 1.4 | 与 scan 融合：scan 骨架优先，README 补 entry/script/extra_args/端口；冲突双值 + provenance（scan/readme/default） | `scan.rs` 草稿结构复用、merge 向导数据形 | 融合与冲突优先级单测 |
| 1.5 | golden fixtures 六类：spring-node 混合 / python / go / 中文 / 纯噪声 / 无 README | `tests/fixtures/readme/` | golden 快照测试 |
| 1.6 | `README_NOT_FOUND` 错误码（显式路径不存在） | `error.rs` | 口径单测 |

## Phase 2 · core：AI 模块

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 2.1 | `ai/mod.rs`：ProviderConfig 读写（appdata）；key 存取走 secrets 后端固定 id `supertask.ai`（写入不回显） | appdata、`secrets/` 复用 | 配置 round-trip + key 不回显单测 |
| 2.2 | AiClient trait + HttpAiClient（ureq，OpenAI 兼容 chat/completions）+ FakeAiClient | 新依赖复用 v2.0 ureq | fake 往返 / 非 2xx→AI_REQUEST_FAILED / AI_TIMEOUT 单测 |
| 2.3 | prompt builders 三任务：explain_logs（tail ≤200 行且 ≤32KB + 服务上下文）/ config_suggest（sanitize 后 yaml + 校验结果）/ enrich_draft（草稿 JSON） | ai/ | 三 builder 输入输出形状单测 |
| 2.4 | sanitize：secret 值 / `supertask.ai` key / 形似 token 行 → `<redacted>`；请求体级断言 | ai/、secrets 读取 | 原始值不出现在请求体单测 |
| 2.5 | 预算与用量：字符÷4 粗估超限 → `AI_CONTEXT_TOO_LARGE`；按日调用计数（appdata） | ai/ | 超限 / 计数单测 |

## Phase 3 · 壳层：IPC + feature 转 live

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 3.1 | `import.readme` / `ai.configure` / `ai.status` / `ai.complete` 四命令薄适配 | `src-tauri/src/commands.rs` | 与 fake 的壳层链路冒烟 |
| 3.2 | features.rs：ai → Live(2.1)；SOON_COMMANDS 移除 ai.complete | `features.rs:33,51-55` | features 单测更新（清单清空断言） |

## Phase 4 · 前端

> Skills：`vercel-react-best-practices`、`vercel-composition-patterns`、`ui-styling`；4.5 审查用 `web-design-guidelines`。

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 4.1 | /ai 页（soon→live）：配置卡（保存 success / 测试 soft）+ 用量卡 + 隐私说明 + MCP 使用说明文档卡 | `pages/ai-page.tsx` 新建（registry 已在扩展组） | 场景 7/8 UI 路径 |
| 4.2 | /discover「从 README 导入」入口 → 复用 merge 向导；provenance 徽标 + confidence 展示 | `discover-page.tsx`、既有向导组件 | 场景 2–6 UI 路径 |
| 4.3 | log-view 可选动作槽：`extraActions?` slot prop 注入「AI 解释」（右区固定槽位、Tooltip 1000ms、loading 态）；run/logs 两处一致 | `components/log-view.tsx` | 共用组件零分叉；遵循工具栏分区约定 |
| 4.4 | /config AI 建议卡：markdown 文本 + 建议 yaml 参考稿 + 「填入编辑器」（outline，不保存） | `config-page.tsx` | 场景 10：填入后未保存状态断言 |
| 4.5 | 四语 keys + 页面审查 + mock fake AI（确定性回文） | `i18n/locales/*`、mock IPC | parity 通过；审查过 |

## Phase 5 · 文档闭环 + 全量回归

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 5.1 | ipc.md §10.13（四命令 + 错误码五枚）；architecture.md（importer / ai 模块一节：确定性导入 + sanitize 原则） | docs/spec/ | 契约同步 |
| 5.2 | yaml.md **零增量**（README 导入只产出既有字段；AI 配置在 appdata 不在 yaml）——核对后在本文记档 | — | 核对结论回填 |
| 5.3 | repository conventions 当前阶段 + 规范真源；inv-1 交付表回改 | living 文档 | 盘点=当前事实 |
| 5.4 | 全量回归四连 + 基线核对 | — | core ≈ 570+ / parity ≈ 985 |

## Phase 6 · 验收

| # | 任务 | 验收 |
|---|------|------|
| 6.1 | CI：fixtures golden + fake AI 全链路（spec §9 场景 1–10 中可自动化项） | 入库可重复 |
| 6.2 | 真机：真实 OpenAI 兼容端点一次冒烟（解释日志 + 配置建议各一次，人工）；中文 README 真工程一份走导入全链路 | 记录进 `docs/verification/2026-xx-xx-v2-1-acceptance.md` |

## 依赖与并行

- **Phase 1 与 Phase 2 完全独立**（导入器离线纯函数；AI 模块独立），可双线并行。
- Phase 3 依赖 1+2；Phase 4 依赖 3（部分卡片可提前开发，接线等 3）；5 → 6。
- 每期独立可合入；ai 命令未接线前保持 FEATURE_SOON 拒绝，禁止假成功。

## 复用清单

- 零新 crate（ureq 由 v2.0 引入；若 2.1 先行则本期引入并在本表记档）。
- 复用：secrets 后端（`supertask.ai`）、appdata、scan 草稿结构与 merge 向导、FakeRunner 注入模式（AiClient/FakeAiClient）、日志解码策略、log-view 共用组件（slot 扩展）。
- 与 1.5 复用核查惯例一致，结论实现期回填本文件。

---

## 执行记录（2026-08-29，AI 块先行落地）

范围：用户拍板「AI 及其前后端对接先行，其余暂缓」。Phase 1（README 导入器）、任务 4.2
（discover 入口）、命令面板三项、Phase 6 真机验收**暂缓**；`import.readme` 命令与
`README_NOT_FOUND` 错误码随 Phase 1 补。

已落地：

- **Phase 2 core AI 模块**：`crates/supertask-core/src/ai/`（mod/client/prompt/sanitize）。
  key 走应用级 secrets 文件（`%APPDATA%/SuperTask/secrets.env`，逻辑 id `supertask.ai`，
  dotenv 子集复用 1.2 基建；永不入云/不回显）。sanitize（secret 值精确替换 + 敏感行整行
  `<redacted>`）、三场景 prompt builder（尾部 200 行/32KiB 截断）、预算（字符÷4，
  context_window 收口）→ `AI_CONTEXT_TOO_LARGE`、按日用量计数（appdata `aiUsage`）。
- **截图对齐升级**（用户对照 dbx 验收后要求补齐）：命名多配置（`aiConfigs` + 默认配置，
  旧单配置 `ai` 字段只读迁移）、8 种 API provider 预设（openai-compatible/claude/deepseek/
  qwen/minimax/gemini/ollama/custom；**无 CLI provider**）、api_style 双风格（OpenAI 兼容
  chat/completions + Anthropic Messages）、auth_method（api-key/bearer）、代理（裸 host:port
  补 http、loopback 绕过）、上下文窗口、模型发现（`GET /models`）、全局自定义指令
  （≤8000）与场景 Prompt 模板（name 50/content 8000/启用总量 16000，Unicode 计数）。
- **Phase 3 壳层**：`src-tauri/src/ai.rs` + 九命令（`ai.status/complete/config.save/
  config.delete/config.default/instructions.save/template.save/template.delete/models`）；
  features ai → Live(2.1)，SOON_COMMANDS 清空（机制保留）。
- **Phase 4 前端**：`/ai` 页（配置列表 + 编辑表单 + 全局指令 + 模板 + 用量 + 隐私 + MCP 说明）、
  log-view `extraActions` 渲染 prop 槽位 + 共享 `AiExplainButton`（run/logs 零分叉）、
  config RawTab「AI 建议」卡（建议 yaml 围栏提取 + 整段填入编辑器不保存）、mock 确定性
  回文 AI、四语 parity 1035 keys。
- **Phase 5 文档**：ipc.md §10.13（九命令 + 约束）、architecture.md §8（AI 模块）、
  repository conventions 当前阶段、inventory inv-1/inv-3 回改。
- 测试基线：core 408 单测全绿（AI 新增 ~38）；`cargo check -p supertask` 通过；
  `npm run build` 通过；parity 1035×4 全齐。CLI 无新命令（本期无）。

**偏差备案（相对 feature spec v2026-08-29）**：

1. §4.3「无自动重试」→ 仅临时错误（429/500/502/503/504/超时/网络）线性退避重试
   ≤`max_retries`（0–10，默认 2，dbx 对齐）；一次业务调用成功只计 1 次用量，失败不计。
2. §5 IPC 契约从 4 命令扩为 9 命令（多配置/模板/全局指令/模型发现），`ai.configure`
   被 `ai.config.*` 族替代；§4.1 单配置 `{base_url,model,timeout_secs,max_tokens}` 升级为
   命名多配置（新增 provider/api_style/auth_method/proxy/context_window/max_retries 字段）。
3. §4.1「AiClient trait」落地为 `AiHttp` trait（含 GET 供模型发现），命名沿 cloud
   `HttpExecutor` 先例。
4. dbx 截图中的 CLI provider、Agent 回合上限、Ask/Agent 默认模式**明确不做**
   （v2.1 非目标：不重复 MCP/agent；IDE 场景由 1.5 `supertask mcp` 覆盖）。
5. 命令面板三条入口（spec §7）随 discover 入口一并暂缓（本轮未做）。
6. 连接测试 = 最小 explain_logs 请求，计入当日用量（spec §7「发一次最小请求」的实现口径）。

## 执行记录（2026-08-29，第二轮：README 导入器 + discover 入口 + 命令面板）

范围：Phase 1 全部任务 + Phase 3 增量（`import.readme` 族）+ 任务 4.2（discover 入口）
+ 命令面板三入口 + Phase 5 文档闭环。至此 v2.1 自动化范围全部落地，仅剩 Phase 6.2 真机验收。

已落地：

- **Phase 1 core 导入器**：`crates/supertask-core/src/importer/`（mod + readme）。README 发现
  （大小写不敏感 `.md`/`.markdown`，`.md` 优先）、解码（UTF-8 → GBK 兜底，沿 engine 解码策略）、
  fenced + 行内命令抽取（提示符剥离/续行拼接/链拆/VAR= 前缀剥离）、规则表分类（service/script/
  忽略 + 中英章节加权 + 行内 code 置信度上限 medium + 归一化 argv 去重）、`export PORT=` 只进
  端口提示、噪声计数。与 scan 融合：scan 事实优先，README 补全缺失字段，冲突 scan 保留 +
  README 值进建议列。
- **merge.rs 扩展**：`FieldMeta`/`FieldMetas`（字段来源 scan/readme + confidence + 冲突
  readme_value）、`ScriptMergeItem` + `preview_with_sources()`（脚本合并项；普通 scan 预览
  序列化不变）、`MergeChoice.target: service|script`（缺省 service，1.1 契约兼容）+
  `apply` 脚本分支（add 插入 / update 整体替换，确认后 cmds 只来自文档）。
- **错误码** `README_NOT_FOUND`（error.rs + 序列化回归）。
- **Phase 3 增量**：`src-tauri` `import.readme` / `import.readmeApply` 两命令（确定性重导入
  + `merge::preview_with_sources` / `merge::apply` + saveForm），lib.rs 注册。
- **前端**：`components/scan-merge.tsx` 共享向导（config-page 内嵌向导抽出 + 新增
  `ProvenanceChips` 徽标与 `ScriptItemRow`）；config-page 改为复用（UI 零变化）；/discover 页
  「从 README 导入」outline 入口（outline 按钮 → 向导面板，空草稿给人话提示卡，
  `?readme=1` 供命令面板直达）；命令面板三入口（README 导入 → `/discover?readme=1`、
  AI 解释当前日志 → `logs.snapshot` 尾 200 行 + `ai.complete` + 结果对话框、打开 AI 设置 → `/ai`）；
  mock `import.readme/readmeApply`（README-only 新增 + 冲突建议列 + 端口提示确定性样例）。
- **测试**：core `importer::` 15 个单测 + `tests/golden/readme/` 五类 golden
  （spring-node / python / go / zh / noise，fixtures 在 `tests/fixtures/readme/`）；
  基线 core 453（+45）/ cli 20 全绿；`cargo check -p supertask`、`npm run build`、
  parity 1057×4 通过。
- **文档**：ipc.md §10.14（README 导入两命令 + 确定性约束）+ §10.13 错误码行更新；
  architecture.md §9（导入器模块）；repository conventions 当前阶段；inventory inv-1/inv-3 回改。

**偏差备案（续）**：

7. fenced 语言白名单在 spec §3.2 基础上追加 `text/plain/plaintext/zsh/terminal/powershell/pwsh`
   （本仓库 README 的命令块用 ```text；分类严格、混入片段只进噪声计数，风险可控）。
8. spec §3.1「未指定且未发现 → 空草稿」落地为「scan 骨架 + 人话提示 warning」（向导仍可用，
   空草稿 = scan 也为空时自然成立）；显式路径不存在仍是 `README_NOT_FOUND` 硬错误。
9. spec §5 契约只列 `import.readme`；实现补 `import.readmeApply`（scanApply 会重扫描，
   无法应用 README 补全字段/脚本，故按同型新增；确定性重导入保证与预览一致）。
10. 测试修复：`ai::complete_ollama_allows_missing_key` 原以 `key=None` 回退读真实
    `%APPDATA%/SuperTask/secrets.env`（真机配置 key 后测试即挂，环境依赖缺陷）；
    改传 `Some("")` 保持「空 key」语义并消除环境耦合。
11. Phase 6.1 增补 `tests/real_ai_smoke.rs`（`#[ignore]` opt-in）：读真实 appdata 默认配置
    + secrets key，各发一次 explain_logs / config_suggest（消耗真实配额，默认跳过）。
    **2026-08-29 已实跑通过**（本地真实端点，model `gpt-5.6-luna`，25.5s）：日志解释给出
    BindException 排查方向，配置建议返回参考 yaml；用量 +2。§6.2 真机验收中的「真实 AI
    端点冒烟」就此完成，剩 GUI 真机人工验收与中文 README 真工程导入链路。
12. mock 冒烟（浏览器）：discover README 导入向导（provenance 徽标/脚本草稿/冲突建议列/
    端口提示/噪声提示/勾选应用 3 项）与命令面板三入口（AI 解释未配置 → `AI_NOT_CONFIGURED`
    toast；配置后回文显示 200 行解释；README 导入 `?readme=1` 直达；AI 设置导航）全部实测通过。
