# SuperTask 1.0 功能规格（深化）

> 日期：2026-08-25  
> 状态：**技术选型已确认**；本文把 1.0 功能收到可验收、可实现的粒度  
> 上位文档：[v1 摘要](2026-08-25-v1-feature-plan.md) · [UI 占位](2026-08-25-ui-extensibility.md) · [路线](2026-08-25-product-roadmap.md)  
> 栈：[ADR-0002](../adr/0002-tauri-rust-react.md)

本文是 1.0 的功能真源。YAML/IPC 字段与通道以 `docs/spec/` 为准。

---

## 0. 已锁定的技术选型

| 层 | 选定 | 禁止 |
|----|------|------|
| 壳 | Tauri 2（Windows WebView2） | Electron；Tauri+Go sidecar |
| 引擎 | Rust（与壳同进程） | 1.0 再引入 Go |
| UI | React + TypeScript + Vite + shadcn/ui | Vue；Next.js |
| 路由 | React Router | |
| 前端状态 | Zustand（运行时）+ 功能注册表 | 壳上堆 if |
| 配置 | `supertask.yaml` + serde；未知字段进 `extra` | deny_unknown_fields |
| 日志 | 每服务文件 + 内存环形缓冲 | ELK |
| 进程 | Windows Job Object，`KILL_ON_JOB_CLOSE` | 只 kill 父进程 |

Rust crate 方向（实现时再钉版本）：`tauri` 2、`tokio`、`serde`、`serde_yaml`、`thiserror`。Windows：`windows` crate 建 Job Object。

探测与启动一律 **Rust `std::process` / tokio spawn**，前端不拿 shell。

---

## 1. 1.0 要完成的作业

用户打开一个已有的 Maven 多模块 Spring Boot + Node 仓库，不必手写整份 YAML 也能跑起来：看状态、改端口/环境变量、看日志、停干净、打开目录。

三个主路径：

1. **首次**：添加文件夹 → 无 yaml 则扫描生成草稿 → 人改端口 → 启动全部  
2. **日常**：打开最近工作区 → 启动全部 / 单个重启 → 盯日志  
3. **失败**：缺 JDK/Maven/Node、端口没起来、进程崩了 → 说人话，不装成功

---

## 2. 范围切割

### 做

工作区最近列表、扫描生成草稿、yaml 读写、Spring `spring-boot:run`、Node `dev`/`start`、`depends_on` 顺序、起停重启、env/端口表单、原始 YAML 编辑、实时日志、健康（tcp/http）、工具链探测、打开资源管理器、AppShell 全导航占位、命令面板（仅 live 命令）、中文 UI。

### 不做（有占位）

模板、git、mise 安装、Docker、网关、云、AI、托盘、自动更新、`package`+jar、Gradle、日志搜索/导出、CPU 内存、端口占用一键改、密钥文件、PTY 终端、CLI/MCP。

### 1.0 里故意做薄的

- 扫描只覆盖「根 pom 的 modules + 一层子目录 package.json」，不解析 pnpm workspace 图  
- yaml 表单保存 **不保留注释和键顺序**；原始 YAML 页按文本保存  
- 同一工作区同一时间只跑一个 `scripts` 任务  
- 健康检查无历史曲线  

---

## 3. 领域模型

```
App
  recent: WorkspaceRef[]          # 应用数据目录，不进项目
  theme: system | light | dark

Workspace
  root: Path
  spec: SuperTaskFile             # 根目录 supertask.yaml
  runtime: map<ServiceId, ServiceRuntime>
  scriptRuntime: ScriptRuntime | null

ServiceSpec
  id, kind: spring-boot | node
  ...见 schema

ServiceRuntime
  state: stopped | starting | running | unhealthy | stopping | exited
  pid?: u32
  lastExit?: { code, at }
  lastError?: string
  health?: { ok, at, detail }
  logCursor: u64
```

服务 id = yaml 里的 key，`^[a-zA-Z][a-zA-Z0-9_-]{0,63}$`。

---

## 4. 状态机

```
          start                 健康通过
 stopped ──────► starting ──────────────► running
                    │                      │  ▲
                    │ 退出码/spawn 失败     │  │ 健康恢复
                    ▼                      │  │
                  exited ◄── 进程死 ──────┤  │
                    │                      ▼  │
                    │                   unhealthy
                    │                      │
                    │            stop      │
                    └──────── stopping ◄───┘
                                 │
                                 ▼
                              stopped
```

