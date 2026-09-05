# ADR-0002: Tauri 2 + Rust + React + TypeScript

## Status

Accepted。产品确认于 2026-08-25。2026-08-26 已脚手架 Tauri 2 + React + Vite + shadcn。

## Context

第一稿（ADR-0001）选了 Wails v2 + Go + Vue，理由是进程引擎用 Go 更快。第二轮要求：

- 前端是否应改为更主流的 React + TS  
- 后台 Go vs Rust 按三年期评估  
- 壳层做完总体分析再推荐  
- 产品名 SuperTask；Spring 1.0 只 `run`  

后期大量能力（git/docker/mise/nginx）将以 **spawn CLI** 实现，不链 Docker SDK。壳和引擎若语言不同，只能 sidecar，1.0 不接受。

## Decision

采用一套栈：

- 桌面壳：**Tauri 2**  
- 引擎：**Rust**  
- 前端：**React + TypeScript + Vite + shadcn/ui**  
- 产品名：**SuperTask**  
- Spring 1.0：`mvn -pl … spring-boot:run`；`package`+jar 放到 1.x  

否决 Electron、否决 Tauri+Go sidecar、否决 Vue 作为 1.0 前端。

若未来明确拒绝 Rust，整栈改 Wails 2 + Go + React，而不是在 Tauri 里挂 Go。

## Consequences

### Positive

- 和 2026 主流轻量桌面路径一致（Tauri + React）  
- 自动更新、权限模型、跨平台与前端生态一条线  
- 无 sidecar，签名和进程生命周期简单  
- 前端组件与以后的云控制台可同技术  

### Negative

- 1.0 比 Go+Wails 慢（预估数周级，不是数月）  
- Windows Job Object 要自己写测试，社区帖子少于 Go  
- 团队要能维护 Rust  

### Neutral

- Docker 后期用 CLI，放弃 Go 生态这张牌  
- 移动端 Tauri 用得上的概率低，不当决策理由  

## Alternatives Considered

**Wails 2 + Go + React**  
1.0 更快、Job Object 资料多。桌面插件与长期生态弱于 Tauri；前端已定 React 后，Wails 的相对优势只剩 Go。作为唯一官方备选。

**Wails + Go + Vue（ADR-0001）**  
前端与主流桌面拼装不一致。取代。

**Electron + React**  
轻量需求不满足。

**Tauri + Go sidecar**  
双进程、双日志、双发布。拒绝。

## References

- [docs/research/2026-08-25-stack-evaluation.md](../research/2026-08-25-stack-evaluation.md)  
- [docs/plans/2026-08-25-product-roadmap.md](../plans/2026-08-25-product-roadmap.md)
