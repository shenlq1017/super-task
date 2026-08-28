# SuperTask 1.0 前端工作计划

> 日期：2026-08-26  
> 状态：骨架已落地；本文件是**后续前端实现真源**（任务级）。  
> 上位：`AGENTS.md` · [IPC](../spec/ipc.md) · [UI 占位](2026-08-25-ui-extensibility.md) · [界面设计](2026-08-26-ui-design-1.0-2.1.md) · [1.0 功能规格](2026-08-25-v1-0-feature-spec.md)  
> 执行：新会话打开本文件后，**先读并遵循** `<user-home>\.agents\skills\executing-plans-0.1.0\SKILL.md`（分批 3 个任务、验证、停下来等人）。

---

## 0. 怎么用这份计划

1. 宣布：正在用 **executing-plans** 执行本计划。  
2. 每条任务开头先 **Read 该任务点名的 skill 全文**（不要凭记忆）。  
3. 不要跳过「验证」；失败就停在该任务，不要堆下一页。  
4. Job Object 已在 core 测绿，允许接 Maven spawn；仍禁止在 Tauri command 闭包里写业务。  
5. 视觉以方案 H / `docs/plans/2026-08-26-ui-design-1.0-2.1.md` 为准（Linear 浅色、品牌紫 `#5E6AD2`），**不要**再发明第三套布局。

---

## 1. 已落地骨架（不要重做）

| 路径 | 职责 |
|------|------|
| `src/` | React 19 + Vite 7 + HashRouter |
| `src-tauri/` | Tauri 2 薄适配；已接 `session.hello`、`app.load` |
| `crates/supertask-core` | 业务引擎；UI 不得绕过 IPC |
| `src/features/registry.ts` | 仅 UI 文案/分组；**status/path 来自 hello** |
| `src/ipc/protocol.ts` | 命令名、错误码、DTO，对齐 `ipc.md` |
| `src/ipc/invoke.ts` | Tauri `invoke`；浏览器走 `mock.ts` |
| `src/providers/session-provider.tsx` | `state / actions / meta` 上下文 |
| `src/app/AppShell.tsx` | 11 入口导航，按 features 渲染，无 id if 堆 |
| `src/pages/*` | live 页 stub + `ComingSoonPage` |
| `src/components/ui/*` | shadcn（radix-nova）：button / badge / separator |
| `components.json` | shadcn 工程配置 |

命令：

```text
npm run dev          # 浏览器，mock IPC
npm run tauri dev    # WebView + 真 hello/load
cargo test -p supertask-core
```

---

## 2. 技能总表（后续任务按此点名）

路径写死，避免 agent 找错副本。

| 简称 | 路径 | 何时 |
|------|------|------|
| **executing-plans** | `<user-home>\.agents\skills\executing-plans-0.1.0\SKILL.md` | 执行本计划的每一个会话 |
| **shadcn** | `c:\project\my\super-task\.cursor\skills\shadcn\SKILL.md` | 任何组件增删、表单、Sidebar、Command、Toast、Field |
| **shadcn/styling** | `c:\project\my\super-task\.cursor\skills\shadcn\rules\styling.md` | className、gap、语义色、`cn()` |
| **shadcn/forms** | `c:\project\my\super-task\.cursor\skills\shadcn\rules\forms.md` | 配置页、扫描向导、env 表 |
| **shadcn/composition** | `c:\project\my\super-task\.cursor\skills\shadcn\rules\composition.md` | Card/Dialog/Tabs/Empty/Alert |
| **shadcn/icons** | `c:\project\my\super-task\.cursor\skills\shadcn\rules\icons.md` | Lucide、`data-icon`、禁止 emoji |
| **react-perf** | `c:\project\my\super-task\.cursor\skills\vercel-react-best-practices\SKILL.md` | 写/改 React 时先看分类再打开具体 rule |
| **composition** | `c:\project\my\super-task\.cursor\skills\vercel-composition-patterns\SKILL.md` | Provider、禁止 boolean 模式爆炸 |
| **web-guidelines** | `c:\project\my\super-task\.cursor\skills\web-design-guidelines\SKILL.md` | 每个页面做完做一次 UI 审查 |
| **ui-styling** | `<user-home>\.claude\skills\ui-styling\SKILL.md` | token、亮色主题、shadcn+Tailwind 合层 |
| **ui-ux-pro-max** | `<user-home>\.claude\skills\ui-ux-pro-max\SKILL.md` | 对照设计文档的图标/动效/对比度清单 |
| **webapp-testing** | `<user-home>\.agents\skills\webapp-testing\SKILL.md` | `tauri dev` / `vite` 下点选验证 |