规则：

- **starting**：进程已拉起，在 `grace_secs` 内健康失败不算 `unhealthy`，只显示 starting。超时仍不健康 → `unhealthy`（进程还在）或 `exited`（进程没了）。  
- **running**：进程在，且最近一次健康成功（或 `health: none` 且进程在）。  
- **unhealthy**：进程在，过了 grace 且健康失败。  
- **stopping**：已发杀树，等 Job 结束，超时再 `TerminateJobObject`。  
- **exited**：非 stop 导致的退出。可点启动。  
- 启动中禁止再 start；可 stop（取消）。  
- `health: none`：不进 unhealthy，进程在就是 running。

默认 grace：spring-boot **45s**，node **15s**。yaml 可改。

---

## 5. `supertask.yaml` v1

文件位置：工作区根，优先 `supertask.yaml`，其次 `supertask.yml`。不要两个都有；都有则报错让人选。

```yaml
version: 1
name: mall
root: .                    # 相对文件所在目录，1.0 只允许 "."

env:                       # 工作区默认，服务 env 覆盖
  SPRING_PROFILES_ACTIVE: local

services:
  user-api:
    kind: spring-boot
    module: user-service   # -pl 参数；可写 :user-service 或路径
    port: 8081
    grace_secs: 45
    extra_args: []         # 追加到 mvn 命令
    health:
      type: http           # none | tcp | http
      http: http://127.0.0.1:8081/actuator/health
      interval_secs: 2
      timeout_secs: 2
    env:
      SERVER_PORT: "8081"
    depends_on: []

  web:
    kind: node
    dir: web
    package_manager: pnpm  # npm | pnpm | yarn；可省略=探测
    script: dev
    port: 5173
    grace_secs: 15
    extra_args: []
    health:
      type: tcp
    env:
      PORT: "5173"
    depends_on: [user-api]

scripts:
  bootstrap:
    desc: 安装依赖
    cmds:
      - mvn -q -DskipTests install
      - pnpm --dir web install
    timeout_secs: 1800

# 1.0 忽略但必须 round-trip 进 extra，不得删：
# templates / toolchain / git / docker / gateway / cloud / ai
```

### 校验（保存或启动前）

- `version` 必须为 `1`（更高版本：能读则警告，不能读则拒绝）  
- 至少一个 service  
- `kind` 仅 `spring-boot` | `node`  
- spring-boot 必须有 `module`  
- node 必须有 `dir`  
- `port` 1–65535，同一工作区端口重复 → 警告不阻断（1.2 再阻断）  
- `depends_on` 必须存在；成环 → 拒绝启动并指出环  
- `cmds` 非空字符串数组  

### 端口写入进程环境

启动时合并 env：`系统环境` ⊂ `spec.env` ⊂ `service.env`。然后：

- spring-boot：若结果里没有 `SERVER_PORT`，注入 `SERVER_PORT={port}`  
- node：若没有 `PORT`，注入 `PORT={port}`  

表单改端口：改 `port` + 对应那一个 env 键，写回 yaml。

### 表单 vs 原始 YAML

- **配置页两个 Tab**：表单 | 原文  
- 表单保存：结构化序列化，**注释丢失**，保存前 toast 一次说明  
- 原文保存：原样写盘，再解析；解析失败则拒绝保存并标行号  
- 未保存切换 Tab：若另一侧脏，确认框  

---

## 6. 扫描生成草稿（无 yaml 时）

添加工作区若根目录没有 yaml → 跑扫描 → 弹出草稿预览 → 确认后写入。有 yaml → 只读入。

### Maven

1. 根目录要有 `pom.xml`，否则 spring 服务为 0（允许纯 Node 工作区）  
2. 读 `<modules><module>`  
3. 对每个 module 路径读它的 pom：  
   - 含 `spring-boot-maven-plugin`，或  
   - `packaging` 为 `jar` 且构建里出现 `spring-boot`  
4. 跳过 `packaging=pom` 且无该插件的父模块  
5. `id`：artifactId 转成服务 id（非法字符变 `-`）  
6. `module`：用 pom 里的 module 路径（`-pl user-service`）  
7. `port`：8080 + 下标（8080、8081…），已占用的跳过算法 1.0 不做，只分配递增  
8. `health.http`：`http://127.0.0.1:{port}/actuator/health`  
9. `depends_on`：1.0 **不猜**，全空  

