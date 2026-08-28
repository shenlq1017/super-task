# SuperTask

Windows 优先的轻量桌面工作台：用 YAML 可视化启停 Spring Boot 多模块与 Node 服务。

当前：引擎在 `crates/supertask-core`；桌面壳已脚手架（`src/` + `src-tauri`），业务页按前端计划实现。

```text
cargo test -p supertask-core
npm run dev
npm run tauri dev
```

- YAML 规范：[docs/spec/yaml.md](docs/spec/yaml.md)
- IPC 契约：[docs/spec/ipc.md](docs/spec/ipc.md)
- 架构：[docs/spec/architecture.md](docs/spec/architecture.md)
- 前端计划：[docs/plans/2026-08-26-frontend-work-plan.md](docs/plans/2026-08-26-frontend-work-plan.md)
- 给 agent：[AGENTS.md](AGENTS.md)