react-perf 要打开的 **具体 rule 文件**（均在 `.cursor/skills/vercel-react-best-practices/rules/`）：

| Rule 文件 | 用在 |
|-----------|------|
| `bundle-barrel-imports.md` | 禁止自写 `index.ts` 再 export *；lucide 走 Vite `optimizeDeps`（已配） |
| `client-event-listeners.md` | `st.runtime` / `st.logs` 全局只订一次 |
| `rerender-defer-reads.md` | 日志环高频，按钮回调不要订阅整表 |
| `rerender-derived-state.md` | `isRunning` 从 snapshot 派生，不另存 |
| `rerender-use-ref-transient-values.md` | 日志跟随滚动、光标 seq |
| `rerender-no-inline-components.md` | 服务卡片 map 里不定义内联组件 |
| `rendering-content-visibility.md` | 日志长列表 |
| `rerender-use-deferred-value.md` | 命令面板过滤 |
| `rerender-transitions.md` | 非紧急状态刷新 `startTransition` |

composition 要打开的 rule：

| Rule 文件 | 用在 |
|-----------|------|
| `architecture-avoid-boolean-props.md` | ServiceCard / Drawer 不要 `isSoon isEnv isLog` |
| `architecture-compound-components.md` | LogView、ServiceDrawer |
| `state-context-interface.md` | Runtime / Logs / Yaml 三个 Provider 的 state/actions/meta |
| `state-lift-state.md` | 抽屉与列表共享选中 id |
| `state-decouple-implementation.md` | 页面不直接 `invoke`，走 actions |
| `react19-no-forwardref.md` | 已 React 19：不要 `forwardRef`，`use()` 读 context |

---

## 3. 前端分层（后续必须遵守）

```
pages/*              路由页，组合，不直接 spawn
app/AppShell         只读 features[]，禁止 feature.id 长 if
providers/*          唯一知道 invoke / listen 的地方
ipc/*                命令名、DTO、错误
features/registry    标签与分组
components/ui        只通过 shadcn CLI 添加
components/*         业务组合（Card、LogView），无 IPC
```

**禁止：** 页面里 `invoke("runtime.startOne")`；禁止 `shell.exec`；禁止按行拉日志；禁止 soon 页假列表。

IPC 命令以 `docs/spec/ipc.md` 点分名为准（`runtime.startOne`），与 `src/ipc/protocol.ts` 的 `cmd` 常量一致。Tauri 用 `#[tauri::command(rename = "...")]`。

---

## Phase A — 壳层打磨（不接起停）

### A1. Linear token 对齐

- **Skills：** `ui-styling` · `shadcn/styling` · `ui-ux-pro-max`
- **做：** 把 `src/index.css` 的 `--background/--foreground/--border/--muted` 收到设计文档 §0（`#F7F8F8` / `#222326` / `#E6E6E6` / `#62666D`）。`--primary` 保持 `#5E6AD2`。字体：Inter Variable（UI）+ 等宽（路径/日志）；Geist 可留作 fallback。圆角按 xs5/sm8/md12，无直角。
- **不要：** 改成深色实装（dark 类可留，1.0 开关占位）。
- **验证：** `npm run dev` 侧栏/主区对比原型 `docs/prototypes/prototype-h-linear.html` 和 `docs/prototypes/supertask/`。

### A2. shadcn 布局组件

- **Skills：** `shadcn`（先 `npx shadcn@latest info --json` 再 `docs` 再 `add`）· `shadcn/composition` · `shadcn/icons`
- **添加：** `tooltip` `scroll-area` `sonner` `command` `dialog` `dropdown-menu` `sidebar`（若 sidebar 过重，用现有 aside + Tooltip，不要手写一套）。
- **做：** AppShell 收起 3.25rem 图标轨、展开 14.5rem；⌘K 打开 Command 面板（只跳转 live；soon 项 toast「将在 x.x 提供」）。账号按钮：未登录样式，点击 toast 2.0，禁止假登录。
- **验证：** 11 个入口都在；soon 可进 ComingSoon；Esc 关面板。

### A3. 可达性与审查

- **Skills：** `web-guidelines`（先 WebFetch guidelines URL）· `ui-ux-pro-max`
- **做：** 图标按钮 `aria-label`；`focus-visible`；`prefers-reduced-motion` 降级呼吸动画（可先 CSS 变量）。
- **验证：** Tab 能走完侧栏；无横向滚动（≥960px）。

