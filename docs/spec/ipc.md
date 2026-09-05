# SuperTask 后端对接契约（IPC）

> 传输：Tauri 2 `invoke`（请求/响应）+ `event`（推送）  
> 协议版本：`protocol = 1`  
> 前端 **禁止** 直接 shell / 任意 fs。所有副作用走本契约。

本文是接口真源。Rust 类型在 `crates/supertask-core` 的 `ipc` 模块；Tauri 命令只做适配。

---

## 1. 两种通道，不要混用

| 通道 | 用途 | 特点 |
|------|------|------|
| **Command**（invoke） | 问一次答一次 | 打开工作区、起停、存 yaml、快照、探测 |
| **Event**（listen） | 持续推送 | 日志行、运行时状态变化 |

**日志不是 Command。** 不要 `invoke` 每行拉一次，也不要每行一个无批次 Event（UI 和 IPC 都会打满）。

| 错误做法 | 正确做法 |
|----------|----------|
| `get_log()` 轮询 | `logs.subscribe` + `st-logs` 批次事件 |
| 每行 `emit("log_line")` | `st-logs` 最多 50ms 或 32 行一批 |
| 把整份日志文件当 invoke 返回 | `logs.snapshot` 只给环形缓冲（默认 ≤2000 行） |
| 健康检查每 2s 推一次 | 仅 **状态变化** 时发 `st-runtime` |

重连（WebView 刷新）：先 `runtime.snapshot` + `logs.snapshot(since_seq)`，再 subscribe。

---

## 2. 公共信封

### 2.1 Command 成功

返回值直接是 `data`（Tauri 惯例）。所有结构带：

```json
{ "protocol": 1 }
```

需要版本协商的命令（`session.hello`）除外，见下。

### 2.2 Command 失败

统一错误（序列化到 Tauri `Err`）：

```json
{
  "protocol": 1,
  "code": "CYCLE",
  "message": "依赖成环：web → api → web",
  "retryable": false,
  "details": { "cycle": ["web", "api", "web"] }
}
```

`code` 是稳定枚举，UI 用 code 分支，`message` 给人看（中文）。

### 2.3 Event 信封

> **事件名约定（2026-08-30）**：Tauri v2 的事件名只允许字母数字与 `-` `/` `:` `_`，
> **不允许点号**（`st.logs` 这类名字会让前端 `listen()` 直接被拒、Rust `emit` 静默失败）。
> 因此全部事件用连字符：`st-runtime` / `st-logs` / `st-metrics` / `st-operation` / `st-term` / `st-ai`。
> 常量真源：core `ipc::event` + 前端 `protocol.ts event`，不要在调用点手写字符串。

```json
{
  "protocol": 1,
  "event": "st-logs",
  "workspace_id": "<canonical path>",
  "ts_ms": 1710000000000,
  "payload": {}
}
```

前端按 `event` 字段分发。Tauri 事件名与 `event` 相同，减少双份。

---

## 3. 会话与特性占位

### `session.hello`

- 通道：Command  
- 入参：`{ "client": "ui", "protocol": 1 }`  
- 出参：

```json
{
  "protocol": 1,
  "engine": "supertask-core",
  "engine_version": "0.1.0",
  "product_version": "1.0.0-dev",
  "os": "windows",
  "features": [
    { "id": "run", "status": "live", "since": "1.0" },
    { "id": "workspaces", "status": "live", "since": "1.1" },
    { "id": "templates", "status": "soon", "since": "1.1" }
  ]
}
```

`features` 是前后端共同的功能注册表。UI 导航、命令面板、后端「是否执行」都读它。`status`: `live` | `preview` | `soon`。

`soon` 的 Command 若被调用：返回 `FEATURE_SOON`（不是 404），`details.since` 告诉版本。禁止静默 no-op 装作成功。

---

## 4. Command 清单

命名：`域.动作`。Tauri 注册同名（或 snake_case 别名一层，文档以点分为准）。

### 4.1 应用

| 命令 | 入参 | 出参 | 说明 |
|------|------|------|------|
| `session.hello` | client, protocol | 见上 | 握手 |
| `app.load` | — | prefs + recents + probe | 启动 |
| `app.savePrefs` | `{ theme, restoreLast, locale }` | `{ ok: true }` | 只写 app data，不写项目；`locale` 1.4 新增 |

### 4.2 工作区

| 命令 | 入参 | 出参 |
|------|------|------|
| `workspace.add` | `{ path }` 必须已存在的绝对目录 | `{ workspace_id, spec, warnings[] }` |
| `workspace.open` | `{ path }` | 同上 + runtime snapshot |
| `workspace.close` | `{ workspace_id }` | `{ ok }` 停掉该仓全部进程后释放 |
| `workspace.detach` | — | `{ ok }` **切换工作区专用**：不停进程，活服务移交后台注册表；重开同根工作区时按 service_id 接管（job 仍存活则直接 Running）。同一应用会话内有效；应用退出时清场 |
| `workspace.forget` | `{ path }`（兼容别名 `id`） | 只改最近列表 / `lastWorkspace`，**不删盘**（若仍打开则先 close） |
| `workspace.scanDraft` | `{ path }` | `{ workspace_id, spec, warnings[], warning_items?[] }` **不写盘**；`warning_items` 为 additive `{code,message}` |
| `workspace.init` | `{ path, spec }` | `{ workspace_id, spec, warnings[], warning_items?[] }` 以给定 spec 初始化新工作区（写盘 supertask.yaml 并打开），记入 recents |
| `workspace.openExplorer` | `{ workspace_id, rel?: string }` | `{ ok }` rel 必须在沙箱内 |
| `system.discover` | — | `ForeignService[]`（pid/name/kind/ports/cwd/cmd_line/parent_pid/cpu_percent/memory_bytes） 本机监听端口的 java/node/python 进程，只读。`cpu_percent` 为整机口径差分采样，**首次调用为 null**；`memory_bytes` 为物理内存工作集（字节）；两者读取失败（受保护进程）均为 null |
| `system.killProcess` | `{ pid }` | `{ ok }` 终止该监听进程整棵树（`taskkill /T /F`）。护栏：pid ≤ 4 / SuperTask 自身 / 当前无 LISTEN 端口 → 拒绝（`JobKill`）；UI 侧二次确认 |
| `workspace.adoptPreview` | `{ workspace_id }` | `AdoptPreviewOut`（§10.16）孤儿进程纳管 dry-run：与当前工作区相关的监听进程 → generic 服务草稿 + 冲突/警告报告。**纯内存计算，不落盘、不杀进程**；命令行与草稿参数已脱敏 |
| `workspace.adoptApply` | `{ workspace_id, choices: [{pid, action}], base_hash }` | `{ spec, hash, warnings[] }` 只新增用户确认的服务草稿；**apply 前重新发现进程**（退出的 pid 按警告跳过），写回走 saveForm（base_hash 冲突 → `YAML_CONFLICT`） |
| `workspace.needsResolve` | `{ workspace_id, refresh? }` | `NeedsResolveOut`（§10.17）声明式需求 needs 解析：逐条给出四态（已存在/可安装/可归档供给/不可满足）+ reason 解释。**resolve-only dry-run：不安装、不下载、不写盘** |
| `workspace.dataList` | `{ workspace_id }` | `DataListOut`（§10.18）数据卷与各自快照列表（created_at 降序）。**只读** |
| `workspace.dataSnapshotCreate` | `{ workspace_id, volume_id, note? }` | `DataSnapshotCreatedOut`（§10.18）为数据卷创建离线快照；绑定服务未停止 → `SNAPSHOT_BUSY` |
| `workspace.dataRestorePreview` | `{ workspace_id, volume_id, snapshot_id }` | `DataRestorePreviewOut`（§10.18）恢复预览：覆盖面与 `remove_count`，纯只读 |
| `workspace.dataRestore` | `{ workspace_id, volume_id, snapshot_id }` | `DataRestoreOut`（§10.18）恢复：整包校验 → stash → 解压 → 失败回滚 |
| `workspace.dataSnapshotDelete` | `{ workspace_id, volume_id, snapshot_id }` | `DataSnapshotDeletedOut`（§10.18）删除单个快照文件 |

`workspace_id` = 规范化绝对路径。之后所有运行时命令带这个 id，**禁止**再传任意路径去 spawn。

1.0 同时只打开 **一个** 工作区。`open` 第二个时先 close 第一个。

### 4.3 YAML

| 命令 | 入参 | 出参 | 语义 |
|------|------|------|------|
| `yaml.get` | `{ workspace_id }` | `{ text, spec, hash }` | 磁盘原文 + 解析结果 |
| `yaml.saveText` | `{ workspace_id, text, base_hash }` | `{ spec, hash, warnings[] }` | 原文覆盖；hash 冲突 → `YAML_CONFLICT` |
| `yaml.saveForm` | `{ workspace_id, spec, base_hash }` | `{ spec, hash, warnings[] }` | 结构化写回，注释丢失 |

`hash`：原文 UTF-8 的 blake3 或 sha256 短显。防止双 Tab 互踩。

