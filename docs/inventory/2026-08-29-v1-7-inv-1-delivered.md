# inv-1 · 版本交付盘点（1.0–1.6 已交付功能详单）

> 2026-08-29。盘点稿，供复核。来源：各版本 feature-spec / progress 文档、repository conventions、docs/spec/cli.md、roadmap。
> 复核方式：每条括注出处文档；与原文冲突时以原文为准并回改本文。

## 1.0 — 能跑（Windows）

来源：[v1-0-feature-spec](../plans/2026-08-25-v1-0-feature-spec.md)、roadmap §1.0。

- 工作区模型 + `supertask.yaml`（version: 1，services 一等公民、scripts 一次性任务、depends_on 启动链、成环启动硬失败）。
- 无 yaml 时扫描生成草稿（pom.xml 多模块 + package.json）。
- spring-boot 多模块 `run`（`mvn -pl <module> spring-boot:run`，单模块省略 `-pl`）+ node（`<pm>.cmd run <script>`）。
- 端口 / env 管理；日志（`st-logs` 批次事件 + `logs.snapshot`，禁止按行 invoke）；服务状态机；停止杀整棵进程树（Windows Job Object）。
- 工具链探测（JDK/Maven/Node，不代装）；打开源码目录。
- 完整导航壳：模板/环境/容器/网关/Git/云/AI 占位可见（roadmap 原则 3「1.0 就按 2.0 的信息架构占位」）。
- 命令面板骨架（只搜已启用命令）；UI 中文。

## 1.1 — 能开始

来源：[v1-1-feature-spec](../plans/2026-08-27-v1-1-feature-spec.md)、repository conventions。

- 模板系统（初始两套以上；后经 2026-08-28 升级扩到 5 套，见下）。
- git clone / pull / 分支状态显示；打开 Cursor / IDEA / VS Code / 资源管理器。
- 系统托盘、自动更新（签名密钥与 updater endpoint/pubkey 仍为占位，见 inv-4）。
- workspaces / discover（发现）独立页面 live（`features.rs:27-28`，since 1.1）。
- 扫描向导升级：可 merge，不是一次性生成。

**模板升级（2026-08-28，Phase 0–4 已落地）**，来源：[templates-upgrade-plan](../plans/2026-08-28-templates-upgrade-plan.md)、repository conventions：
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
- UI 四语 i18n：zh-CN / zh-TW / en-US / ja-JP（locale 文件实测存在），parity 845 keys（repository conventions）。
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

来源：[v1-6-feature-spec](../plans/2026-08-29-v1-feature-spec.md)、[v1-6-progress](../plans/2026-08-29-v1-progress.md)、repository conventions 当前阶段。

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
- `crates/supertask-cloud-server` 已加入 workspace；Config、Argon2 auth/token、entity 数据层、AppState、SQLite migration、HTTP router/API、`/healthz`、配额/遥测 handler 和本地 in-process API 集成测试已落地。（**注**：管理面 `/admin/api/*` 与自带 Web 控制台由 v2.0.1 追加，见下文「v2.0.1 — 云管理控制台」。）
- 已完成客户端 `cloud.endpoint.set` Tauri IPC 注册与统一 401 refresh/replay 一次自动化接线；仍未完成或未验收：secrets `sync:true` 与 vault 编排、welcome 云端恢复、settings 遥测 UI、正式端点运营/HTTPS 和 v2.0 真机验收。（~~未知 type 服务端放行~~ 已交付：`tests/api.rs::unknown_entity_type_is_stored_and_filtered`）

## 测试基线（2026-08-29，repository conventions）

- `cargo test -p supertask-core`：361 全绿（历史：1.2 时 162 → 1.3 时 232 → 现 361）。
- CLI 测试 20 全绿；集成测试 22 通过、1 ignored（1.3 时点数据）；前端 `npm run build` 通过。
- 四语 parity 845 keys。
- 错误码总量：`error.rs` ErrorCode 枚举约 101 个变体（grep 计数，含 DOCKER_/COMPOSE_/TEMPLATE_/GATEWAY_ 系列）。

## 当前阶段定位（repository conventions 原文摘要）

1.6 Phase 1–7 自动化范围全部落地，剩 §14.4 真机验收；1.5 剩 §13.4 人工真机项；1.4 剩打包与真机矩阵。即：**1.x 功能面已铺满 1.0–1.6 全部计划项，横向扩展是新主题**。

## 2.1 — AI 块先行落地（2026-08-29；README 导入器暂缓）

来源：[v2.1 implementation plan 执行记录](../plans/2026-08-29-v2-1-implementation-plan.md)、[ipc.md §10.13](../spec/ipc.md)、[architecture.md §8](../spec/architecture.md)。

