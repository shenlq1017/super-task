# v2.2 实现计划（生态：自定义 kind 插件 / WSL2 / 可复现导出）

> 2026-08-29。依据：[2026-08-29-v2-2-feature-spec.md](2026-08-29-v2-2-feature-spec.md)（规划稿，随拍板同步修订）。
> 前置：v2.1 收尾基线；kinds 云同步（Phase 6）硬依赖 v2.0 已交付。
> 执行约定：先读 `<user-home>\.agents\skills\executing-plans-0.1.0\SKILL.md`；前端任务点名 skill；CLI 构建用 `CARGO_TARGET_DIR=target-cli`。

## 基线与每期回归

- 参照基线（v2.1 收尾）：core ≈ 570+ / cli ≈ 28；parity ≈ 985。kickoff 实测后回填。
- 每 Phase 收尾必跑：core / cli 测试、`frontend/` 内 `npm run build`、i18n parity 脚本。
- 目标基线：core ≈ 640+（新增 ~70）、cli ≈ 34、parity ≈ 1030。
- 单测零真实外部依赖：WSL 探测走 fixture 字节流；插件安装走临时目录；真机项全部后置 Phase 8。

---

## Phase 1 · core：KindRegistry 与 manifest 校验

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 1.1 | `kinds/mod.rs`：manifest typed 结构（严格解析：未知键拒绝）+ `KindRegistry`（appdata `kinds/` 目录装载；app 启动 + 开工作区双时机） | 新模块、appdata 复用 | 合法/非法 manifest 矩阵单测（缺 argv / 未知占位符 / 未知键 / 未知 version） |
| 1.2 | id 规则与冲突：`^[a-z][a-z0-9-]*$`；撞内置 → `KIND_ID_CONFLICT` 插件禁用（内置永远赢）；插件互撞按目录名字母序取先 + warning | kinds/ | 冲突三态单测 |
| 1.3 | 坏 manifest 容错：单插件禁用 + warning，**不崩 app.load**（沿 probe「坏工具不阻塞」先例） | engine 装载路径 | 混合好坏插件装载单测 |
| 1.4 | `kinds.install / kinds.remove` 本体：目录拷入/删除（remove 不影响运行中服务） | kinds/ | 安装/卸载/占用中卸载单测 |

## Phase 2 · core：七散点接线（主干）

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 2.1 | `runnable_kind` 注册表兜底（内置优先） | `spec/file.rs:506` | 内置/插件/未知三态单测 |
| 2.2 | validate 插件分支：按 fields schema（required/type/default）校验，值在 flatten extra round-trip（**ServiceSpec 零改动**核对） | `spec/validate.rs:43-76` | schema 校验矩阵 + round-trip 单测 |
| 2.3 | launcher argv 模板渲染：字面量 + 声明字段值**单元素插入**；`{{ extra_args }}` 最末规则；path 类字段沙箱校验；program PATH 未命中 `MISSING_TOOL` | `launcher.rs`（match 分支 + 新 fn） | 渲染矩阵（占位符/args 拼接/沙箱越界/PATH 未命中）单测 |
| 2.4 | 端口 env 注入：manifest `port_env`；显式 env 优先（沿 1.7 merge_env 规则）；grace/health 默认值 | `launcher.rs:202-219` 一带 | 注入优先级单测 |
| 2.5 | 扫描接线：插件特征进优先级表（go.mod 之后、按 id 字母序）；entry_guess / 端口建议（ports.rs::suggest 复用） | `scan.rs:19-21`（特征表） | 混合目录优先级矩阵单测 |
| 2.6 | `ToolchainProbe.custom: Vec<ToolProbe>`（带 name，`#[serde(default)]`）；插件探测进 custom 行 | `probe.rs:16-29` | additive serde 单测（旧 payload 兼容） |
| 2.7 | 安装链解耦小重构：`InstallRequest {mise_tool, winget_id, version}` 抽出；插件路径由 manifest.install 构造，复用 mise→winget→重解析 | `toolchain/install.rs`、`manifest.rs` | FakeRunner 安装单测 ×插件路径 + 既有六工具回归 |
| 2.8 | 引擎冒烟：fake spanner 起停插件 kind stub（deno fixture manifest） | `engine.rs` 测试区 | 起→running→停无残留（沿 1.0 测试模式） |

## Phase 3 · core：WSL 运行时（与 Phase 1→2 并行）

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 3.1 | `ServiceSpec` 增 `runtime`（native 缺省省略）/ `wsl_distro`；validate：非 Windows 或实验关 → 打开 warning；启动硬错误 `WSL_RUNTIME_UNSUPPORTED` 口径 | `spec/file.rs:70-127`、`spec/validate.rs`、`error.rs` | round-trip + 双口径单测 |
| 3.2 | `wsl/mod.rs` 探测：`wsl --status` / `wsl -l -q`；**UTF-16LE 解码（encoding_rs 复用）+ UTF-8 双探测**；结果进 `ToolchainProbe.wsl`（三态 + distro 列表） | `probe.rs` | UTF-16/UTF-8 双 fixture 解码单测；三态单测 |
| 3.3 | 路径翻译：`C:\a\b` ↔ `/mnt/c/a/b` 双向 + 独立测试 | `wsl/` | 翻译矩阵单测（含盘符大小写/UNC 拒绝） |
| 3.4 | launcher 集成：`runtime: wsl` → `wsl.exe -d <distro> --cd <unix(dir)> -e <argv…>`；启动前 `command -v` 预检（MISSING_TOOL 口径，提示 WSL 内安装）；venv 解析对 wsl 显式跳过（文档标注） | `launcher.rs` | argv 构造单测（fake runner 断言命令行） |
| 3.5 | 停止策略：Job Object 杀 wsl.exe 树（既有机制零改动核对）；逃生口命令构造（`wsl --terminate`，由前端触发，core 只提供命令组装） | engine/proc 既有链 | 单测断言无 core 改动回归；逃生口命令组装单测 |