### 4.4 运行时（请求/响应）

| 命令 | 入参 | 出参 |
|------|------|------|
| `runtime.snapshot` | `{ workspace_id }` | `{ services: {id: ServiceRuntime}, script?: ScriptRuntime }` |
| `runtime.startOne` | `{ workspace_id, id }` | `{ accepted: true }` 异步，结果走 event |
| `runtime.startAll` | `{ workspace_id }` | `{ accepted, order: string[] }` |
| `runtime.stopOne` | `{ workspace_id, id }` | `{ accepted: true }` |
| `runtime.stopAll` | `{ workspace_id }` | `{ accepted: true }` |
| `runtime.restartOne` | `{ workspace_id, id }` | `{ accepted: true }` |

起停 **立即返回 accepted**，不要等 Maven 起来（可能 40s）。UI 靠 `st-runtime`。

若已 starting：`startOne` → `ALREADY_IN_PROGRESS`。  
缺工具：`MISSING_TOOL`（同步返回，不 accepted）。  
成环：`startAll` 同步 `CYCLE`。

### 4.5 脚本

| 命令 | 入参 | 出参 |
|------|------|------|
| `script.run` | `{ workspace_id, id }` | `{ accepted }` 或 `SCRIPT_BUSY` |
| `script.cancel` | `{ workspace_id, id }` | `{ accepted }` |

**没有** `script.runRaw`。禁止 IPC 传 `cmds`。

### 4.6 探测与工具链（1.2 增量）

| 命令 | 入参 | 出参 |
|------|------|------|
| `toolchain.probe` | — | `{ java…yarn }` 每项 `{ found, version, path? }` + `gradle: { found, version, path }`（1.4 §9：仅信息展示，不提供安装；version 探测 `gradle --version`）+ `managers: { mise, winget }`（1.2） |
| `toolchain.install` | `{ tool, version?, manager?, persist?, base_hash? }` | `{ operation_id }`（1.2 起 live，hub 长操作 `toolchain.install`） |
| `toolchain.upgrade` | 同上 | `{ operation_id }`（1.2 起 live） |

安装行为、错误码（`TOOLCHAIN_*`）、persist 写回语义见 1.2 功能规格 §4/§13.1。`tool` 只接受 `java|maven|node|npm|pnpm|yarn`。

### 4.6.1 Docker（1.3 增量）

| 命令 | 入参 | 出参 |
|------|------|------|
| `docker.probe` | `{ refresh?: bool }` | `{ found, version?, compose_version?, running }`（会话缓存，refresh 强制刷新） |
| `docker.ps` | `{ workspace_id }` | `{ containers: ContainerSummary[] }`（限当前 compose project，无 compose 文件则空） |
| `docker.images` | `{}` | `{ images: ImageSummary[] }`（本机只读，无缓存承诺） |
| `docker.build` | `{ workspace_id, name }` | `{ operation_id }`（长操作 `docker.build`，可取消） |
| `docker.buildCancel` | `{ workspace_id, operation_id }` | `{ ok }`（best effort，已提交层缓存不回滚） |

`ContainerSummary = { service, container_id, image, state, health?, ports: number[] }`；`ImageSummary = { repository, tag, id, size_bytes, created_ms }`。compose 服务的起停/快照/日志/搜索/端口复用现有命令（§4.4/§4.7），无新增；compose 服务构建经 `runtime.build`（kind `compose.build`）。构建事件走 `st-operation`（Engine 内置 hub 与壳层 hub 均桥接）。错误码 `DOCKER_*`/`COMPOSE_*` 见 1.3 规格 §10.1。

### 4.7 日志（请求/响应部分）

| 命令 | 入参 | 出参 |
|------|------|------|
| `logs.subscribe` | `{ workspace_id, sources?: LogSource[], since_seq?: number }` | `{ ok, cursor: { next_seq } }` |
| `logs.unsubscribe` | `{ workspace_id }` | `{ ok }` |
| `logs.snapshot` | `{ workspace_id, source, limit?: number }` | `{ items: LogLine[], next_seq }` |
| `logs.clearView` | `{ workspace_id, source }` | `{ ok }` 只清内存环，**不删文件** |

`sources` 省略 = 当前工作区全部服务+脚本。

### 4.8 占位命令（必须注册）

当前**没有**处于 soon 状态的命令——下列占位均已转 live，注册表 `SOON_COMMANDS` 为空：

（1.1 起已转 live：`templates.list` / `templates.create` / `git.clone` / `git.status` / `git.pull` / `workspace.openIde`，见第 10 节；1.2 起已转 live：`toolchain.install` / `toolchain.upgrade`，见 §4.6；1.3 起已转 live：`docker.probe` / `docker.ps` / `docker.images` / `docker.build` / `docker.buildCancel`，见 §4.6.1；1.6 起已转 live：`gateway.apply` 及全部 `gateway.*`，见 §10.10；2.0 起已转 live：全部 `cloud.*`，见 §10.12；2.1 起已转 live：全部 `ai.*`（含 `ai.complete`）与 `import.readme` / `import.readmeApply`，见 §10.13 / §10.14。）

新增 soon 功能时遵守原约定：命令必须注册，调用一律返回 `FEATURE_SOON`（不是 404），`details.since` 告诉版本，禁止静默 no-op。

### 4.9 环境深化与运维命令（1.2 phase 3–7 与后续增量回填）

以下命令均已注册并被前端调用，补录进契约：

**端口与配置（1.2 §5–§6）**

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `ports.inspect` | `{ workspace_id, id, port? }` | `{ items }`：端口占用 + 引擎托管判定；传 `port` 只查该候选，缺省查全部已配置端口 |
| `ports.suggest` | `{ workspace_id, id }` | `{ candidates }`：建议端口候选 |
| `ports.assign` | `{ workspace_id, id, port, base_hash, restart? }` | `{ operation_id?, spec, hash, restart_required, notes }`：一键改端口并结构化写回；运行中且未确认 `restart` 时只预览不落盘 |
| `env.effective` | `{ workspace_id, id }` | 服务最近一次启动实际注入的生效环境快照（`key/value/source`，source ∈ workspace/env_file/service/port/network/toolchain/other）；引擎自报 |
| `spring.inspect` | `{ workspace_id, id }` | spring-boot 服务项目自身配置静态解析（application.yml/properties，只读不写回） |

**密钥（1.2 §6.4；值只在本次 IPC 中传输，core 不落日志/事件）**

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `secrets.status` | `{ workspace_id }` | 只返回 key 名与状态，**绝不返回值** |
| `secrets.set` | `{ workspace_id, key, value }` | `{ ok, key }` |
| `secrets.delete` | `{ workspace_id, key }` | `{ ok, key }` |
| `secrets.validate` | `{ workspace_id, id? }` | required 缺失只列 key 名 |

**日志运维（1.2 §8；search/export/retention 均为 hub 长操作，进度走 `st-operation`）**

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `logs.search` | `{ workspace_id, source?, query, case_sensitive?, limit? }` | `{ operation_id }`；literal 搜索历史文件，结果 `{ items, truncated, files_scanned }` |
| `logs.export` | `{ workspace_id, source?, query?, case_sensitive?, format, destination_path }` | `{ operation_id }`；`format` 为 text/jsonl；**不覆盖已有文件**；destination 应来自 native save 对话框 |
| `logs.retention.run` | `{ workspace_id }` | `{ operation_id }`；按顶层 `log_retention` 清理轮转文件，结果 `{ deleted_files, deleted_bytes }` |

**指标 / Profile / 构建（1.2 §9–§11）**

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `metrics.snapshot` | `{ workspace_id }` | 最近一次服务进程树指标采样 |
| `metrics.subscribe` / `metrics.unsubscribe` | `{ workspace_id }` | `{ ok }`；控制 `st-metrics` 推送开关 |
| `profiles.list` | `{ workspace_id }` | `{ active }` + 各 profile 的 enabled 服务计数 |
| `profiles.activate` | `{ workspace_id, id, base_hash }` | `{ spec, hash, active }`：结构化保存激活 profile；忙 → `PROFILE_SWITCH_BUSY`；hash 冲突 → `YAML_CONFLICT` |
| `runtime.build` | `{ workspace_id, id }` | `{ operation_id }`（长操作，可取消）：本地服务 package + artifact 解析，**不启动**；compose 服务路由到 `compose build <service>` |

