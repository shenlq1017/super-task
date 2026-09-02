# inv-5 · 候选方向的事实约束与待拍板问题

> 2026-08-29。盘点稿，供复核。**本文不定方案**：只列每个方向受哪些事实约束、代价在哪、捷径在哪；方案在规划文档里做。
> 证据细节见 inv-2（代码）、inv-3（前端）、inv-4（欠账）。

## 方向一：运行时横向扩展（Python / Go 等后台语言）

### 事实约束

1. **路线图原位 2.2**：roadmap 版本地图「2.2 生态：插件、WSL、更多语言」+ §2.2「Python / Go 服务 kind」；yaml.md §4.2 同样标注 python/go 首版 **2.2**。提前实现 = 路线图重排（2.2 剩余项插件/WSL2 顺延），需要显式决策。
2. **地基友好**（inv-2 §2-3）：`kind` 是 String + flatten extra，六处字符串 match 散点是必改项；健康检查/进程/指标/日志/网关与 kind 解耦，零或近零改动。
3. **先例可估成本**：1.3 compose 是完整链路（typed 字段 → 校验 → 适配层 → 运行时 → IPC → 页面 → 扫描导入）；1.6 对 `ToolchainProbe` 加字段也有先例（`gateway` 字段）。
4. **工具链链路可复制但需扩表**（inv-2 §4）：`ToolKind`、`mise_tool_name`、`winget_id` 三处扩展；安装/重解析链不用重写。
5. **扫描识别是净新增**（inv-2 §5）：现有零 Python/Go 特征。且生态分叉比 node 复杂——Python：uv / venv / poetry / 系统 python / requirements vs pyproject；Go：`go run .` vs 编译产物 / go.work。这决定扫描草稿与默认启动命令的设计复杂度。
6. **模板零代码**（inv-2 §6 + repository conventions）：新 kind 落地后加模板是纯数据。

### 潜在捷径（事实，非建议）

- `kind: generic`（argv 通用进程）自 1.0 预留（yaml.md §4.2「1.x」），一直 `KIND_UNSUPPORTED`。走 generic 可以先解锁「任意命令当服务」而跳过 per-language 的扫描/模板/专有字段。
- 专用 kind 与 generic 不互斥（generic 也列在 1.x 预留里，二者最终可能都要）。

## 方向二：易用性提升

### 事实约束

1. **现成候选池 = 1.2 三个功能缺口**（inv-4 A1–A3）：分组 UI（字段就绪、UI 缺失）、mirror/registry 运行时接线（字段透传、注入缺失）、系统级崩溃通知。均为「有半成品」的项，性价比可量化。
2. **命令面板已存在**（inv-3 §4）：`command-palette.tsx` 已挂 AppShell；易用性整合可基于它（如把「改端口」「导出」等高频动作入面板），无需新建入口设施。
3. **既有交互规范完备**（repository conventions UI 两节）：按钮变体/悬浮/圆角/对比度/尺寸、日志视图约定都有真源约束，易用性改动有既定风格可循。
4. 缺口：目前**没有真机验收反馈**（B1–B5 未做），易用性优先级排序缺少用户数据——排序本身就是待拍板项。

## 方向三：功能入口 / 信息架构调整

### 事实约束

1. **用户点名项的现状**（inv-3 §3）：工作区包**导出在设置页**（`settings-page.tsx:280-299` 导出卡）、**导入在 welcome 页**（首启引导位）；`/workspaces` 独立页已 live。功能本体（core `pkg.rs` + CLI `export/import`）完整且与入口解耦——迁移是纯前端页面卡片级工作。
2. **导航是数据驱动**（inv-3 §1）：双注册表（core `features.rs` + 前端 `registry.ts` 三组 workspace/extend/system），增删/换组不改壳层；但注意 core 端 feature 表变更会牵动 `session.hello` 契约（ipc.md）与四语 `nav.*` keys。
3. **待决策的边界**（用户提问衍生）：
   - 只挪导出 vs 导出+导入一起归位 `/workspaces`（导入现在是首启引导，挪走会影响首次使用路径）；
   - 设置页原位置留不留链接/跳转；
   - `/workspaces` 页信息密度（列表 + 锁状态 + 导出导入操作）是否会过载。

## v2.0 云现状补记（不改变本候选文档的方案性质）

- 客户端云模块与 FakeCloudProvider 自动化范围已落地；`crates/supertask-cloud-server` 的本地 HTTP router/API、`/healthz`、配额/遥测和 in-process API 集成测试也已完成。正式 HTTPS 部署与真机验收仍未完成。
- 客户端端点默认仍为占位域名；`CloudHandle` 已具备内部端点校验/重载能力，`cloud.endpoint.set` 已注册为 Tauri IPC，浏览器 mock 保留 local-only 降级。官方服务端运营方/正式端点仍是开放问题，不应从“可自托管协议”推断为已提供官方云服务。
- 这些是现状与约束，不是对 2.1/2.2 候选方向的方案拍板；详细云协议见 [docs/spec/cloud.md](../spec/cloud.md)，服务端约束见 [docs/spec/cloud-server.md](../spec/cloud-server.md)。

## 待拍板问题（规划文档动笔前需确认）

> **2026-08-29 已全部拍板**：① v1.7 = 三支柱组合；② Python/Go 提前至 1.7（插件/WSL2 留 2.2，generic 兜底「更多语言」）；③ 专用 kind + generic 同版；④ 探测 + 一键安装都做；⑤ 导出+导入都进 `/workspaces`（welcome 首启导入保留，settings 不留副本）；⑥ A1–A4 进本轮，B/C 单开验收专项。决策依据见 [v1.7 规格 §2.1–2.2](../plans/2026-08-29-v1-7-feature-spec.md)。下表为决策过程存档。

| # | 问题 | 选项线索 |
|---|------|----------|
| 1 | 版本号与主题范围 | v1.7？单主题（只做其一）还是组合（如 Python/Go + 入口调整 + 吸收 A1）？ |
| 2 | 路线图重排 | Python/Go 从 2.2 提前后，插件/WSL2 顺延到 2.3 还是与之并列？ |
| 3 | Python/Go 实现形态 | 专用 kind（高成本高完整度）/ 先 generic（低成本）/ 两步走 |
| 4 | 工具链安装 | Python/Go 是否接入 mise/winget 一键安装，还是先只探测 |
| 5 | 入口调整范围 | 只挪导出 / 导出+导入都进工作区模块；设置页留不留入口 |
| 6 | 欠账吸收 | A1（mirror/registry 接线）、A2（崩溃通知）、A3（分组 UI）哪些进本轮；验收债（B/C 类）是否单开专项 |

## 复核清单（建议核对顺序）

1. inv-2 §2.2 六处 match 散点：逐个打开 file:line 确认（这是成本估算的支点）。
2. inv-3 §3 导出/导入入口位置：打开 settings-page / welcome-page 目视确认。
3. inv-4 A1–A3：对照 v1-2-progress `:21,50` 原文确认「半成品」程度。
4. roadmap §2.2 与 yaml.md §4.2 的 python/go 标注：确认 2.2 定位无歧义。