---

## Phase B — Tauri 接引擎（仍无运行页业务 UI）

### B1. Engine 进 App 状态

- **Skills：** 无前端 skill；Rust 保持薄适配
- **做：** `src-tauri` 用 `Mutex<Engine>`（或 `parking_lot`）托管 **一个** ActiveWorkspace。`workspace.open/close/add` 调 `Engine`。command 闭包只：反序列化 → `engine.xxx()` → `IpcError`。
- **插件：** `tauri-plugin-dialog` 选目录（`workspace.add`）。
- **验证：** `cargo check -p supertask`；用 knife4j 父目录 `workspace.open` 有 yaml 或 `NO_YAML`。

### B2. 补齐 IPC 命令与事件桥

- **Skills：** `react-perf` → `client-event-listeners.md`
- **做：** 按 `ipc.md` 注册剩余 command（yaml.*、runtime.*、logs.*、script.*、toolchain.probe、soon 命令返回 `FEATURE_SOON`）。前端 `src/ipc/protocol.ts` 已有名字则不要再造一份。
- **事件：** `listen("st.runtime")` / `listen("st.logs")` 只在一个 Provider 里订阅；重连：先 snapshot 再 subscribe。
- **验证：** 未 subscribe 时不狂推；`FEATURE_SOON` 的 `templates.list` 有 code。

### B3. Runtime / Logs / Workspace Provider

- **Skills：** `composition` → `state-context-interface.md` `state-lift-state.md` `state-decouple-implementation.md` · `react-perf` → `rerender-derived-state.md` `rerender-defer-reads.md`
- **做：**
  - `WorkspaceProvider`：open/close/forget、当前 `workspace_id`
  - `RuntimeProvider`：snapshot merge + `st.runtime`；actions：`startOne(id)` 等只传 id
  - `LogsProvider`：环形条、`subscribe`、按 source 过滤；seq 去重
- **不要：** 三个 Provider 互相 import 实现；UI 只 `use(RuntimeContext)`。
- **验证：** 无工作区时 live 页显示引导，不崩。

---

## Phase C — 欢迎与工作区

### C1. Welcome 页

- **Skills：** `shadcn/composition`（Empty、Button、Card）· `webapp-testing`
- **做：** 添加工作区（dialog）→ 无 yaml 则进扫描向导（C2 可先 toast「去扫描」）；最近列表；forget 不删盘。`app.load.restoreLast` 为真且路径仍在 → 直达 `/run`。
- **验证：** 规格 §16 场景 1。

### C2. 扫描向导

- **Skills：** `shadcn/forms` · `shadcn/composition`（Dialog）
- **做：** `workspace.scanDraft` → 列出 spring/node；可改端口；确认后 `yaml.saveText` 或引擎写草稿（**不在 UI 拼 mvn 命令**）。警告：子模块提示打开父工程（core 已有文案）。
- **验证：** 规格场景 2（可用临时 pom 树，不必上 knife4j）。

---

## Phase D — 运行页（核心循环）

### D1. 服务卡片网格

- **Skills：** `composition` → `architecture-avoid-boolean-props.md` `architecture-compound-components.md` · `shadcn`（Card、Badge、Button、Spinner）· `shadcn/icons` · `react-perf` → `rerender-no-inline-components.md` `rerender-transitions.md`
- **做：** 每服务一张卡：状态点、id、kind、port、pid、时长、健康。按钮按 **state 派生可用性**，不要 8 个 boolean props。点击非按钮 → 打开抽屉（选中 id 在 Runtime 或独立 Selection context）。
- **状态色：** running `#27A644` / unhealthy 黄 / exited `#DC2626` / stopped 灰；色+文字双通道。
- **验证：** 卡片随 `st.runtime` 变；startOne 立即 Starting，不转圈卡死 invoke。

### D2. 起停接 Engine

- **Skills：** 无新 UI skill
- **做：** 启动全部 / 停止全部 / 重启；错误 `IpcFailure.code` 映射规格 §15 中文。`MISSING_TOOL` 不得显示「已运行」。
- **验证：** knife4j 父仓 + 手写/扫描 yaml；`/v3/api-docs` 200；停止后 8080 无监听。规格场景 3、5、6、10。

### D3. 服务抽屉 Tab