**应用与 AI 扩展**

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `system.metrics` | `{ temp?: "off"\|"auto"\|"fast" }` | 主机级采样（状态栏与「系统监控」页用）：CPU 总占用及四分占比（系统/用户/Nice/闲置）、内存 已用/总量/可用/交换空间、磁盘 已用/总量、CPU 温度、网络上传下载速率与本机 IPv4。整机视角，与 `metrics.snapshot` 口径不同；占比拆分与网络速率为差分采样，首次调用为 null；取不到的字段为 null（Windows 无 Nice，恒 0；交换空间在 Windows 为提交内存口径）；不持久化、不进日志、不上传；Windows `fast` 档常驻采样进程 |
| `system.info` | — | 静态系统信息（监控页「系统信息」卡片用）：`platform`（windows/macos/linux）、`arch`、`osName`/`osVersion`（Windows 注册表 ProductName 与 DisplayVersion+Build；Linux os-release）、`cpuPhysicalCores`/`cpuLogicalCores`、`totalMemoryBytes`、`appVersion`（workspace 统一升版，与安装包一致）。纯静态只读，与 `system.metrics` 动态采样分列；取不到的字段为 null 不伪造为 0；不含主机名/用户名/路径；不持久化、不进日志、不进 MCP |
| `ai.cli.probe` | `{ provider, cli_path?, cli_env? }` | 本机 AI CLI 可执行文件探测（跑 `--version`，**不保存配置**）；2.1「本地 CLI 供应商」支持 |
| `app.writeTextFile` | `{ path, contents }` | `{ ok }`：文本写入用户选定路径（日志视图「下载」等）；路径由前端 save 对话框提供；失败 `LOG_EXPORT_FAILED` |

---

## 5. Event 清单

### `st-runtime`

**何时发：** 任一服务/脚本的 `state`、`pid`、`lastError`、`health.ok` **发生变化**。健康检查成功但状态已是 running → **不发**。

```json
{
  "protocol": 1,
  "event": "st-runtime",
  "workspace_id": "...",
  "ts_ms": 0,
  "payload": {
    "reason": "state",
    "services": { "web": { "id": "web", "state": "starting", "pid": 1234 } },
    "script": null
  }
}
```

payload 可以是 **增量**（只含变化的 id）。UI 应 merge；想省事可每次 snapshot 全量（1.0 服务 ≤64，全量可接受）。1.0 实现选用 **全量快照** 简化正确性，避免漏 merge。性能：状态变化低频，全量没问题。

### `st-logs`（流式）

```json
{
  "protocol": 1,
  "event": "st-logs",
  "workspace_id": "...",
  "ts_ms": 0,
  "payload": {
    "items": [
      {
        "seq": 1844,
        "source": { "kind": "service", "id": "user-api" },
        "stream": "stdout",
        "ts_ms": 1710000000123,
        "text": "Started DemoApplication"
      }
    ]
  }
}
```

| 字段 | 规则 |
|------|------|
| `seq` | 工作区级单调递增 u64，从 1 起。用于 snapshot `since_seq` 与去重 |
| `source.kind` | `service` \| `script` \| `system` |
| `stream` | `stdout` \| `stderr` \| `system` |
| `text` | 单行，**不含** `\n`；超 **8 KiB** 截断并加 `…` |
| 批次 | 满 32 条 **或** 距首条 50ms，先到先发 |
| 积压 | 有界队列 4096 条；溢出丢 **最旧** 事件（环里仍在，可 snapshot 补） |
| 订阅 | 未 `subscribe` 不推事件（省 CPU）。环仍写 |

`system` 行：引擎自己的说明（「依赖 api 未启动」），给 UI 时间线，不进 Maven 文件也可进 `.supertask/logs/system.log`。

### `st.script`

脚本状态变化（running/exited）。也可并进 `st-runtime.script`。1.0 只走 `st-runtime`，本事件 reserved，不必发。

---

## 6. `ServiceRuntime` 形状

```json
{
  "id": "user-api",
  "state": "stopped | starting | running | unhealthy | stopping | exited",
  "pid": 1234,
  "port": 8081,
  "kind": "spring-boot",
  "health": { "ok": true, "at_ms": 0, "detail": "http 200" },
  "started_at_ms": 0,
  "last_exit": { "code": 1, "at_ms": 0 },
  "exit_reason": "crash",
  "restart_attempt": 2,
  "last_error": null,
  "log_seq": 1844
}
```

`state` 枚举与 YAML 无关，是运行时。UI 翻译中文。

`exit_reason`：`"crash"`（意外退出）/ `"stop"`（手动停止）；缺省 = 无退出记录（崩溃通知用）。
`restart_attempt`（2.2，additive，缺省不序列化）：`restart` 策略自动重启的当前/最近尝试序号（1 起）；
退避等待与重启进行中出现，手动启动后消失。

---

## 7. 错误码

| code | 何时 |
|------|------|
| `PROTOCOL` | protocol 不匹配 |
| `FEATURE_SOON` | 占位命令 |
| `FEATURE_DISABLED` | live 但 enabled false |
| `NO_WORKSPACE` | 未打开工作区 |
| `NOT_FOUND` | 服务/脚本 id 不存在 |
| `NO_YAML` | 目录无配置 |
| `YAML_PARSE` | 语法错误，details.line |
| `YAML_DUP_FILE` | yaml+yml |
| `YAML_TOO_LARGE` | >1MiB |
| `YAML_CONFLICT` | hash 不匹配 |
| `PLATFORM_UNSUPPORTED`（1.4） | 能力在当前平台不可用（如 Linux 更新安装） |
| `BUILD_TOOL_AMBIGUOUS`（1.4） | module 同时存在 Maven 与 Gradle 构建文件 |
| `GRADLE_WRAPPER_MISSING`（1.4） | 无 gradle wrapper 且 PATH 无 gradle |
| `TASKFILE_NOT_FOUND`（1.4） | 工作区无 Taskfile |
| `TASKFILE_INVALID`（1.4） | Taskfile 版本/语法不支持 |
| `SPEC_INVALID` | 校验失败 |
| `SPEC_NEWER` | version>1 警告（若当错误则拒绝打开） |
| `KIND_UNSUPPORTED` | kind 本版不能启动 |
| `LAUNCH_UNSUPPORTED` | 如 jar |
| `CYCLE` | depends_on 环 |
| `MISSING_TOOL` | PATH 无 java/mvn/node… |
| `CWD_MISSING` | 目录不存在 |
| `PATH_ESCAPE` | 路径逃出工作区 |
| `HEALTH_HOST_FORBIDDEN` | 健康检查非 loopback |
| `SPAWN` | CreateProcess 失败 |
| `ALREADY_IN_PROGRESS` | 重复 start |
| `DEP_DEAD` | 依赖 exited |
| `JOB_KILL` | 杀树超时 |
| `SCRIPT_BUSY` | 已有脚本在跑 |
| `PORT_DUP` | 仅警告时走 warnings[]，不作为硬错误（1.0） |

1.1 新增（详见第 10 节）：

| code | 何时 |
|------|------|
| `TARGET_NOT_EMPTY` | 模板/clone 目标目录非空 |
| `TEMPLATE_INVALID` | 内置模板 manifest 或摘要校验失败 |
| `TEMPLATE_WRITE` | 模板复制失败 |
| `GIT_NOT_FOUND` | PATH 无 git.exe |
| `GIT_NOT_REPOSITORY` | 目录不是 Git 仓库 |
| `GIT_DIRTY` | 有未提交修改，默认禁止 pull |
| `GIT_WORKSPACE_BUSY` | 有服务正在运行或切换状态 |
| `GIT_AUTH` | Git 认证失败 |
| `GIT_REMOTE` | remote 不存在或不可访问 |
| `GIT_BRANCH` | 分支不存在或无法跟踪 |
| `GIT_CONFLICT` | pull 产生冲突（保留现场） |
| `GIT_FAILED` | Git 其他非零退出 |
| `IDE_NOT_FOUND` | 固定候选中没有目标 IDE |
| `AUTOSTART_FAILED` | 开机启动注册失败 |
| `UPDATE_BLOCKED_RUNNING` | 工作区仍有运行中任务 |
| `UPDATE_SIGNATURE` | 更新包签名校验失败 |
| `UPDATE_FAILED` | 更新检查/下载/安装失败 |

2.2 新增（详见第 10 节）：

| code | 何时 |
|------|------|
| `NEEDS_INVALID` | needs 项非法（id/版本要求格式、lts 别名、条目数超限） |
| `DATA_INVALID` | `data.volumes` 声明非法（卷 id/`dir` 沙箱、`.supertask` 排除、卷间重复或嵌套、绑定服务不存在，§10.18） |
| `SNAPSHOT_NOT_FOUND` | 数据卷下快照不存在/不可读 |
| `SNAPSHOT_INVALID` | 快照 zip/manifest 损坏、条目哈希不符、zip-slip、超上限（条目数/总字节）、落盘失败 |
| `SNAPSHOT_VERSION` | 快照 format 高于支持版本 |
| `SNAPSHOT_BUSY` | 绑定服务未停止，禁止快照/恢复 |

---

## 8. 安全（契约级）

1. spawn 的 argv 只由 **已加载 spec** 生成，Command 只传 id。  
2. `workspace.openExplorer` 的 `rel` 做 canonicalize，必须 `starts_with(workspace_root)`。  
3. 不提供 `shell.exec`。  
4. 健康检查只允许 loopback。  
5. 日志 Event 不含环境变量值（避免把 secret 再广播一遍；进程自己 print 的拦不住）。  
6. `app.*` 只写应用数据目录，不写工作区。  

---

## 9. 性能预算（1.0）

