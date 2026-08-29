# v1.7 功能规格：横向扩展（运行时 / 易用性 / 信息架构）

> 2026-08-29。状态：**规格已拍板**（依据 [docs/inventory/](../inventory/2026-08-29-v1-7-inventory.md) 盘点与复核）。
> 实现计划：[2026-08-29-v1-7-implementation-plan.md](2026-08-29-v1-7-implementation-plan.md)。
> 一句话：**把工作台从「Spring + Node」横向扩到 Python / Go / 任意进程，顺手关掉 1.2 三个功能欠账，把工作区包入口归位。**

---

## 1. 背景

1.x 主线（1.0–1.6：工作区 / 启停 / 日志 / 配置 / 环境 / 容器 / 网关 / CLI / MCP / 导出包）已落地。本轮按用户方向做横向扩展：① 支持更多后台运行时（python、go、其他）；② 易用性提升；③ 功能入口调整（工作区包入口归位工作区模块）。

## 2. 盘点评估结论与拍板

### 2.1 盘点合理性评估

**成立的结论**（已逐条对码复核）：

- 地基友好：`ServiceSpec.kind` 是 String + flatten extra；健康检查/进程/指标/日志/网关/依赖图与 kind 解耦；compose（1.3）与 gateway probe（1.6）是可复制的完整先例。
- 模板是纯数据，新 kind 落地后加模板零 Rust 代码。
- 入口调整低风险：功能本体（`pkg.rs` + CLI）与入口解耦，迁移是页面卡片级；导航数据驱动（双注册表），重排不触壳层逻辑。
- 欠账集中在 1.2 的 A1–A3，均有半成品基础。

**两处修正**（本轮核查发现，已回改盘点文档）：

1. inv-2 §2.2 的 match 散点实际是 **7 处不是 6 处**：漏了 `spec/file.rs:506` `runnable_kind()`（kind 可启动性的唯一闸门，launcher.rs:182 调用）。
2. inv-4 A1 的半成品程度**比 v1-2-progress 记载更浅**：`EffectiveNetwork` 实际只有代理字段（`network.rs:20-25`），maven/npm 镜像字段从未进入它（只在 YAML 层 `NetworkSpec`，`spec/file.rs:320-322`）；`tool_env` 全仓唯一调用方是壳层工具链**安装**链路（`src-tauri/src/commands.rs:530`，给 mise/winget 传代理）。**已启动服务既无代理也无镜像注入**。A1 实为「spec + /env UI 有，运行时全未接线」，成本略高于盘点估计，但方案不变。

**方向判断**：三个方向均合理，组合为一个版本、按支柱分 phase（每 phase 可独立落地）；验收债（B 类）与发布工程（C 类）不混入本轮，单开验收专项。

### 2.2 拍板表（对应 inv-5 六问）

| # | 问题 | 决策 |
|---|------|------|
| 1 | 版本与范围 | **v1.7 = 三支柱组合**：运行时扩展（python/go/generic）+ 易用性（A1/A2/A3）+ 入口调整 |
| 2 | 路线图重排 | Python/Go 从 2.2 **提前至 1.7**；2.2 保留插件/WSL2，「更多语言」由 generic kind 兜底 |
| 3 | 实现形态 | **专用 kind（python/go）+ generic 同版落地**：generic 基建在专用 kind 之后近乎免费，且直接回应「等其他后台」（deno/bun/.NET/Rust…） |
| 4 | 工具链安装 | 探测 + 安装都做：复用 mise/winget 链（`python@3.12` / winget `Python.Python.3.12`、`GoLang.Go`），不代装原则不变（安装是 /env 页显式按钮） |
| 5 | 入口范围 | **导出 + 导入都进 `/workspaces`**；welcome 首启导入保留（onboarding 路径不动）；settings 导出卡**移除**不留副本 |
| 6 | 欠账吸收 | **A1 全量关闭**（含 pip/goproxy 扩展）、A2、A3 进本轮；A4 Playwright 起步（骨架用例）；B/C 单开验收专项，不阻塞开发 |

## 3. 目标与非目标

