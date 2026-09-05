# SuperTask 引擎架构（后端）

> 实现位置：`crates/supertask-core`  
> 壳：以后 `src-tauri` 只做 IPC 适配，不把业务写进 command 闭包。

---

## 1. 分层

```
React UI（frontend/，React 19 + Vite 8 + Tailwind 4）
    │ invoke / listen   （契约：docs/spec/ipc.md）
Tauri commands（薄适配，校验 workspace_id 属于当前会话）
    │
Engine （本 crate）
    ├─ spec     YAML 解析 / 校验 / 写回
    ├─ features 功能注册表（live/soon）
    ├─ graph    depends_on 拓扑
    ├─ runtime  状态机
    ├─ proc     spawn + Job Object（Windows）
    ├─ health   loopback tcp/http
    ├─ log      环 + 文件 + 批次发射
    ├─ probe    PATH 探测
    ├─ sandbox  路径约束
    └─ ipc      类型与错误码（无 Tauri 依赖）
```

以上为 1.0 起的核心子系统。除上述之外，`supertask-core` 还包含以下功能模块（`src/lib.rs` 声明），
按版本逐步落地：`launcher`（各 kind 启动命令规划）、`spring`（Maven/Gradle 深度支持）、
`scan`（扫描项目生成配置草稿）、`merge`（结构化写回合并）、`template`（工作区模板）、
`taskfile`（Taskfile 导入）、`importer`（README 导入）、`adopt`（孤儿进程纳管：发现结果 → generic 服务草稿，ipc.md §10.16）、`toolchain` / `probe`（工具链探测、
mise/winget 安装）、`docker`（compose）、`gateway`（nginx/caddy/apache 渲染与托管）、
`git` / `ide`（git 状态与 IDE 打开）、`ports` / `network` / `host_metrics` / `metrics`
（端口、代理、指标）、`profiles`（多环境覆盖）、`secrets`（密钥声明与存取）、`pkg`
（工作区打包导入导出）、`term`（终端会话）、`cloud`（云客户端）、`ai`（AI 辅助）、
`lock`（工作区锁）、`appdata`、`operation`、`error` 等。

引擎 **不依赖 Tauri**，便于单测和以后做 CLI（1.5）。

---

## 2. 占位怎么做才不会拆房

- **功能注册表**在引擎里，`session.hello` 返回。新版本加 feature 一行，UI 与 `FEATURE_SOON` 命令跟着走。  
- **YAML 具名 reserved 段**（gateway/cloud/…）一等字段，不是「丢掉的 unknown」。  
- **`kind` 为开放字符串**，未知 kind 可加载不可启动。  
- **进程启动走 `Launcher` trait**：`SpringBootRun` / `NodeScript`；`Compose` / `Jar` 占位实现直接 `KIND_UNSUPPORTED`。  
- **不要**为 soon 功能建空的安装器、空的 Docker 客户端。占位停在注册表 + 拒绝码。

---

## 3. 运行时模型

一个 `Engine` 进程内 1.0 **至多一个 ActiveWorkspace**。

```
ActiveWorkspace
  root: PathBuf           # canonical
  spec: SuperTaskFile
  spec_hash: String
  runtimes: HashMap<Id, ServiceSlot>
  script: Option<ScriptSlot>
  log_hub: LogHub
  subscribers: u32        # logs.subscribe 计数
```

`ServiceSlot`：状态、Job 句柄、健康任务 cancel、日志文件。

状态变迁只通过 `runtime::apply`，禁止各处直接改 enum。

启动全部：拓扑序、同层串行、等待到 running|unhealthy|exited 再下一个（见功能规格）。

---

## 4. 性能