## Phase 4 · core：可复现导出（与 Phase 1→3 并行）

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 4.1 | `export/mise.rs`：ToolchainSpec 钉扎 → `[tools]` 表纯函数 | 新模块、新依赖 `toml` | golden ×2（全钉扎/部分钉扎） |
| 4.2 | `export/devcontainer.rs`：features 映射表（数据化）+ forwardPorts + name | 新模块、serde_json 复用 | golden ×2（含端口冲突顺延） |
| 4.3 | CLI：`supertask export --format mise\|devcontainer [-o]`（默认 zip 回归）；`EXPORT_FORMAT_UNKNOWN` | `crates/supertask-cli`、`pkg.rs` 复用 | cli 单测 + zip 默认行为回归 |

## Phase 5 · 壳层 + 前端

> Skills：`vercel-react-best-practices`、`vercel-composition-patterns`、`ui-styling`；5.5 审查用 `web-design-guidelines`。

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 5.1 | IPC：kinds.list/install/remove、wsl.status、export.render 薄适配 | `src-tauri/src/commands.rs` | 与 core 的壳层链路冒烟 |
| 5.2 | /env 插件工具卡（probe.custom 动态渲染；探测+安装对齐 java/node 卡） | `env-page.tsx` | 场景 5 UI 路径 |
| 5.3 | /config：kind 下拉含插件（徽标）+ **schema 驱动字段表单**（path/dir/args/string 四型组件）+ runtime/wsl_distro（实验开关后可见）+ 导出配置菜单 | `config-page.tsx`（可能抽 `kind-field-form.tsx`，勿大桶导出） | 场景 1/12 UI 路径；保存带 base_hash |
| 5.4 | /settings 实验区（WSL 开关默认关 + 风险说明）；运行页 WSL 逃生口按钮（destructive + 确认） | `settings-page.tsx`、`run-page.tsx` | 开关生效链路 |
| 5.5 | 四语 keys + 页面审查 + mock（插件 fixture + wsl 三态 mock） | `i18n/locales/*`、mock IPC | parity 通过；审查过 |

## Phase 6 · kinds 云同步（依赖 v2.0）

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 6.1 | 实体 type=`kind`（内容 = 插件目录 zip）；安装与云拉取共用 Phase 1 校验管线 | `cloud/sync.rs` 实体适配扩展 | fake 同步往返单测；旧客户端 skip 未知 type 断言 |
| 6.2 | /cloud 同步中心实体列表展示 kind 类型 | `cloud-page.tsx` | 列表渲染 |

## Phase 7 · 文档闭环 + 全量回归

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 7.1 | yaml.md：`runtime` / `wsl_distro` 字段、自定义 kind 一节（manifest 全 schema + 安全边界说明） | `docs/spec/yaml.md` + `supertask.schema.json` | 与实现一致 |
| 7.2 | cloud.md：kind 实体；ipc.md §10.14（五命令 + 错误码六枚）；cli.md（export --format）；architecture.md（KindRegistry 一节、WSL 实验边界） | 对应文档 | 契约同步 |
| 7.3 | AGENTS.md 当前阶段 + 规范真源；inv-1 交付表、inv-4 D 类排期表回改（插件/WSL2/导出销账） | living 文档 | 盘点=当前事实 |
| 7.4 | 全量回归四连 + 基线核对 | — | core ≈ 640+ / cli ≈ 34 / parity ≈ 1030 |

## Phase 8 · 验收

| # | 任务 | 验收 |
|---|------|------|
| 8.1 | CI：spec §10 场景 1–7、10–12 可自动化项全自动化 | 入库可重复 |
| 8.2 | 真机（Windows + WSL2）：deno 插件端到端（安装→扫描→配置→启动→停止）；WSL 启停健康（含 localhostForwarding 排障一次）；导出 golden 人工核对；双会话 kinds 云同步 | 记录进 `docs/verification/2026-xx-xx-v2-2-acceptance.md` |
| 8.3 | Playwright（skill：`webapp-testing`）：插件安装→schema 表单→导出配置主链路 | 用例入库 |

## 依赖与并行

- 串行主干：1 → 2（注册表先于接线）。
- **Phase 3、4 与主干完全并行**（互不依赖）。
- Phase 5 依赖 1–4；Phase 6 依赖 5 + **v2.0 已交付**；7 → 8。
- 每期独立可合入；插件 kind 未过校验管线前不进 runnable 兜底（禁止半成品吞掉 KIND_UNSUPPORTED 语义）。

## 复用清单（新依赖一条）

| 依赖 | 用途 | 理由 |
|------|------|------|
| `toml` 0.8 | mise.toml 渲染 | Rust 生态标准 toml 序列化器，活跃维护；手拼字符串易错 |

其余零新依赖：encoding_rs（WSL UTF-16 解码）、sha2、serde_json（devcontainer）、sandbox / ports / probe / install 链、GatewaySlot 托管模式、golden 测试模式（1.6 render 先例）全部复用。与 1.5 复用核查惯例一致，结论实现期回填本文件。
