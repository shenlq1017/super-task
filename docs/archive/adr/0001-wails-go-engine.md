# ADR-0001: 用 Wails v2 + Go 做 1.0 桌面壳与进程引擎

## Status

Superseded by [ADR-0002](0002-tauri-rust-react.md)

第一稿选 Wails + Go + Vue。第二轮把前端定为 React+TS，并按三年桌面产品重选壳层后作废。

## Context

产品 1.0 是 Windows 本机多服务启动台。核心负载是：拉起 Maven/Node、接住 stdout、按依赖启动、杀掉整棵进程树、把状态推到 UI。

同时要求轻量、界面精美、以后跨平台。

候选：Electron、Tauri 2 + Rust、Wails 2 + Go、纯 Go 托盘 + 浏览器打开 localhost。

## Decision

1.0 采用 **Wails v2 + Go 引擎 + Vue 3 前端**。  
配置格式用自有 `supertask.yaml`，不兼容 Taskfile / compose。  
1.0 只 **探测** JDK/Maven/Node，不安装。

## Consequences

### Positive

- 安装包和内存对得上「轻量」
- 进程与 Windows Job Object 和 UI 绑定在同一 Go 进程，少一条 sidecar
- Vue 足够做精美界面，不必上 Electron

### Negative

- 团队要写 Go；若完全不会，学习成本高于 Electron
- Wails 3 更现代但未选（求稳）
- 自有 YAML 意味着不能直接复用现成 Taskfile 生态

### Neutral

- 以后若做移动端，Tauri 更顺；那是 2.x 的问题
- 纯浏览器 UI 更快出原型，但「打开目录 / 托盘 / 像个产品」会补一遍壳

## Alternatives Considered

**Electron**  
体积和内存与需求相反。否决。

**Tauri 2 + Rust**  
壳很好。进程引擎用 Rust 可行，但 Windows 进程树 + 日后 git/docker CLI 封装，Go 更省事。若坚持 Rust 可整体换，不要 Go sidecar 叠 Tauri。

**Go + 系统浏览器**  
原型可以。正式 1.0 仍建议 Wails，避免「这是个网页」。

**兼容 Taskfile 或 compose**  
表面省事，模型不对。Task 没有长期 service；compose 假设容器。做导入器比做兼容层便宜。
