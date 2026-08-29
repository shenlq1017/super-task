# v2.1 功能规格：智能（README 导入 / AI 助手）

> 2026-08-29。状态：**规划稿（待评审拍板；拍板后更新本行与 AGENTS.md 当前阶段）**。
> 实现计划：[2026-08-29-v2-1-implementation-plan.md](2026-08-29-v2-1-implementation-plan.md)。
> 一句话：**不联网也先能用——README / 命令文档确定性导入生成 `supertask.yaml` 草稿（人确认后写盘）；AI 助手用用户自己的 OpenAI 兼容 Key，只建议、给参考、绝不自动改。**

---

## 1. 背景与版本序列

- 对应 roadmap 原需求 12（README 生成部署 YAML）与 11（AI），2.1 主题「能读文档 / 能问 AI」。
- 前置：v2.0 收尾基线。
- **与 v2.0 的关系：无硬功能依赖**——README 导入完全离线；AI 的 key 是本地 secret，不走账号体系。roadmap 排序（2.0 → 2.1）是产品排序而非技术依赖；唯一共享物是 HTTP 客户端 `ureq`（v2.0 引入；若 2.1 先行则由 2.1 引入）。

## 2. 目标与非目标

**目标**：

1. README / 命令文档 → `DraftPlan`（**确定性规则引擎，非 LLM**），与文件系统扫描融合，经既有 merge 向导**人确认后写盘**；
2. AI 助手三场景：解释日志 / 配置建议 / 草稿增强（试验性）；
3. AI provider 配置（OpenAI 兼容端点 + 用户自有 key）；
4. 数据卫生：secret 永不进 prompt、上下文截断、用量可见、零后台调用。

**非目标**：

- 未经确认自动写 yaml（roadmap 原话「人确认后写入」是硬约束）；
- 后台 / 定时自动调用 AI（只有用户点击才发起请求）；
- 自研模型、内置代理服务、官方推理端点（直接请求用户配置的 base_url）；
- 流式输出（本期非流式；流式列为后续候选）；
- 把 MCP 重复一遍（1.5 已交付 `supertask mcp`，IDE 侧 agent 起停服务已可用；本期 AI 是**应用内**助手）；
- 解析非 Markdown 的 README（.txt/.rst 不做）。

## 3. README 导入器（确定性规则引擎）

### 3.1 输入与发现

- 工作区根目录下：`README.md` > `readme.md` / `README.markdown`（大小写不敏感的 `.md`/`.markdown`）。显式指定路径不存在 → `README_NOT_FOUND`；未指定且未发现 → 空草稿 + 人话提示（非错误）。
- 编码：沿 core 既有解码策略（UTF-8 优先、GBK 兜底、lossy 替换）。

### 3.2 命令抽取