解析 pom 用简单 XML（快速 XML crate），不引入完整 Maven 模型。读失败的 module 跳过并在草稿里列警告。

### Node

1. 候选：`package.json`（根）以及 **仅一层** 子目录的 `package.json`  
2. 跳过：`node_modules`、`target`、`dist`、`.` 开头目录  
3. 根既是 Maven 又有 package.json：若 scripts 只有后端味道（`test`/`build` 无 `dev`/`start`）可跳过根 Node，避免把 Java 仓根当前端  
4. 选用脚本：`dev` 否则 `start`，都没有 → 仍生成但标警告、不能启动直到人选 script  
5. 包管理器：`packageManager` 字段 → 否则 lockfile（`pnpm-lock.yaml` > `yarn.lock` > `package-lock.json`）→ 否则 `npm`  
6. `port`：从 5173 起递增；若 `dir` 像后端 API（scripts 含 `spring`）跳过  
7. `depends_on`：若同时扫到 spring 服务，**每个 node 依赖全部 spring**（粗，但 1.0 能用）。人可在预览里改  

草稿预览可取消不写盘。

---

## 7. 启动命令（Windows）

工作目录：

- spring-boot：工作区根  
- node：`root/dir`  
- scripts：工作区根  

可执行文件：在 `PATH` 上找，Windows 优先 `mvn.cmd`、`npm.cmd`、`pnpm.cmd`、`yarn.cmd`、`node.exe`。找不到 → 失败原因带「当前 PATH 里没有 xxx」。

Spring（1.0 唯一）：

```
mvn.cmd -pl <module> spring-boot:run
```

单模块（`module: "."`）省略 `-pl`。不要默认 `-am`：该 goal 会作用到 reactor 全部项目，聚合 POM 没有 spring-boot 插件时直接失败。also-make 放 `extra_args` 或先跑 bootstrap `mvn install`。

`extra_args` 接在后面。不要 `package`。不要自行拼 java -jar。

Node：

```
<pm>.cmd run <script> -- ...extra_args
```

`pnpm`/`yarn` 同样。`cwd` 已是 `dir` 时不要再 `--dir`，以免混乱。

进程创建：

- 重定向 stdout/stderr 到管道（合并）  
- 不弹控制台窗口  
- 创建后立刻加入 Job Object  
- 环境块用上面的合并结果  
- 工作目录不存在 → 启动失败  

### 停止

1. 状态 → stopping  
2. `TerminateJobObject`（1.0 不做优雅 SIGINT 模拟；Windows 上 Ctrl+C 对 mvn 不可靠）  
3. 等结束，最多 8s  
4. 仍在则记错误，状态 exited/stopped 以 Job 是否空为准  

**停止全部**：按 `depends_on` **逆序**；无依赖关系的并行杀（1.0 也可全串行，更简单则串行）。

### 启动全部

1. 拓扑排序，环则全体不启动  
2. 按层启动：同一层 1.0 **串行**（少踩端口/CPU）  
3. 每个服务等到 `running` 或 `unhealthy`/`exited` 再启动下一个  
4. 依赖 `exited` → 依赖方 **不启动**，原因写「依赖 X 失败」  
5. 依赖 `unhealthy` → **仍启动**（避免 actuator 没配卡死整仓），横幅警告  

单个启动：先启动未在运行的依赖（同一套等待规则）。

---

## 8. 健康检查

| type | 成功 | 1.0 行为 |
|------|------|----------|
| none | 进程还在 | 永不 unhealthy |
| tcp | 对 `127.0.0.1:port` connect 成功 | 默认 node |
| http | GET，**2xx** 为成功 | 默认 spring；actuator 503 算失败 |

- 从工作区本机打，不跟代理走（绕过系统 HTTP_PROXY）  
- interval 默认 2s，timeout 默认 2s  
- 进程死后立刻 exited，停掉检查  

---

## 9. 日志

- 磁盘：`{workspace}/.supertask/logs/{serviceId}.log`  
- 1.0 单文件，超过 **10MB** 截成保留尾部 2MB（不做多文件轮转）  
- 内存：每服务 **2000 行** 环形缓冲，供 UI  
- 行格式：`HH:mm:ss.SSS | stdout|stderr | 原文`（时间本地）  
- UI：跟随底部、暂停跟随、清屏（只清内存视图，不清文件）  
- 编码：按 UTF-8 解，非法字节用替换符；Windows Maven 乱码 1.0 接受，可在设置里预留「控制台代码页」但 **1.0 不做开关**  
- 事件：`log_line { serviceId, line }` 推前端；断开重连时 `logs_snapshot`  