| 项 | 预算 |
|----|------|
| invoke 常规（snapshot/probe） | p95 < 50ms（不含冷启动探测子进程） |
| `startOne` accepted 返回 | < 20ms（不含等就绪） |
| `st-logs` 批次 | ≤ 20 批/秒/工作区（50ms 聚合） |
| 环形缓冲 | 每源 ≤ ring_lines，内存约 2000×8KiB 上限截断后远小于此 |
| 打开工作区 | 不同时跑第二份 supervisor |

探测 `java -version` 等允许慢（几百 ms），UI 应显示探测中，结果走 `app.load` / `toolchain.probe` 的响应，不走日志流。

---

## 10. 1.1 扩展（模板 / Git / IDE / 扫描合并 / 应用数据 / 更新）

### 10.0 长操作模型 `st-operation`

clone、pull、模板创建、更新下载/安装统一走 operation：

1. Command 校验参数后立即返回 `{ operation_id }`（目标 < 50ms）。
2. 后台线程执行，进度经 `st-operation` 推送。
3. **每个 operation 只有一个终态**；重复终态按 `operation_id` 去重。应用重启后未完成 operation 视为失败/未知，不显示为仍在运行。
4. 不把认证信息、完整环境变量或敏感 URL 写入事件（URL 脱敏）。

```json
{
  "protocol": 1,
  "event": "st-operation",
  "workspace_id": null,
  "ts_ms": 0,
  "payload": {
    "operation_id": "op-123",
    "kind": "git.pull",
    "state": "queued | running | succeeded | failed",
    "progress": 0.4,
    "message": "正在拉取 origin/main",
    "error_code": null,
    "result": null
  }
}
```

`progress` 可为 `null`（不伪造无法测量的百分比）；`succeeded` 时 `result` 为结果对象（如 `{ "workspace_id": "..." }`）；`failed` 时 `error_code` 为稳定错误码。`kind` ∈ `git.clone | git.pull | templates.create | app.update`。

### 10.1 Templates

```text
templates.list
  input:  {}
  output: { templates: TemplateSummary[] }

TemplateSummary = {
  id: string,
  version: string,
  name: string,
  description: string,
  stacks: string[],      // ["spring-boot", "node"]
  files: string[],       // 相对路径概览，仅展示
  source: "builtin" | "local",   // 模板来源（本地 = %APPDATA%/SuperTask/templates/<id>/）
  invalid: boolean,      // 仅 local：清单损坏
  invalid_reason: string | null,
  params?: { key, label, required }[],   // 创建参数声明
  blocks?: { id, label, kind, requires, default_port, services }[],  // 组合模板服务块
}

templates.create
  input:  { template_id, parent_path, directory_name, source?, params?,
            blocks?, ports? }   // blocks/ports 仅组合模板：选中块与服务端口分配
  output: { operation_id }

templates.preview
  input:  { template_id, source?, blocks?, ports?, params? }
  output: { services: object, files: string[], warnings: string[] }
```

`directory_name` 必须是单层目录名（禁止 `..`、路径分隔符、UNC）；目标不存在则创建，存在则必须为空，否则 `TARGET_NOT_EMPTY`。operation `succeeded` 的 `result = { workspace_id }`。创建完成后写入含 `templates:` 元数据的 `supertask.yaml`（组合模板的 yaml 由所选块的 services 片段生成，`{{port}}` 占位随端口分配替换）。与 builtin 同 id 的 local 模板在 list 中跳过、create 拒绝（`TEMPLATE_ID_CONFLICT`）。`templates.preview` 是纯计算，无任何落盘副作用；组合校验（依赖闭合 `TEMPLATE_BLOCK_DEP`、端口查重 `TEMPLATE_BLOCK_PORT`）与 create 共用同一实现。参数错误码：`TEMPLATE_PARAM_MISSING` / `TEMPLATE_PARAM_UNKNOWN`。

### 10.2 Git

```text
git.clone
  input:  { url, target_path, branch? }
  output: { operation_id }

git.status
  input:  { workspace_id }
  output: GitStatus

GitStatus = {
  workspace_id: string,
  is_repository: boolean,
  branch: string | null,     // detached 时 null
  detached: boolean,
  dirty: boolean,
  ahead: number,
  behind: number,
  staged: number,
  unstaged: number,
  untracked: number,
  remote: string | null,
}

git.pull
  input:  { workspace_id, remote?, branch?, allow_dirty? }
  output: { operation_id }
```

约束：

- URL 不允许内嵌 `user:pass@` 凭据；认证交给 Git Credential Manager。
- clone 目标必须不存在或为空目录，否则 `TARGET_NOT_EMPTY`。
- pull 前先 status；`dirty=true` 且未传 `allow_dirty` → `GIT_DIRTY`（Command 同步失败，不发 operation）。
- 有服务处于 `starting/running/unhealthy/stopping` 或脚本运行中 → 同步 `GIT_WORKSPACE_BUSY`。
- 冲突 → operation 失败 `GIT_CONFLICT`，保留现场，不 reset/checkout/clean/stash。
- clone/pull 成功后 `result` 含 `{ workspace_id }`；pull 改动 YAML 时由 UI 提示重新加载。

### 10.3 IDE

```text
workspace.openIde
  input:  { workspace_id, ide }
  output: { accepted: true, ide, path }
```

`ide` 仅接受 `explorer | cursor | idea | code`。后端从 PATH 与固定安装位置探测候选，未命中 `IDE_NOT_FOUND`。返回 `path` 仅展示用。

### 10.4 扫描合并（增量扫描向导）

```text
workspace.scanPreview
  input:  { workspace_id }
  output: { items: ScanMergeItem[], warnings: string[] }

ScanMergeItem = {
  service_id: string,                    // 候选键
  status: "added" | "match_same" | "match_diff" | "missing" | "id_conflict",
  discovered: ServiceSpec | null,
  current: ServiceSpec | null,
  field_diffs: string[],                 // 有差异的扫描器负责字段名
  candidate_id: string | null,           // id_conflict 时的稳定候选 id
  selected: boolean,                     // 默认动作（added=false，其余 true=保留）
}

workspace.scanApply
  input:  { workspace_id, choices: [{ id, action: "add"|"keep"|"update", fields?: string[] }], base_hash }
  output: { spec, hash, warnings }
```

匹配规则：① 同 id；② 同 `kind` 且（spring `module` 相同 / node `dir` 相同）；③ 其余为新发现。`update` 只覆盖扫描器负责字段（`kind`、`module`/`dir`、`package_manager`），用户字段（port/ports/env/env_file/depends_on/health/grace/extra_args/cwd/launch/labels/group/restart/logging/resources/`x-*`）一律保留。未发现项不删除只警告。写回走 `yaml.saveForm` 机制，`base_hash` 冲突 → `YAML_CONFLICT`。

### 10.5 应用偏好与 app data

`app.load` 的 `prefs` 扩展为：

```json
{ "theme": "light", "restoreLast": true, "closeToTray": true, "startOnLogin": false, "updateCheck": true, "locale": "auto" }
```

`app.savePrefs` 接受同样形状（全部可选，只写传入键）。1.4 新增 `locale`（app data v3）：`auto | zh-CN | zh-TW | en-US | ja-JP`，默认 `auto`（跟随系统，检测规则见 1.4 规格 §6.1）。存储位置：`%APPDATA%/SuperTask/app.json`（临时文件 + 替换写入）。
`app.load` additive 字段：`recent_entries?: [{ path, display_name, last_opened_ms? }]`（与 `recents` 同序）、`last_workspace?: string | null`。`workspace.open` / `workspace.init` 成功后会 `record_open` 写入 recents 与时间戳。
`workspace.add` / `open` / `scanDraft` / `init` 的 `WorkspaceOpenOut` additive：`warning_items?: [{ code, message }]`（与 `warnings` 并行）。1.0 的 `st:lastWorkspace` / `st:recents` localStorage 通过一次性迁移命令并入：

```text
app.importRecents
  input:  { recents: string[], last?: string | null }
  output: { ok: true }
```

前端在 app data 为空且 localStorage 有数据时调用一次，成功后清除旧 key。

### 10.6 更新

```text
app.update.check      input: {}                output: { operation_id }
app.update.install    input: { version }       output: { operation_id }
```

- 自动检查跟随 `updateCheck` 偏好（启动后后台一次，失败不阻塞）。
- 发现更新只提示；安装必须用户确认。
- 服务处于 `starting/running/unhealthy/stopping`、脚本运行中、或 Git/模板/扫描 operation 进行中 → 同步 `UPDATE_BLOCKED_RUNNING`。
- 更新包必须通过签名校验，签名失败 `UPDATE_SIGNATURE` 拒绝安装；下载/安装失败 `UPDATE_FAILED`，当前版本保持可用。
- 1.4：Linux `app.update.check` 可用；`app.update.install` 同步返回 `PLATFORM_UNSUPPORTED`（附手动替换 AppImage 指引）。

### 10.7 退出与托盘（壳行为，非 Command）