**目标**：① python/go/generic 三 kind 端到端可启动（YAML → 校验 → 启动 → 健康 → 日志 → 指标 → 网关路由 → CLI/MCP）；② 工具链探测 + 一键安装；③ 扫描识别 Python/Go 工程生成草稿；④ 镜像/代理运行时注入（A1，扩展 pip index 与 GOPROXY）；⑤ 服务分组 UI（A3）；⑥ 崩溃通知（A2，零 core 改动）；⑦ 工作区包入口归位 + 导航五组重排；⑧ 内置模板 +2。

**非目标**：插件/自定义 kind（仍 2.2）；WSL2（2.2）；Python 包管理器安装/依赖安装代跑（uv/poetry install 由用户 scripts 完成，工作台只管长进程）；`go build` 产物运行（跑 jar 的 Go 等价物后排，`go run` 已覆盖开发场景）；云/AI/账号（2.0/2.1）；UI 拼 cmdline（generic 只能 YAML 手写）；Linux/macOS 新适配（沿用 1.4 现状，`proc/unix.rs` 天然覆盖三新 kind）。

## 4. 运行时扩展：三个新 kind

### 4.1 字段定义（yaml.md §4 增量）

**kind: python**

```yaml
services:
  api:
    kind: python
    dir: backend          # 必填，相对 root，沙箱校验（同 node.dir）
    entry: main.py        # 与 module 恰一必填：脚本模式，相对 dir 的文件
    # module: uvicorn     # 模块模式：python -m <module>；复用既有 module 字段（per-kind 语义）
    port: 8000
    extra_args: []        # 追加在 entry/module 之后
```

**kind: go**

```yaml
services:
  api:
    kind: go
    dir: backend          # 可选，默认 "."（Go 工程根常即工作区根；与 node.dir 必填的差异备案）
    package: ./cmd/server # 可选，默认 "."；go run 的包路径，相对 dir
    port: 8080
    extra_args: []        # 追加在 package 之后 = 传给被运行程序；go build flags 不支持（需要时用 scripts）
```

**kind: generic**

```yaml
services:
  worker:
    kind: generic
    program: deno         # 必填；PATH 名（PATHEXT 解析）或工作区内相对路径（含路径分隔符时，沙箱校验）
    args: [run, --allow-net, main.ts]   # 可选
    dir: .                # 可选，默认 "."（工作目录）
    port: 4800
    extra_args: []        # 追加在 args 之后
```

新增 typed 字段（`ServiceSpec`）：`entry`、`package`、`program`、`args`（均 Option/Vec，缺省省略，round-trip）。

### 4.2 启动命令与解释器解析

| kind | argv | 工作目录 |
|------|------|----------|
| python | `<python> [entry]` 或 `<python> -m <module>` + extra_args | `dir` |
| go | `go run <package>` + extra_args | `dir` |
| generic | `<program> [args]` + extra_args | `dir`（缺省 "."） |

**python 解释器解析顺序**：`dir/.venv` → `dir/venv` → `root/.venv` → `root/venv` → PATH（`resolve_program("python")`）。Windows venv 用 `Scripts/python.exe`，Unix 用 `bin/python`。`.venv` 与 `venv` 并存取 `.venv` 并警告一次。**不探测 uv/poetry 专属行为**——它们装出的 venv 就是普通 venv，天然覆盖。

**go**：`resolve_program("go")`；`go.work` 存在时不特殊处理（go 自行跨模块解析，文档说明即可）。

### 4.3 校验与字段合法性矩阵

- `runnable_kind`（`spec/file.rs:506`）增加三 kind。
- `spec/validate.rs` per-kind 分支扩展，字段合法性（非法即 `SPEC_INVALID`，沿用 compose 的矩阵做法）：

