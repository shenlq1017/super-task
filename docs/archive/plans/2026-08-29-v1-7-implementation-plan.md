# v1.7 实现计划（横向扩展：运行时 / 易用性 / 信息架构）

> 2026-08-29。依据：[2026-08-29-v1-7-feature-spec.md](2026-08-29-v1-7-feature-spec.md)（含盘点评估与拍板表）。
> 状态：**Phase 0–7 自动化范围已完成（2026-08-29）；剩 Phase 8 真机验收矩阵 + Playwright 起步。**
> 基线达成：core 348 lib（全目标 374）/ cli 20 全绿；前端 `npm run build` 通过；四语 parity 875 keys（基线 845）。
> 执行约定：先读 `project tooling/executing-plans-0.1.0\SKILL.md`（注：该路径 2026-08-29 实测不存在，按计划直接执行）；前端任务点名 skill；CLI 构建用 `CARGO_TARGET_DIR=target-cli` 防与桌面 dev 产物撞名。

## 实现期偏差与决策备案（2026-08-29）

1. **Phase 2.0 env_delta 核查结论**：`ResolvedTool.env_delta`（mise PATH 前置）此前**全仓无消费方**。接线方式：仅显式 `toolchain.manager: mise` 时，启动前 `mise which` 解析当前 kind 主工具并把 env_delta 并入启动 env（失败静默回退 PATH）——`launcher::apply_pinned_mise_env`，engine 在 real spawner 下调用。
2. **Phase 4 网络 UI**：盘点修正——网络配置此前**只有 spec 完全无 UI**（inv-4 原记「/env UI 已有」不实，已回改）。已在 `/env` 页新增「网络」卡（代理模式/HTTP/HTTPS/maven/npm/pip/goproxy），保存走 `yaml.saveForm`。
3. **CLI/MCP app 级网络默认**：CLI 无 appdata 通道，本轮不注入 app 级镜像默认；workspace `network:` 段在 CLI/MCP 同样生效（引擎层注入）。app 级默认仅桌面端（open 前注入）。
4. **崩溃通知开关**存 localStorage `st:crashNotify`（默认开），不进 app.json prefs（避免 prefs schema 变更）；系统通知用官方 `tauri-plugin-notification`（Cargo + npm + capability 各一行）。
5. **venv 并存警告**（`.venv`+`venv` 并存取 `.venv`）为静默选择，未接 warnings 通道（plan_service 无 warnings 出口，gradle 式旁路成本高于收益，已备案）。
6. **运行时 PORT 注入**：engine 侧实际走 `ports::port_env_key`（已扩展 python/go）；launcher `merge_env` 同步扩展保持直接调用路径一致。

## 基线与每期回归

- 基线（2026-08-29）：core 361 / cli 20 全绿；四语 parity 845 keys。
- 每 Phase 收尾必跑：
  1. `cargo test -p supertask-core`
  2. `CARGO_TARGET_DIR=target-cli cargo test -p supertask-cli`
  3. `frontend/` 内 `npm run build`
  4. i18n parity 脚本（四语 keys 一致）
- 目标基线：core ≈ 430+（新增 ~70）、parity ≈ 880。

---

## Phase 0 · 文档拍板（✅ 2026-08-29 完成）

- [x] 功能规格 + 本实现计划
- [x] roadmap 版本地图插 1.7、§2.2 移除 Python/Go
- [x] repository conventions：当前阶段 / 规范真源 / 已拍板 14–16
- [x] 盘点回改（living 约定）：inv-2 散点 6→7 处；inv-4 A1 现状修正 + A1–A4 排期标注；inv-5 决策记录

