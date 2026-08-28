# SuperTask 1.4 功能规格

> 日期：2026-08-28  
> 状态：范围与默认决策已确认，待实现（前置：1.2 / 1.3 交付或明确裁剪）  
> 上位文档：[产品路线](2026-08-25-product-roadmap.md) · [1.2 功能规格](2026-08-27-v1-2-feature-spec.md) · [1.3 功能规格](2026-08-28-v1-3-feature-spec.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [引擎架构](../spec/architecture.md)

本文把路线中的 1.4「能出门」收到可实现、可测试、可交付的粒度。1.4 沿三条轴展开：**平台**（macOS、Linux 与 Windows 行为等价，同一份 core）、**构建工具**（Gradle 多模块 Spring）、**语言**（UI 中英），外加把存量 Taskfile 项目搬进来的**一次性导入**。1.4 不是新功能堆叠，而是把 1.0–1.3 的能力抬到三个平台并补齐 Java 世界的另一半。

## 1. 目标与边界

### 1.1 产品目标

1. **能出门**：macOS / Linux 开发者打开同一个工作区，起停、日志、健康、端口、密钥、profile、工具链行为与 Windows 等价。
2. **能清场**：三平台上 SuperTask 退出的进程树无残留（Windows Job Object / Unix 进程组）。
3. **能构建**：Gradle 多模块工程 `bootRun` 与 `bootJar`，对齐 1.2 的 jar 规则。
4. **能看懂**：UI 提供 zh-CN / en-US 两种语言，设置页可切换，错误提示按错误码本地化。
5. **能搬进来**：Taskfile 项目通过预览式向导把 tasks 映射成 `scripts`，一次性导入。

### 1.2 版本范围

| 能力 | 1.4 行为 |
|------|----------|
| 平台 | Windows 10/11、macOS 13+（aarch64 / x86_64）、Linux x64（Ubuntu 22.04+ / Debian 12 级别） |
| 进程树 | Windows Job Object 不变；macOS/Linux 进程组 `killpg` + 引擎退出显式清理 |
| 工具链 | mise 在 macOS/Linux 作为安装 provider；winget 仅 Windows |
| Gradle | `build_tool: gradle`（缺省探测），`bootRun` / `bootJar` |
| i18n | zh-CN（默认）/ en-US；app data v3 新增 `locale` |
| Taskfile | v3 YAML 文件导入 → `scripts` 向导（预览 + 应用） |
| 打包 | macOS dmg（签名+公证）、Linux AppImage；自动更新仅 Windows/macOS |
| 1.3 容器 | docker CLI 跨平台，macOS/Linux 一并验收 |

### 1.3 明确不做

以下能力不进入 1.4：

- 移动端、Web 版、Windows-on-ARM、Linux ARM 桌面
- Linux 发行版深度适配矩阵（Snap/Flatpak/deb/rpm 仓库）；只交付 AppImage
- brew / apt 代装工具链（探测可以，安装不提供；安装 provider 只有 mise）
- Gradle Kotlin DSL 深度解析、`gradle init`、多工程 composite build 管理
- Gradle 全局安装入口（只支持 wrapper；理由见 §5.1）
- i18n 框架之外的 RTL、更多语种、后端 message 全量翻译（后端 message 保持中文）
- Taskfile 的 vars 插值、`includes`、动态 task、运行时（导入是一次性迁移，不做 Taskfile 执行器）
- Linux 自动更新安装（检查可用，安装提示手动替换）
- CLI、MCP、导出包（1.5）；插件、WSL2（2.2）

YAML 继续 `version: 1`，IPC 继续 protocol 1。**Windows 行为零变化**是硬约束：平台抽象不得引入 Windows 回归。

## 2. 用户场景与成功标准

### 2.1 macOS / Linux 开发者

1. 用户在 MacBook（或 Ubuntu 机器）安装 dmg（AppImage），打开 Spring + Node 工作区。
2. 探测、起停、依赖拓扑、健康检查、日志批次、端口冲突、`.env.local`、profile、jar 启动全部可用，交互与 Windows 版一致。
3. 关闭应用后 `pgrep` 不到残留的 `java`/`node`；应用崩溃（强杀 SuperTask）后 Linux 上直系子进程随引擎退出，macOS 上正常退出路径同样无残留，异常崩溃的兜底差异在文档明示。
4. 托盘、系统通知、开机自启按平台能力提供；缺失桌面环境（无托盘的 Linux WM）时降级为普通窗口 + 应用内通知，不崩溃。
5. 中文 UI 默认；切到 English 后导航、页面、命令面板、常见错误提示全部切换。

### 2.2 Gradle 多模块

1. 用户打开 Gradle 多模块工程（`settings.gradle.kts` include 多个模块，模块有 Spring Boot 插件）。
2. 扫描向导列出可运行模块，生成 `kind: spring-boot` + `build_tool: gradle` 草稿。
3. 启动执行 `./gradlew :user-service:bootRun`（Windows `gradlew.bat`）；跨模块依赖由 Gradle 自身解析，无 `-pl` 类问题。
4. `launch: jar` 执行 `bootJar` 后在 `build/libs` 识别唯一可执行 jar 并 `java -jar`，排除规则与 1.2 一致。
5. 工程没有 wrapper 且 PATH 无 gradle 时，启动返回 `GRADLE_WRAPPER_MISSING` 并给出 `gradle wrapper` 指引，不代装。

### 2.3 英文 UI

1. 设置页「语言」提供「跟随系统 / 简体中文 / English」。
2. 切换即时生效，无需重启；选择持久化到 app data。
3. 导航、命令面板、表单、向导、operation 文案为双语；后端错误 `message` 保持中文，UI 优先按 `code` 显示本地化文案，`message` 作为详情折叠展示。
4. 未知 locale 回落 zh-CN 并在设置页提示。

### 2.4 Taskfile 导入

1. 工作区有 `Taskfile.yml`（v3）。用户在配置页进入「导入 Taskfile」向导。
2. 预览表逐条列出：task 名 → 目标 script id、cmds 数、将忽略的字段、警告（插值、自定义 shell、deps 等）。
3. 用户勾选后写入 `scripts`；`desc`/`env`/`dir` 映射，`internal: true` 默认跳过。
4. 含 `{{.VAR}}` 等插值的命令默认不勾选，可强制导入原文（用户自己改）。
5. 导入是一次性动作：之后 Taskfile 与 `supertask.yaml` 无联动，修改不双向同步。

## 3. 总体架构（平台层）

```text
supertask-core
    ├─ proc/        进程树平台层（trait ProcessTree）
    │    ├─ windows   Job Object（现行为原样搬入，含 accounting）
    │    └─ unix      进程组 + killpg + 引擎退出清理 + 指标降级
    ├─ launcher     平台程序后缀表（mvn/mvn.cmd、gradlew/gradlew.bat）
    ├─ probe        PATH 优先 + 平台已知位置补充
    ├─ port         PortInspector 平台实现（/proc、lsof）
    ├─ discover     外部进程发现平台实现
    ├─ appdata      平台数据目录
    └─ shell        脚本执行 shell（cmd.exe / bash）
```

### 3.1 平台能力矩阵

| 能力 | Windows | macOS | Linux |
|------|---------|-------|-------|
| 进程树终止 | Job Object（不变） | 进程组 SIGTERM→SIGKILL | 同 macOS + PDEATHSIG |
| 孤儿兜底（引擎崩溃） | KILL_ON_JOB_CLOSE | 明示局限，正常退出清理 | PDEATHSIG + subreaper 尽力 |
| CPU/内存指标 | Job accounting（不变） | sysctl 查询，失败 null | /proc 聚合，失败 null |
| 监听表 | GetExtendedTcpTable（不变） | lsof | /proc/net/tcp{,6} + /proc 映射 |
| 数据目录 | %APPDATA%/SuperTask | ~/Library/Application Support/SuperTask | $XDG_CONFIG_HOME/supertask |
| 通知/托盘/自启 | 现有插件（不变） | tauri 插件 | tauri 插件 + XDG autostart |
| 工具链安装 provider | mise / winget | mise | mise |
| 更新 | 签名安装（不变） | 签名+公证 dmg 安装 | 仅检查，安装手动 |
| compose（1.3） | Docker Desktop | Docker Desktop | 原生 docker / Desktop |

### 3.2 分层职责与守恒规则

- `supertask-core` 引入 `proc` 平台层：`job.rs` 的 Windows 实现整体迁入 `proc/windows`，行为与错误码不变；Unix 实现满足同一 trait 契约（创建、终止、超时、accounting 可选）。
- 平台差异只允许存在于 `cfg` 分支与平台模块内；业务代码（状态机、拓扑、spec、日志）不出现 `if os`。
- `session.hello` 的 `os` 字段如实返回 `windows | macos | linux`，前端不做平台分支布局。

## 4. 进程与平台运行时

### 4.1 进程树模型（Unix）

- 引擎 spawn 的每个直系子进程创建独立进程组（`process_group(0)`），组内再 spawn 的孙进程自然同组。
- 停止：`killpg(pgid, SIGTERM)` → 宽限（默认 5s，`grace_secs` 不影响此值）→ `SIGKILL`；超时仍存活 → `JOB_KILL`（复用现有错误码）。
- Linux：直系子进程 `pre_exec` 设置 `PR_SET_PDEATHSIG(SIGKILL)`，引擎异常崩溃时直系子进程随之退出；孙进程通过 `PR_SET_CHILD_SUBREAPER` 收养后按组终止，尽力清场。
- macOS：无 PDEATHSIG 等价物；正常退出路径显式清理全部进程组，**异常崩溃可能残留孙进程**，此局限写入用户文档与验收口径（不隐瞒、不假装）。
- 指标（1.2）：Linux 按 `/proc` 聚合进程组 CPU/内存；macOS 用 sysctl 查询进程组；任一子进程查询失败降级 partial，整组不可读 → `metrics: null` + `METRICS_UNAVAILABLE`，不影响状态机。

### 4.2 Launcher 与 shell

- 程序名后缀表：Windows `mvn.cmd`/`npm.cmd`/`gradlew.bat`/`java.exe`；macOS/Linux 无后缀。解析顺序 PATH → 平台已知位置（§4.3）。
- Gradle wrapper 无执行位（`gradlew` mode 不含 x）：通过 `sh gradlew` 执行并警告一次，不静默失败。
- `scripts.cmds` 执行 shell：Windows 维持 `cmd.exe /C`（不变）；macOS/Linux 经 `bash -c`，PATH 无 bash 时回落 `sh -c` 并在脚本日志头部警告 bash 语法风险。
- 脚本 cmds 仍只来自 YAML；平台差异不改变 IPC 只传 id 的规则。

### 4.3 探测与工具链 provider

- probe 顺序不变（PATH 优先），补充平台已知位置仅作 PATH 未命中时的候选：
  - macOS：`/opt/homebrew/bin`、`/usr/local/bin`、`~/.sdkman/candidates/*`、`~/.nvm/versions/*/bin`
  - Linux：`~/.local/share/mise/shims`、`/usr/lib/jvm/*/bin`
- `toolchain.install` 的 `auto`：macOS/Linux 只有 mise 可选，无 mise → `TOOLCHAIN_MANAGER_MISSING`（文案提示安装 mise，不代装 mise 本身）；winget 分支仅 Windows。
- Java/Maven/Node 版本解析、`MISSING_TOOL` 语义跨平台一致。
- Gradle 只探测显示（`gradle: { found, version, path }`），不提供安装入口：wrapper 是唯一推荐执行方式，全局 gradle 仅作 fallback。

### 4.4 端口与外部进程发现

- PortInspector（1.2 抽象）补 Unix 实现：Linux 读 `/proc/net/tcp`、`/proc/net/tcp6` 与 `/proc/<pid>/fd` 关联 PID；macOS spawn `lsof -nP -iTCP -sTCP:LISTEN`（系统自带）解析。
- 读不到监听表 → `PORT_SCAN_FAILED`，与 Windows 口径一致；不把「无法检查」当「端口可用」。
- `system.discover` / `system.killProcess` 同步补 Unix 实现；kill 护栏从 `pid ≤ 4` 调整为 `pid == 1`、SuperTask 自身、无 LISTEN 端口（三平台统一规则：init 进程与系统保留范围拒绝）。

### 4.5 桌面集成

- app data：路径见 §3.1；schema 升 version 3（§8），迁移沿用 1.2 规则（保留未知键，写入失败用内存值不覆盖旧文件）。
- 通知：tauri notification 插件三平台；`systemNotifications` 偏好沿用。
- 托盘：三平台；Linux 无托盘协议的环境自动降级隐藏托盘入口，`closeToTray` 行为退化为直接退出前询问。
- 开机自启：tauri autostart 插件（macOS LaunchAgent、Linux XDG autostart）；失败 `AUTOSTART_FAILED` 文案按平台给指引。
- 更新：Windows 不变；macOS 走签名+公证 dmg，`UPDATE_SIGNATURE`/`UPDATE_FAILED` 语义不变；Linux `app.update.check` 可用，`app.update.install` 同步返回 `PLATFORM_UNSUPPORTED` 并附手动替换指引。

## 5. Gradle 多模块

### 5.1 配置与探测

```yaml
services:
  user-api:
    kind: spring-boot
    build_tool: gradle      # maven | gradle；缺省按构建文件探测
    module: user-service
    launch: run             # gradlew :user-service:bootRun
```

- 探测规则：module 目录（单模块工程为 root）有 `build.gradle` / `build.gradle.kts` → gradle；有 `pom.xml` → maven；**两者并存 → `BUILD_TOOL_AMBIGUOUS`**（`SPEC_INVALID`，打开时警告 + 启动硬错误）；都没有 → 打开警告，启动时按工具缺失处理。
- 显式 `build_tool` 跳过探测；非法值 `SPEC_INVALID`。
- 执行优先 wrapper：root（或 module）存在 `gradlew`（Unix）/ `gradlew.bat`（Windows）则用 wrapper；否则用 PATH 解析的 `gradle`；都无 → `GRADLE_WRAPPER_MISSING`，文案建议 `gradle wrapper --gradle-version <x>`，不代装。
- 不提供全局 Gradle 安装：wrapper 版本即工程真源，避免两套版本漂移（与 1.2「能探测就别安装」一致）。

### 5.2 `bootRun`

- argv：`gradlew[.bat] [:]<module>:bootRun` + `extra_args`；module 为 `"."`（单模块）时省略任务路径前缀，直接 `bootRun`。
- 无 Maven `-pl`/`-am` 问题：Gradle 自身解析跨模块任务依赖，`depends_on`（SuperTask 层）语义不变。
- 默认 `grace_secs` 45、默认 `health.type: tcp`，与 Maven 路径一致；`SERVER_PORT` 注入规则不变。
- 工作目录：工作区 root（与 Maven 一致）；Gradle 在 root 执行 `:module:bootRun` 天然正确。

### 5.3 `bootJar`（`launch: jar`）

- 构建命令：`gradlew[.bat] [:]<module>:bootJar`，`build_args` 按 argv 追加，默认不追加 `-DskipTests` 等价物（Gradle bootJar 默认不跑测试；用户需要时写 `build_args`）。
- artifact 识别在 `module/build/libs`：排除 `*-plain.jar`、`*-sources.jar`、`*-javadoc.jar`、非 jar 文件；唯一候选使用，零候选 `ARTIFACT_MISSING`，多候选 `JAR_AMBIGUOUS`（不按修改时间猜），路径逃逸 `PATH_ESCAPE`——全部复用 1.2 §11.3 规则。
- `java -jar` 启动、Job/进程组管理、健康、停止与 1.2 完全一致；`building` 状态标注「构建阶段（gradle）」。

### 5.4 扫描与工具链

- 扫描器新增 Gradle 探测（文本级，不执行 gradle）：root 有 `settings.gradle(.kts)` 时解析 `include 'x'` / `include(":x")` 提取模块；模块构建文件含 `org.springframework.boot`（插件 id）→ 生成 `kind: spring-boot` + `build_tool: gradle` 草稿；纯 java 库模块忽略。
- include 解析不了的动态语法（循环 include 等）→ 跳过并警告，不阻塞其余模块。
- merge 向导字段所有权扩展：扫描器负责字段增加 `build_tool`（`update` 动作可覆盖，用户其余字段保留）。
- `toolchain.probe` 输出增加 `gradle` 项（found/version/path，仅信息展示）。

## 6. UI 中英（i18n）

### 6.1 locale 模型

- 支持 locale：`zh-CN`（默认）、`en-US`；偏好值 `auto` 表示跟随 OS。
- 生效顺序：app data `locale` 显式值 > `auto` 检测 OS > `zh-CN`；未知值回落 zh-CN 并在设置页提示。
- 切换即时生效（重新挂载 i18n provider），无需重启应用或重开工作区。

### 6.2 前端约定

- 引入 `react-i18next`（或同等轻量方案），资源文件按 namespace 组织（导航/命令面板/页面/向导/错误码）。
- feature registry 的 `navLabel` 改为 `labelKey`；导航、命令面板、路由标题全部经 i18n 渲染。**禁止组件内硬编码中文字符串**，新代码评审清单包含此项。
- 错误展示：UI 持有 `code → 双语文案` 映射，命中时优先显示本地化文案；后端 `message`（中文）作为可展开详情保留。未命中 code 显示 message 原文。
- operation message 由后端生成（中文），UI 对常见 `kind` 提供本地化模板，参数化字段（服务名、版本）照传。
- 不承诺英文文档；1.4 只做 UI 层双语。

### 6.3 后端

- 后端 `message` 字段保持中文（契约不变：`code` 是稳定枚举，`message` 给人看）。
- 系统通知文案（崩溃通知等）由壳层生成：按错误码/事件类型取双语模板，locale 读 app data。

## 7. Taskfile 导入

### 7.1 支持范围与映射

读工作区根的 `Taskfile.yml` / `Taskfile.yaml`（不递归、不跟随 `includes`）。只支持 Taskfile **v3**（`version: '3'`；v2 → `TASKFILE_INVALID` 提示不支持）。

| Taskfile 字段 | 映射到 `scripts` | 说明 |
|---------------|------------------|------|
| task 名 | script id | 按 id 规则合法化；冲突加 `-task` 后缀并提示 |
| `desc` | `desc` | |
| `cmds`（字符串或 `cmd:`/`silent:` 映射） | `cmds` | `silent` 丢弃；映射形式取 `cmd` |
| `env` | `env` | |
| `dir` | `cwd` | 沙箱校验，逃逸 → 该项警告不导入 |
| `internal: true` | 跳过 | 预览标灰 |
| `deps` | 忽略 | 警告（`scripts.depends_on` reserved） |
| `sources` / `generates` / `method` / `status` | 忽略 | 警告 |
| `platforms` 约束 | 忽略 | 警告；导入后的 script 无平台约束 |
| `shell`（task 级 powershell 等非默认） | 跳过 | 警告：执行 shell 不一致 |
| cmds 含 `{{…}}` / `$VAR` 插值 | 默认不勾选 | 警告列出变量；可强制导入原文 |
| `includes`、动态 task、`loop` | 跳过 | 警告 |

- 全局 `env` 合并进每个 task 的 `env`（覆盖顺序：task 覆盖全局）；全局 `vars` 不解析。
- 目标 `scripts` 已有同名 id：预览标 `id_conflict`，默认 `keep`（不覆盖已有脚本）。

### 7.2 命令

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
}
```

- Apply 走 `yaml.saveForm` 机制，`base_hash` 冲突 → `YAML_CONFLICT`；只增改所选 `scripts.*`，其余字段不动。
- 文件不存在 → `TASKFILE_NOT_FOUND`；语法/版本错误 → `TASKFILE_INVALID`（details 含行号时带上）。
- 导入是一次性迁移：之后不监听 Taskfile 变化、不双向同步；再次导入按 id_conflict 规则合并。

## 8. YAML 与应用数据兼容

### 8.1 新增字段

| 字段 | 1.4 行为 | 旧版本行为 |
|------|----------|------------|
| `services.*.build_tool` | `maven \| gradle`，缺省探测 | 旧版经 extra round-trip，不参与启动 |

- `build_tool: maven` 显式值等价于现状（缺省）；无其他 YAML 变化。
- schema 补 `build_tool` 枚举；兼容测试：旧客户端结构化保存不丢该字段。

### 8.2 应用数据 version 3

```json
{
  "version": 3,
  "locale": "auto"
}
```

- v2 → v3 迁移：保留全部未知键，新增 `locale` 默认 `auto`；写入失败沿用 1.2 规则（内存值生效，不覆盖旧文件）。
- 各平台数据目录独立，不做跨平台迁移（1.5 导出包负责搬家）。

## 9. IPC 契约增量

protocol 保持 1，全部为新增命令与既有命令的输出扩展：

```text
import.taskfilePreview   { workspace_id } → { tasks, warnings }
import.taskfileApply     { workspace_id, selected, base_hash } → { spec, hash, warnings }
```

- `app.savePrefs` 入参扩展 `locale?: "auto" | "zh-CN" | "en-US"`；`app.load` 的 prefs 同步返回。
- `toolchain.probe` 输出增加 `gradle?: { found, version, path }`。
- `session.hello` 的 `os` 如实返回三平台值（字段已有，无结构变化）。
- `workspace.scanPreview` / `scanApply` 的扫描器负责字段增加 `build_tool`（结构不变）。
- Linux 上 `app.update.install` → `PLATFORM_UNSUPPORTED`（同步错误，不发 operation）。

## 10. 错误与安全要求

### 10.1 新增稳定错误码

| code | 触发条件 |
|------|----------|
| `PLATFORM_UNSUPPORTED` | 能力在当前平台不可用（如 Linux 更新安装） |
| `BUILD_TOOL_AMBIGUOUS` | module 同时存在 Maven 与 Gradle 构建文件 |
| `GRADLE_WRAPPER_MISSING` | 无 wrapper 且 PATH 无 gradle |
| `TASKFILE_NOT_FOUND` | 工作区无 Taskfile |
| `TASKFILE_INVALID` | Taskfile 版本/语法不支持 |

其余错误复用现有码：进程组终止超时用 `JOB_KILL`，监听表读取失败用 `PORT_SCAN_FAILED`，gradle 构建失败用 `BUILD_FAILED`，jar 歧义用 `JAR_AMBIGUOUS` / `ARTIFACT_MISSING`。

### 10.2 安全边界

- Unix 进程组只作用于引擎 spawn 的直系子进程及其后代；`killpg` 目标 pgid 必须由引擎分配记录，不暴露任意 pgid/pid 接口。
- `system.killProcess` 三平台统一护栏（init/系统保留、自身、须有 LISTEN 端口）。
- 平台已知位置的探测只读，不写 shell 配置、不改 PATH。
- Taskfile 解析是纯 YAML 读取 + 文本级检查，不执行任何命令；导入内容全部经过既有 script 校验（cmds 非空、cwd 沙箱、timeout 上限）。
- `bash -c` 执行的 cmds 来自用户 YAML（可信边界与 1.0 `cmd.exe /C` 相同）；插值变量导入时的警告必须列出，防止用户无感带入敏感变量。
- i18n 不改变错误码稳定性；`message` 不承载机密（沿用既有脱敏规则）。

## 11. 前端范围

### 11.1 i18n 落地

- 新增 i18n provider 与资源文件；feature registry `navLabel` → `labelKey` 迁移。
- 设置页「外观」组新增语言选择（跟随系统/简体中文/English）；切换即时生效。
- 错误呈现组件统一：code 本地化优先、message 详情折叠；命令面板与 operation 文案双语。
- Playwright 用例以 zh-CN 断言为主，抽一条 en-US 冒烟（导航 + 起停 + 一个错误路径）。

### 11.2 Gradle 与 Taskfile

- env 页工具链卡片增加 Gradle 行（wrapper 提示、全局版本只读）。
- 运行页/配置页对 `build_tool: gradle` 服务显示构建工具标识；jar 流程状态沿用 1.2 UI。
- 配置页新增「导入 Taskfile」入口：预览表（task → script id、警告、勾选）→ 应用 → YAML diff 确认，交互样式对齐 1.1 扫描合并向导。

### 11.3 平台差异展示

- `session.hello.os` 仅用于「关于」页与诊断信息展示；不做平台分支布局。
- Linux 无托盘环境：托盘入口隐藏、关闭行为退化文案；macOS 异常崩溃孤儿局限在设置页「关于」给出说明链接。

## 12. 非功能要求

### 性能

- 平台抽象不得降低 Windows 路径性能：`proc/windows` 保持 Job Object 直调（cfg 静态分发，无运行时开销设计）。
- Unix 指标采样、监听表读取开销与 1.2 Windows 口径同量级；不可用路径必须快速返回（`PORT_SCAN_FAILED`、`metrics: null`），不重试风暴。
- `import.taskfilePreview` p95 < 100ms（纯文件解析）。

### 可靠性

- 三平台 CI 全绿为合并门槛：`cargo test -p supertask-core`、`cargo check` 壳、`tsc` + `vite build`。
- Windows 回归零容忍：平台层合入后 1.0–1.3 全部既有测试不改编断言通过。
- Unix 杀树：正常退出无残留为验收硬项；异常崩溃残留仅 macOS 明示局限。
- app data v3 迁移失败不破坏 v2 文件。

### 资源与隐私

- `/proc`、sysctl、lsof 查询只读且限本引擎进程组与监听表；不上传平台信息（`os` 字段仅本地握手使用）。
- Taskfile 内容不进入日志与遥测；导入预览仅驻留内存。

## 13. 测试与验收

### 13.1 Core 单元测试（三平台跑）

- `proc` 平台层契约测试（spawn sleep/cat fixture：终止、超时、部分退出）。
- 程序后缀表、wrapper 执行位回落、bash/sh 回落警告。
- `build_tool` 探测/并存拒绝、gradle argv 生成、bootJar 排除规则（`*-plain.jar` 等）。
- Gradle include 文本级解析、动态语法跳过警告。
- Taskfile 映射表逐行对应用例（internal 跳过、插值默认不选、id 合法化与冲突、全局 env 合并）。
- app data v3 迁移、locale 回落。

### 13.2 集成测试

- fake gradlew 脚本桩：bootRun/bootJar argv、非零退出 `BUILD_FAILED`、多 jar `JAR_AMBIGUOUS`。
- fixture Taskfile：preview 输出、apply 后 YAML 快照、`YAML_CONFLICT` 路径。
- Unix 容器（CI）：进程组终止无残留（`pgrep` 断言）、退出清场、指标 partial 降级。
- compose（1.3）在 Linux CI 可选跑（有 docker 的 runner），macOS 手动验收。

### 13.3 真机验收矩阵

| 场景 | Windows | macOS | Linux |
|------|---------|-------|-------|
| Spring+Node 起停/日志/健康 | 回归 | ✅ | ✅ |
| 停止/退出无残留进程 | 回归 | ✅（含正常退出） | ✅（含 PDEATHSIG） |
| 端口检查 / discover / kill 护栏 | 回归 | ✅ | ✅ |
| 工具链 probe + mise 安装 | 回归 | ✅ | ✅ |
| secrets / profile / jar | 回归 | ✅ | ✅ |
| Gradle demo 工程全流程 | ✅ | ✅ | ✅ |
| compose sidecar（1.3） | 回归 | ✅ | ✅ |
| 托盘/通知/自启 | 回归 | ✅ | ✅（无托盘 WM 降级） |
| 更新 | 回归（签名） | ✅（签名+公证） | 仅检查 |
| i18n 切换 | ✅ | ✅ | ✅ |

### 13.4 前端与回归

- tsc / vite build / Playwright（zh-CN 全量 + en-US 冒烟）。
- 回归 1.0–1.3 场景清单三平台抽样（Windows 全量）。
- 旧客户端模型读写 1.4 YAML：`build_tool` round-trip 不丢。

## 14. 交付顺序

1. **平台抽象层**：`proc` trait + Windows 迁移（行为不变）+ Unix 进程组实现；CI matrix 三平台跑 core 全量测试。
2. **平台服务补齐**：launcher 后缀/shell、probe 位置、PortInspector/discover Unix、appdata/通知/托盘/自启/更新分平台。
3. **Gradle**：`build_tool` 探测、bootRun、bootJar、扫描器、probe 展示。
4. **i18n**：框架接入、文案 key 化迁移、设置页、错误码本地化。
5. **Taskfile 导入**：preview/apply 命令与向导。
6. **打包与发布**：macOS 签名+公证 dmg、Linux AppImage、更新通道分平台。
7. **三平台真机验收**：§13.3 矩阵 + 回归。

依赖关系：PortInspector/discover/崩溃通知依赖 1.2；compose 验收依赖 1.3；`proc` 平台层（步骤 1）无外部依赖可最早启动，且是其余步骤的地基。i18n（步骤 4）与 Gradle（步骤 3）可并行。

## 15. 已确认决策

- 跨平台 = 同一份 core + `proc` 平台层，不分叉仓库、不引平台专属引擎。
- Unix 进程树用进程组 `killpg`；Linux 加 PDEATHSIG/subreaper 兜底，macOS 异常崩溃局限明示，不假装等价 Job Object。
- Windows 行为零变化是合入门槛；平台差异只准存在于平台模块与 cfg 分支。
- 脚本 shell：Windows `cmd.exe /C`（现状）、macOS/Linux `bash -c`（缺失回落 `sh -c` 并警告）。
- 工具链安装 provider：macOS/Linux 只有 mise（缺失报 `TOOLCHAIN_MANAGER_MISSING`），winget 仅 Windows；不提供 brew/apt 代装。
- Gradle 只支持 wrapper 执行（PATH gradle 仅 fallback），不提供全局 Gradle 安装；bootJar 完整复用 1.2 artifact 规则。
- i18n 范围 zh-CN / en-US；后端 `message` 保持中文，UI 以错误码本地化优先；禁止组件硬编码文案。
- Taskfile 导入是一次性预览式迁移向导，不做 Taskfile 运行时与双向同步。
- app data 升 version 3 仅新增 `locale`；YAML `version: 1`、protocol 1 不变。
- 自动更新：Windows/macOS 支持，Linux 检查可用、安装返回 `PLATFORM_UNSUPPORTED` 手动替换。
