# v2.2 功能规格：生态（自定义 kind 插件 / WSL2 / 可复现导出）

> 2026-08-29。状态：**规划稿（待评审拍板；拍板后更新本行与 AGENTS.md 当前阶段）**。
> 实现计划：[2026-08-29-v2-2-implementation-plan.md](2026-08-29-v2-2-implementation-plan.md)。
> 一句话：**「更多语言」不写死进核心——自定义 kind 是数据化插件（manifest 描述 argv / 字段 / 探测 / 扫描，零代码执行），WSL2 运行时进实验区，再送 mise.toml / devcontainer.json 导出。**

---

## 1. 背景与版本序列

- Python / Go 已提前至 v1.7 落地（AGENTS.md 已拍板 14）；「更多语言」（deno / bun / .NET / Rust…）由 1.7 的 generic kind 人工兜底。本版本把「每个人手写一遍 generic」变成**可安装、可识别、可分享的插件**（roadmap 缺口表「插件 / 自定义 kind：避免核心膨胀」）。
- WSL2（roadmap「Windows 上跑 Linux 工具链」）与可复现导出（roadmap 2.2 备注「需要可复现时导出 devcontainer / mise.toml」）同版收口。**仍不自研 Nix。**
- 依赖：v1.7 的 generic kind 与 launcher 结构（argv 渲染先例）；kinds 云同步依赖 v2.0（实体模型与前向兼容规则）。

## 2. 目标与非目标

**目标**：

1. 自定义 kind 插件：manifest 注册表，端到端（校验 / 启动 / 探测 / 安装 / 扫描 / UI 表单），零代码执行；
2. kinds 云同步实体（复用 v2.0 协议，additive）；
3. WSL2 运行时（`runtime: wsl`，实验开关，默认关）；
4. 可复现导出：mise.toml / devcontainer.json 纯函数渲染 + CLI / GUI 入口。

**非目标**：

- WASM / 动态库 / 任意代码插件（**manifest 是数据不是代码**——这是本设计的核心边界）；
- 插件市场 / 插件签名（安装 = 本地文件夹或云同步；分发信任沿 v2.0 账号体系）；
- WSL 内工具链自动安装（探测 + 指引，不代装——不代装原则不变）；
- WSL 内服务指标（显示「—」，诚实降级）；
- mise.toml / devcontainer **导入**（只出不进）；
- macOS / Linux 上的 WSL（Windows only）；
- 核心新增语言 kind（新语言一律走插件或 generic；核心 kind 表冻结在 1.7 形态）。

## 3. 自定义 kind 插件

### 3.1 manifest（`%APPDATA%/SuperTask/kinds/<id>/kind.yaml`）

```yaml
id: deno                 # 必填；^[a-z][a-z0-9-]*$；与内置 kind 撞名 → KIND_ID_CONFLICT，该插件禁用 + warning（内置永远赢）
version: 1               # manifest schema 版本；未知 version → KIND_PLUGIN_INVALID
label: Deno              # 展示名（i18n 不覆盖插件标签，原样展示）
runtime:
  argv: [deno, run, --allow-net, "{{ entry }}"]   # 必填；占位符只允许 fields 声明的字段；{{ extra_args }} 若出现必须是最末元素（未出现则自动追加）
  port_env: PORT         # 可选；有 port 时注入该 env（对齐 node 规则；不配则不注入，同 generic）
  grace: 15              # 默认 15
  health: tcp            # none | tcp | http；有 port 默认 tcp
fields:                  # 字段类型系统：path | dir | args | string
  entry: {type: path, required: true}     # path = 工作区相对路径 + 沙箱校验（复用 sandbox）
  dir:   {type: dir, required: false, default: "."}
scan:                    # 可选；缺省则该 kind 不参与扫描识别
  feature: deno.json     # 目录含此文件 → 生成该 kind 草稿（特征优先级表中排在 go.mod 之后、compose 独立识别不变）
  entry_guess: [main.ts, mod.ts, server.ts]
  port: 4800             # 端口建议起点（走 ports.rs::suggest 顺延）
toolchain:               # 可选
  name: deno             # /env 页展示名
  probe: [deno, --version]
  install: {mise: deno, winget: DenoLand.Deno}
```

