# inv-3 · 前端信息架构与功能入口盘点

> 2026-08-29。盘点稿，供复核。前端在 `frontend/`：React 19 + Vite + shadcn（radix-nova），Hash 路由。

## 1. 双注册表（导航的事实来源）

- **core 端** `crates/supertask-core/src/features.rs:20-36`：13 个 feature，字段 `{id, path, status, since}`：
  - live：run/logs/config(1.0)、templates(1.1)、env/workspaces/discover/git(1.1)、docker(1.3)、gateway(1.6)、settings(1.0)、cloud(2.0)、ai(2.1)；
  - soon：无（SOON_COMMANDS 清空，机制保留）。
- **前端** `frontend/src/features/registry.ts`：只存导航元数据（labelKey + group），不存 live/soon（status 来自 session.hello）。
  - `NAV_META`（`:14-28`）：三个组——`workspace`（run/logs/config/env/workspaces/discover/templates/git/docker/gateway，共 10 项）、`extend`（cloud/ai）、`system`（settings，底部 pinned，`:31`）。
  - 约束（repository conventions）：导航只 map `session.hello.features`；**禁止 AppShell 按 feature id 写长 if**；禁止大桶 re-export。
- 含义：**入口增删/换组是纯注册表数据改动**，不触壳层逻辑。

## 2. 路由与页面清单（实测 `frontend/src/pages/`）

| path | 页面 | 备注 |
|------|------|------|
| /welcome | welcome-page.tsx | 首启；**1.5 pkg 导入入口在此**（1.5 progress：「welcome 导入与设置导出入口」） |
| /run | run-page.tsx | 最大页面（2000+ 行）：ServiceCard、详情头（打开/重启/构建/启动/停止）、脚本卡、详情 Tab（日志/终端/环境/健康/配置/指标 [+容器/代理]）、端口检查/建议/改端口；「终端」Tab（2026-08-30）接 `components/terminal-view.tsx`（xterm.js，PTY 会话随 Tab 开关，ipc.md §10.15） |
| /logs | logs-page.tsx | 与运行页共用 `components/log-view.tsx` + `log-line.tsx` |
| /config | config-page.tsx | YAML 编辑、secrets/profiles |
| /env | env-page.tsx | 工具链探测/安装、网络（代理/镜像） |
| /workspaces | workspaces-page.tsx | 工作区列表/切换；锁冲突 toast 在此链路 |
| /discover | discover-page.tsx | 系统进程发现 + **2.1「从 README 导入」入口**（outline 按钮 → `import.readme` → 共享 merge 向导 `components/scan-merge.tsx`（provenance 徽标 + 脚本项）；空草稿人话提示卡；`?readme=1` 命令面板直达） |
| /templates | templates-page.tsx | 模板组合向导 |
| /git | git-page.tsx | clone/pull/状态 |
| /docker | docker-page.tsx | compose 运行时/构建/导入 |
| /gateway | gateway-page.tsx | 五卡 + 空态 + diff 应用 + trust 确认 |
| /settings | settings-page.tsx | 见 §3 |
| /cloud | cloud-page.tsx | live（登录/会话/同步中心/冲突/配额；端点高级设置走 typed `cloud.endpoint.set`，浏览器 mock 为 local-only 降级） |
| /ai | ai-page.tsx | live（2.1 AI 块：配置列表+编辑表单（8 provider 预设/代理/重试/模型拉取）+ 全局指令 + Prompt 模板 + 用量/隐私/MCP 说明）；log-view `extraActions` 槽位接 `AiExplainButton`（run/logs），config RawTab「AI 建议」卡 |

**2.1 第二轮增量（2026-08-29）**：config-page 内嵌 merge 向导组件抽出为 `frontend/src/components/scan-merge.tsx`（`ScanPreviewPanel`/`ScanItemRow`/`DiffFieldRow`/`ScanStatusBadge`/`ProvenanceChips`/`ScriptItemRow`），config 与 discover 共用；命令面板新增三入口（`palette.readmeImport` → `/discover?readme=1`、`palette.aiExplainLogs`（`logs.snapshot` 尾 200 行 + `ai.complete` + 结果对话框）、`palette.aiSettings` → `/ai`）。

## 3. 设置页现状（`settings-page.tsx` 实测区块）

- 更新卡（`:122` `updateTitle`；升级确认 `:216`）；
- **导出卡（`:280-299`，`exportConfirmTitle`）——1.5 工作区包导出入口在设置页**；
- 通用（`:378` `general`）、外观（`:413` `appearance`）、关于（`:470` `about`）。

**入口迁移相关事实**：导出功能本体在 core `pkg.rs`（CLI `supertask export/import` 已全量可用），桌面只是入口位置问题；`/workspaces` 页已 live（`workspaces-page.tsx`，含打开本地目录、dialog 插件）。迁移 = 页面卡片级别。

## 4. 全局交互设施

- **命令面板已存在**：`components/command-palette.tsx`（AppShell 挂载；roadmap「1.0 骨架、1.2 填满」已兑现）——易用性工作可在其上叠加，不必新建。
- workspace-switcher 组件存在（`components/workspace-switcher.tsx`）。
- Toast、TooltipProvider（delayDuration 1000ms）、确认对话框、破坏性按钮红色规范——均有全站约定（repository conventions「UI 按钮约定」节）。
- 未见全局快捷键中心/全局搜索（除命令面板）——盘点为「不存在」，如需属于新增。

## 5. 运行页与分组（1.2 遗留的实态）

- `ServiceSpec.group` 字段 reserved（`spec/file.rs:79`；yaml.md §4.1「1.2 UI 分组」）。
- run-page.tsx 中的 "group" 全部是 CSS hover 类名（`:327,328,359,1363`），**没有任何按 `group` 字段的服务分组 UI**（无分组标题、无折叠、无按组启停）。
- 即 1.2 遗留「分组等交互细化」= 字段在 spec/yaml 层已就绪，前端呈现缺失。

## 6. i18n

- locale 文件：`frontend/src/i18n/locales/{zh-CN,zh-TW,en-US,ja-JP}.ts`（实测四个文件存在）。
- parity 845 keys（repository conventions）；nav 文案走 `nav.<labelKey>`（registry.ts 注释，1.4 规格 §6.2）。
- 新增页面/入口的成本项：四语 key 同步 + parity 校验。

## 7. mock/开发链路

`npm run dev`（仓库根）= 浏览器 mock IPC（代理到 frontend/）；`npm run tauri dev` = 真 IPC。session-provider 提供 hello/features/meta；日志走 `st-logs` 批次 + `logs.snapshot`。

## 8. 云入口（2.0 当前事实）

- `/cloud` 已从占位切换为 live 页面，包含登录/会话、同步中心、冲突三选一和配额展示；mock provider 可演示离线/冲突路径。
- 云端点高级设置已有 typed API；Tauri `cloud.endpoint.set` 已注册并持久化，浏览器 mock 仍返回 `supported: false, local_only: true` 作为 local-only 降级。
- welcome「从云端恢复」与 settings 遥测/端点 UI 已交付；passphrase 管理 UI 仍待补。
