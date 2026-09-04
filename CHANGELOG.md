# Changelog

All notable changes to SuperTask are documented here.

## [0.1.3] - 2026-09-04

### Features

- 发现页布局整体升级：
  - 顶部只保留右上角一个「从 README 导入」入口，移除提示横幅内的重复按钮，说明文案并入按钮悬浮提示；进程 / 工作区匹配 / 端口冲突 / 工作区端口统计徽标与筛选行明确分行，层级更清晰。
  - 「其他监听进程」不再单独一张卡片，而是并入主表格成为表内可折叠分组行（点击展开 / 收起，状态本地记忆）：与开发进程共用同一个吸顶表头，所有列严格对齐，彻底解决此前两表列宽错位、无表头的问题。
  - 表格改为固定列宽布局，任何窗口宽度下都不会被长内容撑出容器产生横向滚动；进程名、工作目录、工作区匹配等长内容截断显示省略号，悬浮可查看完整内容；PID / CPU / 内存等数值列不再折行。
  - 监听端口列最多展示前 2 个端口胶囊，其余合并为「+N」，悬浮显示全部端口号，多端口进程行高不再膨胀。
  - 排序体验优化：当前排序列在表头以 ↓ 标记，排序按钮激活时高亮显示；CPU 降序在首个采样周期（CPU 尚无读数）自动按内存降序兜底，内存降序亦反向兜底，保证每次点击排序后行序都有可见变化。

### Fixes

- AI CLI 代理（Windows）：修复通过 npm 安装的编码 CLI（实际为 `.cmd` shim，如 `cursor-agent.cmd`）无法启动、报 "program not found" 的问题。现在 spawn 前按 PATH + PATHEXT 顺序解析真实可执行文件；解析不到时保持原名，由系统报原生错误。`.bat` / `.cmd` 由标准库安全转义后经 `cmd.exe` 执行，参数不会被拼接进 shell。
- cursor-cli 供应商预设的程序名由 `agent` 修正为 `cursor-agent`，前端供应商预设同步更新。
- AI 配置对话框补齐「清除 Key」按钮的四语言文案；统一繁体中文界面中 AI 相关术语（连线与认证、本地 CLI、探测等）。
- Select 下拉组件移除列表上下的滚动箭头按钮，长列表滚动更简洁。

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
