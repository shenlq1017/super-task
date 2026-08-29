# inv-1 · 版本交付盘点（1.0–1.6 已交付功能详单）

> 2026-08-29。盘点稿，供复核。来源：各版本 feature-spec / progress 文档、AGENTS.md、docs/spec/cli.md、roadmap。
> 复核方式：每条括注出处文档；与原文冲突时以原文为准并回改本文。

## 1.0 — 能跑（Windows）

来源：[v1-0-feature-spec](../plans/2026-08-25-v1-0-feature-spec.md)、roadmap §1.0。

- 工作区模型 + `supertask.yaml`（version: 1，services 一等公民、scripts 一次性任务、depends_on 启动链、成环启动硬失败）。
- 无 yaml 时扫描生成草稿（pom.xml 多模块 + package.json）。
- spring-boot 多模块 `run`（`mvn -pl <module> spring-boot:run`，单模块省略 `-pl`）+ node（`<pm>.cmd run <script>`）。
- 端口 / env 管理；日志（`st.logs` 批次事件 + `logs.snapshot`，禁止按行 invoke）；服务状态机；停止杀整棵进程树（Windows Job Object）。
- 工具链探测（JDK/Maven/Node，不代装）；打开源码目录。
- 完整导航壳：模板/环境/容器/网关/Git/云/AI 占位可见（roadmap 原则 3「1.0 就按 2.0 的信息架构占位」）。
- 命令面板骨架（只搜已启用命令）；UI 中文。

## 1.1 — 能开始

来源：[v1-1-feature-spec](../plans/2026-08-27-v1-1-feature-spec.md)、AGENTS.md。

- 模板系统（初始两套以上；后经 2026-08-28 升级扩到 5 套，见下）。
- git clone / pull / 分支状态显示；打开 Cursor / IDEA / VS Code / 资源管理器。
- 系统托盘、自动更新（签名密钥与 updater endpoint/pubkey 仍为占位，见 inv-4）。
- workspaces / discover（发现）独立页面 live（`features.rs:27-28`，since 1.1）。
- 扫描向导升级：可 merge，不是一次性生成。

**模板升级（2026-08-28，Phase 0–4 已落地）**，来源：[templates-upgrade-plan](../plans/2026-08-28-templates-upgrade-plan.md)、AGENTS.md：
- 清单数据化（`template_assets/*/template.yaml`）、来源抽象（builtin/local，local 目录 `%APPDATA%/SuperTask/templates/`）。
- `params` 参数化（`{{key}}` + `apply_to`）、`blocks` 组合引擎、`templates.preview` 纯计算预览 + 前端组合向导。
- 新增错误码 `TEMPLATE_ID_CONFLICT` / `TEMPLATE_PARAM_MISSING` / `TEMPLATE_PARAM_UNKNOWN` / `TEMPLATE_BLOCK_DEP` / `TEMPLATE_BLOCK_PORT`。
- 内置 5 套（目录名实测）：`spring-boot-single`、`spring-multimodule-node`、`spring-multimodule-node-minimal`、`spring-node-combo`、`node-fullstack`。

## 1.2 — 能养活

来源：[v1-2-feature-spec](../plans/2026-08-27-v1-2-feature-spec.md)、[v1-2-progress](../plans/2026-08-28-v1-2-progress.md)。

- 工具链安装/升级（mise 优先、winget 兜底；`toolchain/` 模块：provider/resolver/install/runner，安装后立即重新解析）。
- 端口占用检测 + 一键改端口并写回 YAML（改 `port` 与对应 env 键，yaml.md §4.1）。
- secrets（backend: local/env/file）+ `.env.local` 不进 git；profiles（active + items）。
- network 段 typed：proxy（off/system/custom）、maven.mirror、npm.registry（**运行时注入遗留**，见 inv-4）。
- 日志历史搜索 / 导出 / 保留策略（顶层 `log_retention`）。
- CPU/内存指标（`proc/` 模块，Job Object + unix 对应实现）。
- spring `launch: jar`（bootJar/package → `java -jar`，artifact 识别排除 plain/sources/javadoc）。
- 脚本任务 run/cancel 全链路（`script.run`/`script.cancel`；同工作区同时仅一个脚本；cmds 只来自 yaml）。
- 遗留（progress 明确记名）：系统级崩溃通知、mirror/registry 与 `env_delta` 运行时接线、分组等交互细化、Windows 真机验收。

