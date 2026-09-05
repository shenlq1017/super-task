# SuperTask 1.5 实现计划

> 日期：2026-08-29  
> 状态：待实现（功能规格已给出默认决策 §15，本计划按 §14 交付顺序拆任务）  
> 功能规格真源：[2026-08-29-v1-5-feature-spec.md](2026-08-29-v1-5-feature-spec.md)  
> 上位：repository conventions · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [1.4 复用纪律](2026-08-28-v1-4-feature-spec.md)

把规格 §14 的五步交付顺序拆成可执行任务。行为细节、错误码语义、安全边界以功能规格为准；本计划只定文件、顺序、复用选型与完成标准。

## 一句话

先落工作区所有权锁（core `lock` 模块 + 桌面接入，零新依赖），再开 `crates/supertask-cli` 壳（clap + 现有引擎），导出包（core `pkg` 字节流接口 + cli zip）与 MCP（rmcp stdio）可并行，最后 CI matrix 扩展与真机验收收口。业务零分叉：同一份 `supertask-core`，四个前端，一个 owner。

## 复用核查（2026-08-29，动手前对规格 §10 的逐项核实）

按 1.4 §6.3 纪律逐项核对维护状态与替代方案，结论如下；与规格 §10 冲突处以本表为准（本表更新）。

| 用途 | 选型 | 核查结论 | 备选与拒绝理由 |
|------|------|----------|----------------|
| CLI 参数解析 | `clap` v4（derive） | ✅ 事实标准，活跃 | 手写解析不做；`bpaf` 生态小 |
| MCP SDK | `rmcp` | ✅ **已升级为 MCP 官方 Tier-1 SDK**（2026-08-21 合入 tier 提升；最近 release 2026-08-07；实现 MCP 2026-07-28 spec，兼容 2025-11-25）。规格里「动手前核对」已完成 | `rust-mcp-sdk` 社区维护、非官方；手写 JSON-RPC 不做 |
| zip 读写 | `zip` crate | ✅ 活跃，当前 **v8.6.0**（MSRV 1.88）。注意 v2→v8 API 变化大，实现时以 docs.rs 当前 API 为准；zip-slip 防护直接用其 `ZipFile::enclosed_name()`，**不手写路径规范化** | 自写 zip 不做；CVE-2025-29787 已在 2.3.0 修复，pin ≥8 |
| 信号处理（CLI） | `ctrlc` crate | ✅ 但**必须开 `termination` feature** 才覆盖 SIGTERM（规格 §4.2 要求 Ctrl+C 与 SIGTERM 双信号）；Windows 覆盖 CTRL_C/CTRL_CLOSE。二次信号升级强杀由 CLI 自管（ctrlc 不做） | `signal-hook` 更强但 CLI 只需两信号，ctrlc 更贴合；MCP 壳**不装 ctrlc**（rmcp/tokio 已有 signal 支持，避免双重 handler） |
| pid 存活检测（锁 stale 判定） | **复用现有代码，零新依赖** | Windows：`proc/windows.rs` 已用 ToolHelp 枚举 + `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)`，提一个 `pid_alive(pid)` 助手即可；Unix：`nix`（已是 unix 依赖）`kill(pid, 0)` | `sysinfo` 为一个布尔查询拉整套进程表，过重 |
| 文件锁原语 | std `OpenOptions::create_new`，不引 `fd-lock` | 规格锁语义（create-new 占用 + 同 pid 重入 + holder 元数据 + 损坏重建）是自定义协议；OS advisory 锁（fd-lock）只在持句柄期间有效，holder 信息仍要第二个文件，两套机制不如一套 std-only | `fd-lock` 拒绝：收益为零还多一个依赖 |
| manifest 哈希 | 现有 `sha2` | ✅ core 已依赖 | — |
| CLI 带色输出 | `anstyle` + `anstream` | clap 同维护者、极小；`anstream` 自动处理 NO_COLOR/非 TTY，`--no-color` 只需绕过 AutoStream | 若实现期发现暗坑（如 Windows 旧 conhost），降级为纯文本输出，不为此加更多依赖 |
| CLI `logs` 检索 | 现有 `log/search.rs::search_logs` | ✅ 直接复用，CLI 不另写文件读取 | — |
| `--json` 序列化 | 现有 `serde_json` + core 输出结构 | ✅ 规格守恒：不另造第二套模型 | — |
| 前端错误呈现 | 现有 `lib/error-messages.ts` 码映射 + 四语 parity 校验 | ✅ 沿用 1.4 机制，新码只加词条 | — |