- **未知 manifest 键拒绝**（严格解析，`KIND_PLUGIN_INVALID`）；坏 manifest → 该插件禁用 + 打开工作区 warning，**绝不崩 app.load**（沿「坏工具不阻塞」的 probe 先例）。

### 3.2 注册与接线（inv-2 §2.2 七处 match 散点）

| 散点 | 接线方式 |
|------|----------|
| `runnable_kind`（spec/file.rs:506） | 内置表优先，未命中查 KindRegistry |
| `spec/validate.rs` per-kind 分支 | 内置走现状；插件 kind 按 manifest fields schema 校验（required / type / default），非法 `SPEC_INVALID` 带字段名 |
| launcher 计划构建 | argv 模板渲染：字面量 + 声明字段值；**值作为单元素插入不拆词**（extra_args 数组拼接）；path 类字段过沙箱校验；program PATH 未命中 → 既有 `MISSING_TOOL` |
| 端口 env 注入 | runtime.port_env 存在且最终 env 无该键且有 port → 注入（显式 env 永远赢，沿 1.7 规则） |
| `spec/validate.rs:176` 工具链关联 | 插件 kind 关联 manifest.toolchain |
| scan 识别 | manifest.scan.feature 进特征优先级表（pom > gradle > package.json > pyproject/requirements > go.mod > **插件特征（按 id 字母序）**）；entry_guess / port 生成草稿 |
| ToolchainProbe | 增 `custom: Vec<ToolProbe>`（`#[serde(default)]`，带 name；沿 1.6 gateway 加字段先例）；插件探测结果进 custom 行 |

- **ServiceSpec 零改动**：插件字段值天然进 `#[serde(flatten)] extra`（inv-2 §2.1 事实），round-trip 由既有机制保证。
- **KindRegistry**（core 新模块 `kinds/`）：app 启动 + 打开工作区时装载；`Arc<KindRegistry>` 注入 engine（沿 GatewaySlot 托管模式）。
- 安装链小重构：`toolchain/install.rs` 抽出 `InstallRequest {mise_tool, winget_id, version}` 与 `ToolKind` enum 解耦——插件路径直接由 manifest.install 构造请求，复用 mise→winget→重解析全链。

### 3.3 安全模型

- manifest 是**纯数据**：无代码执行、无 env 展开、无 shell 拼接；argv 只由字面量与声明字段值组成。
- path 类字段必须工作区相对且过沙箱（复用既有沙箱错误码）；无任何绕过。
- 本地安装与云同步安装走**同一校验管线**；云同步只落盘 manifest，不触发任何执行。

### 3.4 与 generic kind 的关系

- generic（1.7）：工作区内手写 program/args，无探测 / 无扫描 / 无分享——「一次性」。
- 插件 kind：可安装、可被扫描识别、带工具链卡、可经云分享——「固化路径」。
- 两者并存；对 deno/bun/.NET 等生态，官方或社区出插件，用户装完即得 1.7 级体验。

### 3.5 分发

- 本地：`kinds.install`（选目录拷入 appdata kinds 目录）；`kinds.remove`（卸载 = 删目录；正在运行的服务不受影响，仅影响下次校验/启动）。
- 云（依赖 v2.0）：实体 type=`kind`（内容 = 插件目录 zip）；v2.0 前向兼容规则（未知 type skip）使旧客户端无感。

## 4. WSL2 运行时（实验）

### 4.1 字段（yaml.md 增量）

```yaml
services:
  api:
    kind: node
    runtime: wsl              # native（默认，缺省省略）| wsl
    wsl_distro: Ubuntu-22.04  # 可选；缺省用 WSL 默认 distro
```

- 非 Windows 平台：打开工作区 warning、启动硬错误 `WSL_RUNTIME_UNSUPPORTED`（沿 ENTRY_NOT_FOUND 双口径）。
- 实验开关关闭时：config 页不显示 runtime 字段；validate 对 wsl 给 warning（启动仍按上条硬错误口径）。

### 4.2 探测（`wsl/mod.rs`）

