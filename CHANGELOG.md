# Changelog

All notable changes to SuperTask are documented here.

## [Unreleased]

### Features

#### 孤儿进程纳管（方向二·纳管任意来源）

- 发现页新增「纳管进程」：把当前工作区目录下运行中的外部监听进程反推成
  `kind: generic` 服务草稿（program/args 忠实复刻原命令行、`dir` 取 cwd 相对
  工作区根、`port` 取首个监听端口），dry-run 预览确认后写入 `supertask.yaml`，
  从此走统一的启停 / 健康 / 日志链路——README「AI 起的服务没人收场」的叙事
  兑现为「收编进来，由我统一启停」。
- dry-run 预览分四态：拟新增（默认勾选）/ 已被现有服务覆盖（matched，重开工作区
  即被引擎识别为外部实例，无需纳管）/ ID 冲突（候选 id `<id>-2`，默认不勾）/
  不可纳入（cwd 不可读或在工作区外等，附原因）。同预览内端口互撞、父进程同为
  监听进程、多余监听端口均给出警告。
- **apply 前重算 + 幂等**：预览到确认之间退出的进程按警告跳过；纳管后同进程再次
  预览即 matched；写回走 `yaml.saveForm`（base_hash 冲突 → `YAML_CONFLICT`），
  只新增勾选服务、不触碰其他字段；来源保留在 `labels`（`origin: adopted` /
  `adopted-from: "pid N (name)"`）。
- **脱敏**：命令行中形似密钥的参数值（password/token/secret/api_key=…、Bearer）
  在预览与草稿中一律替换为 `<redacted>`，明文不进 IPC 返回值也不落盘；环境变量
  不读取不回显。不杀任何进程，无确认不写盘。
- 新 IPC `workspace.adoptPreview` / `workspace.adoptApply`（契约 ipc.md §10.16）；
  `system.discover` 的 `ForeignService` 新增 additive 字段 `parent_pid`（三平台，
  读不到为 null）。发现页行内对「cwd 在工作区内且未被认领」的进程提供一键纳管入口。
- 测试：core `adopt::` 22 项离线单测（草稿推导、Windows 命令行切词、路径大小写
  不敏感相对化、脱敏无明文、冲突/幂等/进程退出跳过、yaml 往返校验、确定性）。

#### restart 策略与自动重启（方向一·服务监管）

- `supertask.yaml` 服务级 `restart` 字段自 reserved 转正（2.2）：`never`（默认）/ `on-failure`
  （意外退出且退出码 ≠ 0 时自动重启）/ `always`（意外退出即重启，含 0 正常退出）；
  新增 `max_retries`（1..=100，缺省 5）。取值与组合非法即 `SPEC_INVALID`；
  compose 服务不允许该字段（重启由 compose 文件自管）；不设 `unless-stopped`
  （引擎生命周期即应用会话，会话内与 `always` 行为一致）。
- 引擎自动重启监管（`supertask-core/src/engine.rs`）：`spawn_core` 成功后捕获最小启动计划，
  进程意外退出后由监管线程按 1s 起指数退避（16s 封顶）原样重放——不重规划命令、不复检工具，
  手动 `start` 仍走完整链路（重置计数）。预算耗尽后停在 Exited 并写 `last_error`
  （自动重启 N 次后放弃）；手动停止取消待执行的重启；构建期退出不自动重建；
  应用重启后不自动恢复监管（需手动启动）。spec 校验、运行时状态机、
  重启循环与手动取消均有聚焦测试（引擎 2 项、spec 4 项）。
- 运行页：自动重启退避/进行中的服务卡片显示「⟳ 自动重启 · 第 N 次」徽标；
  `runtime.snapshot` 新增 additive 字段 `restart_attempt`（1 起，手动启动后消失），
  `docs/spec/ipc.md` §6 同步；`docs/spec/yaml.md` §4.1 字段表更新。

#### 系统监控页

- 侧边栏「环境」组新增「系统监控」页（`/monitor`）：整机资源的实时面板，
  五张卡片对齐经典监控面板样式——CPU 负载（绿→黄→红渐变半圆仪表盘 + 指针 + 大号百分比 +
  系统/用户/Nice/闲置四格占比）、CPU 使用率历史（1 Hz 滚动面积图，仅页面打开期间累积，约 2 分钟窗口）、
  内存（进度条 + 已用/总量 + 压力/可用/交换空间）、存储（进度条 + 已用/总量）、
  网络（本机 IPv4 + 上传/下载速率）。深浅色与六色板自动适配，取不到的字段显示「—」。