## Phase 1 · core：python / go / generic 三 kind

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 1.1 | `runnable_kind` 加三 kind；`ServiceSpec` 增 typed 字段 `entry`/`package`/`program`/`args`（skip_serializing_if 缺省省略） | `spec/file.rs:506`、`spec/file.rs:70-127` | round-trip 测试：新字段缺省不出现、未知字段不丢 |
| 1.2 | per-kind 字段合法性矩阵（spec §4.3）：python `dir` 必填 + `entry` XOR `module`；go `package` 可选默认 `"."`；generic `program` 必填 + `args` 可选；非法组合 `SPEC_INVALID` | `spec/validate.rs:43-76` 扩展 | 矩阵单测（6 kind × 合法/非法用例集） |
| 1.3 | `plan_python`：解释器解析 `dir/.venv → dir/venv → root/.venv → root/venv → PATH`（Win `Scripts/python.exe` / Unix `bin/python`；`.venv`+`venv` 并存取 `.venv` + 警告一次）；argv = `python [entry]` 或 `python -m <module>` + extra_args；cwd=`dir` | `launcher.rs`（match 分支 + 新 fn） | venv 优先级 / 并存警告 / PATH 回退 / entry 与 module 两模式单测 |
| 1.4 | `plan_go`：`go run <package>` + extra_args（extra_args 语义=传给程序，文档标注）；`plan_generic`：`program` 含分隔符→工作区相对路径沙箱校验，否则 `resolve_program`（PATHEXT）；argv = `program + args + extra_args`；cwd=`dir` 缺省 `"."` | `launcher.rs` | package 默认值 / 相对路径越界报沙箱错误 / PATH 未命中 `MISSING_TOOL` |
| 1.5 | PORT 注入扩展（node/python/go 注 `PORT`、generic 不注）；grace 默认 python 15 / go 60 / generic 15；health 默认 tcp（有 port） | `launcher.rs:202-219`（merge_env）、kind 默认值处 | 注入与优先级单测（显式 env 赢） |
| 1.6 | `ENTRY_NOT_FOUND` / `PACKAGE_NOT_FOUND`：entry 文件 / package 目录不存在——打开工作区 warning、启动硬错误 | `error.rs`、`spec/validate.rs`（warning 路径）、`launcher.rs`（硬错误路径） | 两口径单测 |
| 1.7 | 引擎冒烟：fake spanner 起停三 kind stub（不弹真 GUI） | `engine.rs` 测试区 | 起→running→停无残留（沿 1.0 测试模式） |

## Phase 2 · 工具链：探测 / 安装 / env_delta 尾巴

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 2.0 | **核查先行**：`toolchain/resolver.rs::env_delta` 在启动链的现用点；未接线则补「toolchain 有钉扎时启动注入 env_delta（PATH/JAVA_HOME 口径）」 | `launcher.rs`/`engine.rs` + resolver | 核查结论写进本文件备注；补线后有单测 |
| 2.1 | `ToolKind::{Python,Go}` + `mise_tool_name`（`python`/`go`）+ `winget_id`（`Python.Python.<maj.min>` / `GoLang.Go`） | `toolchain/mod.rs:21-28`、`manifest.rs:25,56` | 映射表单测（未收录版本 `ToolchainVersionInvalid`） |
| 2.2 | `ToolchainProbe` 增 `python`/`go`（`#[serde(default)]`，gateway 先例）；Windows Store 别名非 0 退出=not found | `probe.rs:16-71` | 探测单测（fake，含别名用例） |
| 2.3 | `ToolchainSpec` 增 `python: "3.12"` / `go: "1.23"` 钉扎 + 校验 | `spec/file.rs`、`spec/validate.rs:176` 关联表 | 钉扎校验单测 |
| 2.4 | 安装链复用回归：mise 优先 / winget 兜底 / 装完重解析对两新工具成立 | `toolchain/install.rs` 测试 | FakeRunner 脚本单测 ×2 工具 |

