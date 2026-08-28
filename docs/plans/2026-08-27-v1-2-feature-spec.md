# SuperTask 1.2 功能规格

> 日期：2026-08-27  
> 状态：范围与默认决策已确认，待实现  
> 上位文档：[产品路线](2026-08-25-product-roadmap.md) · [1.0 功能规格](2026-08-25-v1-0-feature-spec.md) · [1.1 功能规格](2026-08-27-v1-1-feature-spec.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [引擎架构](../spec/architecture.md)

本文把路线中的 1.2 收到可实现、可测试、可交付的粒度。1.2 的目标是让用户少做环境和故障排查工作：工具链可以安装和升级，端口冲突可以定位和修复，敏感环境变量可以放在本地文件，企业网络可以配置，日志可以搜索和导出，服务资源消耗可见，并能用 profile 管理不同运行场景。

## 1. 目标与边界

### 1.1 产品目标

1. **能准备**：发现 JDK、Maven、Node 或包管理器缺失时，用户可以从 SuperTask 发起受控安装或升级。
2. **能修复**：启动前能定位端口占用，并以一次确认完成端口替换、配置写回和可选重启。
3. **能保密**：密码、token 和连接串放在 `.env.local` 或用户环境中，不进入 `supertask.yaml`、app data、事件和遥测。
4. **能连网**：Maven/npm 镜像与公司代理可以按应用或工作区配置，并只作用于需要它们的外部工具调用。
5. **能追查**：日志有历史搜索、导出和保留策略；服务异常退出时能及时通知用户。
6. **能衡量**：运行中的托管服务显示 CPU、内存和进程数，且统计覆盖 Job Object 管理的子进程。
7. **能切换**：用户可以用 `local`、`test` 等 profile 和服务分组管理不同启动组合。
8. **能打包运行**：Spring 服务支持 `mvn package` 后运行可识别的可执行 jar。

### 1.2 版本范围

| 能力 | 1.2 行为 |
|------|----------|
| 工具链 | mise / winget 安装和升级 JDK、Maven、Node；重新探测并用于后续启动 |
| 端口 | 本机占用检查、监听进程摘要、可用端口建议、一键写回和可选重启 |
| 密钥 | `.env.local` / 用户环境读取、缺失检查、掩码编辑、Git 忽略检查 |
| 网络 | HTTP/HTTPS 代理、no_proxy、Maven mirror、npm registry |
| 日志 | 历史搜索、文本/JSONL 导出、大小/数量/天数保留策略 |
| 崩溃 | 异常退出的应用内和系统通知，支持跳转日志 |
| 指标 | 托管服务 CPU、内存、进程数和近一分钟轻量趋势 |
| Profile | 环境覆盖、服务启停覆盖、active profile 切换 |
| 分组 | 服务按 group 展示和批量操作 |
| Spring jar | `package`、artifact 识别、`java -jar` 启动 |

### 1.3 明确不做

以下能力不进入 1.2：

- Docker/Compose、镜像、Redis/MySQL sidecar
- macOS/Linux、Gradle、英文 UI
- CLI、MCP、云同步、账号和远程密钥同步
- 远程模板市场、插件、自定义 kind、WSL2
- Git commit、push、checkout、stash、merge 的专用 UI
- 自动修改 Git 历史、自动解决冲突、自动杀掉占用端口的外部进程
- PTY 终端、完整 APM、指标持久化、指标告警和历史无限曲线
- 复杂 dotenv 插值、跨文件 secret vault、Windows Credential Manager 集成
- 任意代理脚本、PAC 执行、用户自定义 Maven/npm 命令行

1.2 继续只支持 Windows 10/11 和 `spring-boot`、`node` 服务。现有 `supertask.yaml` 继续使用 `version: 1`，新增字段必须对 1.0/1.1 可读取并尽量 round-trip。

## 2. 用户场景与成功标准

### 2.1 工具链安装

1. 用户打开「环境」页，看到 Java、Maven、Node 和当前工作区所需包管理器的探测结果。
2. 缺少工具时，页面显示固定工具名称、推荐版本和安装方式，不显示任意 shell 命令编辑框。
3. 用户确认后发起安装，界面显示 operation 状态；安装失败时保留原状态并给出权限、网络或包不存在原因。
4. 安装完成后重新探测，新的绝对路径和版本显示在页面与状态栏。
5. 用户重新启动服务时使用新工具链；不需要重启 SuperTask。
6. 如果工作区配置了版本要求，安装页面优先使用该要求；没有要求时必须展示将使用的默认版本并允许用户确认。

### 2.2 端口冲突修复

1. 用户启动服务时，SuperTask 在 spawn 前检查其主端口和附加端口。
2. 已占用时显示端口、监听 PID、进程名（若可读取）和建议的可用端口。
3. 用户选择建议端口后，SuperTask 同时更新 `port`、受控的 `SERVER_PORT`/`PORT` 和默认健康检查 URL。
4. 服务正在运行时，必须先明确确认“停止、写回、重新启动”；停止失败则不修改 YAML。
5. 工作区声明的两个服务使用同一个端口时，配置页仍可打开和编辑，但启动被 `PORT_DUP` 阻止并指出冲突服务。
6. 外部进程不能被 SuperTask 自动终止；用户可以打开资源管理器或使用 IDE/系统工具自行处理。

### 2.3 本地密钥

1. 用户在配置页启用 `.env.local` 作为 secret 文件，文件路径必须位于工作区内。
2. 页面只显示 key 名、来源和是否有值，不在列表、日志、事件或 app data 中显示值。
3. 用户可以新增、更新和删除 key；写入使用安全的 dotenv 子集，不执行任何 shell 表达式。
4. 服务启动前检查 required keys；缺少时返回 key 名但不返回任何已有值。
5. 如果 secret 文件已被 Git 跟踪，页面显示高风险警告并提供“添加忽略规则”的明确操作；不自动修改用户仓库。
6. YAML、扫描 merge、profile 切换和应用更新都不能把 secret 值复制到其他文件。

### 2.4 日志与指标

1. 用户可以按服务、脚本、系统源和文本查询搜索当前及轮转日志。
2. 搜索是异步的、有结果上限的，不把整份历史日志一次加载进内存。
3. 用户可以导出当前筛选结果或完整源日志为 UTF-8 文本或 JSONL。
4. 日志超过保留策略时自动轮转和清理；活动文件路径保持 `.log` 不变。
5. 服务非主动停止而退出时，前端显示通知；应用隐藏到托盘时可以使用 Windows 系统通知。
6. 运行页显示托管服务的 CPU、内存、进程数和近一分钟趋势；外部未托管进程显示“不可用”。

### 2.5 Profile 与 jar

1. 用户可以创建或编辑 `local`、`test` 等 profile，覆盖工作区环境变量和服务 enabled/env/port。
2. 切换 active profile 前，若有服务或脚本运行，必须先停止；切换不会隐式杀进程。
3. 分组只改变 UI 展示和批量操作，不改变依赖图和服务启动语义。
4. Spring 服务选择 `launch: jar` 后，启动流程为 package、识别 artifact、再运行 `java -jar`。
5. 构建失败、找不到唯一 jar 或 jar 启动失败必须显示真实错误，不转为 running。

## 3. 总体架构

```text
React UI
    │ Tauri invoke / event
    ▼
Tauri commands / desktop integrations
    │
    ▼
supertask-core
    ├─ toolchain resolver + provider
    ├─ port inspector + assignment
    ├─ secrets / dotenv parser
    ├─ network policy
    ├─ log search / rotation / export
    ├─ metrics sampler
    ├─ profile resolver / group view
    ├─ Spring build + jar launcher
    └─ operation hub
         │
         ├─ mise / winget
         ├─ git / Maven / npm / package manager
         ├─ Windows TCP/process APIs
         └─ Job Object accounting
```

### 3.1 分层职责

- `supertask-core` 负责规则、路径安全、解析、命令 argv、状态机、数据模型和可测试的 provider 接口。
- `src-tauri` 负责 Tauri 插件、native save/open dialog、Windows notification、窗口和系统设置接入；command 闭包只做适配。
- `frontend` 负责环境、日志、端口、secret、profile 和指标页面；前端不直接访问文件系统或外部进程。
- 外部工具只通过固定程序和结构化参数调用。工具版本、包管理器和镜像不能变成任意命令参数。

### 3.2 长操作模型

工具安装/升级、日志搜索/导出、端口修复中的重启和 Spring package 使用 operation：

1. Command 校验参数并快速返回 `operation_id`。
2. core 在后台执行，operation 只允许一个终态：`succeeded`、`failed` 或 `cancelled`。
3. 通过 `st.operation` 推送状态、有限进度和用户可读摘要。
4. UI 可通过 operation id 去重；刷新或重连后以快照恢复最终状态。
5. 输出做敏感值脱敏；不把完整环境块、secret 值、认证信息写入事件。

安装、搜索和导出应支持取消。取消是 best effort：已由外部包管理器提交的安装不承诺回滚，UI 必须显示“取消请求已发送”与最终状态的区别。

## 4. 工具链安装与升级

### 4.1 支持目标

1. Java/JDK
2. Maven
3. Node.js
4. 当前工作区需要的 npm、pnpm 或 yarn

核心交付目标是 JDK、Maven、Node。npm 随 Node 提供；pnpm/yarn 只有在工作区明确使用且所选 provider 支持时才提供安装入口。缺少包管理器时不能假装 Node 已完整可用。

### 4.2 Provider

1. **mise**：优先用于项目版本约束和用户范围的版本管理。使用逻辑工具名与版本，不接受任意 plugin、backend 或 shell hook。
2. **winget**：Windows 用户范围安装的 fallback。包 ID 由应用随版本 manifest 固定映射，不由 YAML 或前端传入。
3. `auto` 选择顺序：工作区有 `toolchain.manager` 时遵循它；否则已有 mise 且可用时选 mise；再否则选择 winget；两者都不可用时返回 `TOOLCHAIN_MANAGER_MISSING`。

默认安装不请求管理员权限。provider 需要管理员权限或进入交互式同意时，必须快速失败并告诉用户到系统安装器完成，不创建隐藏的卡死进程。

### 4.3 版本来源与 YAML

```yaml
toolchain:
  manager: auto       # auto | mise | winget
  java: "21"
  maven: "3.9"
  node: "20"
  package_manager: pnpm
```

版本来源顺序：

1. 当前工作区 `toolchain` 中的逻辑版本。
2. 用户在安装页面临时选择的版本。
3. provider manifest 的稳定默认版本：JDK 21 LTS、Maven 3.9、Node 20 LTS。

安装操作默认不改写 YAML。用户明确选择“固定到工作区”时，才用 `base_hash` 通过结构化保存写入 `toolchain`；发生 `YAML_CONFLICT` 时安装结果保留，但不写配置。

版本输入只允许数字、点号、连字符和有限的 LTS/major 别名；不能包含空格、路径、`@` 以外的 shell 控制字符或 provider 参数。精确包 ID 不暴露给 UI。

### 4.4 解析与启动

安装完成后 provider 返回实际可执行文件和环境增量。`ToolResolver` 在每次服务启动、脚本运行和 package 前重新解析：

- mise 模式优先用项目/用户范围解析结果。
- winget 模式使用 probe 找到的绝对路径，并刷新当前进程可见 PATH。
- 解析失败返回 `MISSING_TOOL`，不接受“安装命令返回 0”作为工具可用的唯一依据。
- 工具链解析不改变健康检查的 loopback 和代理规则。

### 4.5 错误与回滚

- 安装下载失败、版本不存在、源不可访问或权限不足时，保留已有工具和原 YAML。
- 升级失败不先删除旧版本；只有 provider 自己完成替换后才刷新 probe。
- SuperTask 不负责卸载、降级回滚或清理 provider 的全局缓存。
- operation 日志只保留摘要和脱敏输出，完整 provider 临时日志按本地应用诊断策略处理。

## 5. 端口占用与一键改端口

### 5.1 检查模型

新增 `PortInspector` 抽象。Windows 实现读取本机 TCP 监听表和 PID，优先使用系统 API；不能读取时返回 `PORT_SCAN_FAILED`，不把“无法检查”当作“端口可用”。

每个服务返回：

```json
{
  "id": "user-api",
  "port": 8081,
  "available": false,
  "listeners": [
    { "address": "127.0.0.1", "protocol": "tcp", "pid": 4120, "process": "java.exe", "owned": false }
  ]
}
```

`owned` 只有在 PID 能与当前 Engine 的 Job/进程对应时才为 true。外部 listener 不能被停止。IPv4/IPv6 监听均纳入检查；健康检查仍只访问 loopback。

### 5.2 建议算法

- 默认从当前端口的下一个值开始，向上扫描可用端口。
- 跳过工作区内其他服务的 `port`/`ports`、系统保留端口和已发现的监听端口。
- 返回最多 5 个候选；单次建议最多检查 128 个候选，超出则返回 `PORT_NO_AVAILABLE`。
- 默认候选范围为 1024-65535；用户可以在 UI 选择已有合法端口，但不能选择已发现占用的端口而绕过警告。
- 多个服务冲突时按用户选择顺序逐个重新建议，不能把同一候选分给多个服务。

### 5.3 配置写回

`ports.assign` 是受控的原子配置操作，只接受当前 workspace、service id、新 port 和 `base_hash`。core 在内存中生成新 spec 后校验并保存：

1. 更新服务 `port`。
2. 如果 `SERVER_PORT`/`PORT` 不存在，保持启动时自动注入规则。
3. 如果对应键存在且值等于旧 port，更新为新 port。
4. 如果对应键存在但不是旧 port，保留原值并在结果中提示“显式环境变量未改”；不静默覆盖。
5. 如果 health 为默认 loopback URL 且端口为旧 port，更新 URL；自定义 URL 保留并提示。
6. 其他服务、未知字段、reserved 字段和 profile 原文不变。

保存使用 `base_hash`。冲突时不写入任何字段，返回 `YAML_CONFLICT`。

### 5.4 运行中服务

- 服务停止或未启动：保存后返回 `restart_required: false`。
- 服务运行中：默认只预览变化；用户点击“改端口并重启”后，core 按 stop → 保存 → start 顺序执行。
- stop 失败、Job 未结束或 YAML 写入失败时，后续步骤不执行；若 stop 已成功但保存失败，服务保持 stopped 并明确提示。
- 新端口启动失败时保留新配置，不自动恢复旧端口；用户可以从配置页再次修改。

### 5.5 重复声明

1.2 将 `PORT_DUP` 从 1.0 的 warning 提升为启动前硬错误。打开和编辑仍允许，便于修复。

- `runtime.startOne` 检查该服务与其依赖的有效端口。
- `runtime.startAll` 检查整个 active profile 的端口集合。
- 端口重复错误 details 至少包含端口和冲突服务 id。
- 外部占用使用 `PORT_IN_USE`，与 YAML 内部重复区分。

## 6. Secrets 与 `.env.local`

### 6.1 配置模型

```yaml
secrets:
  backend: file       # file | env
  file: .env.local
  required:
    - DB_PASSWORD
    - JWT_SECRET

services:
  api:
    env_file:
      - .env.local
```

规则：

- `backend: file` 读取顶层 `secrets.file`；路径相对工作区根且必须在沙箱内。
- `backend: env` 只读取 SuperTask 启动时的用户环境，不把值复制到 app data。
- `env_file` 按声明顺序读取，服务级文件只作用于该服务。
- `required` 只保存 key 名，不保存 key 值；key 使用 Windows 环境变量合法名称。
- `local` 作为 1.0 示例的兼容别名，等价于 `file` + 默认 `.env.local`。

### 6.2 dotenv 子集

使用无 shell 执行的 dotenv 解析器，只支持：

- `KEY=VALUE`
- 空行和以 `#` 开头的注释
- 单引号或双引号包裹的单行值
- 值内的普通空格和 `=`

1.2 不支持命令替换、变量插值、跨行值、`export` 语句和反引号。非法行返回行号 `SECRET_PARSE`，不会部分应用文件。

### 6.3 环境合并优先级

服务启动时按下列顺序合并，后者覆盖前者：

```text
当前用户环境
  < 工作区 env
  < active profile.env
  < 顶层 secrets/env_file
  < 服务 env_file
  < 服务 env
  < active profile.services[id].env
  < 端口自动注入（仅对应键不存在时）
```

profile 不能覆盖 `kind`、`module`、`dir`、`extra_args`、`cwd` 或启动命令，避免 profile 变成任意执行入口。

### 6.4 UI 与 Git 保护

- `secrets.status` 只返回 key 名、来源、存在性、解析状态和 Git tracked/ignored 状态，不返回值。
- `secrets.set`/`secrets.delete` 的值只在当前 IPC 请求中传输；前端不得写入 localStorage、React 持久化缓存或日志。
- 文件写回采用临时文件 + 替换；替换失败时保留原文件。
- `.env.local` 未被忽略时只报警；添加 `.gitignore` 规则必须由用户确认，且只追加精确规则，不重排或删除原内容。
- 如果 key 已被 Git 跟踪，SuperTask 不自动执行 `git rm --cached`，只显示修复指引。
- 错误信息只列缺失 key 名；日志、metrics、operation 和通知不能包含值。

## 7. 网络代理与镜像

### 7.1 配置模型

工作区可写入项目级默认，用户也可以在设置中配置应用级默认：

```yaml
network:
  proxy:
    mode: off             # off | system | custom
    http: http://127.0.0.1:7890
    https: http://127.0.0.1:7890
    no_proxy:
      - 127.0.0.1
      - localhost
  maven:
    mirror: https://maven.example.com/repository/public
  npm:
    registry: https://registry.example.com
```

规则：

- `network` 是 1.2 新增的顶层结构化字段；1.0 作为 extra 读取和写回。
- workspace 值覆盖 app 默认；字段未配置时继承 app 默认。
- `off` 不注入代理；`system` 读取 Windows 系统代理并转换为外部工具可用的环境；`custom` 只使用显式 URL。
- 不执行 PAC 脚本，不自动探测或上传代理凭据。
- URL 只允许 `http`/`https`，禁止内嵌用户名密码；非法 URL 返回 `PROXY_INVALID`。

### 7.2 作用范围

- Maven mirror 只作用于 Maven package、bootstrap 和受 SuperTask 管理的 Maven 调用。
- npm registry 只作用于受管理的 npm/pnpm/yarn 调用和 Node 依赖操作。
- 代理可用于工具链 provider 和上述外部工具；不用于 loopback health check。
- Git 是否使用代理交给用户已有 Git 配置；SuperTask 不修改全局 Git config。
- 服务运行时默认继承工作区配置的工具代理环境，但健康检查始终绕过系统 HTTP proxy。

### 7.3 实现约束

- provider 生成临时 Maven settings 或受控环境变量；不修改用户全局 `settings.xml`、`.npmrc` 或 yarn 配置。
- 临时配置放在 `.supertask/runtime`，不进入项目 YAML，不写 secret 值；操作完成后清理。
- `no_proxy` 默认包含 `127.0.0.1`、`localhost` 和 `::1`。
- 代理失效时返回真实外部工具错误，并允许用户重试或关闭代理；不静默回退到直连。

## 8. 日志搜索、导出与保留

### 8.1 文件布局

保持 1.0 的活动文件路径：

```text
{workspace}/.supertask/logs/{serviceId}.log
{workspace}/.supertask/logs/{serviceId}.log.1
{workspace}/.supertask/logs/scripts/{scriptId}.log
{workspace}/.supertask/logs/system.log
```

`.log` 是当前文件，`.log.1` 是最近的轮转文件，数字越大越旧。日志行格式和单行 8 KiB 上限保持 1.0 规则。

### 8.2 保留策略

保留现有 `logging.max_bytes`、`ring_lines` 和 `retain_tail_bytes`，新增顶层 `log_retention`，避免旧客户端结构化保存时丢掉新字段：

```yaml
logging:
  max_bytes: 10485760
  ring_lines: 2000
  retain_tail_bytes: 2097152

log_retention:
  max_files: 5
  max_age_days: 7
  max_total_bytes: 67108864
```

默认值：每个源最多 5 个轮转文件、保留 7 天、单工作区日志总量最多 64 MiB。清理顺序是先删除超龄文件，再删除最旧文件，最后按总大小限制删除；活动 `.log` 不在清理时被直接删除。

轮转在写入前执行。达到 `max_bytes` 后关闭当前文件，按倒序重命名，创建新的 `.log`。重命名失败不丢当前写入，返回 `LOG_RETENTION_FAILED` 并继续保留可写活动文件。

### 8.3 搜索

- 1.2 默认只支持 literal 文本匹配，不支持正则，避免用户输入导致回溯或资源耗尽。
- 支持 source、大小写敏感、limit 和可选的文件范围；不支持依赖不可靠的旧行时间戳做精确时间范围查询。
- query 长度最多 256 个 Unicode 字符；limit 默认 200，最大 5000。
- 搜索按活动文件到最旧轮转文件顺序流式读取，结果包含 source、文件名、行号、原始文本和可解析时间（无日期时为 null）。
- 结果超过 limit 返回 `truncated: true`，不假装已经搜索完整历史。
- 搜索 operation 取消后不返回部分成功状态，UI 显示已取消。

### 8.4 导出

```text
logs.export
  input:  { workspace_id, source?, query?, format, destination_path }
  output: { operation_id }
```

- `format` 只支持 `text` 和 `jsonl`。
- destination 必须来自系统保存对话框或经过路径沙箱校验；不能写到工作区外的任意路径，除非用户在 native dialog 中明确选择。
- 不覆盖已有文件，或在覆盖前显示 native 确认。
- export 读取轮转文件并按时间新到旧或原始文件顺序保持稳定；UI 明确显示导出范围和条数。
- 导出是用户主动操作，允许包含日志中的敏感内容；系统不上传、不缓存导出文件。

### 8.5 崩溃通知

进程退出且 `stop_requested` 为 false 时，保留现有 `exited` 状态并发出 `st.runtime`，`reason` 为 `crash`。Tauri 层根据设置发送 Windows 系统通知，前端同时显示 toast。

- 通知只包含服务 id、退出码和“查看日志”动作，不包含最后一行日志或环境变量。
- 同一服务同一进程只通知一次；多服务在 2 秒内连续退出时合并为一条摘要通知。
- 用户关闭系统通知时，应用内 toast 仍按设置显示。
- 通知失败不改变服务状态，不阻塞 Engine。
- 默认开启应用内通知，系统通知可在设置中关闭；不上传 crash report。

## 9. CPU、内存与进程指标

### 9.1 指标范围

每个 service 的当前指标：

```json
{
  "cpu_percent": 12.4,
  "memory_bytes": 734003200,
  "process_count": 4,
  "sampled_at_ms": 1710000000000
}
```

- 指标只覆盖 SuperTask 自己创建并加入 Job Object 的进程树。
- `cpu_percent` 是采样窗口内 Job 总 CPU 时间折算后的近似值，可超过 100% 表示多核使用。
- `memory_bytes` 是 Job 进程树的当前工作集近似值；不可读取时为 null。
- 外部监听或 1.1 识别的非托管服务返回 `metrics: null`。
- 不持久化指标，不进入日志文件，不上传遥测。

### 9.2 采样与事件

- 只有运行页或指标消费者订阅时才启动 sampler。
- 默认每 1 秒采样一次，每次最多发一条 `st.metrics`，并对 workspace 内服务批量发送。
- 事件只发送活动服务和发生变化的指标；单个服务保留最近 60 个样本供 UI 画轻量趋势。
- `runtime.snapshot` 返回最新一次 metrics 快照；没有采样时为 null。
- 采样失败不影响服务状态和健康检查，details 只记录 `METRICS_UNAVAILABLE`。

### 9.3 Windows 实现

优先通过 Job Object accounting 获取 CPU 和进程数，再按 Job 内进程查询工作集补充内存。不能因为某一个子进程查询失败而把整个服务判为异常；应返回部分可用结果和可读提示。

## 10. Profile 与服务分组

### 10.1 Profile 结构

```yaml
profiles:
  active: local
  items:
    local:
      env:
        SPRING_PROFILES_ACTIVE: local
      services:
        web:
          enabled: true
          env:
            VITE_API_URL: http://127.0.0.1:8081
    test:
      env:
        SPRING_PROFILES_ACTIVE: test
      services:
        web:
          enabled: false
```

约束：

- profile id 必须符合现有 id 规则；最多 32 个 profile。
- `active` 必须引用已存在的 profile；没有 `profiles` 时使用隐式 `default`，不改写 YAML。
- 每个 profile 只能覆盖 `env`、服务的 `enabled`、`env` 和 `port`。
- 不允许 profile 改变 kind、module、dir、package manager、script、depends_on、health、cwd、extra_args 或 secrets 来源。
- profile 中的未知字段继续 round-trip，但不参与运行时解析。

### 10.2 生效与切换

有效配置由 base spec 叠加 active profile 得到，不把合并结果永久写回 base service 字段。

环境优先级与第 6 节一致。profile service `port` 覆盖 base port，并使用和端口修复相同的健康 URL 更新规则；每次切换后重新做重复端口和占用检查。

切换 active profile：

1. 检查是否有服务、脚本或其他 workspace operation 运行。
2. 有运行项时返回 `PROFILE_SWITCH_BUSY`；UI 先请求用户停止全部。
3. 使用 `base_hash` 修改 `profiles.active` 并校验完整 spec。
4. 保存成功后刷新 runtime snapshot、Git status、端口诊断和日志筛选。
5. 不自动启动服务；用户明确点击启动全部后使用新 profile。

### 10.3 分组

`services.*.group` 在 1.2 从 reserved 提升为可用字段：

- group 是显示名称，最多 64 个字符；空值归入“未分组”。
- UI 按 group 展开区段，显示每组运行状态摘要。
- 支持组级启动、停止、重启和日志筛选。
- 组级操作仍遵守全局依赖拓扑；如果依赖在其他组，自动包含必要依赖并在结果中说明。
- group 不改变服务 id、依赖边、端口或 profile 语义。

## 11. Spring package 与 jar 启动

### 11.1 配置

```yaml
services:
  user-api:
    kind: spring-boot
    module: user-service
    launch: jar
    build_args:
      - -DskipTests
    port: 8081
```

- `launch: run` 继续使用 1.0 的 `mvn spring-boot:run`。
- `launch: jar` 只适用于 `spring-boot`。
- `build_args` 是结构化 argv，1.0 会将其作为 service extra round-trip，但不会执行。
- 不提供任意 jar 路径；artifact 必须由 core 在对应 module 的 `target` 中识别。

### 11.2 启动流程

1. 校验 Java、Maven、端口、secret、依赖和 active profile。
2. 执行 `mvn.cmd [-pl module] package`，不默认加 `-am`。
3. 默认追加 `-DskipTests`；用户配置的 `build_args` 按 argv 追加，不能覆盖工作目录或程序。
4. package 成功后扫描 module `target`。
5. 排除 `original-*.jar`、`*-sources.jar`、`*-javadoc.jar` 和非 jar 文件。
6. 找到唯一可执行候选后运行解析出的绝对路径：`java.exe -jar <artifact>`，再追加服务 `extra_args`。
7. 进程加入 Job Object，使用同一健康检查和日志管道。

package 输出进入该服务日志，并用 operation message 标注“构建阶段”。构建阶段使用 `building` 运行状态；进入 jar 进程后转为 `starting`，健康通过后转为 `running`。

### 11.3 Artifact 选择

- 优先选择文件名包含 POM artifactId 且不是排除项的 jar。
- 只有一个候选时使用它。
- 没有候选返回 `ARTIFACT_MISSING`。
- 多个候选无法确定时返回 `JAR_AMBIGUOUS`，details 列出相对文件名；不按修改时间猜测。
- target 不在工作区内或 artifact 路径逃逸时返回 `PATH_ESCAPE`。

### 11.4 构建失败

- Maven 非零退出返回 `BUILD_FAILED`，保留 package 日志，服务不进入 running。
- 已有旧 jar 时，新的 package 失败不能自动运行旧 jar，除非用户明确选择后续版本定义的“运行上次成功构建”。
- package 与运行使用同一 profile/env/代理设置，但 build 阶段不启动健康检查。
- stop 会终止 package 或 jar 的 Job Object；重新 start 必须从干净的 build/run 流程开始。

## 12. YAML 与应用数据兼容

### 12.1 新增或启用字段

| 字段 | 1.2 行为 | 旧版本行为 |
|------|----------|------------|
| `toolchain` | typed 读取和安装目标 | 1.0/1.1 读取并保留，不执行 |
| `secrets` | typed 读取文件/环境 | 1.0/1.1 读取并警告，不执行 |
| `network` | 代理和镜像 | 1.0/1.1 作为顶层 extra 保留 |
| `profiles` | active 和有限 overlay | 1.0/1.1 作为 Value 保留，不执行 |
| `services.*.group` | UI 分组 | 1.0/1.1 读取并保留，不参与分组 |
| `services.*.env_file` | 读取 dotenv | 1.0/1.1 读取但不执行 |
| `services.*.launch: jar` | package + jar | 1.0/1.1 打开但启动返回不支持 |
| `services.*.build_args` | package argv | 1.0/1.1 通过 extra 保留 |
| `log_retention` | 日志轮转/清理 | 1.0/1.1 作为顶层 extra 保留 |

`logging` 不新增嵌套字段，避免旧版 `LoggingSpec` 结构化保存时丢掉 1.2 的保留策略。所有 1.2 新增顶层段必须纳入 schema 的 `additionalProperties` 兼容测试。

### 12.2 应用数据

沿用 1.1 的 `%APPDATA%/SuperTask/app.json`，新增：

```json
{
  "version": 2,
  "toolchainManager": "auto",
  "network": {
    "proxyMode": "off",
    "http": null,
    "https": null,
    "noProxy": ["127.0.0.1", "localhost", "::1"]
  },
  "logNotifications": true,
  "systemNotifications": true,
  "metricsEnabled": true
}
```

迁移规则：

- 1.1 的 version 1 文件升级为 version 2 时保留全部未知字段。
- 如果迁移写入失败，继续使用内存默认值，不覆盖旧文件。
- app data 不保存工具链安装日志、secret 值、Git 凭据、日志正文或指标历史。
- workspace profile active 仍以 YAML 为准；app data 只保存页面筛选和通知偏好。

## 13. IPC 契约增量

1.2 继续使用 protocol 1 的公共信封，采用新增字段和新增命令的兼容方式。所有长操作遵循 1.1 的 `operation_id` 和 `st.operation` 事件。

### 13.1 Toolchain

```text
toolchain.probe
  input:  {}
  output: ToolchainProbe + source/provider/requirement metadata

toolchain.install
  input:  { tool, version?, manager?, persist?, base_hash? }
  output: { operation_id }

toolchain.upgrade
  input:  { tool, version?, manager?, persist?, base_hash? }
  output: { operation_id }
```

`tool` 仅接受 `java | maven | node | npm | pnpm | yarn`。`persist` 默认为 false；为 true 时必须携带 `base_hash`，用于把版本要求安全写回 YAML。

### 13.2 Ports

```text
ports.inspect
  input:  { workspace_id }
  output: { items: PortInspection[] }

ports.suggest
  input:  { workspace_id, id }
  output: { candidates: number[] }

ports.assign
  input:  { workspace_id, id, port, base_hash, restart? }
  output: { operation_id? , spec, hash, restart_required }
```

`restart` 默认为 false；运行中服务只有在用户明确选择 true 后才执行 stop/save/start。端口更新失败不能返回新的 spec。

### 13.3 Secrets and network

```text
secrets.status
  input:  { workspace_id }
  output: { backend, file?, keys: SecretKeyStatus[], git_ignored }

secrets.set
  input:  { workspace_id, key, value }
  output: { ok, key }

secrets.delete
  input:  { workspace_id, key }
  output: { ok, key }

secrets.validate
  input:  { workspace_id, id? }
  output: { ok, missing: string[], warnings: string[] }

network.save
  input:  { workspace_id?, config, base_hash? }
  output: { ok, hash? }
```

`secrets.status`、`validate` 和错误结果禁止返回 secret 值。`secrets.set` 的 value 不得被 core 写入 operation message 或普通日志。network 配置的 workspace 版本使用 YAML `base_hash`；app 默认写 app data。

### 13.4 Logs and metrics

```text
logs.search
  input:  { workspace_id, source?, query, case_sensitive?, limit? }
  output: { operation_id }

logs.export
  input:  { workspace_id, source?, query?, format, destination_path }
  output: { operation_id }

logs.retention.run
  input:  { workspace_id }
  output: { operation_id }

metrics.snapshot
  input:  { workspace_id }
  output: { services: { id: ServiceMetrics|null } }

metrics.subscribe
  input:  { workspace_id }
  output: { ok }

metrics.unsubscribe
  input:  { workspace_id }
  output: { ok }
```

指标事件：

```json
{
  "protocol": 1,
  "event": "st.metrics",
  "workspace_id": "C:/work/mall",
  "ts_ms": 1710000000000,
  "payload": {
    "services": {
      "api": {
        "cpu_percent": 12.4,
        "memory_bytes": 734003200,
        "process_count": 4,
        "sampled_at_ms": 1710000000000
      }
    }
  }
}
```

### 13.5 Profiles and build

```text
profiles.list
  input:  { workspace_id }
  output: { active, profiles: ProfileSummary[] }

profiles.activate
  input:  { workspace_id, id, base_hash }
  output: { spec, hash, active }

runtime.build
  input:  { workspace_id, id }
  output: { operation_id }
```

`runtime.build` 只对 `spring-boot` + `launch: jar` 有效；其他服务返回 `LAUNCH_UNSUPPORTED`。`runtime.startOne` 对 jar 服务执行同样的 build/run pipeline。

## 14. 错误与安全要求

### 14.1 新增稳定错误码

| code | 触发条件 |
|------|----------|
| `TOOLCHAIN_MANAGER_MISSING` | mise 和 winget 都不可用 |
| `TOOLCHAIN_VERSION_INVALID` | 版本格式或 provider 版本不支持 |
| `TOOLCHAIN_INSTALL_FAILED` | 安装或升级非零退出 |
| `TOOLCHAIN_PERMISSION` | 需要管理员或交互式权限 |
| `PORT_IN_USE` | 目标端口被外部 listener 占用 |
| `PORT_SCAN_FAILED` | 无法读取本机监听表 |
| `PORT_NO_AVAILABLE` | 建议范围内无可用端口 |
| `SECRET_FILE_MISSING` | secret 文件不存在 |
| `SECRET_PARSE` | dotenv 语法错误 |
| `SECRET_MISSING` | required key 不存在 |
| `SECRET_GIT_TRACKED` | secret 文件已被 Git 跟踪 |
| `PROXY_INVALID` | 代理或镜像 URL 不合法 |
| `LOG_QUERY_INVALID` | 搜索参数超限或非法 |
| `LOG_EXPORT_FAILED` | 日志导出失败 |
| `LOG_RETENTION_FAILED` | 轮转或清理失败 |
| `METRICS_UNAVAILABLE` | 指标暂时不可读取 |
| `PROFILE_NOT_FOUND` | active 或目标 profile 不存在 |
| `PROFILE_INVALID` | profile 结构或覆盖字段非法 |
| `PROFILE_SWITCH_BUSY` | 运行项或 operation 未结束 |
| `PROFILE_DISABLED` | profile 下服务被禁用 |
| `BUILD_FAILED` | Maven package 失败 |
| `BUILD_BUSY` | 同一服务已有 build |
| `ARTIFACT_MISSING` | package 后没有可执行 jar |
| `JAR_AMBIGUOUS` | 找到多个无法确定的 jar |
| `LAUNCH_UNSUPPORTED` | 非 Spring 或不支持的 launch |

`PORT_DUP` 从 warning 提升为启动硬错误，但仍允许打开和编辑。`FEATURE_SOON` 不再用于 1.2 已实现的 toolchain install；Docker、网关、云、AI 等未到版本的命令继续返回 `FEATURE_SOON`。

### 14.2 安全边界

- 工具链 provider、Maven、npm 和 Java 启动全部使用固定程序与结构化 argv，不提供通用 shell。
- 只允许固定逻辑工具和 provider manifest 中的包映射；前端不能传 package id、plugin id 或额外 flags。
- dotenv 值只进入目标子进程环境或用户明确的 `secrets.set` 写入请求，不进入 YAML、app data、日志和事件。
- 所有 workspace、env_file、artifact、日志 export 路径都做 canonicalize 和 sandbox 校验。
- 端口检查只读本机监听状态，不提供杀任意 PID 的接口。
- 代理和 registry URL 禁止内嵌凭据；健康检查不跟随代理。
- Git ignored 检查不执行破坏性 Git 命令；添加 `.gitignore` 规则必须显式确认。
- 指标只读取 SuperTask 管理的 Job，不允许借指标接口查询任意系统进程。
- updater、遥测、云同步沿用 1.1 安全策略；1.2 不上传代码、secret、日志或指标。

## 15. 前端范围

### 15.1 `/env`

- ProbeBar 保持实时探测，增加工具需求、版本来源和 provider 选择。
- 分别展示安装、升级、重新探测和“固定到工作区”动作。
- 安装中的按钮显示 operation 状态，禁止重复安装同一个工具。
- 需要管理员权限、版本不存在、网络失败和重启 PATH 的情况显示具体下一步。
- 包管理器入口只在当前工作区实际需要时出现。

### 15.2 运行页与配置页

- 服务卡片显示端口冲突、监听 PID 摘要、CPU、内存和进程数。
- 端口冲突提供建议端口和“改端口”“改端口并重启”两个明确动作。
- 配置页增加 Profile 选择器、profile 覆盖编辑、服务 group 编辑和 secret 文件状态。
- 环境 Tab 的 secret 值默认掩码；保存成功后不回显明文。
- jar 服务显示构建中、artifact 选择失败和运行中状态。

### 15.3 `/logs`

- 增加 literal 搜索、大小写开关、源过滤、结果数量和取消按钮。
- 结果显示文件、行号、时间和原文；点击结果定位对应历史文件内容。
- 增加文本/JSONL 导出，native save dialog 负责目标选择。
- 保留实时 LogView；历史搜索和实时流使用不同明确状态，不混淆“已搜索完”和“正在追踪”。
- 设置页提供日志保留策略和通知开关，修改后只影响后续轮转和通知行为。

### 15.4 UI 约束

- 路由和命令继续由 feature registry 驱动；不在 `AppShell` 中按版本堆条件。
- 所有长操作显示 operation 状态和可恢复错误；禁止假成功 toast。
- 继续使用方案 H / Linear 浅色 token、Lucide 图标、可见 focus、无 emoji 和响应式布局。

## 16. 非功能要求

### 性能

- `toolchain.probe`、`ports.inspect`、`profiles.list` 和 `metrics.snapshot` 在正常本机环境下 p95 < 100ms，不含外部安装或冷启动。
- `toolchain.install`、`runtime.build`、`logs.search` 和 `logs.export` 的 accepted 响应目标 < 50ms，不阻塞 UI。
- 日志搜索按流读取，内存额外占用与结果上限成正比，不随历史文件总量线性增长。
- metrics sampler 默认每秒最多一批事件；没有订阅者时不采样。
- 一次 workspace 最多 64 个服务、每个服务最多 60 个指标样本，不写入无限历史。

### 可靠性

- 任何 YAML 写回都使用 `base_hash`；端口、profile、toolchain pin 和 network workspace 配置冲突时不部分保存。
- 安装/升级失败不删除旧工具，不显示已安装成功。
- dotenv 解析失败不部分应用；原 secret 文件保持不变。
- 日志轮转失败不关闭服务日志管道；活动文件继续可写并给出告警。
- 搜索或导出取消不破坏原日志；导出不会覆盖用户已有文件。
- profile 切换和端口修改不隐式启动服务；服务停止失败时不继续修改运行态。
- build 失败不运行旧 artifact；Job Object 继续覆盖 package 和 jar 进程树。

### 资源与隐私

- secret 值只能存在于目标进程环境、瞬时请求内存和用户指定的 `.env.local`；不进入持久化诊断。
- 日志保留默认有界，清理前后都不影响当前内存环。
- 指标采样 CPU 开销目标低于 1%（64 个服务上限的正常空闲场景），采样失败可降级为 null。
- 外部 CLI 输出限制大小并做脱敏，避免 provider 或 Maven 输出撑爆内存。

## 17. 测试与验收

### 17.1 Core 单元测试

- provider 选择顺序、工具版本校验、固定 manifest 映射和权限失败。
- fake tool runner 验证 install/upgrade argv 不含任意用户 flags。
- 端口 inspector 解析无监听、单监听、IPv4/IPv6、托管 PID 和外部 PID。
- 建议端口跳过工作区重复端口、系统监听端口和已选候选。
- `ports.assign` 正确更新默认 env/health，保留显式值和未知字段。
- dotenv 支持规则、非法行、引号、空值和 required key 缺失。
- env 合并优先级和 secret 值不出现在错误/事件字符串中。
- proxy URL 校验、no_proxy 默认值和 Maven/npm 配置生成。
- 日志轮转顺序、年龄/数量/总大小清理、搜索 limit 和导出格式。
- crash reason 只对非 stop 退出产生，重复退出不重复通知。
- CPU/memory 采样计算、不可读子进程降级和 null metrics。
- profile overlay、非法覆盖、端口重复、禁用依赖和切换忙状态。
- jar 排除规则、唯一候选、缺失候选、多候选和 build 失败。

### 17.2 集成测试

1. 使用 fake mise/winget 完成安装成功、版本不支持、权限失败和取消。
2. 使用临时 listener 验证端口冲突提示、建议端口和配置写回。
3. 使用临时 `.env.local` 验证服务能读到 secret、错误不会泄露值、Git tracked 会报警。
4. 使用代理/registry fake server 验证 Maven/npm 配置只作用于受控操作，health 不走代理。
5. 生成超过限制的日志文件，验证轮转、历史搜索、截断结果和 JSONL 导出。
6. 启动会异常退出的 fixture，验证 `st.runtime(reason=crash)`、toast 和系统通知触发条件。
7. 使用 fake Job accounting 验证 CPU、内存和进程数快照及事件频率。
8. profile 切换后验证 env、enabled、port 生效，服务运行时拒绝切换。
9. 使用 fixture Maven 工程验证 package + jar、构建失败和多 jar 歧义。

### 17.3 Windows 验收

- Windows 10/11 真实 PATH 中分别缺少/存在 mise、winget、Java、Maven、Node、pnpm 时文案和行为正确。
- 端口由外部 java/node 监听时，SuperTask 只提示和建议，不杀外部 PID。
- 安装工具后新启动的服务使用新版本，不要求重启应用。
- `.env.local` 不进入 Git status 的 tracked 文件；添加 ignore 规则前有确认。
- 日志轮转不会阻止正在运行服务的 stdout/stderr 写入。
- 托管 Java 子进程树的 CPU/内存统计包含子进程；外部服务显示不可用。
- `launch: jar` 的 package、健康检查、停止和 Job Object 清理全部通过。

### 17.4 前端与回归

- TypeScript 检查、生产构建和 Playwright 通过。
- Playwright 覆盖环境安装、端口修复、secret 掩码、Git 忽略警告、日志搜索/导出、profile 切换和指标展示。
- 回归 1.0：起停、日志实时流、YAML 冲突、未知字段保留、工具缺失错误和停止后无残留。
- 回归 1.1：模板/Git/IDE/托盘/更新页面和 operation 事件不退化。
- 用 1.0/1.1 客户端模型读取 1.2 YAML 后执行结构化保存，验证新顶层字段 `network`、`log_retention`、`profiles`、`toolchain`、`secrets` 不丢。

## 18. 交付顺序

1. **公共模型与兼容层**：错误码、operation/metrics 事件、YAML typed 结构、app data version 2、schema 测试。
2. **工具链与网络**：provider、resolver、probe 扩展、代理/mirror 配置和安装页基础数据。
3. **端口与 secrets**：PortInspector、ports.assign、dotenv、required 检查、Git ignore 诊断。
4. **日志与通知**：轮转/保留、search/export、crash reason、系统通知和日志页。
5. **指标与 runtime**：Job accounting、metrics sampler、`building` 状态和运行页指标。
6. **profile/group**：overlay resolver、切换、依赖和组级操作。
7. **Spring jar**：package pipeline、artifact 识别、jar launcher 和停止回收。
8. **Windows 集成验收**：真实工具链、代理、监听进程、托盘应用数据和回归场景。

依赖关系：工具链 resolver 先于 jar 和安装页；YAML/app data 兼容层先于 profile、secrets 和 network；日志保留先于搜索/导出；metrics 不能改变现有 runtime 状态机的正确性。

## 19. 已确认决策

- 1.2 定位为“省事”，不引入 Docker、跨平台、云或 CLI。
- 工具链使用受控的 mise/winget provider；优先 workspace 版本要求，默认不改写 YAML。
- 端口冲突只检查和修复配置，不提供杀任意外部 PID；`PORT_DUP` 在启动时升级为硬错误。
- `.env.local` 是本地文件方案，不使用远程 vault；secret 值不进入 YAML、app data、日志、事件和遥测。
- dotenv 只支持无 shell 的安全子集，不支持插值和命令执行。
- 代理和 mirror 只作用于受 SuperTask 管理的外部工具；健康检查绕过代理，Git 不改全局配置。
- 日志继续使用文件 + 环形缓冲，不上数据库；新增轮转文件、流式搜索和导出。
- 指标只覆盖 SuperTask Job Object 管理的服务，不做持久化历史和告警。
- Profile 只覆盖 env、enabled 和 port；分组只影响 UI 和批量操作。
- Spring jar 启动先 package，artifact 必须可唯一识别；不提供任意 jar 路径，不默认 `-am`。
- 1.2 保持 YAML `version: 1` 和 protocol 1，新增能力采用兼容扩展。
