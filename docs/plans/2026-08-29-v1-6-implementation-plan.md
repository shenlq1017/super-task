# SuperTask 1.6 实现计划

> 日期：2026-08-29  
> 状态：待实现（功能规格默认决策见 §16，本计划按规格 §15 交付顺序拆任务）  
> 功能规格真源：[2026-08-29-v1-6-feature-spec.md](2026-08-29-v1-6-feature-spec.md)  
> 上位：[AGENTS.md](../../AGENTS.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [1.5 复用纪律](2026-08-29-v1-5-feature-spec.md)

把规格 §15 的七步交付顺序拆成可执行任务。行为细节、错误码语义、安全边界以功能规格为准；本计划只定文件、顺序、复用选型与完成标准。配套进度文档 `[2026-08-29-v1-6-progress.md]` 随实现更新。

## 一句话

先把 `gateway:` 段变成 typed 路由模型（spec + 校验 + 五枚错误码），再用纯函数渲染三家配置（golden 测试锁定、零新依赖），随后接上「探测 → `nginx -t`/`caddy validate`/`httpd -t` 校验 → GatewaySlot 托管」的引擎链路，最后 gateway.* IPC、`/gateway` live 页与 CLI 联动收口。网关是引擎的平级托管对象，不是 services 成员。

## 复用核查（2026-08-29，动手前对规格 §10 的逐项核实）

| 用途 | 选型 | 核查结论 | 备选与拒绝理由 |
|------|------|----------|----------------|
| nginx 配置生成 | **自研 render（typed IR → 字符串）** | crates.io 无维护良好的生成器：`nginx-config` 0.13 停维护约 8 年；`nginx-discovery`/`nginx_lint_parser` 均为面向分析的解析器且小生态 | 生成面小（白名单指令集 + golden 测试），引第三方解析器反增加维护风险 |
| 配置模型借鉴 | [nginxconfig.io](https://github.com/digitalocean/nginxconfig.io)（MIT） | Vue 前端项目、无后端无可复用库；借鉴 domain→routing→https 抽象与产物组织，用户文档致谢，不引代码 | — |
| 可视化闭环借鉴 | [nginxWebUI](https://github.com/cym1102/nginxWebUI)（Java+layui） | 借鉴「配置分块 CRUD → 生成 conf → 校验 → 重载 → 状态」的信息架构与交互闭环；open 版 GPL + solon/sqlite/root/web 管理栈与本机桌面模型不兼容，只借思路与术语，不引代码与安全模型 | 引其代码/依赖不可行且无必要（本质一轮询 web 管理端，与本项目引擎托管模型重叠度低） |
| 真值校验 | spawn CLI（`nginx -t` / `caddy validate` / `httpd -t`） | 官方命令行即接口；Windows 版 `nginx -t` 会真实 bind 监听端口（端口占用即失败）——行为纳入测试预期，不绕过 | 所有解析 crate 均自述「不做 nginx -t」 |
| 进程树/日志/健康/指标/端口 | 全部复用现有 core | `proc/`（ProcessTree trait）、log 批次（GBK/UTF-8 + ANSI 剥离）、health TCP 双栈、metrics、`ports::OwnRuntime` | 网关 slot 是这些机制的新用户 |
| caddy 版本/信任 | `caddy version` / `caddy trust` | 官方 CLI；`caddy validate --config X --adapter caddyfile` 会加载 provision 模块，能查出证书/文件级错误 | 不走 admin API（生成 `admin off`，停止靠进程树） |
| 前端可视化 | 现有 shadcn 组件 + Linear 浅色 token | 路由表即图，无图算法需求 | 不嵌图库/第三方配置编辑器 |
| 新增 crate 依赖 | **零**（core 与前端均零新增） | — | 规格红线 |

## 约束（贯穿各 phase）

- 业务只进 `crates/supertask-core`；Tauri/CLI 壳不写业务闭包；`supertask-core` 保持纯同步。
- render 是纯函数：输入 IR 输出文本，无 IO、无平台分支（平台差异只在 argv/探测，不在配置内容——caddy/apache/nginx 配置本身跨平台一致，仅路径分隔与 LoadModule 路径例外，进 render 入参）。
- 未配置 gateway 的工作区零行为变化（`Option<GatewaySlot>` 空路径），既有测试断言不改编通过。
- 生成的配置只含白名单指令；无用户原文注入点；`upstream` 语法校验拒绝 URL/userinfo/scheme。
- 只监听 loopback；`caddy trust` 仅 UI 确认后执行，CLI 不提供。
- 单测/集成测试不得拉起真 nginx/caddy/httpd：用脚本桩（fake 校验桩 exit 0/1 + stderr 固定文案；fake 网关桩监听 TCP）；真机验证进 §14.4 人工矩阵。沿用外部 GUI 隔离与临时目录生命周期纪律。
- YAML `version: 1`、protocol 1、app data v3 不变；yaml 保存带 `base_hash`。
- 文档同步：yaml.md（gateway 段转正）、ipc.md（§10.10 + 错误码表 + features 状态）、architecture.md（GatewaySlot 一节）随对应 phase 落，不留欠账。

## Phase 1 — 路由模型与静态校验（规格 §4、§9.1；最早可启动）

### 任务 1.1 错误码五枚

- **文件：** `crates/supertask-core/src/error.rs`
- **做：** `GATEWAY_NOT_CONFIGURED`、`GATEWAY_ROUTE_INVALID`、`GATEWAY_BINARY_MISSING`、`GATEWAY_CONFIG_INVALID`、`GATEWAY_START_FAILED`（SCREAMING_SNAKE_CASE 序列化，与既有测试口径对齐）。
- **测试：** 序列化快照；旧码不变。
- **完成标准：** 码表与规格 §9.1 一一对应。

### 任务 1.2 spec typed 模型与校验

- **文件：** `crates/supertask-core/src/spec/`（模型定义 + validate.rs 增量；`spec/mod.rs` 挂载）
- **做：** `GatewayConf { kind, enabled, port, bin, tls, routes }`、`GatewayRoute { host, path, target, upstream }`；`gateway: {}` 保持「读回仍在 + 未配置」语义；校验规则按规格 §4.1（kind 枚举、port 1024–65535、(host,path) 去重、target/upstream 恰一、target 存在且有 port、gateway.port 不得撞任一服务 port）；打开工作区 warning、apply/start 硬错误两条路径。
- **测试：** §14.1 校验逐条用例；`gateway: {}` 与未知字段 round-trip 不丢（沿用 reserved 测试模式）。
- **完成标准：** 旧 yaml（无 gateway 段 / `gateway: {}`）读写行为与 1.5 完全一致（快照回归）。

### 任务 1.3 中间表示 IR

- **文件：** 新建 `crates/supertask-core/src/gateway/mod.rs`、`gateway/model.rs`
- **做：** 渲染前 IR：`ResolvedGateway { kind, port, tls, server_groups: [{host, locations: [{path, upstream: {host, port}}]}], log_dir, prefix }`；`resolve(spec, gateway) -> Result<IR>` 把 target 服务 id 解析为 upstream（端口来自当前 yaml；监听表 v4/v6 选择在引擎调用侧做，IR 接受既定地址，保持纯函数）。
- **测试：** target→upstream 解析、host 分组、空 host catch-all。
- **完成标准：** render 与引擎解耦：IR 不含 engine 类型。

## Phase 2 — render 三家（规格 §5；依赖 1，纯函数可并行推进）

### 任务 2.1 nginx.conf 渲染

- **文件：** `gateway/render/nginx.rs`
- **做：** 按规格 §5.1 白名单指令集：`worker_processes 1`、`daemon off`、pid/error_log/access_log 指 `.supertask/gateway/`、host 分组 server 块（空 host = default_server）、`listen 127.0.0.1:<port>`、location 最长前缀、`proxy_pass` + 标准转发头 + `proxy_http_version 1.1` + WebSocket 升级头、404 兜底 server。
- **测试：** golden 快照：单路由 / 多 host 分组 / catch-all / IPv6 上游 `[::1]`。
- **完成标准：** 快照锁定；输出为自包含单文件。

### 任务 2.2 Caddyfile 渲染

- **文件：** `gateway/render/caddy.rs`
- **做：** 全局 `admin off`；host 分组站点块；`tls internal` → `https://localhost:<port>` + `tls internal`，`tls off` → `http://localhost:<port>`；path matcher 前缀 → `reverse_proxy`。
- **测试：** golden 快照（tls on/off × host 分组）。
- **完成标准：** 同 2.1。

### 任务 2.3 httpd.conf 渲染（简化集）

- **文件：** `gateway/render/apache.rs`
- **做：** 规格 §5.3：`ServerName localhost`、`Listen 127.0.0.1:<port>`、日志路径、最小 LoadModule 集（模块路径前缀作为入参，不内置平台探测）、host 分组 `<VirtualHost>`、`ProxyRequests Off` + `ProxyPreserveHost On` + `ProxyPass/ProxyPassReverse`。
- **测试：** golden 快照（模块前缀注入、多 VirtualHost）；WS 不支持的注释行存在（供 UI 提示对齐）。
- **完成标准：** 同 2.1；渲染不 spawn、不读文件。

## Phase 3 — 探测与校验链（规格 §6；依赖 1）

### 任务 3.1 二进制探测

- **文件：** `gateway/probe.rs`（复用 `probe.rs` 的 `resolve_program` 模式与 1.4 平台已知位置补充）
- **做：** nginx/caddy/httpd（Windows `httpd.exe`）解析顺序：`gateway.bin` → PATH → 平台已知位置（macOS homebrew、Linux `/usr/sbin`/`/usr/bin`）；版本命令 `nginx -v`（stderr）/ `caddy version` / `httpd -v`；`toolchain.probe` 输出增 `gateway: { nginx, caddy, apache }`（对齐 1.4 `gradle` 项结构）。
- **测试：** PATH 命中/未命中、显式 bin 优先、版本输出解析（含 stderr 场景）；探测只读不改 PATH。
- **完成标准：** 未命中 → `GATEWAY_BINARY_MISSING` details 带引擎名 + 平台指引文案。

### 任务 3.2 校验链执行器

- **文件：** `gateway/validate.rs`
- **做：** `validate_gateway(root, ir, kind) -> Result<()>`：render 落盘 `.supertask/gateway/` → spawn `nginx -t -c <abs> -p <prefix> -e stderr` / `caddy validate --config <abs> --adapter caddyfile` / `httpd -t -f <abs>`（10s 超时）→ 非零退出 `GATEWAY_CONFIG_INVALID`（details 带 stdout/stderr 原文）；spawn 失败（二进制缺失）→ `GATEWAY_BINARY_MISSING`。
- **测试：** 校验桩（脚本 exit 0/1 + 固定 stderr）：三引擎错误映射、超时路径、stderr 原文进 details。
- **完成标准：** §14.1 校验链用例全绿；不启动常驻进程。

## Phase 4 — 引擎托管 GatewaySlot（规格 §7；依赖 2、3）

### 任务 4.1 slot 结构与启动/停止

- **文件：** `crates/supertask-core/src/engine.rs`（+ 必要时 `gateway/runtime.rs`）
- **做：** `Engine` 增 `gateway: Option<GatewaySlot>`（未配置零开销）；`gateway_start()` = 静态校验 → 探测 → render 落盘 → validate → spawn（nginx `daemon off` 前台 / caddy `run --config` / httpd 平台 argv，§5）→ Starting；日志泵 source=`gateway`（解码/剥 ANSI 复用）；TCP 健康 `127.0.0.1:port` 双栈回退；`gateway_stop()` 进程树终止（grace → kill，`JOB_KILL` 口径）；启动失败/立即退出 → `GATEWAY_START_FAILED`。
- **测试：** fake 网关桩（监听 TCP 脚本）：start→healthy→stop 全链、失败清场、无残留断言；未配置工作区全量既有测试零改动通过。
- **完成标准：** 规格 §7 行为逐条有测试；Windows 桩路径与 Unix 桩路径都在 CI 跑。

### 任务 4.2 纳入清场与端口/快照

- **文件：** `engine.rs`（`stop_all`、`close`/`detach`、快照结构）、`ports.rs`（`OwnRuntime` 聚合含 gateway 进程树）
- **做：** `stop_all`/引擎退出/CLI 清场包含网关；`ports_inspect` 自身排除集合加 gateway 树；运行时快照/事件（`st.runtime` 或网关专行，结构在实现期对齐既有快照——倾向快照内独立 `gateway` 字段，避免复用 services 列表造成前端误当服务渲染）。
- **测试：** stop_all 含网关、端口自身排除回归、快照结构序列化。
- **完成标准：** 1.2 端口语义对网关成立；1.5 清场语义对网关成立。

### 任务 4.3 apply / restart 语义

- **文件：** `engine.rs`
- **做：** `gateway_apply(conf, base_hash)`：save_form 写 yaml（`YAML_CONFLICT` 路径）→ 重新校验+生成 → 运行中则重启（stop→start，非热重载）→ 返回 `restarted`；`gateway_restart`。
- **测试：** apply 落盘 + 运行中重启 + 冲突路径 + 未运行仅落盘。
- **完成标准：** 规格 §7 apply 行为逐条有测试。

## Phase 5 — IPC 与壳层（依赖 4）

### 任务 5.1 core IPC 类型 + Tauri 命令

- **文件：** `crates/supertask-core/src/ipc/v16.rs`；`src-tauri/src/commands.rs`；`src-tauri/src/lib.rs` 注册
- **做：** 规格 §8 八条命令（status/preview/validate/apply/start/stop/restart/trust）；`gateway.trust` 只包装 `caddy trust` spawn（UI 确认在前端）；preview 纯内存渲染不落盘。
- **测试：** 类型序列化快照；`require_current_workspace` 口径一致。
- **完成标准：** 命令名点分风格、错误信封与既有一致。

### 任务 5.2 feature 转 live + 契约文档

- **文件：** `crates/supertask-core/src/features.rs`（gateway → Live since 1.6；`gateway.apply` 移出 SOON_COMMANDS）；`docs/spec/ipc.md`（§10.10 + 错误码表 + probe 输出）；`docs/spec/yaml.md`（gateway 段转正）；`docs/spec/architecture.md`（GatewaySlot 一节）
- **测试：** features 测试更新（gateway_is_live_since_1_6；reject_soon 不再拦 gateway.apply）。
- **完成标准：** 前端 `session.hello` 收到 live；文档无欠账。

## Phase 6 — 前端网关页（依赖 5；mock 可随 5.1 类型先行）

**点名 skill**（按 AGENTS.md 约定，动手前先读）：
- 页面与组件：`c:\project\my\super-task\.agents\skills\web-design-guidelines\SKILL.md`（页面审查）+ `<user-home>\.claude\skills\ui-styling\SKILL.md`（Linear 浅色 token）
- 新增 shadcn 组件前：`c:\project\my\super-task\.agents\skills\shadcn\SKILL.md`（先 `npx shadcn@latest docs`）
- Provider/组合：`c:\project\my\super-task\.agents\skills\vercel-composition-patterns\SKILL.md`；性能：`c:\project\my\super-task\.agents\skills\vercel-react-best-practices\SKILL.md`
- 图标/动效清单：`<user-home>\.claude\skills\ui-ux-pro-max\SKILL.md`；浏览器点选验证：`<user-home>\.agents\skills\webapp-testing\SKILL.md`

### 任务 6.1 协议层与 provider

- **文件：** `frontend/src/ipc/protocol.ts`、`api.ts`、`mock.ts`；`frontend/src/providers/`（gateway 状态按 composition 约定挂 runtime 或独立 provider，实现期定，禁止 AppShell 堆 if）
- **做：** gateway.* 类型与 api 封装；mock：demo nginx 网关（3 路由、假探测、假 preview 文本、启停状态机）。
- **测试：** `npm run build`；mock 手测路径可走全流程。
- **完成标准：** 类型与 ipc.md §10.10 一致。

### 任务 6.2 /gateway 页面（五卡 + 空态）

- **文件：** `frontend/src/pages/gateway-page.tsx`（+ 子组件按 composition 拆分；路由自动由 feature 注册表转 live，壳零改动）
- **做：** 规格 §11 五卡一空态：总览（kind 选择/端口/状态/启停/校验按钮）、路由表（行编辑 + 服务下拉带端口徽标 + upstream 切换 + 上游存活 dot + 从服务生成草稿）、配置预览（只读代码块 + 复制 + 校验结果条）、HTTPS 卡（caddy 专属：tls 开关 + trust 确认对话框）、工具链卡（三家探测）；应用变更走 preview diff 确认 → apply（`YAML_CONFLICT`/`GATEWAY_*` 四语呈现）；脏状态离开确认。
- **测试：** 四语 parity 校验；webapp-testing 点选冒烟（空态→草稿→diff→应用→校验→启停）。
- **完成标准：** 页面过 web-design-guidelines 审查清单；`nav.gateway` 已存在无需改注册表。

### 任务 6.3 运行页联动 + 命令面板

- **文件：** `frontend/src/pages/run-page.tsx`（网关状态行/启停入口）、命令面板注册
- **做：** 运行页顶部或服务列表外挂「网关」状态行（不混入 services 列表）；命令面板加「启动/停止网关」（i18n key 四语）。
- **测试：** 面板搜索冒烟 + build。
- **完成标准：** 注册表驱动，无壳层 if。

## Phase 7 — CLI 联动、CI 与真机验收（依赖 4–6）

### 任务 7.1 CLI 纳入

- **文件：** `crates/supertask-cli/src/`（up/down/status/doctor）
- **做：** `up` 在服务等待达标后启动网关（enabled 且有效；失败 → stop_all + 退出 1 + stderr 码）；`down`/`restart` 含网关；`status`（表 + `--json`）网关行；`doctor` 增网关探测摘要。不加 `gateway` 子命令、不加 trust。
- **测试：** CLI 集成（桩网关）：up 含网关、失败清场、status `--json` 快照。
- **完成标准：** 规格 §12 逐条落地。

### 任务 7.2 CI 扩展

- **文件：** `.github/workflows/ci.yml`
- **做：** core 测试自动覆盖网关（Phase 1–4 用例随 `cargo test -p supertask-core` 跑）；CLI 测试随 `cargo test -p supertask-cli`；不新增 job，只在既有 matrix 断言范围自然扩展。
- **完成标准：** 三平台 matrix 全绿。

### 任务 7.3 真机验收（人工矩阵）

- **清单：** 规格 §14.4（nginx/caddy/apache 真实工作区、trust/untrust、无残留、CLI 含网关、1.0–1.5 回归抽样）；Playwright 网关页一条中文用例（真机验收期补）。
- **完成标准：** 进度文档记录矩阵勾选；已知局限（如 Windows nginx -t bind 行为、apache 发行版 LoadModule 路径差异）写入用户文档。

## 完成定义（1.6 收口）

- `cargo test -p supertask-core` / `-p supertask-cli`、`cargo check -p supertask`、前端 `npm run build` + 四语 parity 全绿。
- 未配置 gateway 的既有工作区行为零变化（回归抽样）。
- 三引擎在至少一台 Windows 真机各完成一次「路由 → 校验 → 启动 → 访问 → 停止无残留」全流程。
- 文档闭环：yaml.md / ipc.md / architecture.md / cli.md / 用户文档（含 nginxconfig.io 与 nginxWebUI 致谢）。