- 关闭主窗口默认隐藏到托盘（`closeToTray` 可改）。
- 托盘菜单：显示 SuperTask / 打开当前工作区 / 启动全部 / 停止全部 / 退出。
- 退出顺序：标记退出 → `workspace.close` 等待 stopped + Job Object 释放 → 关托盘和窗口；Engine 失败时保留错误，不假报成功。

### 10.8 Taskfile 导入（1.4，feature spec §7）

只读工作区根的 `Taskfile.yml` / `Taskfile.yaml`（不递归、不跟 includes），仅支持 Taskfile **v3**；一次性迁移，之后不监听 Taskfile 变化、不双向同步。预览是纯内存计算，无落盘；Apply 走 `yaml.saveForm` 机制，只增改所选 `scripts.*`，其余字段不动。

```text
import.taskfilePreview
  input:  { workspace_id }
  output: { tasks: TaskfileImportItem[], warnings: string[] }

import.taskfileApply
  input:  { workspace_id, selected: string[] , base_hash }
  output: { spec, hash, warnings: string[] }

TaskfileImportItem = {
  task: string,           # 原名
  script_id: string,      # 目标 id
  cmds_count: number,
  selected: boolean,      # 默认动作
  warnings: string[],     # 该项的忽略/风险说明
  internal: boolean,      # UI 扩展：Taskfile internal 任务，预览标灰不可选
  id_conflict: boolean,   # UI 扩展：目标已存在同名 scripts.*，默认 keep
}
```

映射规则（§7.1）：task 名 → script id（按 id 规则合法化；导入内冲突加 `-task` 后缀并提示）；`desc` → `desc`；`cmds`（字符串或 `cmd:`/`silent:` 映射）→ `cmds`（`silent` 丢弃）；`env` → `env`；`dir` → `cwd`（沙箱校验，逃逸该项警告不导入）；`internal: true` 跳过（预览标灰）；`deps`/`sources`/`generates`/`method`/`status`/`platforms` 忽略并警告；task 级非默认 shell 跳过；cmds 含 `{{…}}` / `$VAR` 插值默认不勾选（警告列出变量，可强制导入原文）；`includes`、动态 task、`loop` 跳过。全局 `env` 合并进每个 task 的 `env`（task 覆盖全局）；全局 `vars` 不解析。

错误：工作区无 Taskfile → `TASKFILE_NOT_FOUND`；版本/语法错误 → `TASKFILE_INVALID`（details 含行号时带上）；`base_hash` 冲突 → `YAML_CONFLICT`。`selected` 不在预览内 → `NOT_FOUND`。解析是纯 YAML 读取 + 文本级检查，不执行任何命令。


### 10.9 导出包（1.5，feature spec §6/§8）

```text
workspace.exportPackage
  input:  { workspace_id, dest_path, with_secrets }
  output: { path, entries: [{ path, bytes }], warnings: string[] }

workspace.importPackage
  input:  { pkg_path, dest_dir? }        # dest_dir 缺省 cwd（CLI 语义）
  output: { root, warnings: string[] }
```

- export 作用于桌面当前工作区（`workspace_id` 不匹配当前 → `NO_WORKSPACE`）；只读操作，不额外取锁。默认排除 `secrets.file` 与全部 `env_file` 声明文件（去重）、`.supertask/`、`.git`；`with_secrets=true` 才逐个入包，UI 需先经风险确认（§9.2）。
- import 只落盘零执行；成功后桌面用返回的 `root` 直接打开工作区。校验链：文件缺失/不可读 → `PKG_NOT_FOUND`；zip/manifest 解析失败、条目哈希不符、路径不安全（zip-slip）→ `PKG_INVALID`；`format` 高于支持版本 → `PKG_VERSION`；目标目录已有 `supertask.yaml` → `PKG_TARGET_EXISTS`（不覆盖，无 force）。
- 包格式：zip（Deflate），`manifest.json { format:1, name, created_at, source_os, app_version, entries:[{path, sha256, bytes}] }` + `supertask.yaml`（原样字节）+ 可选密钥文件；路径一律 `/` 分隔、UTF-8。`format` 只增不破，为 2.0 一键迁移载荷雏形。
- 桌面打开工作区遇 `WORKSPACE_LOCKED`（多入口互斥，feature spec §3.1）：`workspace.open` 以该错误码失败，错误信封新增 additive 可选 `details` 字段（如 `{ holder, pid }`）；protocol 保持 1，旧前端忽略未知字段。

### 10.10 网关（1.6，feature spec §8）

protocol 1 不变，新增 `gateway.*` 命令组；`gateway` feature `soon → live`（since 1.6），
`gateway.apply` 移出占位命令清单（§4.8）。

```text
gateway.status    input:  { workspace_id }
                  output: { configured, enabled, kind?, port?,
                            state?, pid?, last_error?,
                            routes: [{ host?, path, target?, upstream?,
                                       strip_prefix?, cors?, redirect?,
                                       redirect_status?, static_dir?,
                                       target_port?, upstream_alive? }],
                            conf_path? }
                          # state: starting | running | unhealthy | stopped | stopping | exited
                          # upstream_alive: 上游端口 loopback 双栈探测结果
                          # 路由字段与 GatewayRouteSpec 对齐（yaml.md §7.1 三形态）：
                          #   代理 = target/upstream 二选一（可带 strip_prefix/cors）
                          #   重定向 = redirect（+redirect_status，缺省 302）
                          #   静态站点 = static_dir（path 恒为 /）
                          # cors: { origins, methods?, headers?, max_age_secs?, credentials? }

gateway.preview   input:  { workspace_id, gateway? }     # 传配置则渲染草稿，缺省用当前 yaml
                  output: { files: [{ name, content }] } # 纯内存渲染，不落盘

gateway.validate  input:  { workspace_id, gateway? }
                  output: { ok, message?, stderr? }      # 失败不作为 IPC 错误，ok=false 返回

gateway.apply     input:  { workspace_id, gateway, base_hash }
                  output: { spec, hash, restarted, warnings: string[] }
                          # save_form 语义（YAML_CONFLICT 冲突时网关保持运行不受影响）
                          # + 重新生成 + 运行中则重启（stop→start，非热重载）

gateway.start     input:  { workspace_id } → { accepted }
gateway.stop      input:  { workspace_id } → { accepted }
gateway.restart   input:  { workspace_id } → { accepted }
gateway.trust     input:  { workspace_id } → { accepted }
                          # 仅 kind: caddy；spawn `caddy trust`（UI 强制确认在前）
```

- 校验链（start/apply 前置，规格 §6.1）：路由静态校验 → 二进制探测 → 渲染落盘
  `.supertask/gateway/` → spawn `nginx -t -c <conf> -p <prefix> -e stderr` /
  `caddy validate --config <conf> --adapter caddyfile` / `httpd -t -f <conf>`
  （10s 超时，只读命令不常驻）。
- `toolchain.probe` 输出增 `gateway: { nginx: {found,version,path}, caddy: {…}, apache: {…} }`
  （结构对齐 1.4 `gradle` 项）。探测顺序：`gateway.bin` → PATH → 平台已知位置
  （macOS homebrew、Linux /usr/sbin 等；Windows 只认 PATH 与显式 bin）；只探测不代装。
- `st-runtime` 快照新增独立 `gateway` 字段（`GatewayRuntimeView`：kind/state/pid/port/
  health/last_exit/last_error/exit_reason），不进 services 列表——前端勿把网关当服务渲染。
- 日志：网关进程 stdout/stderr 走既有 `st-logs` 批次，source=`{ kind: "gateway", id: "gateway" }`，
  文件 `.supertask/logs/gateway.log`。

1.6 新增错误码：

| code | 何时 |
|------|------|
| `GATEWAY_NOT_CONFIGURED` | 无 gateway 段 / 无 kind / enabled=false 时执行启动类命令；gateway.trust 非 caddy |
| `GATEWAY_ROUTE_INVALID` | 路由静态校验失败（三形态不恰选其一、target 不存在/无端口、path/host 非法（含多域名）、重复、与网关端口冲突、upstream 语法非法、redirect/static_dir/cors 字段非法）；details 带问题列表 |
| `GATEWAY_BINARY_MISSING` | 反代二进制未找到（details/message 带引擎名与平台安装指引；不代装） |
| `GATEWAY_CONFIG_INVALID` | 本机校验失败或超时（details 带 stderr/stdout 原文；Windows 版 `nginx -t` 会真实 bind 端口，端口被外部占用即在此暴露） |
| `GATEWAY_START_FAILED` | 校验通过但 spawn 失败 / 进程立即退出（last_error 进网关日志与状态） |

其余复用现有码（`YAML_CONFLICT`、`ALREADY_IN_PROGRESS`、`JOB_KILL`、`NO_WORKSPACE` 等）。

### 10.11 横向扩展（1.7，feature spec §4–§7）

**零新增命令。** python / go / generic 三 kind 走既有 `runtime.*` / `logs.*` / CLI / MCP 全链路；
服务分组是纯呈现层（`services.*.group` 字段自 reserved 转 live）；崩溃通知是壳层/前端行为
（`st-runtime` 状态迁移 → Toast / 系统通知），均不新增 IPC。

