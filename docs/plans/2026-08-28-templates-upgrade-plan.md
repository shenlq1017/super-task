# 模板模块升级计划（审计 + 分阶段实现）

日期：2026-08-28
状态：**Phase 0–4 已实现（2026-08-28），Phase 5 部分落地**。实现摘要：
- Phase 0：`template_assets/*/template.yaml` 数据驱动清单，嵌入目录扫描发现，manifest↔资产双向一致性测试兜底。
- Phase 1：`TemplateSourceKind`（builtin/local）+ `LocalDirSource`（`%APPDATA%/SuperTask/templates/<id>/`），list 标注 `source/invalid/invalid_reason`，冲突 id 跳过（`TEMPLATE_ID_CONFLICT`）。
- Phase 2：内置模板 2 → 5 套（新增 `spring-boot-single`（module "." 路径）、`node-fullstack`、组合模板 `spring-node-combo`）。
- Phase 3：`params` 声明式参数（`{{key}}` 文本替换 + `apply_to: [yaml.name]`），错误码 `TEMPLATE_PARAM_MISSING/UNKNOWN`。
- Phase 4：`blocks` 组合引擎（依赖自动闭合、端口分配 + `{{port}}` 占位替换、族内端口查重）、`templates.preview` 纯计算 IPC、前端组合向导（勾块 → 端口 → 预览 → 创建）。
- 偏差记录：local 模板未实现 list 时 sha 缓存比对（改为 create 时清单↔文件集合双向校验，保证等价：落盘内容 = 创建时刻磁盘内容）；`templates.rescan` 未做（list 每次现扫）。
- 向导交互落地为创建卡内分区（勾块/端口/预览），而非独立 5 屏——底座选择即模板卡片选择。
- `cargo test -p supertask-core` 204 单测 + 集成全绿；`cargo check -p supertask`、`npm run build` 通过（logs-page 存在与本计划无关的外部在改内容）。

前置调研：本文件 §1 审计结论基于 2026-08-28 代码现场（template.rs 615 行、templates-page.tsx 347 行、v1-1 规格 §4 已交付）。

---

## 1. 现状审计

### 1.1 已交付（v1-1 规格 §4，完成度高）

| 层 | 现状 | 位置 |
|----|------|------|
| core | 2 套内置模板编译期 `include_dir!` 嵌入；manifest 硬编码于 Rust + 逐文件 sha256；`create_template` 管线完整（目录名校验 → 目标非空拒绝 → 先校验后落盘 → 注入 `templates:` 保留段 → parse 复核 → 存在性校验）；8 个单元测试 | `crates/supertask-core/src/template.rs`、`template_assets/` |
| IPC | `templates.list` / `templates.create`（长操作走 operation 事件流，成功返回 `workspace_id`） | `src-tauri/src/commands.rs` L795-844、`frontend/src/ipc/protocol.ts` L478 |
| 前端 | 模板卡片 + 文件概览 + 目录选择（前端预检与后端同语义）+ 进度卡片 + 成功自动打开工作区 | `frontend/src/pages/templates-page.tsx` |
| 注册 | features 后端标记 Live since 1.1；前端导航由注册表驱动 | `crates/supertask-core/src/features.rs` L25 |

### 1.2 缺口（按严重度排序）

1. **来源硬编码**：模板清单写死在 `builtin_manifests()`（Rust 常量），新增模板必须改代码重编译；`include_dir` 嵌入与 manifest 双份维护，靠测试对账。没有 `TemplateSource` 抽象，用户/本地模板无从接入。
2. **模板领域窄**：仅「Spring 多模块 + Node」两套。缺纯 Node、Spring Boot 单模块（`module: "."` 路径从未被模板覆盖）等常见起步形态。
3. **与「扫描 / 导入」没有形成体系**：工作区有三个来源（模板从零创建 / 扫描存量项目 / 导入 compose·Taskfile），但模板页只做自己的事，产品上没有一个统一的「新建工作区」入口叙事。
4. **无参数化**：创建时不能改项目名/端口，产物一律用模板原值；`supertask.yaml` 的 `name`、服务 `port` 无法在创建时定制。
5. **文档滞后**：UI 设计文档（2026-08-26-ui-design）仍把模板页写成 ComingSoon 占位；v1-1 之后没有模板页实装设计稿。
6. 小项：创建成功后只 `openWs`，无「下一步」引导；模板内 `supertask.yaml` 与 yaml.md 字段规范的符合性没有显式测试（当前靠 parse 复核兜底）。

---

## 2. 设计定位与原则