| 字段 | spring-boot | node | compose | python | go | generic |
|------|:---:|:---:|:---:|:---:|:---:|:---:|
| module | ✅必填 | ❌ | ❌ | 与 entry 恰一 | ❌ | ❌ |
| dir | ❌ | ✅必填 | ❌ | ✅必填 | 可选 "." | 可选 "." |
| entry / package / program+args / service / package_manager / script / build_tool / launch / jvm_args | 按 spring/node/compose 现状 | | service 必填 | entry 见上 | package 见上 | program 必填、args 可选 |
| extra_args | ✅ | ✅ | ❌ | ✅ | ✅ | ✅ |

- 启动期检查：python `entry` 文件不存在 → `ENTRY_NOT_FOUND`（打开工作区 warning，启动硬错误）；go `package` 目录不存在 → `PACKAGE_NOT_FOUND`（同口径）。generic 的 program 是 PATH 名未命中 → 既有 `MISSING_TOOL`；相对路径越界 → 既有沙箱错误码。

### 4.4 端口 env 注入与默认值

- **PORT 注入**：kind ∈ {node, python, go} 且最终 env 无 `PORT` 且有 `port` → 注入 `PORT={port}`（服务显式 env 永远优先；spring 的 `SERVER_PORT` 规则不变；**generic 不注入**——无生态约定，避免惊讶）。
- 默认 `grace_secs`：python **15**、go **60**（冷编译宽限）、generic **15**。
- 默认 `health.type`：三者有 port 即 **tcp**，无 port 允许 `none`（沿 1.0 规则）。

### 4.5 与既有子系统的关系（零改动确认）

健康检查 / 进程树停止（Job Object）/ 指标 / 日志批次 / 依赖图 / 网关路由 target / CLI `up|down|status|logs` / MCP 七工具：全部按服务 id 与 PID 工作，三新 kind **自动纳入，零改动**。`doctor` 因 `ToolchainProbe` 扩展（§5）自动覆盖 python/go。`system.discover` 本就枚举本机 python 进程（ipc.md §4.2），无需改。

## 5. 工具链：探测与安装

- `ToolKind` 增 `Python`、`Go`；`ToolchainProbe` 增 `python`、`go` 字段（`#[serde(default)]`，1.6 gateway 字段同款先例；并行探测 + 4s 超时不变）。**Windows Store 别名**：`python --version` 非 0 退出即 not found（`probe_one` 现语义已覆盖）。
- 安装映射：`mise_tool_name`：`python` / `go`；`winget_id`：`Python.Python.<major.minor>`（如 3.12）/ `GoLang.Go`。版本钉扎口径：python `major.minor`、go `major.minor`。
- `ToolchainSpec` 增 `python: "3.12"` / `go: "1.23"`（1.2 钉扎语义同 java/node）。
- 安装链复用：mise 优先 → winget 兜底 → 装完立即重解析；测试全走 FakeRunner。
- **核查项**（实现期第一步）：`toolchain/resolver.rs` 的 `env_delta`（mise 解析出的 PATH 前置）在启动链的现用点；未接线则按「toolchain 有钉扎时启动注入 env_delta」补齐——这是 A1 原文里 `env_delta` 的最后一截。

## 6. 扫描识别（scan.rs 增量）

- 新特征：目录含 `pyproject.toml` 或 `requirements.txt` → python 草稿；含 `go.mod` → go 草稿。
- 每目录只取一个特征，优先级：`pom.xml` > `build.gradle(.kts)` > `package.json` > `pyproject/requirements` > `go.mod`；compose sidecar 识别独立进行（现状不变）。
- python 草稿：`dir` = 相对路径；入口猜测顺序 `manage.py`（→ entry + extra_args `[runserver]`，Django）> `main.py` > `app.py` > `server.py` > `app/main.py`；全不中则 entry 留空 + warning「未识别入口，请在 YAML 指定 entry 或 module」。端口建议 8000 起走 `ports.rs::suggest`。
- go 草稿：`package` 猜测：`cmd/` 下恰一个含 main 包子目录 → `./cmd/<it>`；多个或没有 → `"."` + warning（多候选时提示用户指定）。端口建议 8080 起（撞网关缺省 8080 由 suggest 顺延）。
- 深度限制与 merge 向导沿用现状。

## 7. 网络镜像与代理运行时接线（A1 关闭 + pip/go 扩展）