- `toolchain.probe` 输出 additive 扩展：`python` / `go`（`ToolProbe`，旧前端忽略缺省字段）；
  `toolchain.install`/`upgrade` 的 `tool` 参数接受 `python` / `go`（winget：`Python.Python.<maj.min>` / `GoLang.Go`）。
- `yaml.saveForm` 的 `network` 段新增 `python.index_url` / `go.goproxy`（URL 校验同 mirror/registry）。
- 启动 env 注入（`runtime.start`/`up` 链路，优先级最低，显式 env 永远赢）：
  `npm_config_registry`、`PIP_INDEX_URL`、`GOPROXY`、`MAVEN_ARGS="-s <.supertask/maven-settings.xml 绝对路径>"`、
  代理键（`HTTP(S)_PROXY`/`NO_PROXY` 等，off 不注入；健康检查 loopback 始终剥除代理键）。
- 新错误码：`ENTRY_NOT_FOUND`（python entry 文件不存在）、`PACKAGE_NOT_FOUND`（go package 目录不存在）——
  打开时 warning，启动硬错误。其余复用：`SPEC_INVALID`（字段矩阵）、`MISSING_TOOL`、`KIND_UNSUPPORTED`。

### 10.12 云（2.0，feature spec §11）

当前已注册/接线的九条云命令（本地优先：未登录/离线时全部既有功能零变化）：

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `cloud.login` | `{email, password}` | 会话建立（DPAPI 静态加密存储）；失败 `CLOUD_AUTH_FAILED`；密码不得进入返回值/日志 |
| `cloud.logout` | — | 清会话，保留本地数据与同步状态 |
| `cloud.status` | — | `{logged_in, email, device, endpoint, last_synced_ms, conflicts, conflict_ids, telemetry_enabled, quota}`；配额读取失败不阻塞状态展示 |
| `cloud.sync` | — | 两阶段 pull→push；返回 `{pushed, pulled, pending, skipped, conflicts}`；打开中的工作区或无目标目录时 pending |
| `cloud.resolve` | `{entity_id, choice}` | choice ∈ `local` / `server` / `both`（两端内容都保留） |
| `cloud.migrate.plan` | — | 当前实现返回工具链差量；实体清单完整返回仍待补齐 |
| `cloud.migrate.apply` | `{workspaces:[{entity_id, dir}], include_templates?, include_settings?}` | 设定落盘目录并执行一次同步；安装经既有 `toolchain.install`，模板拉取仍 pending |
| `cloud.telemetry.set` | `{enabled}` | `{enabled}`；持久化到 app data 的 `cloud_telemetry`，默认 false；关闭 = 零网络请求 |
| `cloud.endpoint.set` | `{endpoint}` | `{endpoint}`；只允许绝对 `http`/`https` URL，禁止 userinfo、空 host、空白、query 和 fragment；成功后持久化到 app data 的 `cloud_endpoint` 并让后续请求使用新 provider，失败映射 `CLOUD_PROTOCOL_ERROR`。 |

同步状态约束：`cloud.sync` 与 `cloud.migrate.apply` 仅在同步成功后更新时间 `last_synced_ms` 并写入 `cloud/state.json`；同步返回错误时不更新时间、不保存本次同步状态。

云实体相关约束（详见 `docs/spec/cloud.md`）：PUT 必须传 `{type, data, base_rev, updated_by?}`；
实体 id 由客户端提供，是账号范围内稳定 opaque id。客户端解析实体列表必须逐项处理未知 type：
加入 `skipped` 并报告，不能令整个列表失败。需要认证的 HTTP 请求遇 401 时，只 refresh 一次
并只重放一次原请求；refresh 失效清 session 并返回 `CLOUD_AUTH_FAILED`。

- 错误码：`CLOUD_NOT_LOGGED_IN` / `CLOUD_AUTH_FAILED` / `CLOUD_OFFLINE` / `CLOUD_SYNC_CONFLICT` / `CLOUD_ENCRYPT_REQUIRED` / `CLOUD_QUOTA_EXCEEDED` / `CLOUD_PROTOCOL_ERROR`。
- 不新增事件流（同步为短命令）；向导安装进度复用既有 operation 事件桥。
- 协议真源：`docs/spec/cloud.md`；CI/测试全走 `FakeCloudProvider`，零真实网络。自托管参考服务
  的现状与启动约束见 `docs/spec/cloud-server.md`；该 server crate 的本地 HTTP router/API 与 in-process
  集成测试已完成，正式 HTTPS 部署、运营端点和真机验收仍未完成。

### 10.13 AI 助手（2.1，feature spec §4–§5；截图对齐升级）

九条已注册/接线的 AI 命令。数据卫生硬约束（§4.3）：key 永不进入任何返回值/日志/prompt；
yaml 与日志进 prompt 前掩码（secret 值精确替换 + 形似 token/password/authorization 行整行 `<redacted>`）；
零后台调用（全部命令仅用户显式触发）；超预算 `AI_CONTEXT_TOO_LARGE`。

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `ai.status` | — | `{configs:[{id,name,is_default,provider,model,base_url}], default_id, templates:[{id,name,content,enabled}], global_instructions, key_set, usage_today:{date,count}}`；`key_set` 只回布尔 |
| `ai.complete` | `{task, payload, config_id?, request_id?}` | `{text, usage, model, tokens?}`；task ∈ `explain_logs` / `config_suggest` / `enrich_draft` / `test_connection`；`config_id` 缺省用默认配置；传入非空 `request_id` 时经 `st-ai` 事件流式推送文本增量（见 §5），完整结果仍走本命令返回值；重试后成功只计 1 次用量 |
| `ai.config.save` | `{input:{id?, name, base_url, model, provider, auth_method?, timeout_secs?, max_tokens?, context_window?, proxy_enabled?, proxy_url?, max_retries?, api_key?}}` | 保存后完整配置回显（不含 key）；`api_key`：缺省不动 / `""` 清除 / 非空覆盖（写入 secrets 固定 id `supertask.ai`）；name 唯一（大小写不敏感）；首个配置自动成为默认 |
| `ai.config.delete` | `{id}` | 删除命名配置；默认被删后回退首个；旧单配置视图删除 `default` = 清空遗留字段 |
| `ai.config.default` | `{id}` | 设为默认配置 |
| `ai.instructions.save` | `{text}` | 全局自定义指令（trim；空串清除；≤8000 字符），注入所有场景 system |
| `ai.template.save` | `{input:{id?, name, content, enabled}}` | 场景 Prompt 模板（name ≤50 / content ≤8000 / 启用总量 ≤16000 字符，Unicode 计数）；启用者注入 system |
| `ai.template.delete` | `{id}` | 删除模板 |
| `ai.models` | `{config_id?}` | 模型发现：OpenAI 兼容 `GET {base_url}/models` → `Vec<模型 id>`；anthropic 风格报 `AI_REQUEST_FAILED`（手动填模型） |

配置模型（appdata `aiConfigs` 命名多配置 + `aiDefaultConfig`；旧单配置 `ai` 字段作为迁移来源，
首次保存时自动迁入并在落盘时清除）：`{base_url, model, timeout_secs(1–600,默认120), max_tokens(默认8192,上限32768),
provider 预设(8 种 API provider，无 CLI), api_style(openai_completions | anthropic_messages),
auth_method(api_key→Bearer/x-api-key | bearer), context_window?, proxy_enabled?, proxy_url?, max_retries(0–10,默认2)}`。
代理：裸 `host:port` 自动补 `http://`；loopback 端点强制绕过。重试：仅临时错误
（429/500/502/503/504/超时/网络），指数不做、500ms×尝试数线性退避——**偏差备案**：feature spec §4.3
原文「无自动重试」放宽为上述有界重试（对齐用户对齐 dbx 截图的验收预期）。

- 错误码：`AI_NOT_CONFIGURED` / `AI_REQUEST_FAILED` / `AI_TIMEOUT` / `AI_CONTEXT_TOO_LARGE`
  / `README_NOT_FOUND`。
- 不新增事件流；无 CLI provider、无 Agent/Ask 对话模式（IDE 场景由 1.5 `supertask mcp` 覆盖，非目标）。
- 测试：core `ai::` 40+ 单测（fake 传输矩阵，零真实网络）；mock 模式为确定性回文回显。

### 10.14 README 导入（2.1，feature spec §3）