**模板模块的定位：工作区三大来源之一（从零创建），与扫描（存量项目）、导入（compose 1.3 / Taskfile 1.4）并列。**

延续既有拍板，不推翻：

- 一等公民是 service，模板的最终产物就是 **`supertask.yaml` + 骨架文件**，模板不引入第二套配置体系。
- lazy / Karpathy 原则：本地目录就是模板库，不上数据库、不做远程市场（v1-1 明确排除，本计划仍排除）、不内嵌运行时。
- 安全规则不放松：单层目录名、禁 `..`/UNC/盘符、目标非空拒绝、失败不落盘、禁止假成功。
- `templates:` 保留段语义不变（`source/id/version`），新增字段一律向后兼容。

**扩展性核心决策：把「模板来源」抽象成 trait，内置模板降级为该 trait 的第一个实现。**

```
TemplateSource（core trait）
├── BuiltinSource    编译期 include_dir（现有资产不动，manifest 改为读 template.yaml）
└── LocalDirSource   用户模板目录（appdata/templates/<id>/template.yaml + 文件）
```

所有模板统一为「一个目录 + 一份 `template.yaml` 清单」：builtin 与 local 走同一条创建管线，逻辑只剩一条。

---

## 3. 目标架构

### 3.1 模板清单规范 `template.yaml`

每个模板目录根放一份 `template.yaml`（builtin 与 local 同格式）：

```yaml
id: spring-node-basic          # 全局唯一；local 与 builtin 冲突时 builtin 胜，local 报 TEMPLATE_ID_CONFLICT 跳过
version: "2"
name: Spring + Node 基础
description: 单模块 Spring Boot + 前端 Node
stacks: [spring-boot, node]
# 可选：创建参数（Phase 3）
params:
  - key: project_name
    label: 项目名
    required: true
    apply_to: [yaml.name]       # 声明变量落到哪里，core 按声明替换，不做自由模板引擎
```

- builtin：`template.yaml` 随 `include_dir` 嵌入，manifest 从 Rust 常量改为运行时解析该文件（**先校验后使用**，解析失败 = `TEMPLATE_INVALID`，测试兜底防 shipped 即坏）。
- local：`template.yaml` + 同目录文件；sha256 不预存，list 时现算（本地文件用户可改，校验目标是「创建落盘的内容 = list 时看到的内容」，在 create 前重算并比对）。
- 模板内 `supertask.yaml` 必须通过 `parse_yaml` + 字段规范校验（显式测试，见 Phase 0）。

### 3.2 创建管线（不变式保持）

```
list   = sources.map(list) → 按 source 标注 → 去重（builtin 优先）
create = 查模板（含 source）→ 目录名校验（现有黑名单不动）
       → 父目录 canonicalize → 目标不存在/空检查
       → 变量替换（Phase 3，无 params 时跳过）
       → 逐文件 sha 比对 → 落盘 → 注入 templates: {source, id, version}
       → parse_yaml 复核 → 关键文件存在性校验
```

任何一步失败：已写文件保留（v1-1 语义），返回结构化错误码。

### 3.3 错误码（在 v1-1 五个基础上新增）

| 码 | 场景 |
|----|------|
| `TEMPLATE_INVALID` | manifest/资产校验失败（已有，覆盖 template.yaml 解析失败） |
| `TEMPLATE_ID_CONFLICT` | local 模板 id 与 builtin 冲突 |
| `TEMPLATE_PARAM_MISSING` | required 参数缺失（Phase 3） |
| `TEMPLATE_PARAM_UNKNOWN` | apply_to 目标字段不存在（Phase 3） |

### 3.4 IPC 演进（全部向后兼容）

- `templates.list` → 返回项增加 `source: "builtin" | "local"`、`params?`（旧前端多收字段无害）。
- `templates.create` → `directory_name` 之外新增可选 `params: Record<string,string>`。
- 新增 `templates.rescan`（local 目录变更后重扫；也可降级为 list 每次现扫，先不做）。

### 3.5 前端信息架构

模板页升级为「**新建工作区**」叙事（不改路由名，改页面内结构）：

```
/templates
├── 来源分段控件：官方模板 | 本地模板（有 local 时才出现）
├── 模板卡片网格：name / description / stacks 徽章 / version / source 徽标 / 文件概览
├── 创建向导（Phase 4）：底座 → 勾选服务块 → 参数/端口 → 预览 → 创建
│   （无 blocks 的模板退化为单步：选模板 → 填目录 → 创建，即现行流程）
└── 底部入口区：扫描已有项目 → /discover；导入 compose / Taskfile → 对应版本 soon 徽标
```