### 7.1 现状（修正后）

`EffectiveNetwork` 只有代理字段且启动链无人调用；maven/npm 镜像只在 YAML 与 /env UI。本节一次性接线并扩展。

### 7.2 设计

- `NetworkSpec` 增 typed：`python: {index_url}`、`go: {goproxy}`（结构对齐 `MavenNetworkSpec`）。
- `EffectiveNetwork` 增：`maven_mirror` / `npm_registry` / `pip_index` / `go_goproxy`（workspace 覆盖 app 默认，规则沿 1.2 §7.2）；`AppNetwork`（appdata）同步补四个 app 级默认，缺省 None。
- **注入点**：`launcher` 规划 `CommandSpec` 时统一合并——注入优先级**最低**：`注入 env` → 当前用户环境起步的 workspace env → service env（显式值永远赢）。
- 注入键：
  - npm registry → `npm_config_registry`（npm run 子进程继承）
  - python index → `PIP_INDEX_URL`
  - go → `GOPROXY`
  - maven mirror → 生成 `.supertask/maven-settings.xml`（仅含 `supertask-mirror`，`mirrorOf: *`，url = 配置值；磁盘产物是缓存不是编辑对象，同网关产物口径）+ env `MAVEN_ARGS="-s <绝对路径>"`；用户已显式设置 `MAVEN_ARGS` 时不覆盖、给 warning
  - 代理：现有 `tool_env` 的 `HTTP(S)_PROXY/NO_PROXY` 一并入启动 env；健康检查继续 `strip_proxy_vars`（loopback 不走代理，现状保留）
- CLI/MCP 同享（引擎层注入，一处实现全体生效）。

## 8. 易用性

### 8.1 服务分组（A3）

- yaml.md：`group` 从 reserved 转 **1.7 live**（string；结构体字段早已 typed，`spec/file.rs:79`）。
- 运行页：服务卡按 `group` 聚类（组序 = YAML 首次出现序，未分组最后标「未分组」）；组头 = 名称 + 运行计数 + 折叠（localStorage `st:runGroupCollapsed`）+ 组级「启动组 / 停止组」（逐服务走现有命令；停止组用 destructive + 既有确认模式）。
- 快照/状态机/CLI 不变（group 是纯呈现层）。

### 8.2 崩溃通知（A2，零 core 改动）

- 事实基础：`ServiceRuntime` 已含 `state`（含 `exited`/`unhealthy`）与 `last_exit.code`（ipc.md §6）。
- 前端 AppShell 挂 `st.runtime` 监听：`→exited 且 code≠0` 或 `→unhealthy` → 窗口聚焦时 Toast；失焦（含托盘）时系统通知（Tauri notification 插件）。同一服务 10s 去重；用户主动停止（`stopping→stopped`）不触发。
- 设置页「通用」加开关（默认开）。

## 9. 信息架构调整

### 9.1 工作区包入口归位

- `/workspaces` 新增「工作区包」卡：**导出**（with-secrets 开关 + save 对话框 + ConfirmDialog，自 settings 整卡迁移）+ **导入**（选包 → 选目标目录 → 只落盘 → 提示打开，对齐 welcome 现交互）。
- settings 移除导出卡；welcome 首启导入**保留**。锁状态提示（`WORKSPACE_LOCKED` toast）沿用。

### 9.2 导航重排（纯前端 registry + i18n）

| 组 | 成员 |
|----|------|
| 工作台 | run、logs |
| 工作区 | workspaces、discover、templates、config、git |
| 环境 | env、docker、gateway |
| 扩展 | cloud(soon)、ai(soon) |
| 系统 | settings（pinned 底部，现状） |

- `registry.ts` 的 `NavGroup` 与 `NAV_META` 调整；core `features.rs` **不动**（无新 feature id，无 session.hello 契约变更）；四语补组名与变更页 keys（parity 脚本通过）。
- 命令面板动作指向核对（导出/导入若有面板项，指向新位置）。

## 10. 模板