两条已注册/接线的导入命令。导入器为**确定性规则引擎**（纯函数、零网络、零 LLM）：fenced code
block（sh/bash/shell/zsh/console/powershell/text/plain/无标注）+ 行内 code 抽取 → `&&`/`||`/`;`/`|`/
行尾 `&` 链拆 → `VAR=value` / `export|set` 前缀剥离（PORT → 端口提示、其余 → env 提示，均只进
warnings 不直接写字段）→ 规则表分类 service / script / 忽略（章节加权：Run|Getting Started|Quick
Start|Development|启动|快速开始|运行 ×2、Install|安装 ×1.5，行内 code 上限中置信度）→ 归一化
argv 去重（取首个上下文）。与文件系统扫描融合（spec §3.4）：**scan 事实优先**——scan 已识别的
服务为骨架，README 只补全 scan 缺失的 entry/script/extra_args 等字段；字段冲突时 scan 值保留、
README 值经 `fields_meta.readme_value` 进「建议」列（向导双值可见，provenance ∈ scan/readme）。

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `import.readme` | `{workspace_id, path?}` | `{items, script_items?, warnings, readme_path}`：与 `workspace.scanPreview` 同形（`merge::preview_with_sources`），`items[].fields_meta` 携带字段来源+置信度；`script_items` 为脚本合并项（MergeChoice `target:"script"`）；`path` 显式指定且不存在 → `README_NOT_FOUND`；未指定且未发现 → scan 骨架 + 人话提示（非错误） |
| `import.readmeApply` | `{workspace_id, path?, choices, base_hash}` | 同 `workspace.scanApply` 语义：应用前重导入（确定性可重复）→ `merge::apply`（含 `target:"script"` 脚本项，update 整体替换脚本）→ saveForm（hash 冲突 → `YAML_CONFLICT`） |

- 应用链复用 1.1 merge 向导与 `scanApply` 的 base_hash 机制；向导确认是最后闸门，未确认不落盘。
- 错误码：`README_NOT_FOUND`（显式路径不存在）；其余沿用 `YAML_CONFLICT` / `NO_WORKSPACE` 等。
- 测试：core `importer::` 单测（抽取/分类矩阵、章节加权、去重、GBK 解码、scan 融合冲突优先级）
  + `tests/golden/readme/` 五类 golden 快照（spring-node / python / go / 中文 / 纯噪声；无 README
  用临时目录断言提示）；mock 模式确定性样例（README-only 新增 + 冲突建议列 + 端口提示）。

### 10.15 运行页终端（2026-08-30，用户点名实现；UI 设计文档曾标「待排期」）

PTY 终端四条命令 + 一条事件流。会话是 **UI 作用域**：随前端 Tab 挂载打开、卸载关闭，不进
Engine 工作区状态机、不占工作区锁；应用退出时壳层 `close_all` 清场。PTY 复用 wezterm 系
`portable-pty`（Windows ConPTY / Unix openpty）；**UI 永不拼 cmdline**——终端程序由后端决定
（Windows PowerShell 优先回落 cmd；Unix `$SHELL` 回落 bash/sh），前端只传语义参数。

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| `term.open` | `{workspace_id, service_id?, cols?, rows?}` | `{session_id, shell}`：`service_id` 缺省 = 工作区根 + 工作区环境链；指定服务 = 服务 cwd（与启动一致，复用 plan `cwd_rel`）+ §6.3 服务环境链 + 1.7 §7 镜像/代理注入（注入最低优先级）。上限 8 会话（`TERM_LIMIT`） |
| `term.write` | `{session_id, data}` | `{accepted}`：xterm onData 原样透传（回车 `\r`）；会话不存在/已退出 → `TERM_SESSION_NOT_FOUND` |
| `term.resize` | `{session_id, cols, rows}` | `{accepted}`：clamp 2–1000 |
| `term.close` | `{session_id}` | `{accepted}`：幂等；ConPTY 句柄关闭即终止其上进程树（无需 Job Object） |

- 事件 `st-term`：信封 `workspace_id` 恒为 null；负载 `{session_id, kind: "output"|"exited",
  data?, exit_code?}`。`data` 为 lossy UTF-8（含 ANSI 序列，前端 xterm 直接渲染）；`exited`
  后会话自动移除（`wait` 线程 finalize）。
- ConPTY 启动握手：conhost 先发 `\x1b[6n` 等终端回 DSR 光标报告后才渲染——xterm.js 自动回应，
  无需后端处理。
- 错误码：`TERM_SESSION_NOT_FOUND` / `TERM_SPAWN_FAILED` / `TERM_LIMIT`（均随 `TERM_*` 前缀新入码表）。
- 测试：core `term::` 单测（shell 选择、会话缺省错误/幂等关闭）+ `#[ignore]` 真机 ConPTY 冒烟
  （`cargo test -p supertask-core term:: -- --ignored`：开 shell → 代答 DSR → echo 回显 →
  exit → 清场断言）；mock 模式确定性假 shell（help/echo/pwd/ls/dir/ver/date/clear/exit，
  事件序列与真链路同形）。
- 前端：运行页服务抽屉「终端」Tab（`components/terminal-view.tsx`，xterm.js + FitAddon），
  会话随 Tab 卸载关闭；退出/错误态提供「重新打开」。

### 10.16 孤儿进程纳管（方向二·纳管任意来源，2026-09-05）

把发现页看到的本机监听进程反推成服务草稿：先预览（dry-run）、人确认、再写回
`supertask.yaml`。与 Taskfile / scan merge / README 导入同一机制：**preview 纯内存、
apply 前重算、写盘走 `yaml.saveForm`（base_hash 乐观锁）**。核心实现
`crates/supertask-core/src/adopt.rs`；发现页入口（表头「纳管进程」+ 行内 🧲）。

**草稿规则（只写可安全推断的字段）**

- `kind` 恒为 `generic`，`program`/`args` 从命令行反推（忠实复刻原启动方式）；
  不伪造 spring-boot/node 等专用字段（module/entry 推断不可靠，错值会被 spec 校验硬拒）。
  进程归类（java/node/…）仅作为提示字段 `process_kind` 展示。
- `dir` = 进程 cwd 相对工作区根（cwd == 根则缺省；**cwd 不在工作区内则不可纳入**——
  spec 无法表达工作区外目录）；`port` = 首个监听端口；`env` 恒为空（OS 层读不到，
  继承工作区环境）；健康检查留空由 `apply_defaults` 按端口补 tcp。
- `labels: { origin: adopted, adopted-from: "pid <pid> (<name>)" }` 保留来源。
- 服务 id 从 cwd 目录名（或进程名）合法化推导；与现有服务冲突 → `id_conflict` +
  候选 id（`<id>-2` 起），默认不勾。
- **脱敏**：命令行中形似密钥的参数（`--password=x` / `-Dsecret=x` / `key=token` /
  `=Bearer …`）值替换为 `<redacted>`——预览（IPC 返回值）与草稿（写盘内容）都脱敏，
  `=` 独立传值形式不处理（无法可靠配对，宁可放过不误伤）。环境变量不读取不回显。

**`workspace.adoptPreview` 出参 `AdoptPreviewOut { items[], warnings[] }`**
（mirror `adopt.rs::AdoptItem`）

| 字段 | 说明 |
|------|------|
| `status` | `adoptable`（cwd 在工作区内、草稿就绪）/ `matched`（端口已被现有服务声明，重开工作区即被引擎识别为该服务的外部实例，无需纳管）/ `id_conflict` / `unadoptable`（`reason` 说明：cwd 不可读、端口被声明但进程在工作区外等） |
| `pid` / `process_name` / `process_kind` / `ports` / `cwd` | 发现事实（透传 + cwd 相对化解释） |
| `parent_pid` / `parent_name` | 进程关系（`system.discover` additive 字段；`parent_name` 仅当父进程也在发现列表中）；父进程同为监听进程时给确认警告 |
| `cmd_line` | **脱敏后**的完整命令行（展示用） |
| `service_id` / `candidate_id` / `draft` / `warnings[]` / `selected` | 草稿与默认动作（干净项默认勾选，有警告/冲突/参数未知默认不勾） |

包含规则：cwd 在工作区内、或命令行命中工作区根、或端口被现有服务声明；其余进程计入
`warnings` 的「未列入」计数。同一预览内两个草稿撞端口 → 后者警告 + 默认不勾
（`PORT_DUP` 语义前置）。

**`workspace.adoptApply` 入参 `choices: [{ pid, action: "add"|"keep" }]`**
`apply` 前用最新发现快照重算预览：预览到确认之间退出的 pid 按警告跳过（不报错）；
matched / unadoptable 不动作；重复添加同一 pid 或目标 id 已存在 → 跳过 + 警告。
**幂等**：纳管后同进程再次预览即 `matched`，apply 为 no-op。

**错误码**：不新增。`NoWorkspace` / `Discover`（发现表读取失败）/ `YamlConflict`
（base_hash）；草稿错误在保存链路由既有 spec 校验兜底（`SpecInvalid` 等）。

**测试**：core `adopt::` 22 项单测（草稿推导、Windows 引号/反斜杠切词、大小写不敏感
相对化、脱敏（预览 JSON 与 yaml 文本均无明文）、matched/id_conflict/unadoptable、
同端口冲突、apply 幂等、进程退出跳过、to_yaml/parse_yaml 往返校验、确定性）。

### 10.17 声明式需求 needs 解析（方向三·环境供给，2026-09-05）