scripts 日志：`scripts/{name}.log`，同一套 UI 过滤器里类型=任务。

建议在扫描时若无 `.gitignore` 项，**不自动改 gitignore**（避免碰用户仓）。设置里一行文案：请自行忽略 `.supertask/`。

---

## 10. 工具链探测

状态栏与 `/env` 页顶共用 `ProbeBar`。

探测：`java -version`、`mvn -v`、`node -v`、以及将用到的包管理器 `-v`。显示：找到/未找到 + 版本第一行摘要。

启动前检查：

- spring 服务：需要 java + mvn  
- node 服务：需要 node + 该服务 package_manager  

缺则该次启动失败，文案示例：「未找到 mvn。请安装 Maven 并确保在 PATH 中。1.2 将支持一键安装。」不打开浏览器乱下。

`/env` 1.0：ProbeBar + ComingSoon（安装升级）。不要假装能装。

---

## 11. 工作区与应用数据

**应用数据**（Tauri app data，例如 `%APPDATA%/SuperTask/`）：

- `app.json`：最近 20 个工作区路径、最后打开的、主题  
- 不存代码、不存 yaml 副本  

**项目内**：

- `supertask.yaml`  
- `.supertask/logs/`  

添加工作区：系统文件夹对话框，必须选已存在目录。移除：只从最近列表拿掉，不删磁盘。

打开目录：`explorer.exe /select,` 或打开 `root`。1.0 只资源管理器。

启动应用：若有「最后打开」且路径仍在，直接进该工作区；否则欢迎页（添加 / 选最近）。

---

## 12. 界面（1.0 逐页）

欢迎页：添加工作区、最近列表（路径、上次打开）。无最近时不要空白无按钮。

**运行 `/run`**

- 顶栏：启动全部、停止全部、打开目录  
- 卡片：id、kind、port、状态点、一键起停重启  
- 选中后右侧抽屉：日志 | 环境 | 健康 | 终端(禁) | 指标(禁) | 容器(禁) | 代理(禁)  
- 环境 Tab：port 数字、env 键值表、保存（写 yaml 并提示需重启才生效）。**运行中保存不自动重启**，黄条「未重启」  
- 健康 Tab：类型、最近结果、最近时间、失败原因  
- 空工作区（0 服务）：引导去配置页扫描  

**日志 `/logs`**

- 左服务列表（含正在跑的 script）  
- 右同一套日志视图  
- 无搜索（占位提示 1.2）  

**配置 `/config`**

- 表单：工作区 name、全局 env、服务列表增删、depends_on 多选  
- 原文 YAML  
- 「重新扫描并合并」：1.0 **不做自动 merge**（易毁掉工 yaml）。只在「尚无服务」时显示扫描按钮  

**占位页**：统一 `ComingSoon`：功能名、版本、一句话。禁止假列表。

**设置**：常规（打开最后工作区）、外观、关于（版本号）、工具链/代理/更新/账号分组标题+即将。

**命令面板**（Ctrl+K）：启动全部、停止全部、打开目录、转到运行/日志/配置/设置。soon 项可搜到，回车显示「将在 x.x 提供」。

文案中文。状态英文枚举只在内部，UI 用：已停止 / 启动中 / 运行中 / 不健康 / 停止中 / 已退出。

---

## 13. 前端功能注册表（1.0 值）

| id | path | status | since |
|----|------|--------|-------|
| run | /run | live | 1.0 |
| logs | /logs | live | 1.0 |
| config | /config | live | 1.0 |
| templates | /templates | soon | 1.1 |
| env | /env | live* | 1.0 探测 / 1.2 安装 |
| git | /git | soon | 1.1 |
| docker | /docker | soon | 1.3 |
| gateway | /gateway | soon | 1.6 |
| cloud | /cloud | soon | 2.0 |
| ai | /ai | soon | 2.1 |
| settings | /settings | live | 1.0 |

`env`：页面 live，安装区 soon。账号按钮 soon。

---

## 14. Rust IPC（1.0 命令面）

前端只通过 invoke/event 说话。

