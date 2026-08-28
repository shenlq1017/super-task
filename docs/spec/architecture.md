# SuperTask 引擎架构（后端）

> 实现位置：`crates/supertask-core`  
> 壳：以后 `src-tauri` 只做 IPC 适配，不把业务写进 command 闭包。

---

## 1. 分层

```
React UI
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
crates/supertask-core     # 引擎
src-tauri                 # IPC 薄适配（已脚手架）
src/                      # React UI（已脚手架，业务页占位）
crates/supertask-cli      # 1.5，复用 core
```

壳已脚手架：AppShell + 功能注册表路由 + `session.hello` / `app.load`。其余 command 与运行页按 `docs/plans/2026-08-26-frontend-work-plan.md` 再接 Engine。