- `system.metrics` 主机采样扩展（`supertask-core/src/host_metrics.rs`）：
  新增 CPU 四分占比（Windows 走 `GetSystemTimes` 拆分，Nice 恒 0；Linux 解析 `/proc/stat`）、
  内存可用量与交换空间（Windows 为提交内存口径，Linux 读 `/proc/meminfo` SwapTotal/SwapFree）、
  网络上传下载速率（字节/秒，差分按经过时间归一：Windows `GetIfTable2` 排除 loopback，
  Linux `/proc/net/dev`）与本机 IPv4（优先默认路由网卡；Windows `GetAdaptersAddresses`，
  Linux `getifaddrs`）。全部为可空字段，向后兼容；首次采样占比与速率为 null（无差分基线）。
- 状态栏：详情弹窗新增「打开系统监控」跳转入口；温度档位偏好抽为共享响应式模块
  （`frontend/src/lib/temp-mode.ts`），状态栏与监控页共用同一档位，避免页面轮询把
  Windows 高频档常驻采样器反复拉起/杀掉；`fmtBytes`/`pct`/`loadColor` 等格式化工具
  提取至 `frontend/src/lib/metrics.ts` 供两处共用（无行为变化）。
- Mock（浏览器预览）与四语言词条（zh-CN / en-US / ja-JP / zh-TW）同步补齐。

### Docs

- 文档目录重组：历史规划与设计材料归档至 `docs/archive/`（`plans/` 35 份版本规格与实施计划、
  `research/` 选型调研、`adr/` 早期架构决策、`verification/` 历史验收记录）；删除未实际应用的
  UI 原型稿 `docs/prototypes/`（HTML mockup 与截图，设计已由 `frontend/` 实现取代，git 历史可查）。
  `docs/spec/` 保持为当前功能唯一真源，全部交叉链接已同步修正。
- README 重写：删除版本编号路线图（主题版本 1.x/2.x 与发布版本 v0.x 两套序列），
  改为「未来考虑」方向性内容；新增平台支持状态表（Windows 安装包可用、自动更新已验证，
  macOS / Linux 不推荐）与「获取与更新」章节；如实修正 CI 覆盖范围（当前仅 Windows）；
  构建与开发章节移除「安装包尚未发布」表述。
- `docs/ROADMAP.md` 重写为方向型路线图：按九个能力方向组织，移除版本号与排期，
  每项标注借鉴来源、现状与「价值 · 成本 · 契合度」评级；新增「主机与服务可观测性」方向
  （系统信息面板、指标历史趋势、网络速率、按服务资源归因、MCP 暴露主机指标）与
  ServBay 付费墙对照的机会清单。
- 版本编号路线图与 F/G 编号逐项规划迁出仓库，本地存档于 `.workbuddy/local/VERSIONS-AND-PLAN.md`
  （`.gitignore` 已排除），含主题版本历史、发布版本记录、发版操作清单、
  F1–F28 / G1–G2 / N1–N13 / M1–M6 规划与平台推进专项。
- 新增 `AGENTS.md`：AI 编码代理工作指南（仓库布局、文档地图、构建测试命令、硬性约定、git 约定）。

## [0.1.3] - 2026-09-04

> 本版本包含自 v0.1.1 以来的全部累积变更（PR #14 ~ #25）。

### Features

#### AI 助手

- AI 配置重构为弹框向导：分「基本信息 / 连接与认证（或本地 CLI）/ 模型 / 高级设置」四段布局，高级项（超时、重试、max tokens、上下文窗口、代理）默认折叠并在标题行摘要当前值；弹框内容超高时仅内容区滚动，保存中点击遮罩不会误关；配置列表改为品牌图标 + 名称 + 供应商 · 模型的两行布局。
- 新增 7 个本地编码 CLI 供应商：Claude Code、Codex、OpenCode、Cursor、CodeBuddy Code、Qoder、Pi Coding Agent。凭据由各 CLI 自行管理，无需填写 Key 与 base_url；可执行文件留空走 PATH；「探测」按钮执行 `--version` 直接回显已找到 + 版本或未找到 + 原因。
- Windows 上 CLI 代理启动前按 PATH + PATHEXT 解析真实可执行文件，修复 npm 安装的 `.cmd` shim（如 `cursor-agent.cmd`）报 "program not found" 无法启动的问题；解析失败保持原名由系统报原生错误。cursor-cli 预设程序名由 `agent` 修正为 `cursor-agent`。
- 替换不可靠的原生模型下拉为自定义实现；AI 配置对话框补齐「清除 Key」按钮的四语言文案；繁体中文 AI 相关术语统一（连线与认证、本地 CLI、探测等）。

#### 发现页