- `wsl.exe --status`：exit ≠ 0 或找不到 → `WSL_NOT_FOUND`。
- `wsl.exe -l -q` 列 distro：**输出为 UTF-16LE，用 encoding_rs 解码**（同 GBK 教训的编码坑；新版本 wsl 可能切 UTF-8，解码做双探测 + fixture 覆盖）。
- 指定 distro 不在列表 → `WSL_DISTRO_MISSING`。
- 探测结果进 `ToolchainProbe` 新字段 `wsl`（三态：ok + distro 列表 / not found / engine unreachable，沿 docker probe 三态先例）。

### 4.3 启动与停止

- 启动：`wsl.exe -d <distro> --cd <unix(dir)> -e <argv…>`；路径翻译工具（`C:\a\b` ↔ `/mnt/c/a/b`）；program 在 WSL 内解析——启动前预检 `wsl.exe -d <distro> -e sh -c 'command -v <program>'`，未命中按 `MISSING_TOOL` 口径（提示在 WSL 内安装）。
- 停止：Job Object 杀 Windows 侧 wsl.exe 进程树；WSL 会话回收通常连带终止 Linux 侧进程——**残余风险文档化**（double-fork 守护进程可能逃逸）：运行页提供 `wsl --terminate <distro>` 逃生口按钮（destructive + 确认，警示「将终止该 distro 内全部进程」）。
- Windows 侧 venv/解释器解析（python kind 的 `.venv` 顺序）**不适用**于 wsl runtime——文档标注：WSL 内解释器 = PATH 直接解析。

### 4.4 健康 / 日志 / 指标

- 健康：WSL2 localhost 转发默认开启，tcp/http 检查**零改动**（`.wslconfig` 关闭 localhostForwarding 的排障写进 yaml.md / cloud.md 之外的用户文档）。
- 日志：stdout/stderr 经 wsl.exe 管道，**零改动**（编码注意：WSL 内输出通常 UTF-8，现有解码链兼容）。
- 指标：不可用——UI 显示「—」（不猜测、不假数据）。

## 5. 可复现导出

- 纯函数渲染（对齐 1.6 gateway render 先例 + golden 测试）：
  - **mise.toml**：`ToolchainSpec` 钉扎 → `[tools]` 表（java/node/python/go…有则出）；含注释头「由 SuperTask 导出」。
  - **devcontainer.json**：name（工作区名）、features 映射（java/node/python/go → `ghcr.io/devcontainers/features/*`，映射表数据化）、forwardPorts（服务端口列表）。
- 入口：config 页「导出配置」（outline + 下拉：mise.toml / devcontainer.json → save 对话框）；CLI `supertask export --format mise|devcontainer [-o <path>]`（**默认 zip 行为不变**，向后兼容）。
- 新依赖：`toml` crate（mise 渲染；devcontainer 走既有 serde_json）。

## 6. IPC 契约增量（ipc.md 增 §10.14）

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| kinds.list | — | 已装插件清单（id/label/来源/启用态/校验错误） |
| kinds.install | `{dir}` | 拷入 + 校验；冲突/非法返回对应错误码 |
| kinds.remove | `{id}` | 删目录；运行中服务不受影响 |
| wsl.status | — | 探测三态 + distro 列表 |
| export.render | `{format}` | 渲染文本（前端经 save 对话框落盘） |

## 7. 错误码汇总

| 码 | 场景 |
|----|------|
| `KIND_ID_CONFLICT` | 插件 id 与内置（或另一插件）撞名 → 后者禁用 + warning |
| `KIND_PLUGIN_INVALID` | manifest 非法（缺 argv / 未知占位符 / 未知键 / 未知 version 等） |
| `WSL_NOT_FOUND` | wsl.exe 不可用 / exit ≠ 0 |
| `WSL_DISTRO_MISSING` | 指定 distro 不存在 |
| `WSL_RUNTIME_UNSUPPORTED` | 非 Windows 平台启动 runtime: wsl 服务（硬错误）；打开仅 warning |
| `EXPORT_FORMAT_UNKNOWN` | export --format 未知值 |