依赖落点纪律：`tokio` 只进 `supertask-cli`（MCP 阶段）；`rmcp`/`clap`/`ctrlc`/`anstyle` 全部只进 `supertask-cli`。

**实现期偏差（2026-08-29 记档）**

1. **zip 依赖进 core**（规格 §15 原定留 cli crate）：桌面 `workspace.exportPackage/importPackage` 与 CLI 必须共用同一实现，字节流接口拆两份写入器反而让壳层各带一份格式代码。core Cargo.toml 已注明理由。
2. **新增错误码 `HEALTH_TIMEOUT`**：`up --wait` 健康等待超时的 `--json` 信封需要统一码表（规格 §9.1 五枚未覆盖此 CLI 层场景），已入 core 码表并测试锁定。
3. **CLI 带色输出暂缓**：当前输出为纯文本，`--no-color` 解析保留（no-op，注释说明）。后续如需带色按计划用 `anstyle` + `anstream`。
4. **开发期 bin 撞名**：CLI bin `supertask` 与 src-tauri dev 产物 `supertask.exe` 同名（安装版是 productName `SuperTask.exe`，不受影响）。桌面 dev 进程运行时 `cargo build/test -p supertask-cli` 会因文件锁失败；临时用 `CARGO_TARGET_DIR=target-cli` 隔离，后续如常态化可考虑调整 src-tauri dev bin 名。

## 约束（贯穿各 phase）

- 业务只进 `crates/supertask-core`；CLI/MCP/Tauri 壳不写业务闭包（与 src-tauri 同一规则）。
- `supertask-core` 保持纯同步；MCP 的 tokio 只在壳层，引擎调用经 `spawn_blocking`。
- CLI `--json` 直接序列化 core 结构（RuntimeSnapshot 等），错误码与 IPC 同表。
- 锁只记录 pid/holder/时间戳；stale 判定只查 pid 存活，绝不向任意 pid 发信号。
- 导出默认不含密钥；`--with-secrets` 需显式确认；包内容不进日志/遥测。
- 单测不得启动外部 GUI/真服务；CLI 集成测试用 sleep/cat 桩进程（沿用 1.4 隔离纪律）；桩进程临时目录等确认进程退出后再清理。
- YAML `version: 1`、protocol 1、app data v3 均不变；YAML 保存仍需 `base_hash`。
- workspace `Cargo.toml` members 增 `crates/supertask-cli`（Phase 2 起）。

## Phase 1 — 工作区所有权锁（规格 §3.1、§9.1、§11；最早可启动）

地基，零外部依赖。含五枚新错误码中的 `WORKSPACE_LOCKED`（PKG_* 四枚随 Phase 3）。

### 任务 1.1 ErrorCode `WORKSPACE_LOCKED`

- **文件：** `crates/supertask-core/src/error.rs`
- **做：** 新增稳定码 `WORKSPACE_LOCKED`（serde SCREAMING_SNAKE_CASE）；details 携带 `holder`（desktop/cli/mcp）与 `pid`。
- **测试：** 序列化为规格字符串；details 字段往返。
- **完成标准：** 码表与 §9.1 对应；旧码不变。

### 任务 1.2 core `lock.rs`

- **文件：** 新建 `crates/supertask-core/src/lock.rs`（或 `lock/` 目录）
- **做：**
  - 锁文件 `<root>/.supertask/engine.lock`，JSON `{ pid, holder, started_at_ms }`
  - `acquire(root, holder)`：`create_new` 独占创建；同 pid 重入允许（返回既有锁）；他 pid 存活 → `WORKSPACE_LOCKED`（details 带 holder/pid）；持有 pid 已死或内容非法 JSON → 清理重建（stale 接管）
  - `release(root)`：pid 匹配才删；`query(root)`：只读读取锁内容（status/logs/只读工具用）
  - `pid_alive(pid)` 助手进 `proc/` 平台层：Windows 复用 `proc/windows.rs` 的 OpenProcess/ToolHelp 模式，Unix 走 `nix::kill(pid, 0)`（Signal 0）