- **fenced code block**（```sh / bash / shell / console / 无语言标注）内逐行解析；**行内 code**（单反引号）中的单条命令。
- 链式拆分：`&&`、`;`、行尾 `&` 拆成独立命令；`VAR=value cmd` 剥离前缀赋值并提取为 env 提示。
- `export PORT=8080` 类语句 → 端口提示（进端口建议，不直接写 port 字段）。

### 3.3 命令分类（规则表，全部确定性）

| 类别 | 匹配模式（示例，非穷举） | 产出 |
|------|--------------------------|------|
| service 候选 | `mvn … spring-boot:run`、`<npm\|pnpm\|yarn> run <dev…>`、`gradle bootRun`、`python <file>`、`python -m <module>`、`uvicorn` / `gunicorn …`、`go run …`、`deno run`、`docker compose up` | 服务草稿（kind 由命令 + 目录文件特征共同推断） |
| script 候选 | `mvn package`、`<pm> install` / `build` / `test`、`pip install …`、`go build` / `test`、`docker compose build` / `down` | scripts 草稿（cmds 只来自文档，写入前人确认） |
| 忽略 | git / cd / mkdir / curl / echo / 无法解析行 | —（计入 warning 摘要「N 条命令未识别」） |

- **章节加权**：标题匹配 `Run|Getting Started|Quick Start|Development|启动|快速开始|运行` 的章节内命令置信度 ×2；`Install|安装` 章节内 scripts ×1.5。中英文标题同权。
- **去重**：归一化 argv（剥变量赋值/注释）相同者取首个上下文。
- 每条产出带 `confidence`（高/中/低）与 `provenance: readme`。

### 3.4 与文件系统扫描的融合

- **文件系统事实优先**（README 可能过时）：scan.rs 已识别的服务（pom/package.json/pyproject/go.mod/compose）为骨架；README 命令只**补全** entry / script / extra_args / 端口提示。
- 字段冲突（如 scan 推断 `entry: main.py`、README 写 `app.py`）：取 scan 值，README 值进「建议」列，向导中双值可见、provenance 标注（scan / readme / 默认）。
- 产出 `DraftPlan` 与 `scanPreview` 同形（复用 1.1 merge 向导与 `scanApply` 应用链）。

### 3.5 确定性与可测性

- 纯函数、零网络、零 LLM。golden fixtures：Spring+Node 混合、Python（uvicorn/manage.py）、Go、中文 README、纯噪声命令、无 README 六类。

## 4. AI 助手

### 4.1 Provider 配置

- 配置存 **appdata（应用级）**：`{base_url, model, timeout_secs, max_tokens}`。OpenAI 兼容 `POST {base_url}/chat/completions`。
- API key 存 secrets 后端**固定 id `supertask.ai`**（backend=local；沿 1.2 secrets 基建）。
- **`supertask.ai` 永不入云 vault**（即使 v2.0 开启密钥同步并勾选——它是账号凭证类密钥，v2.0 规格 §7 已硬排除）。
- HTTP 客户端复用 v2.0 的 ureq；AiClient trait 化 + fake 注入（沿 FakeRunner 先例），单测零真实网络。

### 4.2 三个场景

1. **解释日志**（运行页 / 日志页）：取当前筛选下 tail ≤ 200 行且 ≤ 32 KB + 服务上下文（kind / port / 状态）→ prompt → markdown 面板展示。不写任何东西。
2. **配置建议**（配置页）：当前 yaml（**sanitize 后**，见 4.3）+ 校验 warning / 错误列表 → 返回建议文本 + 建议 yaml 全文（参考稿）。「应用」= 建议 yaml **整段填入编辑器**（不做结构化 patch，避免半解析状态）；**保存仍走用户点击 + base_hash 冲突检测**，链路与手工编辑完全一致。
3. **草稿增强**（试验性，UI 标注）：扫描 / README 草稿预览上「AI 增强」soft 按钮——对服务排序、补端口 / 健康检查建议；只改预览不落盘。

### 4.3 数据卫生

- **sanitize**：yaml 与日志在进入 prompt 前掩码——secret 值、`supertask.ai` key、形似 token / password / authorization 的行 → `<redacted>`。有单测断言原始值不出现在请求体。
- **预算**：请求前按字符数粗估 token（÷4），超限 → `AI_CONTEXT_TOO_LARGE`，UI 提示缩小日志范围。
- **用量**：每次调用计数（appdata 计数器，按日）；/ai 页与调用按钮 tooltip 可见当日次数。
- **零后台调用**：仅用户显式点击触发；无定时 / 无自动重试。

### 4.4 错误路径

未配置 → `AI_NOT_CONFIGURED`（引导去 /ai）；HTTP 非 2xx → `AI_REQUEST_FAILED`（透传服务端 message，截断展示）；超时 → `AI_TIMEOUT`；上下文超限 → `AI_CONTEXT_TOO_LARGE`。

## 5. IPC 契约增量（ipc.md 增 §10.13）

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| import.readme | `{path?}` | `DraftPlan`（与 scanPreview 同形；含 provenance/confidence） |
| ai.configure | `{base_url, model, timeout_secs, max_tokens, api_key?}` | api_key 写入 secrets 后端，不回显 |
| ai.status | — | 配置摘要（key 是否已设，不回明文）+ 当日用量 |
| ai.complete | `{task, payload}` | `{text, usage}`；task ∈ explain_logs / config_suggest / enrich_draft |

- `features.rs`：ai → `Live`（since 2.1）；`SOON_COMMANDS` 移除 `ai.complete`（清单清空，`reject_soon_command` 保留机制本身）。

## 6. 错误码汇总

| 码 | 场景 |
|----|------|
| `AI_NOT_CONFIGURED` | 未配置 provider 或未设 key |
| `AI_REQUEST_FAILED` | 端点返回非 2xx |
| `AI_TIMEOUT` | 超过 timeout_secs |
| `AI_CONTEXT_TOO_LARGE` | 上下文超预算 |
| `README_NOT_FOUND` | 显式指定的 README 路径不存在 |

## 7. 前端

- **/ai**（扩展组，soon → live）：provider 配置卡（base_url / model / key / timeout，保存 = success 变体）+ 连接测试（soft 按钮，发一次最小请求）+ 用量卡 + 隐私说明（「数据只发往你配置的端点；密钥永不进入提示词」）。
- **/discover**：「从 README 导入」入口（outline 按钮）→ merge 向导（provenance 徽标 + 置信度展示）。
- **log-view**：右区固定槽位新增可选动作「AI 解释」——以 slot prop 注入（`extraActions?`），运行页与日志页共用组件**零分叉**；遵循工具栏分区约定（右侧操作区、Tooltip 1000ms）。
- **/config**：warning / 错误区旁「AI 建议」soft 按钮 → 建议卡（markdown 文本 + 建议 yaml 参考稿 + 「填入编辑器」outline 按钮）。
- 命令面板：「从 README 导入」「AI 解释当前日志」「打开 AI 设置」。
- 四语 + parity；mock 模式补 fake AI（确定性回文）。

## 8. Phase 划分（概览）

1 core：README 导入器 → 2 core：AI 模块 → 3 壳层 IPC + feature 转 live → 4 前端 → 5 文档闭环 + 回归 → 6 验收。（Phase 1 与 2 完全独立，可并行。）任务级拆解见实现计划。

## 9. 验收标准（场景矩阵）

1. 无 README 的工作区：discover 入口给人话提示（非错误弹窗），无崩溃。
2. Spring+Node 混合 fixture：生成服务草稿（`mvn spring-boot:run` / `npm run dev`）与 scripts（build/install），置信度与 provenance 正确。
3. Python / Go fixture：`uvicorn`、`python -m`、`go run` 识别为服务；`export PORT=8000` 进端口建议。
4. 中文 README（「快速开始 / 启动」标题）：章节加权生效，识别结果与英文等价。
5. README 与文件系统冲突：取 scan 值、README 值进建议列；向导中 provenance 可见。
6. 应用草稿：走既有 merge 向导 → 确认 → 写盘带 base_hash；未确认不落任何文件。
7. AI 未配置：三个入口统一 `AI_NOT_CONFIGURED` 并引导 /ai。
8. /ai 配置 + fake server：连接测试通过；`ai.complete` 返回文本；当日用量 +1。
9. 日志解释：请求体断言 sanitize（secret 值 / key 不出现）；超长日志截断到 200 行 / 32 KB；`AI_TIMEOUT` 路径可演示。
10. 配置建议：「填入编辑器」后编辑器内容更新但**未保存**；用户保存走既有 YAML_CONFLICT 检测。

## 10. 风险与开放问题

- **README 质量参差**是主要不确定源：导入器只匹配高置信命令模式，低置信全部降级为「建议」列；人工确认是最后闸门。
- **AI 建议质量**：只建议不落盘 + 参考稿整段填入（不做结构化 patch）规避半解析风险。
- **用户 key 成本**：零后台调用 + 用量可见 + 每次手动触发。
- 开放问题（实现期定，不阻塞）：草稿增强是否默认隐藏（倾向默认展示但标注试验）；`/ai` 页是否同时承载「MCP 使用说明」（倾向是，纯文档卡）。