## Phase 3 · 扫描识别

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 3.1 | 特征优先级 `pom > gradle > package.json > pyproject/requirements > go.mod`（每目录一特征；compose sidecar 独立识别不变） | `scan.rs:19-21`（特征表）、目录分类处 | 混合目录矩阵单测 |
| 3.2 | python 草稿：入口猜测 `manage.py(→entry+extra_args[runserver]) > main.py > app.py > server.py > app/main.py`；全不中 entry 留空 + warning；端口建议 8000 起 | `scan.rs` | 各猜测分支 + warning 单测 |
| 3.3 | go 草稿：`cmd/` 恰一 main 子目录→`./cmd/<it>`；多/无→`"."` + warning；端口建议 8080 起（suggest 顺延含网关缺省冲突） | `scan.rs`、`ports.rs::suggest` 复用 | 唯一/多候选/无 cmd 三态单测 |
| 3.4 | 深度 >4 层警告文案补 Python/Go 措辞 | `scan.rs:148` | 文案断言 |

## Phase 4 · 网络接线（A1 关闭 + pip/go 扩展）

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 4.1 | `NetworkSpec` 增 `python:{index_url}`/`go:{goproxy}`；`EffectiveNetwork` 增 `maven_mirror/npm_registry/pip_index/go_goproxy`；`AppNetwork` 同步四字段（缺省 None） | `spec/file.rs:320`、`network.rs:20-25`、`appdata.rs:30` | 合并规则单测（workspace 覆盖 app） |
| 4.2 | 启动注入：`CommandSpec` env 统一合并，**注入优先级最低**（注入 → workspace env → service env）；键：`npm_config_registry` / `PIP_INDEX_URL` / `GOPROXY`；代理键（现有 `tool_env`）一并注入 | `launcher.rs`（plan 入口处） | 显式 env 优先 / 注入存在性单测 |
| 4.3 | maven mirror：生成 `.supertask/maven-settings.xml`（mirrorOf *，产物=缓存口径）+ `MAVEN_ARGS="-s <abs>"`；用户已显式设 `MAVEN_ARGS` → 不覆盖 + warning | `network.rs` 或新 `maven_settings.rs`、launcher | settings.xml golden 单测 + 冲突 warning 单测 |
| 4.4 | 健康检查不走代理回归（`strip_proxy_vars` 现状保持） | `health.rs` | 回归单测 |

## Phase 5 · 前端：运行时支持 + 易用性

> Skills：`vercel-react-best-practices`（组件性能）、`vercel-composition-patterns`（provider/组合）、`ui-styling`（token/对比度）；5.7 审查用 `web-design-guidelines`。按钮语义色按 repository conventions 约定表。

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 5.1 | run 页三 kind 支持：kind→图标/文案用**映射表**（禁止 if 长链）；详情头字段按 kind 渲染（entry/module/package/program+args） | `run-page.tsx`、protocol 类型 | 三 kind 卡片与详情正确；旧 kind 渲染零回归 |
| 5.2 | config 页：kind 下拉加三项 + 新字段表单（entry/package/program/args 按合法矩阵显隐） | `config-page.tsx` | 矩阵联动正确；保存带 `base_hash` |
| 5.3 | env 页：python/go 工具卡（探测 + 一键安装，对齐 java/node 卡）+ 网络卡补 pip index / goproxy 字段 | `env-page.tsx` | 卡片与安装流转；missing 文案说人话 |
| 5.4 | 分组 UI（A3）：按 `group` 聚类（YAML 首现序、未分组最后）；组头=名称+运行计数+折叠（localStorage `st:runGroupCollapsed`）+组级启动/停止（停止 destructive+确认）；无 group 旧 yaml 渲染不变 | `run-page.tsx`（可能抽 `service-group.tsx`，勿大桶导出） | 分组/折叠/组级启停；`group` 字段进 config 表单 |
| 5.5 | 崩溃通知（A2）：AppShell 监听 `st.runtime`——`→exited 且 last_exit.code≠0` 或 `→unhealthy` → 聚焦 Toast / 失焦系统通知（plugin-notification，**核查依赖缺则加官方插件**）；10s 去重；主动停止不触发；设置「通用」开关默认开 | `AppShell.tsx`、`settings-page.tsx` | mock 与真机双路径 |
| 5.6 | 四语 keys（run/config/env/notify 增量） | `i18n/locales/*` | parity 脚本通过 |
| 5.7 | 页面审查 | 全部新改页面 | web-design-guidelines 过审 |

