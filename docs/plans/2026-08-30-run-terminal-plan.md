# 运行页终端（PTY）实现计划与执行记录 — 2026-08-30

## 背景与拍板

- 界面设计文档（2026-08-26）将运行页服务抽屉「终端」Tab 标注为「1.5 PTY」占位，v1.2 规格把
  「PTY 终端」延后，run-page 内长期以 locked「待排期」chip 展示；v1.5 实际交付为 CLI/MCP/导出包，
  PTY 终端从未进入任何版本路线。
- **2026-08-30 用户点名实现**（「运行模块终端功能请实现，注意尽可能复用现成的组件框架」），视为
  拍板：终端 Tab 转正，locked 占位移除。范围：**服务终端**（cwd/环境随服务），不做终端复用/分屏、
  不做工作区级独立终端入口（后续可加）。

## 选型（复用核查）

| 层 | 选择 | 理由 |
|----|------|------|
| PTY | `portable-pty` 0.9（wezterm 系） | Windows ConPTY / Unix openpty 成熟封装、活跃维护；不裸写 ConPTY FFI |
| 前端渲染 | `@xterm/xterm` 6 + `@xterm/addon-fit` + `@xterm/addon-web-links` | 事实标准（VS Code 同源）；自动回应 ConPTY 启动 `\x1b[6n` DSR 握手 |
| 事件 | 复用 `st.*` 信封模式，新事件 `st.term` | 与 `st.logs`/`st.runtime` 桥线程同模型（壳层轮询 mpsc → emit） |
| 环境链 | 复用 `build_service_env` / `resolve_cwd` / `network::inject_env` | 服务终端与启动同 cwd、同 §6.3 环境链 + 1.7 §7 镜像注入，零新逻辑 |

被否选项：自研 ConPTY FFI（维护成本）；`tauri-plugin-shell`（无 PTY/交互流）；node-pty + sidecar
（违背「不要 sidecar」拍板）。

## 设计要点

1. **会话是 UI 作用域**：`PtyManager` 在壳层托管（`Arc`），不进 Engine 工作区状态机、不占工作区
   锁；随 Tab 挂载 open、卸载 close；应用退出 `request_exit` 先 `close_all` 再 `engine.close()`。
2. **UI 永不拼 cmdline**：终端程序由后端 `default_shell()` 决定（Windows PowerShell 优先 `-NoLogo`
   回落 COMSPEC；Unix `$SHELL` 回落 bash/sh），前端只传 `serviceId`/尺寸。
3. **进程树终止**：ConPTY 句柄关闭即终止其上进程树，无需 Job Object（终端是用户交互进程，与
   服务 Job Object 托管语义不同）。
4. **上限 8 会话**（`TERM_LIMIT`）；输出 lossy UTF-8 经 `st.term` 流式推送，16KiB 分块。
5. ConPTY 退出瞬时输出可能丢失（conhost 不冲刷）——冒烟测试走交互路径（`/k` + 写入命令）规避，
   真实 xterm.js 长会话不受影响。

## 交付清单

- core：`src/term.rs`（PtyManager + TermEvent + default_shell + `#[ignore]` 真机 ConPTY 冒烟）、
  `engine::term_target`、错误码 `TERM_SESSION_NOT_FOUND`/`TERM_SPAWN_FAILED`/`TERM_LIMIT`、
  ipc `term.open/write/resize/close` + `st.term` + `TermOpenOutput`/`TermEventPayload`。
- 壳层：`src-tauri/src/term.rs`（四命令 + `st.term` 桥线程）、退出清场、退出中拦截复用。
- 前端：`components/terminal-view.tsx`（xterm + Fit + ResizeObserver 防抖 resize + 重开态）、
  运行页「终端」Tab（locked 占位移除，`lockTerminal`/`unscheduled` locale 键删除）、mock 假 shell
  （help/echo/pwd/ls/dir/ver/date/clear/exit，事件序列与真链路同形）、四语 locale +6 键。
- 文档：ipc.md §10.15、本计划、repository conventions、inventory 同步。

## 偏差备案

1. 界面设计文档「终端 = ComingSoon（1.5 PTY）」与本次交付冲突——以 2026-08-30 用户点名为准，
   设计文档不再回改（dated 存档规则）。
2. ConPTY `\x1b[6n` 启动握手：spec 层面无描述；实现记录于 ipc.md §10.15（xterm.js 自动回应，
   冒烟测试手工代答）。
3. 会话不随工作区 close 联动关闭（仅 UI 卸载 + 退出清场）——切换工作区时前端 Tab 卸载会自然
   关会话；若后续出现「关工作区留终端」的真实需求再挂 workspace.close 钩子。

## 验收状态

- core 425 单测全绿（新增 3：shell 选择 / 会话缺省错误与幂等 / 真机 ConPTY 冒烟 opt-in ignored）
  + CLI 20 全绿；`cargo check -p supertask` 通过。
- `npm run build` 通过；locale parity 1061 keys（+6 / −2）。
- 真机 ConPTY 冒烟（本机 Windows）：打开 → DSR 握手 → echo 回显 → exit → 清场，全链路绿。
- 浏览器 mock：Playwright 实测终端 Tab（xterm 挂载、help/echo 回显、提示符、无 console 错误）。
- 剩余：`npm run tauri dev` 真机 GUI 人工验收（PowerShell 交互、窗口缩放、Ctrl+C、长输出滚动）。