成功后仍自动 `openWs` 进运行页；模板可在 `template.yaml` 加 `hint`（创建后提示一句，如「先执行 mvn install」），运行页顶部作为一次性提示条展示（Phase 3 可选）。

---

## 4. 分阶段实现计划

> 每阶段独立可交付、测试绿、不破坏既有行为。建议排序：Phase 0 随当前版本收尾即可做（纯内部重构）；Phase 1–3 建议归入 v1.5 规划（1.3 compose、1.4 平台导入优先级更高）；Phase 4 是文档收尾，随最后实现阶段走。

### Phase 0 — 内部重构：manifest 数据化（无行为变化）

目标：消除双份维护，是后续一切的地基。

1. 为两套 builtin 模板补写 `template_assets/*/template.yaml`。
2. `template.rs`：新增 `TemplateManifest` 解析（serde），`builtin_manifests()` 改为从嵌入资产读取解析；字段与现硬编码逐一等价。
3. 保留 manifest↔资产双向一致性测试；新增「template.yaml 解析失败 → TEMPLATE_INVALID」测试（用一个损坏 fixture）。
4. 新增模板内 `supertask.yaml` 规范符合性测试（parse + services 字段断言）。
5. `cargo test -p supertask-core` 全绿；IPC/前端零改动。

### Phase 1 — TemplateSource 抽象 + LocalDirSource

目标：用户模板可接入，扩展点落地。

1. 定义 `trait TemplateSource { fn id(&self)…; fn list(&self) -> Vec<TemplateSummary>; fn read(&self, tpl, path) -> Result<Vec<u8>> }`；`BuiltinSource` 包装现有实现。
2. `LocalDirSource`：扫描 `appdata/templates/`（Tauri 壳负责传路径，core 不依赖 Tauri）；目录缺 `template.yaml` → 跳过并在 list 结果里带 `invalid: true` + 原因（UI 显示「清单损坏」，不崩溃不假成功）。
3. `create_template` 改为按 source 分发；local 创建前重算 sha 比对。
4. IPC：list 增 `source`；`templates.create` 增 `source` 可选参（默认 builtin）。
5. 前端：来源分段控件 + local 卡片「清单损坏」态；mock 同步两条 local 样例（一好一坏）。
6. 错误码：`TEMPLATE_ID_CONFLICT`。测试：local 正常创建、冲突跳过、清单损坏展示、sha 变更后 create 拒绝。

### Phase 2 — 模板扩充

目标：覆盖常见起步形态，模板数量 2 → 5。

1. `spring-boot-single`：单模块 Spring Boot（`module: "."`，顺带覆盖省略 `-pl` 的命令生成路径）。
2. `node-fullstack`：纯 Node 双服务（api + web，`depends_on` + tcp 健康）。
3. `spring-node-basic` 瘦身版：现有 minimal 改名归位（保持 id 不变，仅调 description）。
4. 每套模板：`template.yaml` + `supertask.yaml` 过规范测试 + 文件概览与 UI 徽章核对。
5. mock 的 `MOCK_TEMPLATES` 同步。

### Phase 3 — 参数化与创建后引导

目标：创建时可定制，产物不再是「一次性原值」。

1. `template.yaml` 支持 `params`（仅 `project_name` → `yaml.name` 与模板内出现 `{{project_name}}` 的文本文件；端口参数化暂不做——改端口已有运行页「改端口并重启」链路，避免两处入口）。
2. 替换发生在落盘前的内存内容上；`{{...}}` 未声明变量原样保留并在 list 时给 warning 字段。
3. IPC `create` 增 `params`；错误码 `TEMPLATE_PARAM_MISSING` / `TEMPLATE_PARAM_UNKNOWN`。
4. 前端创建表单按 `params` 动态渲染输入；实时预览目标路径与项目名。
5. 可选：`template.yaml` 的 `hint` 字段 → 创建成功 toast / 运行页一次性提示条。
6. 测试：替换成功、缺参拒绝、未知 apply_to 拒绝、无 params 模板行为与 Phase 2 完全一致。

### Phase 4 — 组合式模板（多服务向导）

目标：解决单模板组合爆炸——底座 × 服务的每种搭配不再各做一套模板，用户在向导里拼装。复用系统的既有心智「先预览后落盘」（与扫描 → 草稿 → merge preview → apply 同构）。

**数据模型**：`template.yaml` 增加 `blocks`（模板族 = 一个底座 + 可组合的服务块）：