## 8. 前端

- **/env**：`probe.custom` 动态工具卡（探测 + 一键安装走 manifest 映射；对齐 java/node 卡样式）。
- **/config**：kind 下拉含插件 kind（label + 插件徽标）；字段表单 **schema 驱动渲染**（path/dir/args/string 四型 → 对应输入组件；required/default 联动）；runtime/wsl_distro 字段（实验开关后可见）。
- **/config**：导出配置菜单（mise/devcontainer）。
- **/settings**：实验区（WSL 运行时开关，默认关 + 风险说明）。
- 运行页：kind 图标/文案映射表可被插件扩展（label 回退 kind id）；WSL 逃生口按钮（destructive）。
- 四语 + parity；命令面板：安装插件/导出配置动作。

## 9. Phase 划分（概览）

1 注册表 + manifest 校验 → 2 七散点接线 → 3 WSL 运行时 → 4 导出器 → 5 壳层 IPC + 前端 → 6 kinds 云同步 → 7 文档闭环 + 回归 → 8 验收。（3、4 与 1→2 主干并行；6 依赖 v2.0。）任务级拆解见实现计划。

## 10. 验收标准（场景矩阵）

1. deno 插件（fixture）：本地安装 → config 页 kind 下拉出现 deno → 表单按 schema 渲染 → yaml 写盘 round-trip 无损（未知字段不丢的既有断言覆盖插件字段）。
2. deno 服务启动：argv 模板渲染正确（占位符值单元素插入）；port_env 注入且显式 env 优先；停止无残留进程树。
3. 非法 manifest 三例（缺 argv / 未知占位符 / id 撞内置）：`KIND_PLUGIN_INVALID` / `KIND_ID_CONFLICT`；插件禁用，app 与工作区照常打开。
4. 扫描：含 `deno.json` 的目录 → deno 草稿（entry_guess + 端口建议）；特征优先级（pom > … > go.mod > 插件）矩阵单测通过。
5. /env：插件工具卡探测 missing → FakeRunner 安装（mise/winget 映射来自 manifest）→ 重解析 found。
6. 云同步 kinds（依赖 v2.0）：第二会话拉取后插件生效；**旧客户端（不识别 kind 实体）忽略未知 type 不报错**。
7. WSL 探测：`wsl -l -q` 的 UTF-16 fixture 正确解码出 distro 列表；无 WSL → `WSL_NOT_FOUND`；distro 不存在 → `WSL_DISTRO_MISSING`。
8. `runtime: wsl` 服务（真机）：`--cd` 路径翻译正确；启动 → localhost 健康检查转 running；日志分服务可见。
9. WSL 停止（真机）：Job Object 杀 wsl.exe 后 Linux 侧进程退出无残留；若有残留，逃生口按钮可用且带确认。
10. 非 Windows（CI unix）：runtime: wsl 服务打开 warning、启动 `WSL_RUNTIME_UNSUPPORTED`（单测口径）。
11. 导出：`export --format mise|devcontainer` golden 比对；config 页导出走 save 对话框；**默认 zip 行为回归不变**。
12. 实验开关：默认关时 config 不显示 runtime 字段、validate warning；开启后全链路可用。

## 11. 风险与开放问题

- **manifest 表达力边界**：复杂 kind（需要构建步骤 / 特殊停止逻辑）表达不了——这是**有意为之**的边界：核心不膨胀，复杂场景进核心版本或用 scripts 兜底；manifest `version` 字段留演进空间。
- **WSL 停止残余**（守护进程逃逸）：实验标注 + 逃生口 + 真机矩阵覆盖；不做更深的进程组追踪（成本/收益不匹配）。
- **wsl.exe 输出编码随版本漂移**（UTF-16 → UTF-8）：双探测解码 + fixture；真机覆盖当前稳定版。
- **devcontainer features 上游 id 漂移**：映射表数据化集中在 manifest 侧常量，实现期核对当前 tag。
- 开放问题（实现期定，不阻塞）：插件是否允许覆盖内置 kind 的 label 展示（倾向否）；kinds 云同步的配额计入方式（倾向按实体计数不特殊化）。