内置 +2（纯数据，`template_assets/*/template.yaml`）：`python-fastapi`（pyproject + `module: uvicorn` + tcp 健康）、`go-node-fullstack`（go api + node web，含 depends_on）。5 → 7 套。

## 11. IPC 契约增量

- **零新增命令**：三 kind 走既有 `runtime.*`；分组是呈现层；包入口迁移是前端改动。
- 增量 payload：`ToolchainProbe` + python/go（additive，旧前端忽略）；`ServiceRuntime.kind` 为字符串原样透传（前端按 kind 渲染图标/文案）。
- ipc.md 补 §10.11（1.7 增量：probe 扩展、启动注入键表、分组语义、错误码两枚）。

## 12. 错误码汇总

| 码 | 场景 | 口径 |
|----|------|------|
| `ENTRY_NOT_FOUND` | python entry 文件不存在 | 打开 warning，启动硬错误 |
| `PACKAGE_NOT_FOUND` | go package 目录不存在 | 同上 |

其余复用：`SPEC_INVALID`（字段矩阵）、`MISSING_TOOL`（PATH 未命中）、沙箱既有码、`KIND_UNSUPPORTED`（未知 kind 不变）。

## 13. Phase 划分（概览）

Phase 0 文档拍板（本规格/计划/roadmap/AGENTS/盘点回改）→ 1 core 三 kind → 2 工具链 → 3 扫描 → 4 网络接线 → 5 前端运行时+易用性 → 6 信息架构 → 7 模板+文档闭环+基线回归 → 8 v1.7 真机验收矩阵 + Playwright 起步。任务级拆解见实现计划。

## 14. 验收标准（场景矩阵）

1. pyproject 工程打开即出 python 草稿；`.venv` 存在时用 venv 解释器（PATH 无 python 也能起）。
2. `module: uvicorn` + extra_args 的 FastAPI 服务：启动 → tcp 健康转 running → 日志分服务可见 → 停止无残留 python 进程树。
3. go.mod 工程：`go run ./cmd/x` 冷启动 60s 宽限内不误报 unhealthy；PORT 注入生效。
4. generic：`program` 为 PATH 名与工作区相对路径两形态可启动；UI 无拼 cmdline 入口。
5. 三 kind 均可被网关 route 指向（target 解析 port）；`supertask up/down/status/logs` 与 MCP 直接可用。
6. /env 页 python/go 探测缺失 → 一键安装（FakeRunner 单测）后重解析为 found。
7. 配置 npm registry + pip index + goproxy + maven mirror：对应 env 出现在启动 env（服务显式值优先）；maven 走生成的 settings.xml；健康检查不走代理。
8. `group` 字段生效：运行页分组渲染/折叠/组级启停；旧 yaml（无 group）渲染不变。
9. 服务异常退出（code≠0）→ 聚焦 Toast / 失焦系统通知；主动停止不触发；开关可关。
10. `/workspaces` 完成导出与导入闭环（产物与 CLI `export` 一致）；settings 无导出残留；welcome 导入仍在。
11. 导航五组渲染正确、四语齐全、parity 通过；命令面板无死链。
12. 未知 kind 仍 `KIND_UNSUPPORTED` 可打开不可启动；旧 yaml round-trip 无损。

## 15. 风险与开放问题

- **Python 生态分叉**是最大不确定源：v1.7 以「entry XOR module + venv 自动探测」收窄到最小面，uv/poetry 依赖安装明确不做（scripts 兜底）。若真机反馈强烈，1.8 再议 package_manager 式探测。
- **go run 冷编译**在超大工程可能超 60s：grace 可配，验收矩阵用小工程；`go build` 产物模式后排。
- **MAVEN_ARGS 注入**依赖 Maven ≥3.3.1：文档标注；用户显式 MAVEN_ARGS 时只警告不覆盖。
- **winget Python 版本号漂移**（3.12/3.13 清单更替）：manifest 表按 major.mininor 收敛，未收录版本报 `ToolchainVersionInvalid`（现状语义）。
- 开放问题（实现期决定，不阻塞）：分组是否进命令面板；崩溃通知是否进托盘气泡聚合。