```yaml
id: spring-node-family
skeleton: maven-multimodule        # 底座：决定工程结构，块文件路径相对底座
blocks:
  - id: backend
    label: Spring Boot 后端
    kind: spring-boot
    requires: []                    # 依赖的其他块 id
    provides: ["api"]
    default_port: 8081
    files: [backend/pom.xml, backend/src/...]
    yaml:                           # 落入 services.backend 的片段
      kind: spring-boot
      module: backend
      health: { type: http, http: "http://127.0.0.1:{{port}}/actuator/health" }
  - id: web
    label: Node 前端
    kind: node
    requires: ["api"]
    default_port: 5173
    files: [web/...]
```

**组合规则（向导 ≠ 自由拼装）**：

1. 块只在模板族（同一底座）内组合，跨底座拼装不做；
2. 依赖自动闭合：勾选块时 `requires` 未满足 → 自动带上依赖块并 UI 说明，不报错；
3. 端口向导内分配：每块 `default_port` 预填、勾选时族内查重、可手改；只管「本次生成物内部不冲突」，不碰运行时端口检查链路；
4. 块文件自包含：跨服务连接信息走 yaml `env`（如 web 的 `API_BASE`），不做代码级生成耦合；
5. 未选块的文件一律不生成（不用 `enabled: false` 占位，避免空目录干扰）。

**IPC**（对齐 merge 的 preview/apply 模式）：

- `templates.preview`：入参 `template_id + selected_blocks + ports + params`，纯计算返回「将生成的 services 片段 + 文件清单 + 警告」，无副作用；
- `templates.create` 增可选 `blocks/ports`（缺省 = 全块，行为与 Phase 3 完全一致）；
- 新错误码：`TEMPLATE_BLOCK_DEP`（依赖无法闭合）、`TEMPLATE_BLOCK_PORT`（族内端口冲突且未修正）。

**向导交互（5 步）**：选底座（模板族）→ 勾选服务块（依赖自动带出）→ 参数 + 端口核对 → 预览（services 表 + 文件数 + 警告）→ 创建（现有管线：目录校验 → 非空拒绝 → 校验后落盘 → 注入保留段 → parse 复核）。

**测试**：单块创建=旧行为、依赖自动闭合、族内端口查重与手改、preview 无副作用、块 yaml 片段合并后过 parse + 规范测试、块缺失文件 → `TEMPLATE_INVALID`。

### Phase 5 — 文档与规格收尾

1. 新写 `docs/plans/2026-MM-DD-v1-5-templates-spec.md`（或并入 v1.5 规格）：模板来源、template.yaml 规范、params、错误码表、验收标准。
2. `docs/spec/ipc.md` §10.1：补 `source`/`params`/新错误码。
3. `docs/spec/yaml.md`：`templates:` 保留段说明补 `source: local` 取值。
4. UI 设计文档：替换 ComingSoon 描述为实装设计（卡片网格 + 来源分段 + 创建表单 + 底部入口区）。
5. AGENTS.md 决策摘要追加一行：模板来源抽象与「不做远程市场」的重申。

---

## 5. 验收标准（全阶段完成后）

1. `templates.list` 能同时列出 builtin + local 模板并标注来源；local 清单损坏只影响自身条目。
2. 从任意来源创建工作区：目录名非法/目标非空/资产校验失败时，错误码语义正确且不落盘不假成功（沿用 v1-1 用例并参数化到两个 source）。
3. 新增一个模板 = 往目录放文件，不改一行 Rust（local 路径演示）。
4. `params` 创建后 `supertask.yaml` 的 `name` 已替换，`templates:` 保留段正确，merge round-trip 不丢。
5. 前端：来源切换、创建、进度、失败、成功自动打开全链路 mock 与 Tauri 双通。
6. 组合向导：勾选块 → 依赖自动闭合 → 端口查重 → 预览 services 与最终落盘的 `supertask.yaml` 一致；单块/全块默认路径与 Phase 3 行为逐字节一致。
7. `cargo test -p supertask-core`、`npm run build`、`cargo check -p supertask` 全绿。

## 6. 明确不做

- 远程模板市场 / 在线模板编辑 / 第三方远程源（v1-1 拍板，继续排除）。
- 模板引擎级变量替换（循环/条件/过滤器）；只做声明式 `{{key}}`。
- 跨底座的服务块拼装（块只在模板族内组合）。
- 块间代码级生成耦合（跨服务连接只走 yaml `env`，不做代码生成）。
- 端口参数化接入运行时检查（向导内只做生成物内部查重；运行期仍走运行页改端口链路）。
- 创建后自动 `git init` / 自动扫描（v1-1 明确不自动执行）。
- 模板依赖安装（`npm install` 等仍由用户在工具链页/终端完成）。
