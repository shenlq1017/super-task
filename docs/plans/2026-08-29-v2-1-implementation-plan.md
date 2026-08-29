# v2.1 实现计划（智能：README 导入 / AI 助手）

> 2026-08-29。依据：[2026-08-29-v2-1-feature-spec.md](2026-08-29-v2-1-feature-spec.md)（规划稿，随拍板同步修订）。
> 前置：v2.0 收尾基线（若 2.1 先行，ureq 由本期引入，Phase 2 任务不变）。
> 执行约定：先读 `<user-home>\.agents\skills\executing-plans-0.1.0\SKILL.md`；前端任务点名 skill。

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
| 5.3 | AGENTS.md 当前阶段 + 规范真源；inv-1 交付表回改 | living 文档 | 盘点=当前事实 |
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
