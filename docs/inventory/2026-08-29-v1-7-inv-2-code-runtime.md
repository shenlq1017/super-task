# inv-2 · core 代码事实盘点（服务类型 / 启动链 / 探测 / 工具链 / 扫描）

> 2026-08-29。盘点稿，供复核。所有结论带 file:line，可逐条打开核对。
> 2026-08-29 复核修正：§2.2 散点 6→**7** 处（补 `runnable_kind` 闸门，v1.7 规格 §2.1）。
> 引擎在 `crates/supertask-core`（不依赖 Tauri）；Tauri 壳 `src-tauri` 只做 IPC 适配。

## 1. 模块一览（顶层目录实测）

`crates/supertask-core/src/`：`appdata / cloud/ / discover / docker/ / engine / error / features / gateway/ / git / graph / health / ide / ipc/ (v12,v13,v15,v16) / launcher / lock / log/ (batch,file,ring,search) / merge / metrics / network / operation / pkg / ports / probe / proc/ (windows,unix) / profiles / runtime/ / sandbox / scan / secrets / spring / spec/ (file,validate) / taskfile / template / toolchain/ (discover,install,manifest,provider,resolver,runner,versions)`。

另有独立 `crates/supertask-cloud-server/`：当前实测有 Cargo manifest、`lib.rs`、`main.rs`、`config.rs`、`auth.rs`、`entities.rs`、`state.rs`、`error.rs`、`http.rs`、`quota.rs`、`telemetry.rs`、`migrations/0001_init.sql` 和 `tests/api.rs`；已具备本地 HTTP router/API、healthz、配额/遥测和 in-process 集成测试。未知 type 仍受当前四类型校验限制；正式 HTTPS/运营和真机验收未完成。服务端事实与环境变量见 [docs/spec/cloud-server.md](../spec/cloud-server.md)。

## 2. 服务类型（kind）模型

### 2.1 类型定义是「String + 平铺结构」，不是 enum

- `spec/file.rs:70-127` `ServiceSpec`：`pub kind: String`（`:72`）；其余 per-kind 字段全部平铺在同一个 struct（`module`、`build_tool`、`jvm_args`、`dir`、`package_manager`、`script`、`service`…），未知字段走 `#[serde(flatten)] extra: IndexMap<String, Value>`（`:126`）round-trip。
- 含义：**新增 kind 不需要改 `ServiceSpec` 类型定义**；新 kind 的专有字段可以先进 flatten extra（或按需升 typed）。
- `PackageManager` enum（npm/pnpm/yarn/bun，`file.rs`）。
- yaml.md §4.2 kind 表（规格真源）：`spring-boot`、`node`、`compose`、`python`、`go`、`generic` 均已进入当前可启动链路；未知 kind → `KIND_UNSUPPORTED`，仍能打开文件。

### 2.2 kind 的字符串 match 散点（新增 kind 的必改处）

| 位置 | 内容 |
|------|------|
| `spec/file.rs:506-508` | `runnable_kind()`：kind 可启动性唯一闸门（`launcher.rs:182` 调用），未列出 → `KIND_UNSUPPORTED`（2026-08-29 复核补记） |
| `spec/validate.rs:43-76` | per-kind 必填字段校验三分支：spring-boot（module/build_tool 探测）、node（dir/package_manager/script）、compose（service 必填、注入类字段非法 `SPEC_INVALID`） |
| `spec/validate.rs:176` | toolchain 关联校验：`("node", tc.node)` 形式按 kind 对工具 |
| `launcher.rs:190-197` | 启动计划构建：`"spring-boot" => plan_spring(...)` / `"node" => plan_node(...)`；compose 走 docker 模块 |
| `launcher.rs:209-212` | per-kind 端口环境变量注入：spring → `SERVER_PORT`，node → `PORT`（yaml.md §4.1 规则的代码落点） |
| `launcher.rs:434` | `KIND_UNSUPPORTED` 作为兜底守卫 |
| `scan.rs:273,535,577,680,840` | 扫描识别与草稿生成按 kind 写死（spring-boot/node/compose） |

### 2.3 先例：1.3 compose 从零到完整的链路

typed 字段（`ServiceSpec.service` + `DockerSpec`）→ 校验（validate.rs compose 分支）→ `src/docker/` 适配层（固定程序 runner + 2 MiB 输出上限、probe 三态、compose config mtime/sha256 缓存）→ 引擎运行时 → `docker.*` IPC + `/docker` 页 + 扫描导入。**这是估算「新增 kind 端到端成本」的最直接参照。**

## 3. 与 kind 解耦的子系统（新 kind 自动受益）

| 子系统 | 位置 | 解耦方式 |
|--------|------|----------|
| 健康检查 | `spec/file.rs:407-427` `HealthSpec`/`HealthType`（none/tcp/http）；`health.rs` | 只依赖 port + loopback 限制（`HEALTH_HOST_FORBIDDEN`），不看 kind |
| 进程管理/停止 | `proc/`（Windows Job Object + unix）、`operation.rs` | 按 PID/进程树，不看 kind |
| 指标 | `metrics.rs` + `proc/` | 按 PID 采 CPU/内存 |
| 日志 | `log/`（batch/ring/file/search），`.supertask/logs/{serviceId}.log` | 按 serviceId |
| 网关路由 target | `spec/file.rs:175-186` `GatewayRoute`；`gateway/` | 按服务 id 解析 port（yaml.md §7.1），不看 kind |
| 依赖图 | `graph.rs` | 服务 id 拓扑 |