- **测试：** 同 pid 重入、跨 pid 拒绝（fake pid：用测试自身 pid 模拟存活 + 不存在 pid 模拟死亡）、stale 接管、非法 JSON 重建、holder 元数据往返、release 后可重新 acquire。
- **完成标准：** 规格 §13.1 锁用例全绿；无轮询；不新增依赖。

### 任务 1.3 桌面接入与 `WORKSPACE_LOCKED` 呈现

- **文件：** `crates/supertask-core/src/engine.rs`（open/close/detach 挂锁）；`src-tauri/src/commands.rs`（`app.load` 错误透传）；`frontend/src/lib/error-messages.ts` + 四语 locales（`WORKSPACE_LOCKED` 文案：holder/pid + 「关闭持有进程后重试」指引）
- **做：** 桌面 `app.load` 打开工作区时取锁（失败 → `WORKSPACE_LOCKED`，app.load 以该错误失败）；close/detach/进程退出释放；前端按 §11 呈现。
- **完成标准：** 既有 `cargo test -p supertask-core` 与 `npm run build` 全绿；桌面回归（1.0–1.4 抽样）不受影响；同工作区双开桌面 → 第二个明确报锁冲突。

## Phase 2 — CLI crate（规格 §4；依赖 Phase 1）

### 任务 2.1 crate 骨架与只读命令

- **文件：** 新建 `crates/supertask-cli/`（bin `supertask`）；根 `Cargo.toml` members
- **做：** clap derive 全命令骨架；工作区解析（`-w` > `SUPERTASK_WORKSPACE` > cwd 向上搜索，`WORKSPACE_NOT_FOUND`）；全局 `--json` / `--no-color`；退出码约定（0/1/2）；`version`、`doctor`（toolchain + docker probe 摘要）、`status`（复用 core 快照 + `lock::query` 显示 holder）、`logs`（复用 `log/search.rs::search_logs`，`--lines/--grep/--json`）。
- **测试：** 参数解析与退出码单测；`--json` 结构快照。
- **完成标准：** 只读命令在无锁工作区可用；冷启动首输出 < 300ms（不含服务 spawn）。

### 任务 2.2 可变命令与 `up` 附加模型

- **文件：** `crates/supertask-cli/src/`（up/down/restart/script）
- **做：** 首个可变动作取锁（`WORKSPACE_LOCKED` → 打印 holder/pid 退出 1）；`up` 按 §4.2 生命周期：拓扑启动 → `--wait healthy|started|none`（默认 healthy，`--wait-timeout` 300s）→ 交互聚合输出（`[<service>] ` 行前缀，stderr 透传）或 `-- <command…>` 包装（继承 stdio，退出码透传）；ctrlc（`termination` feature）→ 首信号优雅 `stop_all`、二信号强杀；超时/失败 → 停全部、退出 1、stderr 列未达标服务与错误码；`down` 幂等；`restart` 复用 `restart_one` 语义；`script run/cancel` 沿用同工作区单脚本约束。
- **测试：** fake 桩集成测试（§13.2）：拓扑与 wait 两态、健康超时清场、`--` 透传 0/非零、信号停止（发 SIGTERM 模拟）、无残留断言。
- **完成标准：** 三条退出路径（正常/信号/崩溃）桩进程归零；Windows 既有测试零回归。

## Phase 3 — 导出包（规格 §6、§8；与 Phase 2/4 可并行，依赖 Phase 1 仅在桌面 UI 步）

### 任务 3.1 core `pkg.rs`（✅ 已落地，zip 直接入 core，见上方偏差 1）

- **文件：** 新建 `crates/supertask-core/src/pkg.rs`；ErrorCode 增 `PKG_NOT_FOUND`/`PKG_INVALID`/`PKG_VERSION`/`PKG_TARGET_EXISTS`
- **做：** 包格式 §6.1（manifest.json format:1 + entries sha256/bytes）；`export_workspace(root, opts, sink)` 以 `std::io::Write` 字节流接口解耦（zip 归属 cli crate；写入顺序与条目清单在 core 定死）；排除规则（secrets.file + env_file 去重、`.supertask/`、`.git`）；`--with-secrets` 去重入包；manifest 哈希复用 `sha2`。
- **测试：** §13.1 pkg 用例：清单/排除/去重/哈希；往返 yaml 字节等价。
- **完成标准：** core 零新依赖；导出 p95 < 300ms 量级（典型 yaml）。

