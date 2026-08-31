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
- 云客户端协议：[docs/spec/cloud.md](docs/spec/cloud.md)
- 云参考服务配置：[docs/spec/cloud-server.md](docs/spec/cloud-server.md)
- 给 agent：[AGENTS.md](AGENTS.md)

## 本地云参考服务（开发状态）

仓库包含独立 crate `crates/supertask-cloud-server`，用于自托管协议联调。配置、认证/实体/管理员
数据层、SQLite migration、HTTP router/API、healthz、配额/遥测、`/admin/api/*` 管理面与自带 Web
控制台、本地集成测试均已实现；正式部署和真机验收仍未完成。下面是本地开发入口：

```text
cargo run -p supertask-cloud-server
```

默认配置：`SUPERTASK_BIND=127.0.0.1:8787`、`SUPERTASK_DATABASE_URL=sqlite://supertask-cloud.db`。
开发 seed 只有设置 `SUPERTASK_DEV_SEED=1`（或 `true`）才启用，并必须在运行时注入非空
`SUPERTASK_SEED_PASSWORD`；可选设置
`SUPERTASK_SEED_EMAIL`，默认邮箱为 `demo@supertask.local`。不要把密码写入 README、源码、迁移
SQL、日志、shell 脚本或提交文件。完整配置、数据库约束和未实现边界见
[docs/spec/cloud-server.md](docs/spec/cloud-server.md)。

### 云管理控制台

服务端在 `/admin/` 自带一个账号管理控制台（前端在 `cloud-console/`，独立构建、不嵌进二进制）。
启用需要在**运行时**同时注入管理员邮箱与口令（缺其一即配置解析失败，不提供默认口令）：

```text
npm run build:console
SUPERTASK_ADMIN_EMAIL=<operator email> SUPERTASK_ADMIN_PASSWORD=<runtime value> \
  cargo run -p supertask-cloud-server
```

然后浏览器打开 `http://127.0.0.1:8787/admin/`。改前端时的开发模式：`npm run console:dev`
（`127.0.0.1:1430`，把 `/admin/api` 代理到 `8787`）。范围只有账号生命周期与角色；会话吊销、
实体内容浏览、每账号配额覆盖和审计表本期都没有。

两端一起拉起（Windows）：在仓库根目录执行 `powershell -ExecutionPolicy Bypass -File .\start-cloud.ps1`。
脚本各开一个窗口跑服务端与控制台 dev，管理员邮箱/口令不在环境里时交互输入（口令不回显，脚本不落盘），
支持 `-ServerBind`、`-AdminEmail`、`-NoBrowser`；关掉任一窗口另一端一并清理。