## 4. 工具链探测与安装（新语言要过的门）

- `probe.rs:9-14` `ToolProbe { found, version, path }`；`probe.rs:16-32` `ToolchainProbe` **固定字段**：java/maven/gradle/node/npm/pnpm/yarn/bun/python/go + `gateway`（扩展字段使用 `#[serde(default)]`）。
- `probe.rs:52-75,138-178`：`probe_bundle`/`probe_toolchain` 并行探测（每工具独立线程 + 4s 硬超时，坏工具报 not found 不阻塞 `app.load`）；Engine 侧 60 秒 TTL 缓存，显式 refresh 或安装成功后失效。
- `probe.rs:200-224` `resolve_program_with_path`：服务专用 PATH 或进程 PATH（Windows PATHEXT-aware）→ 平台已知目录兜底（Unix：homebrew/sdkman/nvm）→ `MissingTool` 错误；Windows PATH 会从 HKCU/HKLM 的最新值刷新，覆盖“应用启动后安装 Bun”等场景。
- `toolchain/mod.rs` `ToolKind` enum 含 Java/Maven/Node/Npm/Pnpm/Yarn/Bun/Python/Go（`parse` 按 name）。Bun 是 Node 包管理器，也可被 Maven 前端构建插件直接调用。
- `toolchain/manifest.rs`：`mise_tool_name`（`:25`）/ `winget_id`（`:56`）**硬编码映射表**，例 `Java "21" → EclipseAdoptium.Temurin.21.JDK`；未收录版本 → `ToolchainVersionInvalid`。
- `toolchain/install.rs`：安装链 = `mise --version` 探测 → `mise install <tool>@<ver>` 优先 → winget 兜底（内部刷新进程 PATH）→ **装完立即重新解析**（「安装成功 ≠ 工具可用」，`:70`）。测试全走 FakeRunner，不碰真机。
- Python/Go/Bun 已进入同一探测/安装/版本列表链路；Bun 的 `package_manager` 可由 `bun.lock` / `packageManager` 扫描得到，Node 启动计划使用 `bun run`。
- `toolchain/discover.rs` 只读枚举 Java/Node/Maven 已装版本；`launcher.rs:621-760` 将服务级 `SUPERTASK_JAVA_VERSION` 等选择置于子进程 PATH/JAVA_HOME，`.java-version` 是无 YAML pin 时的 Java 回退；Maven reactor 的 install 前置与 run 阶段共用该环境。

## 5. 扫描识别（scan.rs，1453 行）

- 现有识别特征（实测 grep）：
  - `pom.xml`（多模块 reactor 解析，`scan.rs:72-76`）→ spring-boot 服务（`:273,680`）；
  - `build.gradle` / `build.gradle.kts`（1.4，`:665-669`）；
  - `package.json`：BFS 收集（浅层优先，`:540,595-600`），`workspaces` 字段的 monorepo 管理文件自身不算服务（`:525`）→ node 服务（`:577`）；
  - `compose.yaml|yml`、`docker-compose.yaml|yml`（`:19-21`）→ compose sidecar 服务（`:840`）。
- 已识别 `pyproject.toml` / `requirements.txt` / `setup.py` / `go.mod` / `go.work`；Node 扫描同时识别 `bun.lock` / `bun.lockb` 和 `packageManager: bun`。
- 深度限制：>4 层给出人话警告（`:148`）。
- 无 yaml 时的草稿生成、打开已有 yaml 的 merge 向导（1.1）都基于此。

## 6. 其他相关模块事实

- **工作区锁**（1.5）：`lock.rs`（321 行）——所有权锁 + `WORKSPACE_LOCKED`；CLI status/logs/doctor 不取锁（cli.md §命令表）。
- **导出/导入包**（1.5）：`pkg.rs`（485 行）——manifest + supertask.yaml，`--with-secrets` 含声明密钥明文；import 只落盘不启动、目标已有 yaml 拒绝（cli.md）。
- **网关**（1.6）：`gateway/`（model / render/ / validate / probe）——render 纯函数零新依赖；probe 三引擎探测进 `ToolchainProbe.gateway`。
- **MCP**（1.5）：在 `crates/supertask-cli/src/mcp.rs`（不在 core）——rmcp stdio，7 工具（`:15-21`），断连清场。
- **错误码**：`error.rs` `ErrorCode` enum 约 101 个变体；soon 机制：`features.rs:51-62` `SOON_COMMANDS`（cloud.login/cloud.sync/ai.complete）+ `FEATURE_SOON` 拒绝，禁止假成功。
- **taskfile.rs**：1.4 Taskfile 导入（映射成 scripts）。

## 7. 对「新增语言 kind」的成本画像（事实归纳，非方案）

必改散点（§2.2 七处）+ 探测/安装扩展（§4）+ 扫描识别（§5，Python 分叉：uv/venv/poetry/系统 python；Go 分叉：`go run .` vs 编译产物）+ IPC/UI（可参照 compose 与 gateway 的先例）。解耦子系统（§3）零改动或近零改动。
