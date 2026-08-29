# SuperTask 1.5 功能规格

> 日期：2026-08-29  
> 状态：草案——默认决策已给出（§15），待确认后转「待实现」（前置：1.4 交付或明确裁剪）  
> 上位文档：[产品路线](2026-08-25-product-roadmap.md) · [1.4 功能规格](2026-08-28-v1-4-feature-spec.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [引擎架构](../spec/architecture.md)

本文把路线中的 1.5「可搬」收到可实现、可测试、可交付的粒度。1.5 不加新的服务类型，而是把 1.0–1.4 的单引擎能力开放给 GUI 之外的三类入口：**终端（CLI）**、**Agent/编辑器（MCP）**、**离线迁移（导出包）**；同时补上多入口并存的治理基础——**工作区所有权锁**。一句话：同一份 `supertask-core`，四个前端（桌面 / CLI / MCP / 包迁移），一个 owner。

## 1. 目标与边界

### 1.1 产品目标

1. **能脚本**：`supertask up/down/status/logs` 在无 GUI 环境完成起停与健康等待，CI 可用退出码做门禁。
2. **能给 Agent**：Cursor / Claude 等编辑器通过 MCP 工具起停服务、读日志、跑脚本。
3. **能搬**：工作区配置打包成 zip，换机/换平台离线迁移；密钥默认不进包。
4. **不打架**：桌面、CLI、MCP 同时存在时，同一工作区只有一个 owner，冲突说人话（`WORKSPACE_LOCKED`）。

### 1.2 版本范围

| 能力 | 1.5 行为 |
|------|----------|
| CLI | 新 crate `supertask-cli`（bin `supertask`）：up / down / restart / status / logs / script / doctor / version |
| CI 形态 | `supertask up --wait healthy -- <command>` 包装模式，退出码透传 |
| MCP | `supertask mcp` 子命令，stdio 传输，7 个工具（起停/状态/日志/脚本） |
| 导出包 | zip（manifest + supertask.yaml + 可选密钥文件），`supertask export/import` + 桌面 UI 入口 |
| 工作区锁 | `<root>/.supertask/engine.lock`（pid + holder），新错误码 `WORKSPACE_LOCKED` |
| 平台 | 三平台（沿用 1.4 matrix）；CLI/MCP 不引入新平台差异 |
| YAML / IPC | `version: 1`、protocol 1、app data v3 均不变；IPC 新增 2 条命令（§8） |

### 1.3 明确不做

以下能力不进入 1.5：

- **守护进程 / `up -d` 后台存活**：`up` 是附加（attached）模型，CLI 退出即按进程树规则清场（Job Object / PDEATHSIG）。后台常驻与远程控制是独立架构决策，2.x 另立项。
- **跨进程 stop**：不允许 A 进程停掉 B 进程（桌面/CLI/MCP）持有的服务；对被持有的工作区发号施令返回 `WORKSPACE_LOCKED`，不提供后门。
- MCP 的 network/SSE 传输、resources、prompts、鉴权（1.5 仅 stdio 本地，tools only）。
- 云同步、账号、一键迁移（2.0）；导出包只是其离线前置。
- CLI 内置 YAML 编辑器（改配置走文件与桌面 GUI）；`supertask scan`（扫描向导是 GUI 交互）；`supertask build`（jar 构建继续走 GUI / `launch: jar`）。
- Windows 服务 / systemd 单元 / 开机脚本集成。
- 新增服务 kind、新语言、新平台（沿用路线版本 gating）。

Windows 既有场景零回归仍是合入门槛；1.5 对既有流程的唯一显式变更是「同工作区多 owner 从静默竞争变为明确拒绝」（§3.1），此项为正确性修复，随规格一起确认。

## 2. 用户场景与成功标准

### 2.1 CI（无 GUI）

1. GitHub Actions：checkout 代码后执行 `supertask up --wait healthy -- mvn verify`。
2. CLI 按拓扑启动全部服务，等全部健康后 spawn `mvn verify`（继承 stdio），子命令结束即停止全部服务，并以子命令退出码退出。
3. 失败可诊断：启动失败 / 健康超时 → 停止全部、退出码 1，stderr 列出未达标服务与错误码（`MISSING_TOOL`、`PORT_CONFLICT` 等沿用现有码）；`--json` 可机器解析。
4. 无论成功、失败、被取消（SIGTERM/CTRL_C），流水线结束后 `pgrep` / `tasklist` 无残留 `java`/`node`。

### 2.2 Cursor / Agent（MCP）

1. 编辑器 MCP 配置指向 `supertask mcp`（cwd = 项目根）。
2. Agent 依次调用：`supertask_status`（快照）→ `supertask_start`（起依赖）→ `supertask_logs`（读启动日志定位端口）→ 自己跑测试 → `supertask_stop`。全程无 GUI。
3. 编辑器重载/断开 stdio → MCP 进程停止全部服务、释放锁并退出；无孤儿进程。该行为在文档明示（防孤儿优先于会话连续性）。
4. 桌面已打开同一工作区时，MCP 可变工具返回 `WORKSPACE_LOCKED`（details 带 holder=desktop 与 pid），只读工具（status/logs）仍可用。

### 2.3 换机迁移（导出包）

1. 旧机（Windows）：`supertask export --with-secrets -o D:/pkg.zip`，两次确认后密钥文件入包。
2. 新机（macOS）：`git clone` 代码，`supertask import pkg.zip --dest repo/`，随后 `supertask up`（或桌面打开）即可运行。
3. 默认（不带 `--with-secrets`）包内只有 manifest + `supertask.yaml`；`.env.local` 等 secrets / env 文件、`.supertask/` 日志历史、`.git` 一律排除。
4. 导入目标目录已有 `supertask.yaml` → `PKG_TARGET_EXISTS`，不覆盖；包损坏 / 路径逃逸 / 哈希不符 → `PKG_INVALID`。

### 2.4 多入口并存

1. 桌面开着工作区 A，终端 `supertask up`（同 A）→ 立即报 `WORKSPACE_LOCKED`（holder=desktop），不双启、不抢端口。
2. 终端 `supertask up` 挂着工作区 A，另开终端 `supertask status` → 只读可用，显示「owner=cli (pid 1234)」；`supertask logs` 读历史文件同样可用。
3. 上一持有进程崩溃留下的 stale 锁 → 新进程检测 pid 已死，自动接管并重写锁文件。

## 3. 总体架构（多前端单引擎）

```text
┌──────────────┐   ┌────────────────────────────────────────┐
│ src-tauri    │   │ crates/supertask-cli（bin: supertask） │
│ 桌面壳（不变）│   │  up/down/restart/status/logs/script     │
└──────┬───────┘   │  doctor/export/import/mcp/version      │
       │           └───────────────┬─────────────┬──────────┘
       │                           │  (mcp 子命令) │
┌──────▼───────────────────────────▼─────────────▼──────────┐
│ supertask-core（唯一引擎，业务零分叉）                       │
│  + workspace 所有权锁（lock.rs，新）                        │
│  + pkg 导出/导入（pkg.rs，新：manifest/zip/校验）            │
└────────────────────────────────────────────────────────────┘
```

### 3.1 工作区所有权（`engine.lock`）

- 现状问题：桌面、CLI、MCP 各自 host 引擎实例，同一工作区两个 owner 会双启服务、争端口、YAML 写互相覆盖。1.5 引入文件锁治理。
- 锁文件：`<root>/.supertask/engine.lock`（JSON：`{ pid, holder: "desktop"|"cli"|"mcp", started_at_ms }`），与 `.supertask/logs` 同级；`.supertask/` 建议整体 gitignore（写入 README/spec）。
- 获取规则：创建即占（独占创建语义）。**同 pid 重入允许**（桌面重开同路径、进程内测试复用）；他 pid 持有且存活 → `WORKSPACE_LOCKED`（details 带 holder/pid）；持有 pid 已死 → 视为 stale，清理后接管。
- 释放：close / detach（切工作区）/ 进程退出时尽力释放；stale 探测是最终兜底，不依赖优雅退出。
- 获取时机：桌面 `app.load`（打开工作区）、CLI/MCP **首个可变动作**（up/down/restart/script run）；只读路径（`status`、`logs` 历史、`export`、`doctor`）不取锁。
- 锁只记录 pid 与 holder 标签，不杀任意 pid、不暴露 pid 参数接口（安全边界沿用 §9）。

### 3.2 分层职责与守恒规则

- 业务只在 `supertask-core`；CLI 与 MCP 是壳级前端，适配层不写业务闭包（与 src-tauri 同一规则）。
- 三前端共享同一错误码表、同一 YAML 语义、同一日志存储；CLI `--json` 直接序列化 core 输出结构（RuntimeSnapshot 等），**不另造第二套模型**。
- MCP 壳层允许 tokio（rmcp 运行时要求），引擎调用经 `spawn_blocking` 桥接；`supertask-core` 保持纯同步，不引 async。
- 平台差异仍只准存在于平台模块与 cfg 分支（延续 1.4 §3.2）。

## 4. CLI

### 4.1 命令集

| 命令 | 语义 | 取锁 |
|------|------|------|
| `supertask up [ids…]` | 打开工作区 → 按拓扑启动所选（缺省全部）→ 等健康 → 保持附加（§4.2） | 是 |
| `supertask down [ids…]` | 停止所选/全部；本进程非 owner 且锁被持有 → `WORKSPACE_LOCKED`；无 owner 且无服务 → 幂等空操作退出 0 | 是 |
| `supertask restart [ids…]` | 停再起（复用引擎 `restart_one` 语义） | 是 |
| `supertask status` | 服务快照（状态/端口/健康）+ 工作区与锁持有者信息；`--json` | 否（只读） |
| `supertask logs [id]` | 历史日志检索：`--lines N`（默认 200）`--grep PATTERN` `--json`；复用 core log search（读 `.supertask/logs` 文件） | 否（只读） |
| `supertask script run <id>` / `script cancel` | 脚本运行/取消；cmds 只来自 YAML，同工作区单脚本约束沿用 | 是 |
| `supertask export` | §6.2 | 否（只读） |
| `supertask import <pkg>` | §6.3 | 否（落盘目标目录，不打开工作区） |
| `supertask doctor` | toolchain.probe + docker probe 摘要（人读 + `--json`），CI 排障用 | 否 |
| `supertask mcp` | §5 | 惰性（首个可变工具） |
| `supertask version` | 版本与协议/构建信息 | 否 |

### 4.2 `up` 生命周期（附加模型）

1. **解析工作区**：`-w/--workspace <dir>` > 环境变量 `SUPERTASK_WORKSPACE` > 从 cwd 向上搜索 `supertask.yaml`；找不到 → `WORKSPACE_NOT_FOUND`。
2. **取锁并打开**：`WORKSPACE_LOCKED` 时打印 holder 与 pid 后退出 1。
3. **启动**：`engine.open` → 按拓扑 start 所选；解析警告原样输出到 stderr。
4. **等待**：`--wait healthy|started|none`（默认 `healthy`，`--wait-timeout` 默认 300s）。超时 → 停止全部、退出 1、stderr 列出未达标服务与健康检查错误。
5. **保持阶段**（两种形态二选一）：
   - 交互：前台聚合输出各服务日志，行前缀 `[<service>] `；stderr 行透传到终端 stderr；Ctrl+C / SIGTERM → 优雅 `stop_all` → 退出 0；第二次信号不再等待直接强杀。
   - 包装：`-- <command…>` 在健康达标后 spawn 子命令（继承 stdio 与系统 env），子命令退出 → `stop_all` → **透传其退出码**。CI 主用形态。
6. **清场保证**：CLI 无论正常退出、信号、崩溃，进程树按 1.4 平台规则无残留；不提供退出后存活模式。

### 4.3 输出与退出码

- 默认人读（表格/带色，`--no-color` 关闭）；`--json` 输出 `{ ok, data | error: { code, message, details } }`，错误码与 IPC 同表，结构快照测试锁定。
- 退出码：`0` 成功；`1` 运行错误（含健康超时、`WORKSPACE_LOCKED`）；`2` 用法错误。
- CLI 不在 UI/终端拼 cmdline 展示（沿用「不在 UI 拼 cmdline」纪律的终端等价物：输出服务 id、状态与端口，不回显含密钥的完整命令行）。

## 5. MCP 服务器

### 5.1 传输与生命周期

- `supertask mcp`：stdio 传输，MCP 协议版本随 rmcp 支持；tools only（无 resources/prompts/采样）；无网络监听。
- 工作区解析与 CLI 同规则（cwd 向上 + `SUPERTASK_WORKSPACE`）。
- 惰性持有：进程启动即就绪；**首个可变工具**触发取锁 + `engine.open`；`supertask_status` / `supertask_logs` 只读、无需持锁。
- **断连即清场**：stdio 关闭（编辑器退出/重载/崩溃）→ `stop_all` → 释放锁 → 进程退出。防孤儿优先；文档与工具描述明示「编辑器重载会停止服务」。
- 并发：工具调用串行化（引擎互斥锁）；rmcp 的 tokio 层用 `spawn_blocking` 调同步引擎。

### 5.2 工具清单

| tool | 入参 | 语义 |
|------|------|------|
| `supertask_status` | – | 全服务快照：状态、端口、健康、脚本占用；含工作区根与锁持有者 |
| `supertask_start` | `{ services?: string[] }` | 缺省全部；拓扑顺序 |
| `supertask_stop` | `{ services?: string[] }` | 缺省全部 |
| `supertask_restart` | `{ services?: string[] }` | |
| `supertask_logs` | `{ service?: string, lines?: int = 200, grep?: string }` | 历史日志尾部（读文件，不依赖持锁） |
| `supertask_run_script` | `{ id: string }` | 同 CLI `script run` |
| `supertask_cancel_script` | – | |

- 返回统一 `{ ok: true }` 或 `{ error: { code, message, details } }`；错误码与 IPC/CLI 同表。
- 长操作（jar 构建、compose、模板、导入）不进 1.5 工具集——工具只覆盖「起停 + 观察 + 脚本」这一 Agent 高频闭环，避免与 GUI 操作语义分叉。
- Cursor 配置示例（写进用户文档）：

```json
{ "mcpServers": { "supertask": { "command": "supertask", "args": ["mcp"] } } }
```

## 6. 导出包

### 6.1 包格式

zip，路径一律 `/` 分隔，UTF-8：

```text
manifest.json        { "format": 1, "name", "created_at", "source_os",
                       "app_version", "entries": [{ "path", "sha256", "bytes" }] }
supertask.yaml       工作区 spec（原样字节）
<相对路径密钥文件>   可选（--with-secrets 时，路径与 yaml 中声明一致）
```

- `format: 1` 字段只增不破——它是 2.0 一键迁移的载荷雏形。
- 不进包：`.supertask/`（日志/历史/锁）、`.git`、代码本体（git 负责）、app data（平台本地偏好不搬家）。

### 6.2 `export`

- `supertask export [-o <file>] [--with-secrets]`；默认文件名 `supertask-<目录名>-<yyyymmdd-HHmm>.zip` 输出到 cwd。
- 排除规则：默认排除 `secrets.file` 与全部 `env_file` 声明的文件（去重）；`--with-secrets` 才逐个入包并写入 manifest。`secrets.backend: env` → 无文件可打包，输出提示。
- 桌面入口（设置页「导出工作区包」）同规则；含密钥需勾选确认框，文案明示风险（§9.2）。
- 只读操作，不要求持有工作区锁（桌面当前工作区导出时自然持有，不冲突）。

### 6.3 `import`

- `supertask import <pkg> [--dest <dir>]`（dest 缺省 cwd）；桌面 welcome 页「从导出包导入」等价。
- 校验链：文件缺失 → `PKG_NOT_FOUND`；zip 解析失败 / manifest 缺失或损坏 / 条目哈希不符 / **zip-slip（任何条目规范化后逃逸 dest）** → `PKG_INVALID`；`format` 大于支持版本 → `PKG_VERSION`；目标目录存在 `supertask.yaml` → `PKG_TARGET_EXISTS`（不覆盖、无 `--force`）。
- 导入只落盘，不打开、不启动；结束打印下一步（`supertask up` / 桌面打开该目录）。
- 跨平台：yaml 内路径本就是工作区相对路径，天然可搬；导入后首次打开按新平台重新探测工具链（1.4 规则）。

## 7. YAML 与应用数据兼容

- YAML `version: 1` 不变，无新增字段；protocol 1 不变；app data 仍为 v3（`locale` 等不迁移）。
- 唯一新文件是工作区内 `.supertask/engine.lock`（运行时产物）；`.supertask/` 建议加入 `.gitignore`（文档注明）。
- 兼容测试：旧客户端 / 1.4 GUI 对 1.5 工作区的读写行为不变（lock 文件不参与 spec 解析，spec 未知字段保留规则不受影响）。

## 8. IPC 契约增量

protocol 保持 1，新增两条命令（ipc.md 增 §10.9）：

```text
workspace.exportPackage  { workspace_id, dest_path, with_secrets }
                       → { path, entries: [{path, bytes}], warnings: string[] }
workspace.importPackage  { pkg_path, dest_dir? }
                       → { root, warnings: string[] }
```

- `export` 作用于桌面当前工作区；`import` 在 welcome（无工作区上下文）使用，成功后前端直接 `app.load` 返回的 `root`。
- 桌面打开工作区遇到 `WORKSPACE_LOCKED`：`app.load` 以该错误码失败，前端呈现 holder 信息与「关闭持有进程后重试」指引（§11）。
- `session.hello` / 其余命令无结构变化。

## 9. 错误与安全要求

### 9.1 新增稳定错误码

| code | 触发条件 |
|------|----------|
| `WORKSPACE_LOCKED` | 工作区被另一存活进程（desktop/cli/mcp）持有 |
| `PKG_NOT_FOUND` | 导入文件不存在或不可读 |
| `PKG_INVALID` | 包损坏：zip/manifest 解析失败、哈希不符、zip-slip |
| `PKG_VERSION` | manifest format 高于支持版本 |
| `PKG_TARGET_EXISTS` | 导入目标已有 supertask.yaml |

其余复用现有码（`WORKSPACE_NOT_FOUND`、`MISSING_TOOL`、`PORT_CONFLICT`、`JOB_KILL` 等）。

### 9.2 安全边界

- 锁只含 pid/holder/时间戳；stale 判定仅检查 pid 存活性，绝不向任意 pid 发信号。
- 导出默认不含密钥（对齐「密钥默认不同步」红线）；`--with-secrets` / UI 勾选需显式确认，确认文案明示「包内将包含明文密钥」。
- 解包零执行：import 不运行任何命令、不解析模板；逐条 canonicalize 防 zip-slip；manifest sha256 逐文件校验。
- MCP 仅 stdio 本地：无监听端口、无鉴权面、不暴露任意 pid/pgid 参数；工具能力边界 = 引擎既有护栏（kill 守栏、沙箱、同工作区单脚本）。
- CLI `--` 包装命令与 `script cmds` 同一信任边界（用户显式输入，本机执行）；日志脱敏规则（不打印 secret 值）对 CLI/MCP 输出同样生效。
- 导出包内容不进遥测与系统日志（日志只记录路径与字节数）。

## 10. 开源复用清单（选型纪律沿用 1.4 §6.3）

| 用途 | 复用方案 | 说明 |
|------|----------|------|
| CLI 参数解析 | `clap` v4（derive） | 事实标准，子命令/帮助/补全齐全；不手写解析 |
| MCP | `rmcp`（官方 Rust SDK） | stdio server + 类型化 tool 定义；不手写 JSON-RPC；动手前核对近一年发布与 issue 响应 |
| zip 读写 | `zip` crate | Deflate 流式写出；不手写 zip 格式 |
| 信号处理 | `ctrlc` crate | 跨平台 SIGINT / Windows CTRL_C；二次信号升级强杀由 CLI 自管 |
| JSON | 现有 `serde_json` | manifest、`--json` 输出 |
| async 边界 | 仅 MCP 壳层 tokio；core 保持同步 | rmcp 运行时要求；`spawn_blocking` 桥接，不向 core 渗透 |

选型纪律：能用原生就不加依赖；新依赖先核维护状态；`nix`/`procfs` 等 1.4 cfg 纪律不变；`supertask-core` 不因 CLI/MCP 引入任何新依赖（锁与 pkg 模块仅用 std + serde + zip，zip 由 cli crate 依赖、core 的 pkg 模块以字节流接口解耦——实现期若证明放 core 更顺，zip 依赖进 core 需在实现计划里显式说明）。

## 11. 前端范围

- **welcome 页**：「从导出包导入」入口（file picker → `workspace.importPackage` → 打开返回目录）；导入错误按 §9.1 码呈现。
- **设置页**：「导出工作区包」（路径选择 + 含密钥 checkbox + 风险确认；结果 Toast 带条目数）。
- **锁冲突**：`app.load` 返回 `WORKSPACE_LOCKED` 时呈现 holder/pid 与重试指引（四语文案，错误码映射沿用 `lib/error-messages.ts`）。
- i18n：新增 key 四语 parity（沿用 1.4 校验规则）；命令面板不加 CLI/MCP 相关条目（CLI 无 UI 语义）。
- 其余页面零变化；导航注册表不动。

## 12. 非功能要求

### 性能

- CLI 冷启动至首输出 < 300ms（不含服务 spawn）；`status --json` p95 < 50ms（纯文件读取）；`export` p95 < 300ms（典型 yaml + 少量 env 文件）；MCP 工具调用框架开销 < 100ms（不含服务状态转换本身）。
- 锁获取/探测为单文件操作，不引入轮询；stale 检测仅发生在取锁失败路径。

### 可靠性

- 三平台 CI 全绿为合并门槛（matrix 扩展跑 `supertask-cli` 测试）；Windows 零回归：1.0–1.4 既有测试断言不改编通过。
- `up` 在正常退出、信号、崩溃三条路径下三平台无残留进程（CI 断言）；MCP 断连清场同口径。
- 锁文件损坏（非法 JSON）→ 视同 stale，清理重建，不阻塞打开。

### 隐私

- 导出包与遥测/云无任何联动；包内容不进日志；`--json` 输出与 MCP 返回均不携带 secret 值。

## 13. 测试与验收

### 13.1 Core 单元测试（三平台跑）

- 锁：同 pid 重入、跨 pid 拒绝（fake pid 存活）、stale 接管、非法 JSON 重建、holder 元数据往返。
- pkg：export 清单/排除/`--with-secrets` 去重、manifest 哈希；import 往返（yaml 字节等价）、zip-slip 用例（`../`、绝对路径、符号链接条目）、坏哈希、坏 manifest、`PKG_TARGET_EXISTS`、`PKG_VERSION`。

### 13.2 CLI 集成测试

- fake 服务桩（sleep/cat 进程，沿用外部 GUI 隔离纪律）：up 拓扑与 `--wait started/healthy`、健康超时停止、`--` 包装退出码透传（0/非零）、信号停止（测试发 SIGTERM/杀进程模拟）。
- `--json` 结构快照测试；`status` 读锁持有者信息；`logs --grep`；`doctor`。
- 无残留断言：测试结束后工作区无存活桩进程。

### 13.3 MCP 测试

- rmcp in-proc client 逐工具调用断言（起停/状态/日志/脚本）；
- 断连清场：关闭 stdio → 进程停止服务并退出（桩进程存活数归零）；
- 与 CLI 同工作区互斥：可变工具得 `WORKSPACE_LOCKED`，只读工具可用。

### 13.4 真机验收矩阵

| 场景 | Windows | macOS | Linux |
|------|---------|-------|-------|
| CI 形态 `up --wait healthy -- cmd` | ✅（CI） | ✅（CI） | ✅（CI） |
| Ctrl+C / SIGTERM 清场无残留 | 回归 | ✅ | ✅ |
| 锁互斥（桌面 ↔ CLI ↔ MCP） | ✅ | ✅ | ✅ |
| MCP 接 Cursor 手动冒烟 | ✅ | ✅ | ✅ |
| 导出（Windows）→ 导入（macOS/Linux） | 源端 | ✅ | ✅ |
| 桌面导出/导入 UI + 四语 | ✅ | ✅ | ✅ |
| 1.0–1.4 回归抽样 | 全量 | 抽样 | 抽样 |

## 14. 交付顺序

1. **工作区所有权锁**：core `lock` 模块 + 桌面 `app.load` 接入 + `WORKSPACE_LOCKED` 呈现（四语）。地基，无外部依赖。
2. **CLI crate**：clap 骨架、up/down/restart/status/logs/script/doctor、`--json`、信号与 `--` 包装、退出码。
3. **导出包**：core `pkg` 模块 + CLI export/import + 桌面 IPC（§8）与 UI 入口、四语。
4. **MCP**：rmcp 接入、工具集、惰性取锁、断连清场。
5. **CI 扩展与验收**：matrix 增跑 cli/mcp 测试、导出导入跨平台用例、真机矩阵（§13.4）。

依赖：2、4 依赖 1；3 与 2、4 可并行（导出不取锁）；5 收口。1 与 3 最早可启动。

## 15. 默认决策（本稿建议，待确认）

- 多前端单引擎：CLI/MCP 直接复用 `supertask-core`，不分叉、不做 RPC 中间层；**不引入守护进程**，`up` 附加模型 + `--` 包装覆盖 CI 场景。
- 工作区互斥 = 文件锁 + pid 存活探测；同 pid 重入合法；桌面既有流程的唯一变更是多 owner 从静默竞争变明确拒绝。
- MCP 断连即停服务（防孤儿优先）；仅 stdio、仅 7 个工具，长操作不进工具集。
- 导出包默认不含密钥、不搬代码、不搬 app data；包格式 format:1 为 2.0 迁移载荷雏形。
- 新错误码五枚（§9.1）；YAML / protocol / app data 版本均不变。
- `supertask-core` 不新增依赖（zip 留在 cli crate，core pkg 模块以字节流解耦），MCP 的 tokio 只存在于壳层。