### 任务 3.2 import 校验链（cli crate 接 zip）

- **文件：** `crates/supertask-cli/src/`（import/export 子命令接 `pkg.rs`）
- **做：** zip 读取用 `zip` crate v8；校验链 §6.3：`PKG_NOT_FOUND` → zip/manifest 解析（`PKG_INVALID`）→ format 版本（`PKG_VERSION`）→ 逐条 sha256 → zip-slip 防护用 `enclosed_name()`（逃逸 → `PKG_INVALID`）→ 目标已有 `supertask.yaml`（`PKG_TARGET_EXISTS`，无 `--force`）；只落盘不打开不启动；结束打印下一步指引。
- **测试：** zip-slip 用例（`../`、绝对路径、符号链接条目）、坏哈希、坏 manifest、target exists、跨平台往返（CI 内三平台互导）。
- **完成标准：** §6.3 校验链逐码有测试；解包零执行。

### 任务 3.3 IPC 两条命令 + 桌面 UI + 四语

- **文件：** `docs/spec/ipc.md`（§10.9）；`crates/supertask-core/src/ipc/`（命令类型）；`src-tauri/src/commands.rs`（`workspace.exportPackage` / `workspace.importPackage`）；前端 `protocol.ts` 类型、welcome 页导入入口、设置页导出入口（路径选择 + 含密钥 checkbox + 风险确认）、四语 locales
- **做：** IPC 契约 §8；welcome 导入成功后直接 `app.load` 返回的 `root`；设置页导出结果 Toast 带条目数；`app.load` 遇 `WORKSPACE_LOCKED` 呈现复用任务 1.3 文案。
- **完成标准：** 四语 parity 校验通过；桌面导出→CLI 导入、CLI 导出→桌面导入各跑通一轮。

## Phase 4 — MCP（规格 §5；依赖 Phase 1）

### 任务 4.1 rmcp stdio server + 7 工具

- **文件：** `crates/supertask-cli/src/mcp/`
- **做：** `supertask mcp` 子命令；rmcp stdio 传输、tools only；工具清单 §5.2（status/start/stop/restart/logs/run_script/cancel_script）；返回统一 `{ ok }` / `{ error: { code, message, details } }`；tokio 只在本 crate；引擎调用 `spawn_blocking` 桥接、工具调用串行化（引擎互斥）。
- **测试：** rmcp in-proc client 逐工具断言（起停/状态/日志/脚本）。
- **完成标准：** 工具调用框架开销 < 100ms（不含状态转换）；无网络监听。

### 任务 4.2 惰性取锁与断连清场

- **文件：** `crates/supertask-cli/src/mcp/`
- **做：** 首个可变工具取锁 + `engine.open`（只读工具 status/logs 不取锁）；stdio 关闭 → `stop_all` → 释放锁 → 退出；与桌面/CLI 同工作区互斥（可变工具 `WORKSPACE_LOCKED`）。
- **测试：** 断连清场（关 stdio → 桩进程归零）；与 CLI 同工作区互斥用例（§13.3）。
- **完成标准：** 编辑器重载场景无孤儿进程；只读工具在被持有工作区仍可用。

## Phase 5 — CI 扩展与验收（规格 §12、§13.4）

- **做：** CI matrix 增跑 `supertask-cli` 测试（三平台）；`up --wait healthy -- cmd` 三平台 CI 用例；导出（Windows）→ 导入（macOS/Linux）跨平台用例；真机矩阵 §13.4 全项（锁互斥三端、Cursor MCP 冒烟、桌面导出导入 UI 四语、1.0–1.4 回归抽样）；性能指标抽查（§12）。
- **完成标准：** 三平台 CI 全绿为合入门槛；Windows 零回归；无残留断言进 CI。

## 文档债（随 phase 清）

- `docs/spec/ipc.md` §10.9（Phase 3 任务 3.3）。
- CLI 用户文档（新 `docs/spec/cli.md` 或 README 章节）：命令表、退出码、`SUPERTASK_WORKSPACE`、`.supertask/` gitignore 建议、Cursor 配置示例（Phase 2/4 落地时补）。
- repository conventions 规范真源表补本计划链接；进度文档 `2026-08-29-v1-5-progress.md` 随 Phase 1 建。
