# SuperTask 1.1 功能规格

> 日期：2026-08-27  
> 状态：范围与关键产品决策已确认，待实现  
> 上位文档：[产品路线](2026-08-25-product-roadmap.md) · [1.0 功能规格](2026-08-25-v1-0-feature-spec.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [引擎架构](../spec/architecture.md)

本文把路线中的 1.1 收到可实现、可测试、可交付的粒度。1.1 的目标不是扩大服务类型，而是让用户能够从模板开始、从 Git 获取项目、用熟悉的 IDE 接手，并让 SuperTask 作为 Windows 桌面应用可靠地驻留和升级。

## 1. 目标与边界

### 1.1 产品目标

1. **能开始**：用户无需手写项目骨架，可以从官方模板创建一个可运行的工作区。
2. **能接管**：用户可以 clone 已有 Git 仓库，查看当前状态并拉取最新代码。
3. **能开发**：用户可以直接用资源管理器、Cursor、IDEA 或 VS Code 打开当前工作区。
4. **能驻留**：关闭窗口后 SuperTask 默认隐藏到系统托盘，已托管服务继续运行。
5. **能交付**：应用具备签名自动更新闭环，检查更新与安装更新的行为可控、可恢复。

### 1.2 版本范围

| 能力 | 1.1 行为 |
|------|----------|
| 官方模板 | 随应用发布的内置离线模板；至少两套，覆盖 Spring 多模块 + Node |
| Git | clone、status、pull；显示分支与工作区变更摘要 |
| IDE | 资源管理器、Cursor、IntelliJ IDEA、VS Code |
| 扫描向导 | 对已有 YAML 做预览式增量扫描和 merge |
| 系统托盘 | 显示/隐藏窗口、打开工作区、启动/停止全部、退出 |
| 开机启动 | 可选启动 SuperTask；不自动启动项目服务 |
| 自动更新 | 自动检查，用户确认后安装签名更新包 |

### 1.3 明确不做

以下能力不进入 1.1：

- 远程模板市场、模板在线编辑和第三方模板源
- Git commit、push、checkout、branch、stash、merge 的专用 UI
- 自动解决 Git 冲突、自动 reset、自动覆盖本地修改
- 任意 shell、任意 executable 或自定义命令行编辑器
- 工具链安装和升级；归入 1.2 的 mise/winget 能力
- `.env.local`、密钥管理、端口占用自动迁移、profile、服务分组
- Docker/Compose、Spring `package` + jar、Gradle、CLI、MCP
- 云同步、账号、遥测上传
- 更新包下载源的用户自定义

1.1 继续只支持 Windows 10/11，以及 1.0 已支持的 `spring-boot` 和 `node` 服务。

## 2. 用户场景与成功标准

### 2.1 模板创建

1. 用户打开「模板」页，看到内置模板的名称、用途、技术栈、版本和预计目录结构。
2. 用户选择模板和目标父目录，填写项目目录名。
3. 目标目录不存在时创建；目标目录存在且非空时拒绝，不能覆盖或清理用户文件。
4. 模板复制完成后写入 `supertask.yaml`，其中包含模板来源元数据。
5. 创建完成后自动打开新工作区，进入运行页；用户可以直接扫描、修改配置或启动服务。
6. 中途失败时保留已创建文件并给出目标目录，不能显示成功，也不能静默删除用户文件。

### 2.2 Git 获取与同步

1. 用户在 Git 页输入仓库 URL、目标目录和可选分支，执行 clone。
2. clone 完成后自动打开工作区；有 `supertask.yaml` 时读取，没有时进入 1.0 扫描向导。
3. 用户打开已有 Git 工作区时，可以看到当前分支、是否有修改、ahead/behind 和 staged/unstaged/untracked 摘要。
4. 工作区干净时可以执行 pull，并看到远端、分支和结果摘要。
5. 工作区存在未提交修改时，pull 默认拒绝并提示；用户明确确认后才能重试。
6. pull 发生冲突时保留冲突现场，返回错误并提示用户使用 IDE 或 Git 工具处理；SuperTask 不自动恢复现场。
7. Git 未安装、认证失败、远端不存在、分支不存在和网络失败都必须返回可理解的错误。

### 2.3 扫描并合并

1. 用户对已有工作区发起「重新扫描」。
2. 引擎扫描 Maven reactor 和 Node 候选，生成发现结果和警告，不立即修改 YAML。
3. UI 展示新增、匹配、未发现和冲突项，用户可以逐项选择是否合并。
4. 已有服务的端口、环境变量、依赖关系和自定义字段必须保留。
5. 未发现的已有服务不删除，只标记警告。
6. 用户确认后才按 `base_hash` 写回；文件被外部修改时返回 `YAML_CONFLICT`，要求重新加载。

### 2.4 桌面驻留与更新

1. 默认关闭主窗口时隐藏到托盘，运行中的项目服务继续运行。
2. 托盘菜单可恢复主窗口、打开当前工作区、启动全部、停止全部和退出应用。
3. 退出应用前停止当前工作区服务，并等待 Engine 释放 Job Object；失败时不能假装退出成功。
4. 开机启动默认关闭。开启后只启动 SuperTask，不自动启动工作区服务。
5. 应用启动后按设置自动检查更新；发现更新后只提示，不自动安装。
6. 安装更新前必须用户确认，且有服务处于 `starting`、`running`、`unhealthy` 或 `stopping` 时阻止安装。

## 3. 总体架构

```text
React UI
    │ Tauri invoke / event
    ▼
Tauri commands                     Tauri desktop integrations
    │                                ├─ tray
    ▼                                ├─ autostart
supertask-core                      └─ updater
    ├─ template                       
    ├─ git runner                     
    ├─ scan merge                     
    ├─ ide launcher                   
    ├─ app data                       
    └─ operation/event hub
         │
         ├─ git.exe
         ├─ explorer.exe / IDE
         └─ signed updater package
```

### 3.1 分层职责

- `supertask-core`：模板清单与复制、Git argv 及输出解析、扫描结果 merge、IDE 候选解析、应用数据模型、错误码和 operation 状态。
- `src-tauri`：Tauri 插件初始化、系统托盘菜单、窗口事件、updater 适配、命令参数反序列化和结果序列化。
- `frontend`：模板页、Git 页、扫描 merge 预览、IDE 操作入口、托盘/更新设置和 operation 展示。
- 前端不访问文件系统，不调用 shell，不拼接 Git 或 IDE 命令行。

### 3.2 长操作模型

clone、pull、模板创建和更新下载/安装可能超过一次 invoke 的合理等待时间。它们统一使用 operation：

1. Command 快速校验参数并返回 `operation_id`。
2. Engine 在后台执行操作。
3. 通过 `st.operation` 推送 `queued`、`running`、`succeeded` 或 `failed` 状态及有限进度信息。
4. operation 结束后，UI 按返回结果刷新工作区、Git 状态或更新状态。
5. 不把认证信息、完整环境变量或敏感 URL 写入事件。

operation 事件不替代 `st.runtime` 和 `st.logs`。服务日志继续按 1.0 的 `st.logs` 批次推送；Git 和模板操作的输出使用 operation 的摘要，必要时才使用受控的 system 信息。

## 4. 官方模板

### 4.1 模板来源

官方模板采用**随应用发布的内置离线资源**。1.1 不依赖网络获取模板，不提供远程模板市场。

首批模板至少包括：

| id | 用途 | 内容 |
|----|------|------|
| `spring-multimodule-node` | 完整示例 | Spring Boot 多模块后端、Node 前端、基础健康检查和依赖关系 |
| `spring-multimodule-node-minimal` | 最小起步 | 一个可运行 Spring 模块、一个 Node 服务和最小 YAML |

模板资源必须有固定版本和校验摘要。模板元数据由应用提供，不信任模板目录中可执行文件的名字或路径。

### 4.2 模板数据模型

模板清单至少包含：

```json
{
  "id": "spring-multimodule-node",
  "version": "1",
  "name": "Spring 多模块 + Node",
  "description": "用于本机开发的前后端工作区",
  "stacks": ["spring-boot", "node"],
  "files": ["pom.xml", "backend/", "web/", "supertask.yaml"],
  "sha256": "..."
}
```

`files` 只用于展示和校验，不由前端解释。模板复制由 core 依据内置 manifest 执行。

### 4.3 创建规则

- 目标路径必须是已选择的父目录下的单层子目录，禁止 `..`、UNC 逃逸和路径分隔符注入。
- 目标目录不存在时可以创建；存在时必须为空目录才允许复制。
- 不覆盖已有文件；不删除已有文件；不自动写父目录之外的内容。
- 创建完成后对关键文件做存在性和模板摘要校验。
- `supertask.yaml` 中写入：

```yaml
templates:
  source: builtin
  id: spring-multimodule-node
  version: "1"
```

- 若模板包含 `git` 元数据，仅作为来源说明保存，不自动执行 commit 或 push。

### 4.4 失败处理

| 情况 | 行为 |
|------|------|
| 目标目录非空 | `TARGET_NOT_EMPTY`，不修改目录 |
| 模板 id 不存在 | `NOT_FOUND` |
| 模板资源校验失败 | `TEMPLATE_INVALID`，不显示成功 |
| 无法创建文件 | `TEMPLATE_WRITE`，返回失败路径 |
| YAML 无法解析 | `YAML_PARSE`，保留已复制文件并提示手动修复 |

## 5. Git 集成

### 5.1 支持范围

1. `clone`：从远端获取新工作区。
2. `status`：读取当前仓库状态，不修改文件。
3. `pull`：从指定 remote 拉取当前分支，不提供 merge 策略编辑。

所有 Git 操作通过 Rust `std::process` 或 tokio spawn 调用 `git.exe`，不经过 `cmd.exe /C`，不引入 Git SDK。

### 5.2 Clone 规则

```text
git clone [--branch <branch>] <url> <target>
```

- URL 和 branch 是结构化参数；前端不得传完整命令行。
- URL 不允许包含用户名密码形式的内嵌凭据；认证交给 Git Credential Manager 或系统已配置的凭据。
- 目标目录必须不存在或为空目录。
- clone 过程中不能自动执行项目脚本、安装依赖或启动服务。
- clone 成功后返回规范化工作区 id；若没有 YAML，转入扫描向导。
- 输出中对 URL、token、credential helper 信息做脱敏；不把环境变量写入事件或日志。

### 5.3 Status 结果

`git.status` 至少返回：

```json
{
  "workspace_id": "C:/work/mall",
  "is_repository": true,
  "branch": "main",
  "detached": false,
  "dirty": true,
  "ahead": 1,
  "behind": 0,
  "staged": 2,
  "unstaged": 3,
  "untracked": 1,
  "remote": "origin"
}
```

细粒度文件列表不作为 1.1 默认返回，避免 Git 大仓库刷新时把大量路径送入 UI；UI 只展示摘要和刷新时间。

### 5.4 Pull 安全规则

- 默认 remote 为 `origin`，默认拉取当前分支。
- 首次 pull 前读取 status。
- `dirty: true` 时返回 `GIT_DIRTY`，UI 必须展示变更摘要并要求明确确认。
- 用户确认后以 `allow_dirty: true` 重试。确认不是自动 stash、reset 或覆盖的授权；Git 自己决定是否产生冲突。
- 有服务处于 `starting`、`running`、`unhealthy` 或 `stopping` 时，默认阻止 pull，返回 `GIT_WORKSPACE_BUSY`；用户必须先停止服务。
- pull 冲突返回 `GIT_CONFLICT`，保留文件现场，不执行 reset、checkout、clean 或 stash。
- Pull 成功后刷新 Git status；若 YAML 被更新，提示用户重新加载配置，不能自动覆盖未保存的 UI 编辑。

### 5.5 Git 错误码

| code | 说明 |
|------|------|
| `GIT_NOT_FOUND` | PATH 中没有 git.exe |
| `GIT_NOT_REPOSITORY` | 目标目录不是 Git 仓库 |
| `GIT_DIRTY` | 未提交修改，默认禁止 pull |
| `GIT_WORKSPACE_BUSY` | 有服务正在运行或切换状态 |
| `GIT_AUTH` | 认证失败或凭据不可用 |
| `GIT_REMOTE` | remote 不存在或不可访问 |
| `GIT_BRANCH` | 分支不存在或无法跟踪 |
| `GIT_CONFLICT` | pull 产生冲突 |
| `GIT_FAILED` | 其他 Git 非零退出 |

## 6. 扫描向导升级

### 6.1 匹配规则

扫描结果与已有服务按以下顺序匹配：

1. 相同服务 id。
2. 相同 `kind` 且 `spring-boot` 的 `module` 相同，或 `node` 的 `dir` 相同。
3. 其余视为新发现服务。

匹配必须可重复。相同结果多次扫描不能不断生成新服务或改变用户已有 id。

### 6.2 字段所有权

扫描器可以更新：

- `kind`
- `module` 或 `dir`
- Node 包管理器和候选 script
- 新服务的默认 port、health、grace
- 扫描诊断和来源信息

用户配置必须保持：

- 服务 id 和 `enabled`
- `port`、`ports`、服务 env、`env_file`
- `depends_on`
- 自定义 health、grace、extra_args、cwd、launch
- labels、group、restart、logging、resources
- 所有未知字段、reserved 段和 `x-*` 字段

如果用户已经修改了扫描器负责的字段，也视为用户配置并保留；UI 通过差异预览提示“发现值”和“当前值”，不静默覆盖。

### 6.3 Merge 结果

扫描预览将每项标记为：

| 类型 | 默认动作 | 说明 |
|------|----------|------|
| 新增 | 不立即写入 | 用户勾选后加入 |
| 匹配无差异 | 保留 | 不产生无意义改写 |
| 匹配有差异 | 保留当前 | 用户明确选择字段后才更新 |
| 未发现 | 保留 | 不删除，显示警告 |
| id 冲突 | 需要处理 | 生成稳定候选 id 并要求确认 |
| 扫描警告 | 仅提示 | 不阻止其他安全项合并 |

用户确认后由 core 生成合并后的 `SuperTaskFile`，再调用已有 YAML 保存机制。保存必须携带当前 `base_hash`，冲突返回 `YAML_CONFLICT`。

## 7. 打开 IDE

### 7.1 支持的目标

```text
explorer | cursor | idea | code
```

`explorer` 对应资源管理器，`cursor` 对应 Cursor，`idea` 对应 IntelliJ IDEA，`code` 对应 VS Code。

### 7.2 查找与启动

- `explorer` 使用系统资源管理器打开工作区根目录。
- IDE 优先从 PATH 查找，再检查 Windows 常见用户级和系统级安装位置。
- 查找结果只允许命中固定产品名和固定 executable 名称；不接受前端传入任意可执行文件路径。
- 启动参数只包含工作区根路径；不把用户输入拼成 shell 字符串。
- 不要求 IDE 已安装。未安装返回 `IDE_NOT_FOUND`，UI 提示可手动安装或选择其他方式。
- 打开目录成功只表示进程已成功创建，不表示 IDE 已完全加载。

### 7.3 用户设置

1.1 不提供任意 IDE 路径编辑器。可以记录最近一次成功探测到的固定候选，但应用更新后仍需重新验证文件存在且路径未逃逸。

## 8. 系统托盘与开机启动

### 8.1 默认行为

- 关闭窗口默认隐藏到托盘。
- 托管服务不会因隐藏窗口而停止。
- 托盘图标使用应用内置图标，并按当前工作区总体状态更新 tooltip：无工作区、运行中、存在异常或已停止。
- 首次退出行为可以通过设置明确选择；默认仍为隐藏，不使用模糊的双击退出语义。

### 8.2 托盘菜单

至少包含：

- 显示 SuperTask
- 打开当前工作区
- 启动全部
- 停止全部
- 退出 SuperTask

菜单项按当前状态禁用。没有当前工作区时，工作区操作显示为禁用，不创建假工作区。

### 8.3 退出顺序

1. 标记应用正在退出，阻止新的启动、Git pull 和模板操作。
2. 调用当前 Engine 的 workspace close。
3. 等待服务进入 stopped/exited，并释放 Job Object。
4. 关闭托盘和主窗口。
5. 超时或 Engine 失败时保留错误信息，不宣称已安全退出。

应用异常终止时依赖 Windows Job Object 的关闭行为清理由 Engine 创建的进程；正常退出仍必须走显式 close。

### 8.4 开机启动

- 设置项 `startOnLogin` 默认 `false`。
- 开启后只注册 SuperTask 本身，不带 workspace path，不执行 `runtime.startAll`。
- 关闭设置后撤销注册。
- 只能操作当前用户范围，不请求管理员权限。
- 注册失败返回 `AUTOSTART_FAILED`，设置 UI 保持未开启状态。

## 9. 自动更新

### 9.1 更新策略

- 默认允许启动时后台检查一次；检查失败不阻止应用使用。
- 可在设置中关闭自动检查。
- 发现新版本后显示版本号、发布日期和更新摘要；不自动安装。
- 用户明确确认后才下载和安装。
- 更新地址来自构建配置和发布 manifest，不由用户输入。
- 更新包必须通过 Tauri updater 的签名校验；签名失败必须拒绝安装。

### 9.2 安装前置条件

处于以下任意状态时不得安装：

- 服务 `starting`
- 服务 `running`
- 服务 `unhealthy`
- 服务 `stopping`
- 有脚本正在运行
- Git、模板或扫描 operation 正在修改工作区

UI 必须显示阻止原因和下一步，而不是直接失败为“更新不可用”。用户停止全部服务后可以重新安装。

### 9.3 更新状态与错误

```text
idle | checking | available | downloading | ready | installing |
up_to_date | failed | blocked_running
```

至少覆盖：网络不可用、manifest 不合法、签名不匹配、版本不适用、下载中断、安装失败和服务运行中。下载临时文件由 updater 管理，不写入工作区。

更新安装失败时应用保持当前版本可启动；不删除当前版本，不改写工作区文件。

## 10. 应用数据与 YAML 兼容

### 10.1 应用数据

将 1.0 前端 localStorage 的最近工作区状态迁移到 Tauri app data 目录：

```text
%APPDATA%/SuperTask/app.json
```

数据至少包含：

```json
{
  "version": 1,
  "recents": ["C:/work/mall"],
  "lastWorkspace": "C:/work/mall",
  "theme": "light",
  "restoreLast": true,
  "closeToTray": true,
  "startOnLogin": false,
  "updateCheck": true
}
```

规则：

- 最近工作区最多 20 条，按最近打开时间排序。
- 路径打开前必须重新 canonicalize；不存在的路径从可恢复列表中标记失效，不立即删除用户记录。
- forget 只修改 app data，不删除工作区、不停止非当前工作区的外部进程。
- 应用数据写入采用临时文件 + 替换，避免进程中断留下半份 JSON。
- app data 不保存代码、密钥、Git 凭据、服务日志或完整环境变量。

### 10.2 YAML 兼容

1.1 不改变 YAML `version: 1`，也不把 `templates` 或 `git` 提升为强制字段。

- `templates`、`git` 仍按 YAML 规范作为 reserved 段读取和写回。
- 1.1 生成的 YAML 必须可被 1.0 打开；1.0 会忽略 1.1 新增元数据但不得丢失。
- 结构化保存继续保留所有具名 reserved 段和 `x-*` 字段。
- 原文保存继续按 `base_hash` 做并发保护。
- Git pull 更新 YAML 后，若 UI 有未保存内容，必须提示冲突，不自动覆盖内存编辑。

## 11. IPC 契约增量

以下命令加入 `docs/spec/ipc.md` 的 1.1 扩展；命令仍遵循 protocol 1 的公共信封和稳定错误码。

### 11.1 Templates

```text
templates.list
  input:  {}
  output: { templates: TemplateSummary[] }

templates.create
  input:  { template_id, parent_path, directory_name }
  output: { operation_id }
```

模板创建返回的 operation 完成后包含 `workspace_id` 和模板生成结果。

### 11.2 Git

```text
git.clone
  input:  { url, target_path, branch? }
  output: { operation_id }

git.status
  input:  { workspace_id }
  output: GitStatus

git.pull
  input:  { workspace_id, remote?, branch?, allow_dirty? }
  output: { operation_id }
```

约束：

- `workspace_id` 必须是当前 Engine 已打开工作区的 canonical id。
- `allow_dirty` 默认 `false`；只有在 UI 已向用户展示脏状态并取得明确确认后才传 `true`。
- pull 不是强制覆盖操作；该字段不能改变 Git 的冲突行为。
- clone 的 `target_path` 必须由用户选择或由模板创建流程生成，并经过目录边界校验。

### 11.3 IDE

```text
workspace.openIde
  input:  { workspace_id, ide }
  output: { accepted: true, ide, path }
```

`ide` 仅接受 `explorer | cursor | idea | code`。返回 `path` 只用于展示实际命中的固定候选，不允许前端回传为下一次任意 executable。

### 11.4 应用偏好与更新

扩展现有 `app.savePrefs`：

```json
{
  "theme": "light",
  "restoreLast": true,
  "closeToTray": true,
  "startOnLogin": false,
  "updateCheck": true
}
```

新增：

```text
app.update.check
  input:  {}
  output: { operation_id }

app.update.install
  input:  { version }
  output: { operation_id }
```

托盘菜单行为由 Tauri 壳处理；前端设置只通过 `app.savePrefs` 修改偏好，不直接调用系统启动项 API。

### 11.5 Operation 事件

```json
{
  "protocol": 1,
  "event": "st.operation",
  "workspace_id": "C:/work/mall",
  "ts_ms": 0,
  "payload": {
    "operation_id": "op-123",
    "kind": "git.pull",
    "state": "running",
    "progress": 0.4,
    "message": "正在拉取 origin/main"
  }
}
```

`progress` 可为 null；不能伪造无法测量的精确百分比。结束事件必须包含成功结果或稳定错误码。operation id 只用于关联事件，不被当作文件路径或命令参数。

## 12. 错误与安全要求

### 12.1 新增稳定错误码

| code | 触发条件 |
|------|----------|
| `TARGET_NOT_EMPTY` | 模板或 clone 目标目录非空 |
| `TEMPLATE_INVALID` | 内置模板 manifest 或摘要校验失败 |
| `TEMPLATE_WRITE` | 模板复制失败 |
| `GIT_NOT_FOUND` | 找不到 git.exe |
| `GIT_NOT_REPOSITORY` | 目录不是 Git 仓库 |
| `GIT_DIRTY` | 工作区有未提交修改 |
| `GIT_WORKSPACE_BUSY` | 服务或脚本正在运行 |
| `GIT_AUTH` | Git 认证失败 |
| `GIT_REMOTE` | Git remote 不可用 |
| `GIT_BRANCH` | Git 分支无效 |
| `GIT_CONFLICT` | pull 产生冲突 |
| `GIT_FAILED` | Git 其他失败 |
| `IDE_NOT_FOUND` | 固定候选中没有目标 IDE |
| `AUTOSTART_FAILED` | 开机启动注册失败 |
| `UPDATE_BLOCKED_RUNNING` | 工作区仍有运行中任务 |
| `UPDATE_SIGNATURE` | 更新包签名校验失败 |
| `UPDATE_FAILED` | 更新检查、下载或安装失败 |

已有 `FEATURE_SOON` 仅继续用于尚未进入 1.1 的功能；模板和 Git 进入 1.1 后不应再返回 `FEATURE_SOON`。

### 12.2 安全边界

- 所有外部进程调用使用结构化 argv；不提供通用 `shell.exec`。
- Git URL 不携带凭据；Git 输出和 operation message 做敏感信息脱敏。
- 所有相对路径 canonicalize 后必须位于目标 workspace 或用户明确选择的父目录内。
- 模板文件只能来自内置 manifest 指定的资源；不能让 YAML 指定任意模板源或复制路径。
- IDE 启动只能使用固定枚举和后端探测结果。
- 更新只接受构建配置指定的 HTTPS manifest 和签名包。
- 不上传项目内容、Git 状态明细、环境变量或日志；遥测继续默认关闭。

## 13. 前端范围

### 13.1 `/templates`

- 真实读取 `templates.list`，不使用假数据。
- 展示内置模板卡片、技术栈、版本和文件概览。
- 使用目录选择器和目录名输入完成创建。
- 展示 operation 进度、目标目录和失败原因。
- 创建成功后打开工作区并进入 `/run`。

### 13.2 `/git`

- 无工作区时提供 clone 入口。
- 有工作区时展示仓库检测结果、分支、脏状态、ahead/behind、文件计数和刷新按钮。
- Pull 前展示确认框；脏状态默认不可直接 pull。
- 冲突和认证失败显示下一步，不提供假成功状态。
- Git 输出不做成可执行的命令文本，也不让用户编辑命令行。

### 13.3 运行页和设置页

- 运行页服务卡片和抽屉增加「打开」菜单，目标为固定 IDE 枚举。
- 设置页实现关闭到托盘、开机启动和自动检查更新。
- 更新页或设置区展示检查中、可用、阻止安装、安装中和失败状态。
- 页面继续由功能注册表和 provider 驱动；不在 `AppShell` 中按版本写长 if。
- 保持方案 H / Linear 浅色视觉规范、键盘可达性、焦点样式和无 emoji 图标纪律。

## 14. 非功能要求

### 性能

- `templates.list`、`git.status`、偏好读取等只读操作 p95 小于 100ms，不含首次进程冷启动。
- clone/pull/模板创建/更新不阻塞 UI 线程；命令接受响应目标小于 50ms。
- Git status 不默认返回完整文件列表；大仓库刷新仍需保持 UI 可操作。
- operation 事件限速，不能因 Git 输出导致事件风暴。

### 可靠性

- 任意 operation 只能有一个终态；重复事件按 operation id 去重。
- 应用重启后未完成 operation 标记为失败或未知，不显示为仍在运行。
- 模板复制失败不自动删除用户目录。
- pull 冲突不自动清理现场。
- updater 安装失败不破坏当前版本和工作区。
- 正常退出必须优先清理由 Engine 托管的进程。

### 可维护性

- core 中模板、Git、扫描 merge 和 IDE 查找分别封装，Tauri command 只做适配。
- Git runner 使用可替换的执行器，便于单测而不依赖用户机器上的远端服务。
- 模板 manifest、updater manifest 和签名配置属于发布资源，不写入业务代码的隐式常量。
- 所有新增命令和错误码同步更新 Rust 类型、TypeScript DTO、IPC 文档和测试。

## 15. 测试与验收

### 15.1 Core 单元测试

- 模板清单可枚举、版本和摘要校验失败会拒绝。
- 不存在目录、空目录、非空目录和路径逃逸均得到正确结果。
- Git porcelain/status 输出能正确解析 branch、detached、dirty、ahead/behind 和计数。
- Git 输出中的凭据样式会被脱敏。
- 扫描 merge 覆盖新增、匹配、未发现、id 冲突和用户字段保留。
- `templates`、`git`、reserved 和 `x-*` 字段 round-trip 不丢失。
- operation 状态不会从终态回到 running，重复终态可去重。

### 15.2 集成测试

使用临时目录和本地 Git bare repository 验证：

1. clone 到不存在目录。
2. clone 到空目录。
3. clone 到非空目录被拒绝且原文件不变。
4. clean pull 成功并刷新 status。
5. dirty pull 默认返回 `GIT_DIRTY`。
6. `allow_dirty` 确认后 pull 被执行。
7. pull 冲突返回 `GIT_CONFLICT`，冲突文件仍存在。
8. git 不在 PATH 时返回 `GIT_NOT_FOUND`。

### 15.3 Windows 验收

- Windows 10/11 真实 Git 环境下完成 clone/status/pull。
- Cursor、IDEA、VS Code 分别已安装和未安装时行为正确。
- 关闭窗口隐藏到托盘；恢复窗口、启动全部、停止全部和退出菜单可用。
- 开机启动设置重启应用后仍保持正确；只启动 SuperTask，不启动服务。
- 托盘退出后任务管理器中没有 SuperTask 管理的残留 Java/Node 进程。
- 更新签名正确时可安装；签名错误、网络失败和运行中服务时正确阻止或报错。

### 15.4 前端与回归

- `frontend` TypeScript 检查和生产构建通过。
- Playwright 覆盖模板创建、Git 状态、脏 pull 确认、扫描 merge 预览、IDE 错误反馈和设置项。
- 回归 1.0 十条场景：运行、停止、日志、YAML 冲突、未知字段保留和工具链缺失提示不能退化。
- 1.0 版本打开 1.1 生成的 YAML 后，`templates`、`git` 和 `x-*` 字段仍可完整写回。

## 16. 交付顺序

1. **规格和公共契约**：错误码、operation 事件、YAML 元数据、app data 模型。
2. **core 基础能力**：模板 manifest、目录安全、Git runner/status、扫描 merge、IDE 查找。
3. **Tauri 桌面能力**：app data、tray、autostart、updater 和窗口关闭/退出流程。
4. **模板与 Git UI**：模板创建、clone、status、pull 和 operation 状态。
5. **扫描向导 UI**：差异预览、字段选择、YAML 冲突处理。
6. **运行页和设置页**：IDE 菜单、托盘设置、更新设置。
7. **Windows 集成验收与发布**：签名、安装包、updater manifest、回归和真实工作区验证。

## 17. 已确认决策

- 官方模板采用内置离线分发。
- 官方首批至少提供 `spring-multimodule-node` 和 `spring-multimodule-node-minimal`。
- Git pull 遇到脏工作区默认拒绝；用户明确确认后才允许继续。
- 关闭主窗口默认隐藏到托盘，服务继续运行。
- 自动更新默认检查，安装始终需要用户确认。
- 开机启动默认关闭，且不自动启动项目服务。
- 1.1 只做 Git clone/status/pull，不做 commit、push、checkout、stash 和 merge UI。
- IDE 只支持固定枚举和后端候选探测，不开放任意 executable 路径。
- 1.1 不改变 YAML 格式版本，必须保持 1.0 向前读取和 reserved 字段 round-trip。