- `supertask-core/src/ai/`：命名多配置（appdata `aiConfigs` + 默认配置；旧单配置 `ai` 只读迁移）、8 种 API provider 预设（无 CLI provider）、api_style 双风格（OpenAI 兼容 / Anthropic Messages）、auth api-key/bearer、代理（裸 host:port 补 http、loopback 绕过）、key 存应用级 secrets（`supertask.ai`，appdata `secrets.env`，永不入云/不回显）、三场景 prompt + sanitize（尾部 200 行/32KiB、secret 值与敏感行 `<redacted>`）、预算 `AI_CONTEXT_TOO_LARGE`、临时错误线性重试 ≤max_retries（偏差备案）、全局指令 ≤8000 + Prompt 模板（50/8000/启用总量 16000）、按日用量、模型发现。
- 壳层 `src-tauri/src/ai.rs` 九命令：`ai.status/complete/config.save/config.delete/config.default/instructions.save/template.save/template.delete/models`；features ai → Live(2.1)，SOON_COMMANDS 清空。
- 前端：`/ai` 页（配置列表+编辑表单+全局指令+模板+用量+隐私+MCP 说明）、log-view `extraActions` 槽位 + `AiExplainButton`（run/logs 共用）、config RawTab「AI 建议」卡（yaml 围栏整段填入编辑器，不保存）、mock 确定性回文。
- 错误码新增四枚：`AI_NOT_CONFIGURED / AI_REQUEST_FAILED / AI_TIMEOUT / AI_CONTEXT_TOO_LARGE`。
- ~~暂缓：README 导入器（Phase 1）、`/discover` 入口、命令面板三入口~~ → 已在 2026-08-29 第二轮落地（见下）。

## 2.1 第二轮 — README 导入器 + discover 入口 + 命令面板（2026-08-29）

来源：[v2.1 implementation plan 执行记录（第二轮）](../plans/2026-08-29-v2-1-implementation-plan.md)、[ipc.md §10.14](../spec/ipc.md)、[architecture.md §9](../spec/architecture.md)。

- `supertask-core/src/importer/`：README 导入器（确定性规则引擎，零网络零 LLM）——发现（大小写不敏感 `.md`/`.markdown`）、UTF-8→GBK 解码、fenced（含 text/plain）+ 行内命令抽取、`&&`/`;`/`|` 链拆、`VAR=`/`export` 前缀剥离（PORT → 端口提示，其余 env 提示只记变量名）、规则表分类 service/script/忽略、中英章节加权、行内 code 上限 medium、归一化 argv 去重、噪声计数；与 scan 融合 **scan 事实优先**（冲突 scan 保留、README 值进建议列 `merge::FieldMeta` provenance）。
- `merge.rs`：`FieldMeta`/`ScriptMergeItem` + `preview_with_sources()`（脚本合并项）+ `MergeChoice.target: service|script`（缺省 service 兼容 1.1）。
- 错误码 `README_NOT_FOUND`。
- 壳层：`import.readme` / `import.readmeApply`（确定性重导入 + saveForm，§10.14）。
- 前端：共享向导 `components/scan-merge.tsx`（config-page 内嵌向导抽出 + `ProvenanceChips` + `ScriptItemRow`，config/discover 共用）；/discover「从 README 导入」入口（空草稿给人话提示卡；`?readme=1` 面板直达）；命令面板三入口（README 导入 / AI 解释当前日志（`logs.snapshot` 尾 200 行 + `ai.complete`）/ AI 设置）；mock `import.readme*`。
- 测试：core `importer::` 15 单测 + `tests/golden/readme/` 五类 golden（fixtures `tests/fixtures/readme/`）。

## 运行页终端（PTY）— 2026-08-30 用户点名实现

来源：[运行页终端计划与执行记录](../plans/2026-08-30-run-terminal-plan.md)、[ipc.md §10.15](../spec/ipc.md)。

- core `src/term.rs`：PTY 会话管理器（portable-pty 0.9，wezterm 系 ConPTY/openpty；会话 UI 作用域，上限 8，输出 lossy UTF-8 经 mpsc → 壳层 `st-term` 桥）+ `default_shell()`（PowerShell 优先回落 cmd / `$SHELL` 回落 bash/sh）+ `#[ignore]` 真机 ConPTY 冒烟（交互路径：DSR 握手 → echo → exit → 清场）。
- `Engine::term_target`：服务终端 cwd（复用 plan `cwd_rel` + `resolve_cwd`）与环境链（复用 `build_service_env`，含 1.7 镜像/代理注入）。
- 错误码 `TERM_SESSION_NOT_FOUND` / `TERM_SPAWN_FAILED` / `TERM_LIMIT`；命令 `term.open/write/resize/close`；事件 `st-term`。
- 壳层 `src-tauri/src/term.rs`：四命令 + `st-term` 桥线程 + 退出 `close_all` 清场（`request_exit` 先终端后引擎）。
- 前端：运行页服务抽屉「终端」Tab 转正（`components/terminal-view.tsx`，xterm.js 6 + FitAddon + WebLinks，ResizeObserver 防抖 resize，退出/错误态重开）；locked「待排期」占位移除；mock 确定性假 shell（help/echo/pwd/ls/dir/ver/date/clear/exit）。