| 命令 | 作用 |
|------|------|
| `app_load` | 读 app.json + probe |
| `app_save_prefs` | 主题、是否恢复工作区 |
| `workspace_add` | 选目录、扫描或加载 yaml |
| `workspace_open` | 按路径打开 |
| `workspace_forget` | 移出最近 |
| `yaml_save_form` / `yaml_save_text` | 两种保存 |
| `scan_draft` | 仅生成草稿不写盘 |
| `probe` | 工具链 |
| `start_one` / `start_all` / `stop_one` / `stop_all` / `restart_one` | |
| `script_run` / `script_cancel` | |
| `logs_snapshot` | 环形缓冲 |
| `open_in_explorer` | |

事件：`runtime_updated`（服务状态）、`log_line`、`script_updated`。

权限：fs 限于工作区 root + app data；无任意 shell。

---

## 15. 错误目录（UI 必须覆盖）

| 码 | 用户可见 |
|----|----------|
| `NO_YAML` | 尚未配置，去生成草稿 |
| `YAML_PARSE` | 第 N 行解析失败 |
| `YAML_DUP_FILE` | 同时存在 yaml 和 yml |
| `CYCLE` | 依赖成环：A → B → A |
| `MISSING_TOOL` | 未找到 mvn/node/… |
| `CWD_MISSING` | 目录不存在 |
| `SPAWN` | 进程无法启动 + OS 信息 |
| `DEP_DEAD` | 依赖 X 已退出，未启动 Y |
| `JOB_KILL` | 停止超时 |
| `WEBVIEW2` | 缺少 WebView2，给官方安装说明 |

---

## 16. 验收场景（1.0 出货用这个打）

1. **空仓欢迎**：无最近工作区，能添加文件夹。  
2. **扫描**：给定「父 pom + 两个 spring 子模块 + `web/package.json`」，生成两个 spring-boot + 一个 node，node 依赖两个 api。  
3. **启动全部**：顺序 api 再 web；日志有 Maven/Node 输出；状态到 running 或 unhealthy（无 actuator 时 http 失败 → unhealthy，进程仍在）。  
4. **改端口**：表单把 api 改为 8091，保存，重启该服务，健康 URL 用新端口。  
5. **停止全部**：任务管理器无残留 `java.exe`/`node.exe`（该工作区拉起的）。  
6. **缺 mvn**：PATH 去掉后启动失败，文案含 mvn，不出现「已运行」。  
7. **原文 YAML**：加未知顶层键 `gateway: {}`，表单保存不得丢（若走原文保存必保留；表单保存至少进 extra 再写出）。  
8. **占位**：模板/Git/容器/网关/云/AI 可见且不可进功能。  
9. **脚本**：`bootstrap` 跑完有退出码和日志。  
10. **崩溃**：手动杀 java，对应服务变为 exited，日志停。  

场景 3 的 actuator：测试仓至少一个模块暴露 health，或该服务改 `health.type: tcp`。两种都要测。

---

## 17. 实现切片（给下一份实现计划，不是现在写代码）

顺序固定，后面依赖前面。

1. Tauri+React 脚手架、AppShell、注册表、ComingSoon、设置关于  
2. yaml 解析/校验/extra 的 **单元测试**  
3. pom / package.json 扫描的 **单元测试**  
4. Job Object 杀树：对 `ping -t` 或小测试进程的 **集成测试**  
5. supervisor + 健康检查（可用 `python -m http.server` 或静态测试二进制）  
6. 探测 PATH  
7. 运行页 + 日志事件  
8. 配置页表单/原文  
9. 扫描草稿向导  
10. 脚本  
11. 用真实小型 Spring+Node 仓打第 16 节清单  

切片 1 之前不要堆业务 UI。切片 4 不过，不准做启动 Maven。

---

## 18. 风险（1.0 内）

| 风险 | 处理 |
|------|------|
| Maven 乱码 | 接受；不阻塞出货 |
| 无 actuator | 允许改 tcp / none |
| 扫描误把根 package.json 当前端 | 第 6 节启发式；预览可删 |
| 表单保存丢注释 | 文案说明；改端口用表单，改复杂结构用原文 |
| Job Object 在部分环境失败 | 集成测试；失败则启动报错，不要默默只 kill 父进程 |

下一份文档应是 **1.0 实现计划**（任务级），然后才 `create-tauri-app`。