## 1.3 — 能装箱

来源：[v1-3-feature-spec](../plans/2026-08-28-v1-3-feature-spec.md)、[v1-3-progress](../plans/2026-08-28-v1-3-progress.md)。

- `kind: compose`（`docker compose up -d --no-deps <service>`；`service` 必填；grace 默认 60s、health 默认 tcp(port)）。
- `DockerSpec` typed（compose_file/project_name/builds）；`ServiceSpec.service` 字段。
- 镜像 build/tag；compose 运行时 + 构建 + 扫描导入；`/docker` 页 live。
- `src/docker/` CLI 适配层：固定程序 runner + 2 MiB 输出上限；probe 三态 `DOCKER_NOT_FOUND` / `DOCKER_ENGINE_UNREACHABLE` / `DOCKER_COMPOSE_MISSING`；compose config 解析 + mtime/sha256 缓存。
- 11 个 DOCKER_/COMPOSE_ 错误码。
- 剩余：Docker Desktop 真机验收。

## 1.4 — 能出门

来源：[v1-4-feature-spec](../plans/2026-08-28-v1-4-feature-spec.md)、[v1-4-progress](../plans/2026-08-28-v1-4-progress.md)。

- macOS / Linux 支持（含 `proc/unix.rs`、probe 平台已知目录：homebrew、sdkman、nvm——`probe.rs:92-120`）。
- Gradle 多模块：wrapper 优先（`gradlew[.bat]`），否则 PATH gradle，都无 `GRADLE_WRAPPER_MISSING`；`launch: jar` → bootJar，artifact 在 `module/build/libs`，零候选 `ARTIFACT_MISSING`、多候选 `JAR_AMBIGUOUS`（yaml.md §4.3）。
- 构建工具探测：pom.xml → maven，build.gradle(.kts) → gradle，并存 `BUILD_TOOL_AMBIGUOUS`（yaml.md §4.3）。
- UI 四语 i18n：zh-CN / zh-TW / en-US / ja-JP（locale 文件实测存在），parity 845 keys（AGENTS.md）。
- Taskfile 导入（`taskfile.rs` 模块实测存在）。
- 剩余：打包与真机矩阵。

## 1.5 — 能搬家（离线）

来源：[v1-5-feature-spec](../plans/2026-08-29-v1-feature-spec.md)、[v1-5-progress](../plans/2026-08-29-v1-progress.md)、[docs/spec/cli.md](../spec/cli.md)。

- 工作区所有权锁（`lock.rs`，321 行）：`WORKSPACE_LOCKED`；状态/日志类命令不取锁。
- CLI 全命令集（`supertask-cli` crate；bin 与桌面 dev 产物撞名 `supertask.exe`，dev 用 `CARGO_TARGET_DIR=target-cli`）。命令表（cli.md §命令，11 条）：
  `up`（拓扑启动 → 等待 → 启动网关 → 聚合日志或 `--` 包装）/ `down` / `restart` / `status --json`（服务端口 + 网关行 + 锁持有者）/ `logs` / `script run|cancel` / `export [-o] [--with-secrets]` / `import`（只落盘不启动）/ `doctor`（工具链 + docker + 网关三引擎）/ `mcp` / `version`。全局 `--json`（错误码与 IPC 同表）、`--no-color`。
- 导出/导入 zip 包（`pkg.rs`，485 行）：manifest + supertask.yaml；`--with-secrets` 含声明密钥明文；桌面入口：导出在设置页、导入在 welcome（见 inv-3）。
- MCP：`supertask mcp` stdio 服务器（rmcp 0.3/3.1，Tier-1 依赖复用），7 工具实测（`crates/supertask-cli/src/mcp.rs:15-21`）：
  `supertask_status` / `supertask_start` / `supertask_stop` / `supertask_restart` / `supertask_logs` / `supertask_run_script` / `supertask_cancel_script`；断连清场。
