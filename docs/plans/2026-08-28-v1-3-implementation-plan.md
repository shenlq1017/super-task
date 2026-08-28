# SuperTask 1.3 实现计划

> 日期：2026-08-28  
> 状态：phase 1–6 完成（compose 运行时/构建/扫描/前端/docker feature 已翻 live），剩 phase 7 真机验收。进度见 [2026-08-28-v1-3-progress.md](2026-08-28-v1-3-progress.md)  
> 功能规格真源：[2026-08-28-v1-3-feature-spec.md](2026-08-28-v1-3-feature-spec.md)  
> 上位：[AGENTS.md](../../AGENTS.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md)

把规格 §14 交付顺序拆成可执行任务。行为细节、错误码语义、安全边界以功能规格为准。

## 一句话

先落 `docker` typed 段与 `kind: compose` 的解析校验（docker feature 仍 soon），再按 CLI 适配层 → compose 运行时 → 镜像构建 → 导入 → 前端 → 真机验收的顺序接行为。全程只 spawn `docker` 固定程序 + 结构化 argv，up 必带 `--no-deps`，永不 down/rm。

## 约束（贯穿各 phase）

- 业务只进 `crates/supertask-core`；Tauri command 闭包只做 IPC 适配。
- argv 中唯一来自 YAML 的变量：compose 文件路径（沙箱内）、project name、service 名、builds 的 context/dockerfile/tags——全部字符集校验，禁止 `--` 前缀与空格。
- `docker compose` 子命令统一带 `-f <file>`（可选 `-p <name>`）与 `--ansi never`。
- compose 解析不手写 schema：`docker compose -f <file> config --format json`，结果按 mtime+hash 缓存。
- up/stop 走 runtime 状态机（不走 operation）；镜像构建走 operation（可取消）。
- YAML `version: 1`、IPC protocol `1`、app data version `2` 均不变；`docker` feature 翻 live 必须在前端步之后。
- 1.2 前置缺口：端口检查（phase 3）与 crash 通知（phase 5）未交付——依赖它们的步骤（端口参与检查、外部退出崩溃通知）后移，其余先行。
- 容器进程不进 Job Object；退出清场只 stop 本引擎启动过的服务。

## Phase 1 — 模型与兼容层（本轮）

规格 §14.1、§5.1、§8。只做模型、校验与 round-trip 测试；`docker` feature 保持 soon，`kind: compose` 可解析不可启动（`KIND_UNSUPPORTED`）。

### 任务 1.1 ErrorCode 1.3

- **文件：** `crates/supertask-core/src/error.rs`
- **做：** §10.1 十一个稳定码，serde SCREAMING_SNAKE_CASE：`DOCKER_NOT_FOUND`、`DOCKER_ENGINE_UNREACHABLE`、`DOCKER_COMPOSE_MISSING`、`COMPOSE_FILE_MISSING`、`COMPOSE_SERVICE_MISSING`、`COMPOSE_CONFIG_FAILED`、`COMPOSE_UP_FAILED`、`COMPOSE_STOP_FAILED`、`COMPOSE_PORT_MISMATCH`、`DOCKER_BUILD_UNKNOWN`、`IMAGE_BUILD_FAILED`。
- **测试：** 每个码序列化为规格字符串。
- **完成标准：** 码表与 §10.1 一一对应；旧码不变。

### 任务 1.2 Typed `docker` 段与 `service` 字段

- **文件：** `crates/supertask-core/src/spec/file.rs`；调用方 `scan.rs`
- **做：**
  - `SuperTaskFile.docker` 从 `Option<Value>` 改 typed `DockerSpec`：`compose_file` / `project_name` / `builds: Vec<DockerBuild>`（name/context/dockerfile/tags + flatten extra）；未知键 round-trip
  - `ServiceSpec.service: Option<String>`（compose 服务名，typed）
- **不在本任务：** compose 文件解析、启动行为
- **完成标准：** 1.0–1.2 YAML 照常解析；`docker` 段与 `service` 字段 round-trip 不丢（§8.1 schema 兼容要求）

### 任务 1.3 `kind: compose` 与 docker 段校验