- 粘性工具栏 + 汇总统计徽标 + 浮动详情弹框；工作区匹配 / 端口冲突行高亮；类型筛选、展开状态、排序偏好本地持久化；端口筛选防抖；详情或向导打开时自动暂停 30s 刷新并显示「已暂停」提示。
- 布局重构：顶部只保留右上角一个「从 README 导入」入口（说明文案并入按钮悬浮提示），统计徽标与筛选行明确分行；「其他监听进程」并入主表格成为表内可折叠分组行，与吸顶表头共用一套列，彻底解决此前两表列错位、无表头的问题。
- 表格改为固定列宽布局，任何窗口宽度下都不会被长内容撑出容器产生横向滚动；进程名、工作目录、工作区匹配等长内容截断显示省略号并悬浮显示完整内容；PID / CPU / 内存数值列不再折行；端口列最多展示前 2 个胶囊、其余合并为悬浮可查看全部端口的「+N」。
- 排序体验：当前排序列在表头以 ↓ 标记，排序按钮激活时高亮；CPU 降序在首个采样周期（CPU 尚无读数）自动按内存降序兜底，保证点击后行序有可见变化。

#### 工作区

- 后端：最近列表元数据（`recentOpenedAt`、`recent_entries` 含路径 / 显示名 / 打开时间、`last_workspace`）；`workspace.forget` 真正落盘并处理 lastWorkspace 回退；打开 / 初始化成功后记录打开历史；`WorkspaceOpenOut` 新增结构化警告 `warning_items[{code,message}]`；打开资源管理器 / IDE 失败时返回明确错误。
- 前端：工作区页面与切换器浮层视觉打磨，最近列表信息更丰富、忘记操作修复、扫描警告以附加式提示呈现。

#### 模板页

- 改为常驻画廊布局：粘性筛选栏支持全部 / 官方 / 本地来源、技术栈芯片与搜索，筛选偏好本地持久化；预览与创建 / 组合向导改为浮动弹框，创建前二次确认；选中模板有底部粘性操作条；完善空态、加载骨架与错误重试。

#### 环境页

- 工具链探测 UI 升级：总览显示健康度（found/total）、mise / winget 状态、上次探测时间与强制刷新；工具卡片展示版本 / 路径 / 来源徽标并支持一键固定（pin）；安装 / 升级弹框 + 可搜索版本组合框与安装历史；网络设置折叠（本地持久化）、包管理器选择记忆；骨架屏与探测失败空态。

#### 状态栏

- 状态栏新增主机实时指标：CPU、内存、温度等读数一览；CPU 温度采样可在设置中开关；指标采样不引入新依赖。

#### 主题

- 在浅色 / 深色之外新增多套可配置的配色主题，Run 工作台界面令牌与状态清晰度一并打磨。

#### 云端（实验性）

- 客户端：登录态重构为账户概览 + 四格指标（跟踪实体 / 冲突 / 配额 / 上次同步）；同步具备运行时状态（idle / syncing、上次尝试 / 成功 / 错误、推拉结果）；迁移向导覆盖真实缺口（远端实体清单、拒绝空目录、`include_templates` / `include_settings` 真正生效）；同步与迁移共用操作锁，进行中的第二次请求会被拒绝。
- 参考服务端（向后兼容的加法变更）：`/healthz` 探测数据库并返回状态与版本；实体列表附顶层 `name` 并保留完整 `data`；409 冲突响应附带当前实体信封；`updated_by` 回退到 `x-device-id`；配额增加按类型分组计数；遥测提供策略查询端点与批量上报的受理结果。

### Fixes

- 云端同步的 OperationGuard 生命周期显式化，避免守卫被提前释放。
- 发现页 README 向导不再挤占页面布局（改为浮动弹窗）；修复筛选条件刷新后丢失的问题。

## [0.1.1] - 2026-09-03

### Features


- New eclipse-orbit app icon with matching browser favicon and unified
  run-operation icons.
- In-app auto-update now checks a cnb.cool mirror first (faster in
  mainland China) with GitHub Releases as fallback.

### Fixes

- Port placeholder detection now matches on port + working directory +
  program kind; foreign-owned placeholders prompt to change the port and
  block startup instead of being killed.
- Unified menu / tab / button icons and fixed mixed CJK-Latin text
  alignment in group titles.
- Hardened git tests (canonical temp roots, deterministic pull-conflict
  setup) and compiled the gateway probe on unix targets.

### Internal

- CI runs `cargo fmt --check`; release artifacts are mirrored to cnb.cool
  automatically.
- Dependency upgrades: windows 0.62.2 and consolidated minor bumps.

## [0.1.0] - 2026-09-02

Initial open-source release candidate.

- Desktop workbench for Spring Boot, Node, Python, Go, generic processes,
  Docker Compose, and gateway workflows.
- CLI and MCP integration.
- Aggregated logs, PTY terminal, health checks, workspace packages, README
  import, AI assistance, and optional cloud synchronization.
- Experimental self-hosted cloud reference server and admin console.

Known limitations are documented in the repository inventory and cloud server
specification.