| 点 | 做法 |
|----|------|
| 日志 | 读管道的任务只做切行+入环+入有界队列；Event 由聚合器刷 |
| 磁盘 | 写入用 BufWriter；超 max_bytes 截尾，不做每行 fsync |
| 健康 | interval 2s；失败/成功不改变 state 则不 emit |
| 锁 | `parking_lot::Mutex` 分：spec 很少改；runtime 短锁；log 环自己的锁。禁止在锁内 spawn/IO 等待 |
| IPC | 日志订阅才推送；无 UI 时不 emit |
| 启动 | Command 只 enqueue，supervisor 任务执行 |

---

## 5. 安全

| 点 | 做法 |
|----|------|
| 命令注入 | argv 数组，服务启动不经过 shell。脚本 cmds 仅来自 yaml |
| 路径 | `sandbox::confine(root, user_rel)`，canonicalize 后必须前缀匹配 |
| 日志文件名 | 只用校验过的 id，不拼接用户 raw path |
| 健康检查 | 只 loopback |
| 权限 | 无管理员；Job Object 只收本引擎 spawn 的进程 |
| 前端 | 不能传 cmdline；只能传 id |
| 密钥 | yaml 规范禁止存密码；1.0 不读 secrets 段 |

Windows Job Object：`KILL_ON_JOB_CLOSE` + `TerminateJobObject`。创建失败则 **拒绝启动**（`JOB_CREATE`），禁止降级成只杀父进程。

---

## 6. 模块与以后 CLI

```
crates/supertask-core           # 引擎
src-tauri                       # Tauri 2 IPC 薄适配
frontend/                       # React UI（React 19 + Vite 8 + TypeScript + Tailwind 4）
crates/supertask-cli            # CLI 与 MCP，复用 core
crates/supertask-cloud-server   # 云参考服务端（自托管：账号/配额/管理面/控制台托管）
cloud-console/                  # 云管理控制台前端（独立 npm 子项目，构建产物由服务端托管）
```

壳已脚手架：AppShell + 功能注册表路由 + `session.hello` / `app.load`。其余 command 与运行页按 `docs/archive/plans/2026-08-26-frontend-work-plan.md` 再接 Engine。

---

## 7. GatewaySlot（1.6）

网关是引擎的**平级托管对象**，不是 services 成员：不参与 `depends_on` 拓扑、
不进 profile overlay、不被服务启停连带。`Inner.gateway: Option<GatewaySlot>`
——未配置/未启用的工作区为 `None`，热路径零开销。

```
GatewaySlot
  state / pid / port / kind
  job: Arc<dyn ProcessTree>   # 进程树终止、指标聚合、端口排除
  cancel / started / health / last_exit / last_error / exit_reason
```

- 与 ServiceSlot 共享机制：状态机（`runtime::apply`）、日志泵（source=`gateway`，
  GBK/UTF-8 解码 + ANSI 剥离复用）、TCP 健康（loopback 双栈探测自身端口，grace 3s）、
  指标（进程树聚合）、`ports_inspect` 托管进程集合。
- 启动链（`gateway_start` = `up`）：静态校验（`gateway::ensure_static`）→ 二进制探测
  （`gateway.bin` → PATH → 已知位置）→ render 落盘 `.supertask/gateway/` → spawn 校验
  命令（`nginx -t` / `caddy validate` / `httpd -t`，10s 超时，可注入 runner 供测试）
  → spawn 网关进程（nginx `daemon off` 前台 / caddy `run` / httpd 平台 argv）。
  任何一步失败不 spawn：错误码 `GATEWAY_*` 五枚。
- render 是纯函数（IR → 字符串，golden 测试锁定），与引擎、平台解耦；平台差异只在
  argv 与探测，不在配置内容（apache LoadModule 目录由 bin 位置注入）。
- 生命周期：`stop_all` / `close` / `detach`（切工作区终止、不进 DETACHED 移交）/
  CLI `down` / 引擎退出 / MCP 断连清场一律包含网关；`gateway.apply` = save_form 写
  yaml（`YAML_CONFLICT` 时网关保持运行）→ 重新生成 → 运行中则 stop→start（非热重载）。