- **文件：** `crates/supertask-core/src/spec/validate.rs`
- **做：**
  - `kind: compose`：`service` 必填且匹配 `^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$`；非法字段拒绝（`env`/`env_file`/`extra_args`/`cwd`/`restart`/`module`/`dir`/`package_manager`/`launch`/`build_args`/`jvm_args` 出现即 `SPEC_INVALID`）；显式 `health.type: tcp` 仍需 `port`（复用现有规则）
  - `docker` 段：`compose_file`/`context`/`dockerfile` 必须相对路径（禁绝对路径、`..` 段）；`project_name` 字符集同 `service`；builds `name` 非空唯一、`tags` 至少一条且匹配 `[A-Za-z0-9.:/_-]+`、禁 `--` 前缀
  - `kind: compose` 不再落入通用 `KIND_UNSUPPORTED` 警告分支（合法 kind，本版可解析不可启动）
- **完成标准：** 规格测试全绿；compose 服务显式字段违规时打开报 `SPEC_INVALID`

### 任务 1.4 本轮测试与文档

- **测试命令：** `cargo test -p supertask-core`
- **覆盖：** docker 段/`service` round-trip（含未知键）；compose 服务合法与各非法字段用例；tag 格式用例；新错误码序列化；1.2 YAML（无 docker 段）回归
- **文档：** 前端 `protocol.ts` 同步 `docker`/`service` 类型（防 TS 结构落后）；更新进度快照与 AGENTS.md；不改功能规格正文
- **完成标准：** 既有测试全绿；无 CLI 调用、无启动行为、docker feature 仍 soon

## Phase 2 — Docker CLI 适配层

规格 §4。新建 `crates/supertask-core/src/docker/`：`runner.rs`（固定程序 + 结构化 argv + 2 MiB 输出上限 + 超时；复用 toolchain runner 模式，fake 可注入）、`probe.rs`（`docker version --format json` → found/compose_version/running）、`compose_config.rs`（`config --format json` 解析 + mtime+hash 缓存）。测试全走 fake 输出 fixture，不真调 docker。**完成标准：** 探测三态（无 docker / 未运行 / 无 compose 插件）映射 `DOCKER_*` 错误码。

## Phase 3 — compose 运行时

规格 §5。launcher 增加 ComposeService 分支：`up -d --no-deps <service>`（**必带 `--no-deps`**）、`stop`、状态轮询（`compose ps --format json`）、`logs --follow` 接现有管道、退出清场（只 stop 引擎启动过的）。启动同步前置检查失败不 accepted。**依赖 1.2 phase 3/5 的部分（PORT_DUP 硬错误、crash 通知）后移。**

## Phase 4 — 镜像构建

规格 §6。`docker.builds` 条目 → `docker build -f … -t … <context>`（argv 顺序固定）；compose 服务 build → `compose build <service>`；operation 可取消；`DOCKER_BUILD_UNKNOWN` / `IMAGE_BUILD_FAILED`。

## Phase 5 — 导入与扫描

规格 §7。候选顺序 compose.yaml > compose.yml > docker-compose.yml > docker-compose.yaml；草稿字段映射（port=ports[0].published、depends_on 键、build 标记进 labels）；Docker 不可用降级警告 `DOCKER_NOT_FOUND`；merge 匹配规则加「kind: compose 且 service 相同」。

## Phase 6 — 前端

规格 §11。`/docker` 页 ComingSoon → live（feature 翻 live 必须在此步后）；服务抽屉「容器」Tab；compose 卡片「容器托管」；构建 operation 状态。

## Phase 7 — Windows 集成验收

规格 §13.3。Docker Desktop 在线全链路（sidecar + depends_on + 首次拉镜像）、三种分态错误码、端口冲突、构建成败与取消、退出清场、1.0–1.2 回归。

## 建议下一轮

**Phase 3 compose 运行时**（phase 2 已交付：`crates/supertask-core/src/docker/` runner/probe/compose_config + 26 个 fake 测试）。

## 文档债

- docs/spec/yaml.md §4 服务 kind 表未列 `kind: compose`（phase 1 落地后补）。
- docs/spec/ipc.md §4.8 的 docker.ps/build 占位表述在 phase 4 转 live 时更新。