## v2.0.1 — 云管理控制台（2026-08-30）

来源：[v2.0.1 规格](../plans/2026-08-30-v2-0-1-cloud-admin-console-spec.md)、[实施计划与执行记录](../plans/2026-08-30-v2-0-1-cloud-admin-console-plan.md)、[cloud-server.md §8](../spec/cloud-server.md)。

- **范围变更**：网页控制台原列 v2.0 非目标，本次拉进并交付；v2.0 规格 §3 已就地标注。
- 服务端 `crates/supertask-cloud-server`：`admin.rs`（`require_admin` 角色闸门、账号列表带用量聚合、建号/改角色/停用/改密/删除、自我保护、`bootstrap_admin`）+ `admin_http.rs`（十条 `/admin/api/*`）+ `migrations/0002_admin.sql`（`accounts.role`，只接受 `user`/`admin`）+ `tests/admin.rs`（8 条）。
- 管理码 `ADMIN_FORBIDDEN` / `ADMIN_NOT_CONFIGURED` 独立于 `CLOUD_*`；`/admin/api` 收紧同源 CORS，客户端 API 的 permissive 不变；`/admin/` 静态控制台按文件系统读 `SUPERTASK_CONSOLE_DIR`（默认 `cloud-console/dist`），缺 `index.html` 回落构建提示页。
- 环境变量新增 `SUPERTASK_ADMIN_EMAIL` / `SUPERTASK_ADMIN_PASSWORD`（both-or-neither，禁止默认口令）与 `SUPERTASK_CONSOLE_DIR`。
- **修掉真实隐患**：`PRAGMA foreign_keys` 每连接生效 → `SqliteConnectOptions::foreign_keys(true)`；否则 8 连接池上删账号不级联清 `access_tokens`/`refresh_tokens`/`entities`/`telemetry_batches`（回归测试逐表 `COUNT(*)` 断言归零）。
- 前端 `cloud-console/`：独立 Vite 7 + React 19 + Tailwind 4 + radix 工程（不引 npm workspaces；UI 组件从 `frontend/` 同源复制），hash 路由 + `base:"/admin/"`、`sessionStorage` 存 token、「401 → refresh 一次 → 只重放一次」单飞刷新、zh/en `labels.ts`；页面 `login-page` + `accounts-page`（防抖搜索/表格/新建/改密/危险操作二次确认/自身行守卫）。根脚本 `console:dev` / `build:console`。
- **零客户端改动**：`core/cloud/*`、`src-tauri/cloud.rs`、`frontend/**`、`ipc.md` 未动；协议层实测「控制台停用 → `/auth/login` 401 `CLOUD_AUTH_FAILED`」。
- CI 新增 `cloud` job（`cargo test -p supertask-cloud-server` + 控制台构建）——**云服务端测试此前完全不在 CI**。
- 本期明确不做：会话/设备吊销（故控制台「退出登录」只清本地会话，服务端 refresh token 30 天内仍有效）、实体内容浏览、每账号配额覆盖、遥测聚合、审计表、批量导入导出、2FA、密码找回。
- 剩余：正式 HTTPS 部署与运营端点（inv-4 D1）、桌面端 `#/cloud` 那一半真机确认。

## 测试基线（2026-08-30 运行页终端 + v2.0.1 云控制台后）

- `cargo test -p supertask-core`：455 全绿（lib 425 + 集成 30；历史：1.2 时 162 → 1.3 时 232 → v2.1 一轮 408 → v2.1 二轮 453 → 现 455；另有真机 ConPTY 冒烟与 AI 端点冒烟 2 项 opt-in ignored）。
- CLI 测试 20 全绿；前端 `npm run build`、`cargo check -p supertask` 通过。
- **云参考服务 `cargo test -p supertask-cloud-server`：14 全绿**（`admin` 单测 3 + `tests/admin.rs` 8 + `tests/api.rs` 3，全部 in-process router + `:memory:`，不占端口不访问公网）；`cloud-console` `npm run build` 通过（342.85 kB JS / 33.23 kB CSS）；`cargo clippy -p supertask-cloud-server --all-targets` 本特性文件零告警。
- 四语 parity 1061 keys。
