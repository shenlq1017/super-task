# AGENTS.md — AI 代理工作指南

> 面向在本仓库工作的 AI 编码代理（Cursor / Claude Code / Codex 等）与新人。
> 人读的贡献流程见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 项目是什么

SuperTask：一份 `supertask.yaml` 一键拉起/收场本机多服务工作区的桌面应用。
本地优先 · Tauri 2 + Rust（引擎与平台无关）· React 19 前端 · Windows 优先，macOS / Linux 可用。

## 仓库布局

```
crates/supertask-core        # 引擎：spec/runtime/proc/health/gateway/ai/cloud/importer…（业务全在这里）
crates/supertask-cli         # CLI（bin: supertask）与 stdio MCP 服务器
crates/supertask-cloud-server# 自托管云参考服务（axum + SQLite + 内置管理控制台）
src-tauri                    # Tauri 2 IPC 薄适配（#[tauri::command] 只做校验与转发）
frontend/                    # React 19 + Vite 8 + Tailwind 4 桌面 UI
cloud-console/               # 云管理控制台（独立 npm 子项目，构建产物由服务端托管）
examples/                    # 四个开箱即用示例工作区（spring-multi / node-demo / gateway-demo / compose-demo）
docs/                        # 文档（见下）
references/                  # 只读外部参考快照（如 dbx），禁止直接修改
```

## 文档地图（重要）

- **`docs/spec/` 是当前功能的唯一真源**：`yaml.md`（配置规范）、`ipc.md`（IPC 契约）、
  `architecture.md`（引擎架构）、`cli.md`（CLI/MCP）、`cloud.md` / `cloud-server.md`（云协议）、
  `supertask.schema.json`。改了行为必须同步改 spec。
- `docs/inventory/`：按版本的系统现状盘点（交付清单 / 前端 IA / 技术债）。
- **`docs/ROADMAP.md` 是面向未来的方向与可行性真源**：按能力方向组织（服务监管 /
  纳管任意来源 / 环境供给 / 网络与身份 / 主机与服务可观测性 / 数据备份 / AI 原生 /
  多平台 / 长期生态），每项标注来源、现状与「价值 · 成本 · 契合度」评级。
  **不含版本号、不含排期**——仓库不维护版本编号路线图。
  交付后回落进 `CHANGELOG.md`，行为规格进 `docs/spec/`，现状进 `docs/inventory/`。
- **带版本编号的历史记录与逐项实施规划不在仓库内维护**（单人项目），
  本地存档于 `.workbuddy/local/VERSIONS-AND-PLAN.md`（已被 `.gitignore` 排除）：
  主题版本 1.x/2.x 与发布版本 v0.1.x 的记录、F1–F28 / G1–G2 / N1–N13 / M1–M6 逐项规划
  （编号、来源、现状 file:line、改动面、验收标准）、发版操作清单与平台推进专项。
- `docs/archive/`：**历史材料，仅考古用，不代表现状**——
  `plans/`（各版本 feature spec 与实施计划）、`research/`（选型与调研）、
  `adr/`（早期架构决策）、`verification/`（历史验收记录）。
  已被 spec 取代的内容以 spec 为准；不要把 archive 当实现依据。
- `CHANGELOG.md`：版本变更记录（发布说明直接取对应版本段落）；
  `README.md`：平台支持、能力介绍与「未来考虑」方向。

## 构建与测试

```bash
npm ci && npm --prefix frontend ci
npm run tauri:dev                  # 桌面应用开发模式
cargo build -p supertask-cli      # CLI（bin: supertask；桌面 dev 时用 CARGO_TARGET_DIR=target-cli）
cargo test -p supertask-core      # 引擎测试（约 530+ 项）
cargo test -p supertask-cli       # CLI 测试（20 项）
cargo test -p supertask-cloud-server  # 云服务测试（16 项，零真实网络）
cargo fmt --all -- --check        # PR 前必须通过
npm --prefix frontend run build   # 前端构建
```

测试全部离线：网络路径用 fake transport（FakeCloudProvider / 假 AI 传输），不访问公网。

## 硬性约定（违反会被评审打回）

1. **业务逻辑只进 `supertask-core`**；`src-tauri` 命令保持薄适配（校验 workspace_id → 调引擎 → 错误码映射）。
2. **前端禁止 shell / 任意 fs**：一切副作用走 `docs/spec/ipc.md` 契约；服务启动 argv 只能由已加载 spec 生成，UI 只传 id 不传命令。
3. **安全边界**：健康检查只打 loopback；路径必须过 sandbox 约束；密钥/密码/token 不进日志、事件、返回值。
4. **错误码稳定**：新增错误码进 `ipc.md` §7 码表，`CLOUD_*` / `ADMIN_*` / `TERM_*` 等前缀不混用。
5. **事件名只用连字符**（Tauri v2 不允许点号），常量真源 `core ipc::event`，不在调用点手写字符串。
6. **kind 开放字符串**：未知 kind 能加载不能启动；新增 kind 需要过 spec 字段矩阵 + launcher 实现 + 测试。
7. 不提交凭据、本地数据库（*.db）、构建产物、截图、机器特定路径。
8. PR 说明行为变化并带聚焦测试；UI 变更附简短手动验证说明。

## Git 约定（现行实践）

- 分支：`feat/<topic>`（如 `feat/grokbot-discover-upgrade`），PR 合并进 main。
- 提交：conventional commits（`feat:` / `fix:` / `chore:` / `docs:`），UI/引擎混合改动拆开提交。
- 发版：统一升版（workspace + frontend + tauri.conf），CHANGELOG 补条目，tag 形如 `v0.1.3`。

## 当前状态速览

- 版本 `0.1.3`；功能注册表（`features.rs`）13 个页面全部 live，无 soon 占位。
- 六种服务 kind 全部可启动：spring-boot / node / compose / python / go / generic。
- **发布状态**：Windows 安装包（NSIS / MSI）随 GitHub Release 发布，自动更新链路已验证可用
  （`release.yml`：tag 触发 → 构建签名 → draft Release → 镜像到 cnb.cool `stable` 滚动 tag；
  应用内「设置 → 检查更新」走 CNB（国内）/ GitHub（海外）双端点）。
- **平台**：Windows 可用；macOS / Linux **不推荐**——无构建产物、CI 矩阵仅 `windows-latest`、
  无真机验收。推进事项见 `.workbuddy/local/VERSIONS-AND-PLAN.md` 第六节（M1–M6）。
- 云端为自托管参考实现（默认 127.0.0.1:8787，SQLite），正式运营端点未定。
- 下一步方向：见 [docs/ROADMAP.md](docs/ROADMAP.md)。
  当前优先级最高的三类是 ① 数据已就绪、只差接线的项
  （崩溃通知、MCP 暴露主机指标、MCP 输出脱敏、隧道纳管、孤儿进程纳管）；
  ② 编排层欠账（`restart` 策略、log-pattern 就绪判定、compose 导入）；
  ③ 平台推进的 M1（CI 覆盖多平台，成本最低，先破「从未验证」）。