## Phase 6 · 信息架构：入口归位 + 导航重排

> Skills：`vercel-composition-patterns`、`web-design-guidelines`。

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 6.1 | `/workspaces` 新增「工作区包」卡：导出（with-secrets + save 对话框 + Confirm，**整卡自 settings 迁移**）+ 导入（选包→选目录→只落盘→提示打开，对齐 welcome 交互） | `workspaces-page.tsx` | 导出产物与 CLI 一致；锁冲突 toast 沿用 |
| 6.2 | settings 移除导出卡 + 相关 keys 清理 | `settings-page.tsx:280-310`、locales | 无死 keys（parity 校验） |
| 6.3 | 导航五组：工作台(run,logs) / 工作区(workspaces,discover,templates,config,git) / 环境(env,docker,gateway) / 扩展(cloud,ai) / 系统(settings pinned)。**core `features.rs` 不动** | `registry.ts`（NavGroup/NAV_META/GROUP_ORDER）、AppShell、四语组名 | 五组渲染正确；soon 项仍在扩展组 |
| 6.4 | 命令面板动作核对（导出/导入/分组动作指向新位置，无死链） | `command-palette.tsx` | 面板全动作可执行 |
| 6.5 | 四语 + parity | `i18n/locales/*` | 通过 |

## Phase 7 · 模板 + 文档闭环 + 全量回归

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 7.1 | 模板 +2：`python-fastapi`（module: uvicorn + tcp 健康）、`go-node-fullstack`（go api + node web + depends_on）；params/blocks 按现引擎 | `template_assets/`（纯数据） | `templates.preview` 单测 + 向导可走通 |
| 7.2 | yaml.md：§4.2 kind 表（python/go/generic 转可启动，去掉 2.2 标注）、§4 新字段与矩阵、`group` 转 live、network python/go 段、grace/PORT 规则 | `docs/spec/yaml.md` + `supertask.schema.json` | 与实现一致 |
| 7.3 | ipc.md §10.11（1.7 增量：probe 扩展、注入键表、分组语义、错误码两枚、零新命令声明） | `docs/spec/ipc.md` | 契约同步 |
| 7.4 | architecture.md（三 kind 备注、启动注入链一节）、cli.md（doctor 覆盖 python/go 一句） | 对应文档 | 同步 |
| 7.5 | 全量回归四连 + repository conventions 当前阶段更新 + inv 文档按 living 约定回改（inv-1 交付表、inv-4 欠账划账） | 全部 | 基线达标 |

## Phase 8 · 验收（真机矩阵 + Playwright 起步）

| # | 任务 | 验收 |
|---|------|------|
| 8.1 | 按 spec §14 场景 1–12 真机逐条（Windows 主机；Docker/网关场景沿 1.3/1.6 环境要求） | 记录进 `docs/verification/2026-08-xx-v1-7-acceptance.md` |
| 8.2 | Playwright 起步（skill：`webapp-testing`）：主链路冒烟——welcome→扫描→启动→分组→日志→导出导入 | 用例入库可重复跑（偿还 A4 起步） |
| 8.3 | 遗留处置：B 类验收债是否并入本轮矩阵一并跑（拍板项，跑前确认）；结果回写 inv-4 | 清单化 |

## 依赖与并行

- 串行主干：1 → 2 → 3 → 7.1（模板需 kind 落地）。
- **Phase 4、6 与主干无依赖，可并行**；Phase 5 依赖 1+2（kind 与 probe 字段）；Phase 8 最后。
- 每期独立可合入（无半成品宏/feature flag；未完成 kind 不进 `runnable_kind`）。

## 复用清单（零/近零新依赖）

- core：**零新 crate**（venv=路径检查、settings.xml=字符串模板、generic=既有 resolve_program）。
- 前端：除 `@tauri-apps/plugin-notification`（官方插件，5.5 核查后如缺才加）外零新 npm 依赖。
- 模板：零代码。与 1.5 复用核查惯例一致，结论实现期回填本文件。