- CI 三平台跑 cli 测试。剩余：§13.4 人工真机项、macOS/Linux 实机抽验。

## 1.6 — 能对外（网关）

来源：[v1-6-feature-spec](../plans/2026-08-29-v1-feature-spec.md)、[v1-6-progress](../plans/2026-08-29-v1-progress.md)、AGENTS.md 当前阶段。

- 顶层 `gateway:` 段转 typed：kind（nginx 一等 / caddy 本机 HTTPS internal CA / apache 简化反代）、enabled、port（1024–65535 且不撞服务端口）、bin、tls、routes（host+path 前缀 → target 服务 id 或 upstream，互斥）。
- 路由模型 → 三家配置 render 纯函数（零新依赖，6 份 golden 测试）。
- 校验链：`nginx -t` / `caddy validate` / `httpd -t`（ValidateRunner 可注入）；打开时 warning、apply/start 硬错误 `GATEWAY_ROUTE_INVALID`。
- 引擎 `GatewaySlot` 平级托管：快照独立 `gateway` 字段、stop_all/close/detach/CLI 清场全纳入。
- `gateway.*` 八条 IPC + `/gateway` live 页（五卡 + 空态 + diff 应用 + trust 确认）。
- CLI `up/down/restart/status/doctor` 全部纳入网关。
- 实现期决策备案（progress 记录）：apply 先落盘后重启、detach 网关不移交、status 不返回 trusted 布尔。
- 剩余：§14.4 真机人工验收矩阵 + Playwright 用例。

## 2.0 — 云客户端自动化范围（服务端未闭环）

来源：[v2.0 implementation plan](../plans/2026-08-29-v2-0-implementation-plan.md)、[cloud.md](../spec/cloud.md)、[cloud-server.md](../spec/cloud-server.md)。

- `supertask-core/src/cloud/`：provider trait、`HttpCloudProvider`（ureq/rustls）、Fake provider、DPAPI 会话、rev 两阶段同步/冲突解决、vault 加密、迁移差量和默认关闭遥测的客户端模块已落地自动化范围。
- 壳层已接线九条 cloud IPC（含 `cloud.endpoint.set`）；CLI 已有 `cloud status/sync/logout`，其中 CLI sync 明确为只读预览，不做落盘同步。
- `crates/supertask-cloud-server` 已加入 workspace；Config、Argon2 auth/token、entity 数据层、AppState、SQLite migration、HTTP router/API、`/healthz`、配额/遥测 handler 和本地 in-process API 集成测试已落地。
- 已完成客户端 `cloud.endpoint.set` Tauri IPC 注册与统一 401 refresh/replay 一次自动化接线；仍未完成或未验收：secrets `sync:true` 与 vault 编排、welcome 云端恢复、settings 遥测 UI、未知 type 服务端放行、正式端点运营/HTTPS 和 v2.0 真机验收。

## 测试基线（2026-08-29，AGENTS.md）

- `cargo test -p supertask-core`：361 全绿（历史：1.2 时 162 → 1.3 时 232 → 现 361）。
- CLI 测试 20 全绿；集成测试 22 通过、1 ignored（1.3 时点数据）；前端 `npm run build` 通过。
- 四语 parity 845 keys。
- 错误码总量：`error.rs` ErrorCode 枚举约 101 个变体（grep 计数，含 DOCKER_/COMPOSE_/TEMPLATE_/GATEWAY_ 系列）。

## 当前阶段定位（AGENTS.md 原文摘要）

1.6 Phase 1–7 自动化范围全部落地，剩 §14.4 真机验收；1.5 剩 §13.4 人工真机项；1.4 剩打包与真机矩阵。即：**1.x 功能面已铺满 1.0–1.6 全部计划项，横向扩展是新主题**。