- **Skills：** `shadcn/composition`（Tabs 必须 TabsList）· `shadcn/forms`（环境 Tab）
- **做：** 日志 / 环境 / 健康 live；终端/指标/容器/代理 **可见禁用 + 即将版本**。环境改 port 保存 yaml（`base_hash`）+ 黄条「未重启」。
- **验证：** 改端口后健康 URL 用新端口（场景 4）；soon Tab 点不进假终端。

---

## Phase E — 日志

### E1. LogView 复合组件

- **Skills：** `composition` compound · `react-perf` → `rendering-content-visibility.md` `rerender-use-ref-transient-values.md` `client-event-listeners.md` · `shadcn` ScrollArea
- **做：** 等宽、跟随/暂停/清屏（`logs.clearView` 只清环）。批次渲染 `st.logs`；snapshot 补洞。单行截断由引擎保证。
- **禁止：** 每行 invoke；把整文件当 invoke 返回。
- **验证：** Maven 输出可见；暂停后不自动滚；清屏文件仍在 `.supertask/logs`。

### E2. `/logs` 页

- **Skills：** `shadcn` · `web-guidelines`
- **做：** 左服务列表 + 右同一套 LogView；从 run「查看日志」带 query/state 预选 id（不要用第二套历史栈存抽屉）。
- **验证：** 切换服务右栏换源；无搜索（可放「即将 1.2」提示）。

---

## Phase F — 配置

### F1. 表单 Tab

- **Skills：** `shadcn/forms`（FieldGroup/Field，禁止 space-y）· `shadcn/composition`
- **做：** name、env 表、服务增删、depends_on。保存 `yaml.saveForm` + `base_hash`。冲突 `YAML_CONFLICT` 提示刷新。
- **验证：** 表单改 port 后 runtime 健康跟着变（需重启）。场景 4、7（未知字段：表单保存至少进 extra）。

### F2. 原文 YAML Tab

- **Skills：** `shadcn` Textarea · 可后期 CodeMirror，1.0 允许 textarea
- **做：** `yaml.get` 展示原文；`yaml.saveText`。解析失败标行。脏切换确认（AlertDialog）。
- **验证：** 加 `gateway: {}` 原文保存不丢。场景 7。

---

## Phase G — 环境 / 设置 / 脚本

### G1. `/env`

- **Skills：** `shadcn` · `composition` explicit variants（ProbeBar 不是 `isSoon` 装安装器）
- **做：** 复用 statusbar 的 probe；下方 ComingSoon 安装 1.2。不要 `toolchain.install` 假成功。

### G2. `/settings`

- **Skills：** `shadcn/forms` · `shadcn/composition`
- **做：** 常规 restoreLast、外观（1.0 light 实装，dark 开关可见但说明 2.x）、关于版本；工具链/代理/更新/账号 **分组标题 + 即将**，不展开假控件。

### G3. 脚本 bootstrap

- **Skills：** 复用 LogView
- **做：** `script.run` 只传 id；忙时 `SCRIPT_BUSY`。场景 9。

---

## Phase H — 收口

### H1. 错误目录 UI

- **Skills：** `shadcn` Alert + sonner
- **做：** 覆盖 `ipc.md` §7 / 规格 §15 全部 code；`WEBVIEW2` 给安装说明（若能探到）。

### H2. 对照规格第 16 节十条

- **Skills：** `webapp-testing` · executing-plans 停在批次边界
- **做：** 用 knife4j-next 父工程打 3/5/10；用临时仓打 2/4/7。
- **验证：** 清单全过才算 1.0 UI 完成。

### H3. 最终 UI 审查

- **Skills：** `web-guidelines` · `ui-ux-pro-max` · `shadcn/styling`
- **做：** 无 emoji、无 `space-y-*`、无 raw `bg-blue-500`、无自造 Empty。

---

## 明确不做（计划内拒绝）

- Next.js、Vue、Electron、在 UI 拼 cmdline  
- 1.0 深色实装、可拖拽导航、插件 API、日志搜索导出、PTY、CPU 图  
- 为 soon 功能写假数据  
- 新建 `src/components/index.ts` 大桶文件  

---

## 建议批次（executing-plans）

| 批次 | 任务 | 停下来看 |
|------|------|----------|
| 1 | A1–A3 | 壳像不像 H |
| 2 | B1–B3 | IPC 是否全是 id |
| 3 | C1–C2 | 欢迎/扫描 |
| 4 | D1–D3 | 能起停 knife4j |
| 5 | E1–E2 | 日志 |
| 6 | F1–F2 | YAML |
| 7 | G1–G3 + H1–H3 | 出货清单 |
