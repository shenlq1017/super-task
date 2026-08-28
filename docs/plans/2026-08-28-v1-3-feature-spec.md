# SuperTask 1.3 功能规格

> 日期：2026-08-28  
> 状态：范围与默认决策已确认，待实现（前置：1.2 交付或明确裁剪）  
> 上位文档：[产品路线](2026-08-25-product-roadmap.md) · [1.2 功能规格](2026-08-27-v1-2-feature-spec.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [引擎架构](../spec/architecture.md)

本文把路线中的 1.3「能装箱」收到可实现、可测试、可交付的粒度。1.3 的目标是让有状态依赖（Redis、MySQL、消息队列）和存量 Docker Compose 项目进入同一个工作台：compose 文件里的一个服务就是一个 SuperTask 服务，可以单独起停、看日志、被 `depends_on`、参与端口检查；镜像可以构建；`/docker` 页从占位转为可用。全程只 spawn `docker` CLI，不链 Docker SDK。

## 1. 目标与边界

### 1.1 产品目标

1. **能复用**：存量 `compose.yaml` 不改一行就能导入成 `kind: compose` 服务草稿。
2. **能混排**：compose 服务与 `spring-boot`/`node` 服务在同一依赖图、同一运行页、同一日志管道里；Spring 连 Redis 用 `127.0.0.1:映射端口`。
3. **能构建**：compose 文件内的 `build:` 段与工作区显式 `docker.builds` 列表都可以触发镜像构建，走 operation。
4. **能看懂**：`/docker` 页显示引擎状态、容器、镜像；Docker 未安装或 Docker Desktop 未运行时说人话、给下一步。
5. **能清场**：SuperTask 退出的容器全部 stop；不做 down / rm，卷和数据保留。

sidecar 需求（Redis/MySQL）在 1.3 即由 compose 覆盖，不做专用 kind。

### 1.2 版本范围

| 能力 | 1.3 行为 |
|------|----------|
| 服务 | `kind: compose`，一个 compose 服务 = 一个 SuperTask 服务 |
| 起停 | `docker compose up -d --no-deps <svc>` / `docker compose stop <svc>` |
| 日志 | `docker compose logs --follow` 接入现有批次/环形/文件管道 |
| 健康 | 默认 `tcp` 探测主机映射端口；compose healthcheck 只展示不参与状态机 |
| 依赖 | compose 服务与本地服务可互相 `depends_on`；拓扑仍由引擎计算 |
| 端口 | compose 服务主机端口参与 `PORT_DUP` 与 PortInspector 检查 |
| 镜像 | `docker compose build <svc>` 与 `docker.builds` 显式构建 |
| 导入 | 扫描器识别 compose 文件，走 1.1 的 scanPreview/scanApply 向导 |
| 页面 | `/docker` 页 live；服务抽屉「容器」Tab live |

### 1.3 明确不做

以下能力不进入 1.3：

- 生成或修改 Dockerfile、compose 文件；`docker run` 任意容器
- 镜像 push、registry 登录、凭据保存
- `docker system prune`、down、`--rmi`、volume 管理等破坏性操作
- 容器 CPU/内存指标（`docker stats`）；compose 服务 `metrics: null`，1.2 的指标口径仍是 Job Object
- 在容器里 exec、PTY、端口转发 UI
- K8s、远端 Docker host、`DOCKER_HOST` 之外的 context 切换（继承用户环境变量，不管理）
- 向 compose 注入 SuperTask 的 env / secrets；compose 的环境完全由 compose 文件自管
- WSL2 后端（2.2）、网关（1.6）、云（2.0）

1.3 继续只支持 Windows 10/11 桌面 + Docker Desktop 场景验收；docker CLI 本身跨平台，macOS/Linux 的等价验收并入 1.4。YAML 继续 `version: 1`，IPC 继续 protocol 1。

## 2. 用户场景与成功标准

### 2.1 sidecar：Spring + Redis + MySQL

1. 用户工作区有 `compose.yaml`（redis、mysql 两个服务，映射 6379/3306）。
2. 扫描向导或手工配置后，运行页出现 `redis`、`mall-db` 两个 `kind: compose` 服务卡片。
3. `user-api`（spring-boot）`depends_on: [redis, mall-db]`；启动全部时按拓扑先起容器再起 Spring。
4. 首次启动镜像未拉取时，拉取进度写入该服务日志，状态保持 `starting`，不误报失败。
5. 停止 `redis` 只执行 `docker compose stop redis`；Spring 不受影响，重启 `redis` 后恢复。
6. 退出 SuperTask 时，由 SuperTask 启动的容器全部 stop；用户手工 `docker compose up` 的容器不动。

### 2.2 存量 compose 导入

1. 用户对已有 compose 项目执行扫描（首次 `workspace.scanDraft`，已有 YAML 走 `workspace.scanPreview`）。
2. 扫描器发现 compose 文件（多个候选时警告并选优先级最高者），列出全部服务供选择。
3. 用户勾选后生成 `kind: compose` 草稿：`service`、主机端口、compose 内 `depends_on` 映射为 SuperTask `depends_on`。
4. compose 文件内不映射的字段（volumes、networks、healthcheck 配置等）留在 compose 文件里，SuperTask 不复制。
5. Docker 不可用时扫描降级：不生成 compose 草稿，返回带 `DOCKER_NOT_FOUND` 的警告，其余扫描照常。

### 2.3 镜像构建

1. 用户在 `/docker` 页或服务卡点击「构建镜像」。
2. compose 服务触发 `docker compose build <svc>`；`docker.builds` 条目触发 `docker build`，tag 来自 YAML 结构化列表。
3. 构建走 operation：进度、取消、失败原因（含 daemon 错误摘要）可见；输出尾部进入日志。
4. 构建失败不影响已运行容器；成功后下次 `up` 使用新镜像。

### 2.4 Docker 异常

1. PATH 无 docker：`docker.probe` 返回 `found: false`，页面给出安装指引链接，不提供代装（工具链安装 1.2 只覆盖 JDK/Maven/Node）。
2. Docker Desktop 已安装未运行：`DOCKER_ENGINE_UNREACHABLE`，提示启动 Docker Desktop，提供「重试探测」。
3. 有 docker 无 compose 插件：`DOCKER_COMPOSE_MISSING`，compose 相关功能禁用，本地服务不受影响。
4. YAML 引用的服务在 compose 文件中不存在：打开工作区时警告，启动返回 `COMPOSE_SERVICE_MISSING`。

### 2.5 生命周期与日志

1. 容器被外部停止（用户 `docker stop` 或 OOM）：状态转 `exited`，复用 1.2 崩溃通知。
2. compose 服务日志与本地服务日志同规格：批次事件、环形缓冲、`.supertask/logs/{id}.log` 文件、1.2 历史搜索与导出可用。
3. `docker compose logs --follow` 在容器停止后自然结束，reader 不报错；重启服务后重新跟随。
4. compose 服务的 `pid` 与 `metrics` 为 null，卡片显示「容器托管」而非进程信息。

## 3. 总体架构

```text
React UI（/docker 页、服务卡片容器 Tab）
    │ Tauri invoke / event
    ▼
Tauri commands（薄适配）
    │
    ▼
supertask-core
    ├─ spec        docker 段 / kind: compose 校验
    ├─ docker/     CLI 适配层（probe / runner / compose config 解析）
    ├─ launcher    ComposeService 启动器（接入现有状态机）
    ├─ runtime     容器状态轮询、退出清场
    ├─ log         compose logs --follow 接入现有管道
    ├─ scan        compose 探测 → 1.1 merge 向导
    └─ operation   镜像构建
         │
         └─ docker CLI（spawn，固定 argv；不链 SDK）
```

### 3.1 分层职责

- `supertask-core` 的 `docker` 模块只负责：CLI argv 组装、输出解析（`--format json`）、超时与错误映射。不含任何 Docker 业务规则。
- 状态机、依赖拓扑、端口检查、日志管道、operation hub 全部复用现有实现，不为容器另起一套。
- Tauri 层无新增桌面集成；`/docker` 页只消费命令与事件。

### 3.2 长操作模型

- **镜像构建**（`docker.build`、`runtime.build` 对 compose 服务）走 operation：`queued → running → succeeded/failed/cancelled`，`st.operation` 推送，构建输出尾部作为 message 附加。
- **up / stop** 不走 operation：它们是 runtime 状态机的一部分，`runtime.startOne/stopOne` 立即返回 `accepted`，结果走 `st.runtime`（与本地服务一致，UI 不需要两套心智模型）。
- 取消语义：构建取消是 best effort（已提交的层缓存不回滚）；stop 不可取消。

## 4. Docker CLI 适配层

### 4.1 探测

`docker.probe`：

```json
{
  "found": true,
  "version": "27.1.1",
  "compose_version": "2.29.1",
  "running": true
}
```

- `found`：PATH 有 docker 可执行文件。
- `running`：`docker version --format json` 能取到 Server 段（daemon 活着）。
- 探测结果缓存于会话，`/docker` 页可强制刷新；`running=false` 时后续 compose 命令同步失败 `DOCKER_ENGINE_UNREACHABLE`。
- 探测与命令都不修改 `DOCKER_HOST`、context 等 用户环境。

### 4.2 命令执行规则

- 所有 docker 调用使用固定程序 + 结构化 argv，单次输出读取上限（默认 2 MiB，超出截尾并标记 truncated），执行超时（probe 5s，ps/config 10s，build 无超时但可取消）。
- argv 中唯一来自 YAML 的变量是：compose 文件路径（沙箱校验后）、project name、service 名、builds 的 context/dockerfile/tags（均经字符集校验：禁止空格、`--` 前缀、控制字符；service 名还需匹配 compose 解析结果）。
- 退出码非零映射为 §10 错误码，stdout/stderr 尾部（脱敏后）进 `details` 或服务日志。
- `docker compose` 子命令统一带 `-f <file>` 与可选 `-p <project>`；`--ansi never`、`--no-progress`?拉取进度需要保留，用 `--ansi never` 即可。

### 4.3 compose 解析

不手写 compose schema 解析器。需要服务清单/端口/依赖时执行：

```text
docker compose -f <file> config --format json
```

- 结果缓存：文件 mtime + 字节 hash 变化才重新执行；spec 校验与端口检查读缓存，不重复 spawn。
- 从输出提取每个服务的 `name`、主机映射端口（`ports[].published`，多个时第一个为 `port`，其余进 `ports`）、`depends_on` 键、`build` 是否存在、healthcheck 是否配置。
- YAML 中 compose 服务的 `port` 与解析结果不一致：打开时警告（`COMPOSE_PORT_MISMATCH` 进 warnings[]），行为以 YAML `port` 为准（健康、冲突检查），便于用户显式固定。

## 5. `kind: compose` 服务

### 5.1 配置模型

```yaml
docker:
  compose_file: compose.yaml    # 相对 root，沙箱内；缺省按 §7 候选顺序探测
  project_name: mall            # 可选；缺省交给 docker compose 默认（目录名）
  builds:                       # 可选
    - name: mall-user
      context: user-service     # 相对 root，沙箱内
      dockerfile: Dockerfile    # 可选，默认 <context>/Dockerfile
      tags: ["mall-user:local"] # 至少一个；docker.io/ 前缀允许

services:
  redis:
    kind: compose
    service: redis              # 必填：compose 文件内的服务名
    port: 6379                  # 可选：主机映射端口
  mall-db:
    kind: compose
    service: mysql
    port: 3306
    depends_on: [redis]
```

字段规则：

- 通用字段中，`kind: compose` 服务只允许：`enabled`、`group`、`labels`、`port`、`ports`、`depends_on`、`depends_on_ex`、`health`、`grace_secs`、`logging`、`x-*`。
- `env`、`env_file`、`extra_args`、`cwd`、`restart`、`module`、`dir`、`package_manager`、`launch`、`build_args`、`jvm_args` 对 compose 服务 **非法**，出现即 `SPEC_INVALID`——防止用户以为 SuperTask 会向容器注入环境。
- `service` 必须匹配 `^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$` 且存在于 compose 解析结果（不存在 → 打开警告 + 启动 `COMPOSE_SERVICE_MISSING`）。
- 默认 `grace_secs`: **60**（覆盖首次拉镜像的慢启动）；默认 `health.type`: `tcp`（有 `port` 时），无 `port` 则 `none`。
- 1.2 profile 对 compose 服务只能覆盖 `enabled`、`env` 之外的既有白名单（`enabled`/`port`；`env` 本就非法）。
- `restart` 策略由 compose 文件管理，SuperTask 的 `restart` 字段（1.2）不作用于 compose 服务。

### 5.2 启动与停止

启动（`runtime.startOne` / 拓扑顺序内）：

1. 同步检查：docker probe、compose 文件存在、`service` 在 compose 解析结果中、端口检查（1.2 口径）。失败同步返回，不 accepted。
2. 异步执行 `docker compose -f <file> [-p <name>] up -d --no-deps <service>`：
   - **必须带 `--no-deps`**：SuperTask 依赖图是唯一顺序真源，不允许 compose 内部 `depends_on` 悄悄带起未在拓扑中的服务。compose 文件内的等待条件（`service_healthy` 等）不生效，需要顺序就用 SuperTask `depends_on`（导入时已映射）。
   - up 的 stdout/stderr（拉取进度、创建错误）进入该服务日志，状态 `starting`。
3. up 退出 0 后进入健康探测；健康通过 → `running`。up 非零 → 状态回到 `stopped`，`last_error` 为 `COMPOSE_UP_FAILED` + 输出摘要，不进 `running`。

停止：

1. `docker compose stop <service>`；容器退出确认后 → `stopped`。
2. 非零退出 → `COMPOSE_STOP_FAILED`，重新查询容器实际状态并对齐 UI。
3. 超时宽限后仍在运行 → 重发 stop 并报 `JOB_KILL` 语义（复用现有错误码），不提供 `kill`/`rm` 逃生门。

重启 = stop + up，同 1.0 语义。

### 5.3 状态与容器监控

- compose 服务无 `pid`；`ServiceRuntime.pid = null`、`metrics = null`。
- 引擎按健康探测节奏查询容器状态（`docker compose ps --format json <service>`）：容器 `exited` 且非本引擎 stop 请求 → `exited`，reason `crash`，走 1.2 崩溃通知。
- 容器被用户从外部启动（引擎认为 stopped 但端口已被该服务监听）：下次 startOne 前 PortInspector 如实报占用；不做自动接管。
- `runtime.snapshot` 对 compose 服务与其他服务同构，`kind: "compose"` 供 UI 区分图标。

### 5.4 日志

- 启动成功后执行 `docker compose logs --follow --no-color --timestamps <service>`，reader 线程接入现有管道：切行 → 环 → 批次事件 → 文件。
- 不回放历史行（`--follow` 从当前开始）；历史查 1.2 `logs.search` 搜 `.supertask/logs/{id}.log` 文件。
- 容器 stop 后 `--follow` 自然退出，reader 正常收尾；再次启动时重新执行。
- docker CLI 自身输出（非容器 stdout）以 `system` stream 行标注来源。

### 5.5 依赖与端口

- compose 服务与本地服务可互相 `depends_on`；拓扑、`CYCLE` 检查、启动等待语义（等待到 `running|unhealthy|exited`）完全复用。
- compose 服务的主机端口参与：`PORT_DUP` 硬错误（1.2 起）、PortInspector `PORT_IN_USE`（外部进程占用含其他项目容器）、`ports.assign` 建议算法的跳过集合。
- 修改 compose 服务端口：`ports.assign` 只改 YAML `port`（健康 URL 跟随）；**不改 compose 文件**，与解析结果不一致时按 §4.3 警告。用户需自己改 compose 映射，警告文案说明。

### 5.6 退出清场

- 应用退出顺序（1.1 §8.3）扩展：compose 服务与其他服务一起进入关闭流程，按逆拓扑 stop。
- 只 stop 由本引擎启动过的 compose 服务；用户手工起的容器（`docker ps` 里引擎未记录）不动。
- 退出时 Docker daemon 不可达：记录错误，不假报成功；下次启动时 PortInspector 会如实显示端口占用。

## 6. 镜像构建与 tag

### 6.1 触发方式

| 入口 | argv | 输出去向 |
|------|------|----------|
| compose 服务「构建镜像」/ `runtime.build` | `docker compose -f <file> build <service>` | 该服务日志 |
| `/docker` 页 builds 条目 / `docker.build` | `docker build -f <dockerfile> -t <tag>... <context>` | system 源日志 |

### 6.2 规则

- `docker.build` 的 `name` 必须是 YAML `docker.builds` 中已定义的条目（`DOCKER_BUILD_UNKNOWN`）；前端不能传任意 context/dockerfile/tag。
- `tags` 至少一个，格式 `repo[:tag]`，字符集校验（字母数字、`.`、`:`、`/`、`-`、`_`）；禁止 `--` 前缀与空格。
- `context`、`dockerfile` canonicalize 后必须在工作区内，否则 `PATH_ESCAPE`。
- 构建期间可取消（operation cancel）；取消后不删除已构建层，状态如实显示 cancelled。
- 构建输出单行截断按日志 8 KiB 规则；operation message 只带尾部摘要（默认最后 20 行）。
- 不做 build cache 清理、`--no-cache` 选项（需要时用户在 Dockerfile/CLI 层处理；YAML 不开放任意 build flags）。

## 7. 存量 compose 导入（扫描）

### 7.1 文件发现

候选顺序：`compose.yaml` > `compose.yml` > `docker-compose.yml` > `docker-compose.yaml`（工作区根，不递归）。

- 多个候选并存：警告并列出全部，选优先级最高者生成 `docker.compose_file` 显式固定。
- 都不存在：不产生 compose 草稿（不是错误）。

### 7.2 草稿生成

- 通过 §4.3 `docker compose config --format json` 拿到服务清单；Docker 不可用时整段跳过并警告 `DOCKER_NOT_FOUND`。
- 每个 compose 服务生成一个候选：

| compose（规范化输出） | 草稿字段 |
|----------------------|----------|
| 服务名（id 合法时直接用；非法字符替换 `_` 并警告） | `id` / `service` |
| `ports[0].published` | `port` |
| `depends_on` 键 | `depends_on`（引用不存在的 id 时丢弃并警告） |
| `build` 存在 | `labels` 里标注 `supertask.docker.build=true`（仅展示用） |

- 其余 compose 字段（镜像、环境、卷、网络、healthcheck、restart）不进入 YAML；compose 文件仍是容器行为的唯一真源。
- 导入走 1.1 `workspace.scanPreview` / `scanApply` 同一命令与向导 UI；匹配规则增加第 ②' 条：`kind: compose` 且 `service` 相同视为 `match_same`。`/docker` 页的「从 compose 导入」按钮调用同一命令。

## 8. YAML 与应用数据兼容

### 8.1 新增或启用字段

| 字段 | 1.3 行为 | 旧版本行为 |
|------|----------|------------|
| `docker` | typed：`compose_file` / `project_name` / `builds` | 1.0–1.2 具名 reserved，round-trip |
| `services.*.kind: compose` | 可启动 | 1.0–1.2 可解析不可启动（`KIND_UNSUPPORTED`） |
| `services.*.service` | compose 服务名，typed | 旧版经 extra round-trip |

- `docker.builds` 条目内的未知键 round-trip。
- 旧客户端对 1.3 YAML 执行结构化保存不得丢 `docker` 段与 `service` 字段（纳入 schema 兼容测试）。
- app data **不升版**：1.3 无新增应用级偏好，维持 1.2 的 version 2。

### 8.2 schema

`supertask.schema.json` 补 `docker` 段与 `service` 字段；`additionalProperties: true` 策略不变。

## 9. IPC 契约增量

protocol 保持 1。`session.hello` 的 `docker` feature 转 `live`（since 1.3）。

```text
docker.probe
  input:  { refresh? }
  output: { found, version?, compose_version?, running }

docker.ps
  input:  { workspace_id }
  output: { containers: ContainerSummary[] }   # 限于当前 compose project（无 compose 文件则空）

docker.images
  input:  {}
  output: { images: ImageSummary[] }           # 本机只读列表

docker.build
  input:  { workspace_id, name }
  output: { operation_id }

ContainerSummary = { service, container_id, image, state, health?, ports: number[] }
ImageSummary    = { repository, tag, id, size_bytes, created_ms }
```

- `docker.ps` / `docker.build` 从 1.0 的 SOON 占位转 live；`gateway.apply` 等保持 SOON。
- compose 服务的起停、快照、日志、搜索、端口命令全部复用现有命令，无新增。
- `docker.images` / `docker.ps` 为只读命令，无缓存承诺（每次 spawn 查询，p95 见 §12）。

## 10. 错误与安全要求

### 10.1 新增稳定错误码

| code | 触发条件 |
|------|----------|
| `DOCKER_NOT_FOUND` | PATH 无 docker 可执行文件 |
| `DOCKER_ENGINE_UNREACHABLE` | daemon 未运行（如 Docker Desktop 未启动） |
| `DOCKER_COMPOSE_MISSING` | docker 在但 compose 插件不可用 |
| `COMPOSE_FILE_MISSING` | `docker.compose_file` 不存在 |
| `COMPOSE_SERVICE_MISSING` | `service` 不在 compose 文件中 |
| `COMPOSE_CONFIG_FAILED` | `docker compose config` 非零退出或输出不可解析 |
| `COMPOSE_UP_FAILED` | up 非零退出 |
| `COMPOSE_STOP_FAILED` | stop 非零退出 |
| `COMPOSE_PORT_MISMATCH` | YAML port 与 compose 解析不一致（仅 warnings[]） |
| `DOCKER_BUILD_UNKNOWN` | builds 列表无此 name |
| `IMAGE_BUILD_FAILED` | 镜像构建非零退出 |

### 10.2 安全边界

- 只 spawn `docker` 固定程序；argv 变量全部来自 YAML 结构化字段并经字符集/沙箱校验；不提供任意 flags 通道。
- compose 文件、build context、dockerfile 路径 canonicalize 后必须在工作区内。
- 不向容器注入 SuperTask 环境变量或 secret 值；compose 环境由用户 compose 文件自管（含其自身 env_file 机制）。
- 不保存 registry 凭据，不提供 push/login。
- 不提供 prune/down/rm/volume 删除接口；停止只 `stop`。
- 容器进程不在 Job Object 内；进程树终止委托 docker stop，SuperTask 不直接对容器内 PID 发信号。
- docker 输出进日志/事件前按既有单行截断规则处理；daemon 错误摘要不含环境块。

## 11. 前端范围

### 11.1 `/docker` 页（ComingSoon → live）

- 状态卡：引擎版本、compose 版本、运行状态；未安装/未运行的分态文案与「重试探测」。
- 工作区卡片：compose 文件路径、服务表（名称、端口、build 标记）、「从 compose 导入」入口（跳扫描向导）。
- 容器与镜像列表：只读，展示状态/健康/大小；镜像不提供删除。
- 构建入口：builds 条目与 compose 服务，operation 状态与取消。

### 11.2 运行页与抽屉

- compose 服务卡片：kind 图标区分、端口、状态；`pid`/CPU/内存区域显示「容器托管」。
- 抽屉「容器」Tab（占位 → live）：镜像、容器 ID、compose healthcheck 状态（只展示）、最近退出码。
- 起停、分组、profile、端口冲突建议对 compose 服务一视同仁；改端口动作提示「需同步修改 compose 映射」。

### 11.3 UI 约束

- 路由与命令仍由 feature registry 驱动；`docker` feature 翻 live 后导航自动启用，不在 AppShell 加条件。
- 长操作（构建）显示 operation 状态；up/stop 沿用 runtime 状态，不套两层进度。
- 视觉沿用方案 H / Linear 浅色 token，容器类图标用 Lucide 现有集合。

## 12. 非功能要求

### 性能

- `docker.probe`（daemon 在线）p95 < 150ms；`running=false` 的失败路径 < 200ms。
- `docker.ps` / `docker.images` p95 < 500ms（受 daemon 影响，UI 显示加载态）。
- `runtime.startOne` 对 compose 服务 accepted 返回 < 50ms（up 异步执行）。
- compose config 解析结果缓存（mtime+hash），spec 打开与端口检查不重复 spawn。
- `logs --follow` reader 复用现有有界队列；64 服务上限包含 compose 服务。

### 可靠性

- Docker daemon 中途不可达：服务状态如实转 `exited` 或保持并报错，不假报 running；恢复后探测自动纠正。
- up 成功但容器立刻退出：健康失败 + 容器状态查询 → `exited`，不卡 `starting`。
- 日志 reader 崩溃不影响服务状态；重启跟随。
- 退出清场 stop 失败记录错误并保留容器现状，下次启动可见。
- 构建取消/失败不影响运行中容器。

### 资源与隐私

- 不上传镜像清单、容器状态、compose 内容。
- docker CLI 输出读取有上限，防止 `logs` 灌爆内存。
- 不缓存镜像/容器数据到 app data。

## 13. 测试与验收

### 13.1 Core 单元测试

- `kind: compose` spec 校验：非法字段拒绝、`service` 字符集、缺 `service`、compose 服务与 profile 白名单。
- `docker.builds` 校验：tag 格式、context 沙箱、name 查找。
- compose config JSON 解析：端口/依赖提取、id 合法化、非法输出 → `COMPOSE_CONFIG_FAILED`（fixture 文本，不真调 docker）。
- 缓存命中：同 mtime+hash 不重复执行。
- 错误码序列化字符串。

### 13.2 集成测试（fake docker CLI）

- fake `docker` 脚本桩：断言 up 带 `--no-deps` 与单服务名、stop 不带 `--rm`、build argv 顺序。
- 状态机：starting → running → exited（外部退出）→ crash 通知路径。
- 日志管道：`--follow` 输出进环与文件；容器 stop 后 reader 收尾。
- 退出清场：只 stop 引擎启动的服务。
- 导入：fixture compose 文件 → 草稿字段与警告。

### 13.3 Windows 真机验收

- Docker Desktop 在线：redis+mysql sidecar + Spring `depends_on` 全链路（首次含拉镜像）。
- Desktop 已装未运行 / 无 docker / 无 compose 插件三种分态文案与错误码。
- 端口被其他项目容器占用 → `PORT_IN_USE` 建议；YAML 内 compose 服务与本地服务撞端口 → `PORT_DUP`。
- 真实 Dockerfile 构建成功与失败路径；取消后状态。
- 退出应用后 `docker ps` 无本引擎启动的运行容器；手工起的容器仍在。
- compose 服务日志搜索/导出（1.2 功能）可用。

### 13.4 前端与回归

- tsc / vite build / Playwright：`/docker` 页分态、构建 operation、导入向导、compose 卡片与容器 Tab。
- 回归 1.0–1.2：本地服务起停/日志/YAML 冲突/未知字段保留/工具链/端口/secrets/profile/jar 不退化。
- 旧客户端模型读写 1.3 YAML：`docker` 段与 `service` 字段 round-trip 不丢。

## 14. 交付顺序

1. **模型与兼容层**：`docker` typed 段、`kind: compose` spec 校验、错误码、schema/round-trip 测试；`docker` feature 仍 soon。
2. **Docker CLI 适配层**：probe / runner / compose config 解析与缓存 + fake docker 测试床。
3. **compose 运行时**：launcher、up/stop、容器状态轮询、日志接入、退出清场；`runtime.*` 对 compose 生效。
4. **镜像构建**：`docker.builds`、compose build、`runtime.build` 扩展、operation 接线。
5. **导入与扫描**：compose 文件发现、草稿生成、scanPreview/scanApply 扩展。
6. **前端**：`/docker` 页 live、compose 卡片、容器 Tab、feature 翻 live。
7. **Windows 集成验收**：Docker Desktop 全场景 + 回归。

依赖关系：端口检查与崩溃通知依赖 1.2 对应能力；operation hub 来自 1.1；若 1.2 未完，3（端口部分）与 crash 通知后移，其余可先行。`docker` feature 翻 live 必须在第 6 步之后。

## 15. 已确认决策

- 1.3 定位「能装箱」：只消费用户已有 compose/Dockerfile，不生成、不修改、不管理 Docker 资源。
- 一个 compose 服务 = 一个 SuperTask 服务（一等公民是 service，不是「整套 compose」）。
- 只 spawn docker CLI，不链 Docker SDK；compose 解析交给 `docker compose config --format json`。
- up 必带 `--no-deps`：SuperTask 依赖图是顺序唯一真源。
- stop 只 stop；退出清场同；永不 down/rm。
- 健康默认 tcp 打主机映射端口；compose healthcheck 只展示。
- 不向容器注入 env/secrets；compose 文件自管环境。
- 容器指标不做（`metrics: null`），口径与 1.2「只覆盖 Job Object」一致。
- YAML `version: 1`、protocol 1、app data version 2 均不变。