- 路由 target 服务未运行不阻塞网关启动（转发目标不达是上游的事）；`up` 顺序为先
  服务后网关。

---

## 8. AI 模块（2.1）

AI 是**应用级**能力，不走 workspace 引擎：配置/模板/用量存 appdata（`AppData.ai*` 字段），
key 存应用级 secrets 文件（`%APPDATA%/SuperTask/secrets.env`，逻辑 id `supertask.ai`），
与 workspace `.env.local` 完全隔离。壳层 `src-tauri/src/ai.rs` 是唯一编排点（沿 cloud.rs 先例），
业务在 core `ai/`。

```
crates/supertask-core/src/ai/
  mod.rs        # ProviderConfig / NamedAiConfig / 模板 / key 存取 / complete 编排
  client.rs     # AiHttp trait + UreqAiHttp（ureq）；openai 与 anthropic 双风格请求/解析
  prompt.rs     # 三场景 system/user 构建（纯函数，尾部 200 行 / 32KiB 截断）
  sanitize.rs   # 掩码（secret 值精确替换 + 敏感行整行 <redacted>）
```

- 数据卫生（契约见 ipc.md §10.13）：sanitize 在请求体组装前完成，单测断言原始值
  不出现在请求体；错误文本经 `redact_key` 抹 key；预算按字符 ÷4 粗估，
  `context_window` 更小时按窗口收口，超限 `AI_CONTEXT_TOO_LARGE`。
- 传输参考 dbx 快照（`references/dbx/2026-08-29-8f54385/`，Apache-2.0，只读）：
  per-request timeout、代理判定（裸 host:port 补 http、loopback 绕过）、错误脱敏
  （`categorized_http_error`）、provider 预设表与模板限额均取自该调研
  （`docs/archive/research/2026-08-29-v2-1-dbx-ai-reference.md`）。
- 命名多配置 + 默认配置（dbx `ai_configs` 语义的文件存储版）；旧单配置字段只读迁移。
- 重试偏差备案：仅临时错误（429/5xx/超时/网络）线性退避重试 ≤`max_retries`（默认 2），
  spec §4.3 原「无自动重试」据此放宽；一次业务调用只计 1 次用量。
- 明确不做：CLI provider、Agent/Ask 对话、流式输出、后台/定时调用（IDE 场景由
  1.5 `supertask mcp` 覆盖）。

## 9. README 导入器（2.1）

与 AI 模块同期的**确定性规则引擎**（spec §3）：纯函数、零网络、零 LLM，人确认是最后闸门。
业务在 core `importer/`，壳层命令随 `src-tauri/src/commands.rs`（`import.readme` /
`import.readmeApply`，契约 ipc.md §10.14），应用链复用 1.1 merge 向导与 base_hash 机制。

```
crates/supertask-core/src/importer/
  mod.rs        # 模块入口
  readme.rs     # 发现/解码/抽取/分类/与 scan 融合（含 golden fixtures 测试）
```

- 关键不变式：**scan 事实优先**——文件系统扫描骨架不动，README 只补全缺失字段或新增
  scan 不认识的服务/脚本；字段冲突时 scan 值保留、README 值进「建议」列
  （`merge::FieldMeta` provenance，向导双值可见）。
- 确定性可重复：同输入两次导入结果一致，`import.readmeApply` 因此可以应用前重导入
  （与 `workspace.scanApply` 重扫描同型），不需要把草稿在 IPC 里来回传。
- 噪声与提示：无法解析/忽略的命令计入「N 条命令未识别」warning；`export PORT=…`
  只进端口提示不写 port 字段；secret 形态的 env 提示只记变量名不回显值。
- golden 快照：`tests/golden/readme/`（spring-node / python / go / 中文 / 纯噪声五类，
  fixtures 在 `tests/fixtures/readme/`），更新规则同 gateway golden（重写文件后重跑确认）。