工作区 YAML 顶层 `needs: ["node@20", "postgres@16"]` 声明高频工具/中间件需求（字段规格
见 yaml.md §7.2），环境页「声明式需求 needs」卡片一键检查。核心实现
`crates/supertask-core/src/needs.rs`。机制与确定性口径：resolve 是**纯只读 dry-run**
（不安装、不下载、不写盘），结果由 (needs 声明, 工具链探测缓存, 内置归档目录, 当前平台)
完全决定，相同输入两次调用结果一致；与 `toolchain.install` 是分工而非替代——resolve
只回答「缺什么、从哪补」，安装执行复用既有 `toolchain.install` 长操作链路。

**解析规则（四态）**

- `satisfied` 已存在 — PATH 探测（`ToolchainProbe`）或安装枚举（`DiscoveredInstall`）
  命中满足版本要求的安装；不重复安装。
- `installable` 可安装 — 已有供给来源可补齐：mise 优先（任意合法版本要求，具体小版本
  由 mise 安装时解析），其次 winget（内置 manifest 白名单，**包 ID 不由用户输入**）；
  执行复用既有 `toolchain.install` 长操作链路，本切片 resolve 本身不执行安装。
- `archive` 可从归档供给 — 内置免安装归档目录（`ARCHIVE_CATALOG`：postgres 16.4 /
  mysql 8.0 / minio 2024，按平台 windows-x64 / linux-x64 / linux-arm64 / darwin-x64 /
  darwin-arm64 声明）；本切片仅报告可供给性，下载/解压执行器是下一切片。
- `unsatisfiable` 不可满足 — `reason` 说明检查过什么、为什么不行、下一步做什么
  （如安装 mise）。

**版本语义**：数值前缀匹配，非区间表达式——`node@20` 被已装 20.x.y 满足、`@20.11`
被 20.11.z 满足；无 `@` = 存在即满足；不支持 `>=` 等比较表达式与 `lts` 别名。条目语法
`id` / `id@version-req`（id：`^[a-z][a-z0-9_-]{0,31}$`；version-req 沿用工具链版本
字符集 ≤32 字符，额外禁止 `-` 前缀与第二个 `@`）；每条 ≤64 字符、每工作区 ≤32 条
（`MAX_NEEDS`）。

**供给语义**：版本 = 前缀要求（非区间）；来源 = 本机已有安装 / mise·winget（机器级
共享，安装目录由 provider 决定，不写工作区）/ 内置归档目录（版本钉死）；隔离 = needs
只声明要求，不产生项目级隔离（两个工作区共用同一本机安装，项目级版本隔离是后续切片）；
失败回滚 = resolve 无副作用，安装失败由 toolchain 链路只报错不清场，YAML 与已有安装
保持原样，重新 resolve 回到 installable。重复声明去重并计入 `warnings`；resolve 幂等
只读，`refresh: true` 才强制重探工具链。

**`workspace.needsResolve` 出参 `NeedsResolveOut { items[], warnings[] }`**
（mirror `needs.rs::NeedItem`）

| 字段 | 说明 |
|------|------|
| `need` / `id` / `version_req?` | YAML 原始声明 / 解析出的需求 id / 版本要求（无 `@` 时缺省） |
| `status` | `satisfied` \| `installable` \| `archive` \| `unsatisfiable`（serde snake_case） |
| `found_version?` / `found_path?` | satisfied：命中安装的版本与可执行文件路径（PATH 命中）或安装根目录（枚举命中） |
| `via?` / `install_version?` | installable：建议供给器 `mise` \| `winget` 与建议安装版本（无版本要求时为 manifest 默认钉扎） |
| `winget_id?` | installable（winget）：manifest 包 ID，绝不由用户输入 |
| `archive_version?` | archive：目录可供给的版本 |
| `reason` | 结论解释：检查过什么、为什么是这个来源/为什么不行、下一步做什么 |

**入参** `{ workspace_id, refresh? }`：`refresh` 缺省 false（复用引擎探测缓存），
true 强制重探工具链。

**安装执行口径**：installable 项由前端「安装」按钮复用环境页既有 `toolchain.install`
流程（hub 长操作，进度走 `st-operation`）；resolve 全程不落盘、无任何副作用。

**错误码**：运行期不新增（不可满足是状态不是错误）；加载期新增 `NEEDS_INVALID`
（id/版本要求格式、lts 别名、`@` 数量、条目数超限，见 §7）。

**测试**：core `needs::` 28 项离线单测（条目语法、版本前缀矩阵、四态判定、平台差异、
端到端演示：resolve → FakeRunner 安装链 → 重新 resolve satisfied）+ `spec::validate`
3 项（round-trip / 非法条目 `NEEDS_INVALID` / 超 32 条）。

### 10.18 数据快照（方向六·数据与备份，2026-09-05）

工作区 YAML 顶层 `data.volumes` 声明数据卷（字段规格见 yaml.md §7.3），本组命令提供
**离线文件快照闭环**：create → list → restorePreview → restore → delete。核心实现
`crates/supertask-core/src/snapshot.rs`；UI 挂工作区页「数据快照」卡片（与导出包同区）。

**快照格式与存储**

- zip = `manifest.json` + `data/<相对路径>`；manifest
  `{format:1, volume_id, service?, note, created_at(epoch ms), source_os, app_version,
  file_count, total_bytes, entries[{path, sha256, bytes}]}`（条目口径同导出包 pkg）。
- 存储 `<工作区根>/.supertask/snapshots/<volume_id>/<created_at_ms>.zip`；先写
  `.tmp` 再改名，不产生半截快照。快照不进工作区导出包（export_package 只打包
  yaml + 密钥）。
- 上限：单快照条目 ≤20000、解压总字节 ≤512 MiB，超出 → `SNAPSHOT_INVALID`
  （附实际值与上限）。
- 只打包普通文件；符号链接跳过并计入 warnings；空目录本身不保留（只保文件）。
- **离线语义**：快照是文件级快照，不做在线/逻辑备份。绑定服务必须处于
  Stopped/Exited（未绑定卷不受限），否则 create/restore 报 `SNAPSHOT_BUSY`；
  预览不受限。

**恢复语义**

- 恢复 = **目录内容替换为快照内容**：先整包逐条 sha256 校验（任何一条不符即
  `SNAPSHOT_INVALID`，不动目标目录）→ 现有目标目录整体改名至 stash
  （`.supertask/snapshots/<volume_id>/.stash-<ts>`）→ 解压落盘（zip-slip 拒绝）→
  成功后删除 stash；解压失败删除半成品、stash 改名回滚，报 `SNAPSHOT_INVALID`。
- 快照外现存文件恢复后**被删除**——`restorePreview` 必须给出 `remove_count`
  （附 ≤20 条 `remove_sample`），UI 据此确认。
- 恢复不重启服务、不改 supertask.yaml；恢复完成后服务按既有启动链路读取恢复后的
  数据。上次恢复中断遗留的 `.stash-*` 在 `dataList` 中以 warnings 提示。

**`workspace.dataList` 出参 `DataListOut { volumes[], warnings[] }`**
（mirror `ipc::v17`）

| 字段 | 说明 |
|------|------|
| `volumes[].id` / `dir` / `service?` | spec 声明原文（`dir` 为工作区相对路径） |
| `volumes[].snapshots[]` | `DataSnapshotView { id, created_at, bytes, file_count, total_bytes, note }`，按 created_at 降序；`id` = created_at 毫秒字符串，`bytes` = zip 文件大小；manifest 损坏的快照文件跳过并计入 warnings |

**`workspace.dataSnapshotCreate`** 入参 `{ workspace_id, volume_id, note? }`；
出参 `DataSnapshotCreatedOut { volume_id, snapshot, warnings[] }`。volume_id 不存在 →
`NOT_FOUND`。

**`workspace.dataRestorePreview`** 出参
`DataRestorePreviewOut { volume_id, snapshot_id, ready, blockers[], target_exists,
current_files, snapshot_files, total_bytes, remove_count, remove_sample[], warnings[] }`。
`ready=false` 的情形（绑定服务运行中等）进 `blockers`；preview 纯只读、零副作用。

**`workspace.dataRestore`** 出参
`DataRestoreOut { volume_id, snapshot_id, restored_files, removed_files, warnings[] }`。

**`workspace.dataSnapshotDelete`** 出参
`DataSnapshotDeletedOut { volume_id, snapshot_id }`。

**错误码**：新增 `DATA_INVALID`（加载期，§7）/ `SNAPSHOT_NOT_FOUND` / `SNAPSHOT_INVALID` /
`SNAPSHOT_VERSION` / `SNAPSHOT_BUSY`（§7）；卷不存在 → 既有 `NOT_FOUND`。

**测试**：core `snapshot::` 12 项离线单测（round-trip、空目录、缺失目录、哈希不符、
未来 format、zip-slip、损坏跳过、stash 回滚、上限、删除）+ `spec::validate` data 段
3 项（round-trip / id·dir·service 非法 / `.supertask` 与嵌套拒绝）+ `engine::tests`
数据快照 3 项（离线闭环、Running 槽注入 `SNAPSHOT_BUSY` 守护、诊断面）。
