# SuperTask 测试工作区（examples/）

每个子目录都是一个可直接用 SuperTask 打开的独立工作区（含 `supertask.yaml`），按版本文艺矩阵覆盖各类服务形态。桌面端「打开工作区」指向对应目录即可；CLI 用 `supertask status <目录>` / `supertask up <目录>`。

> 这些目录里的构建产物（`target/`、`node_modules/`、`.supertask/`）已 gitignore；样例源码本身很小，可随仓库提交。

## 工作区一览

| 目录 | 覆盖场景 | 前置要求 |
|------|----------|----------|
| `spring-multi/` | Maven 多模块 Spring Boot：扫描草稿、拓扑启动（depends_on）、http/tcp 健康、改端口跟随、脚本、launch jar、网关反代对象 | JDK 17+、Maven（首次构建需联网拉依赖） |
| `node-demo/` | Node 基线：启停、日志流、http 健康、脚本任务；`/api/slow`、`/api/fail` 可模拟慢启动与 500 | Node（零 npm 依赖，离线可跑） |
| `gateway-demo/` | **1.6 网关**：nginx/caddy/apache 三引擎、host/path 路由、转发头核对、本机 HTTPS + trust | Node 必需；nginx/caddy/apache 按需装（`supertask doctor` 可探测） |
| `compose-demo/` | 1.3 容器：compose up/stop、ps/images、`demo-node` 镜像构建 | Docker Desktop（首次需联网拉镜像） |

## 各工作区测什么

### spring-multi/（Spring Boot 多模块）

- `user-api`（:8081，http 健康 `/actuator/health`）← `order-api`（:8082，tcp 健康，depends_on user-api）。
- 测试点：「扫描」应识别出两个模块草稿；启动全部按拓扑先 user 后 order；运行页改 user-api 端口并重启后健康跟随；脚本 `build-all` 走 `mvn package`；把服务 `launch` 改 `jar` 可测 1.2 构建 jar 链路。
- `GET /api/user/ping` 回显 `Host` / `X-Forwarded-For` / `X-Forwarded-Proto` / `X-Real-IP` —— 把 `gateway:` 段加进本工作区后可作为网关反代的真实 Spring 上游。

### node-demo/（Node 零依赖基线）

- `web`（:5173）与 `api`（:9001，http 健康 `/health`）。
- 每个服务内置调试路由：`/health`、`/api/echo`（回显请求头）、`/api/slow?ms=N`（慢响应，测停止/健康超时）、`/api/fail`（500，测日志）。
- 脚本 `hello` 验证 `script.run` 链路与日志采集。

### gateway-demo/（1.6 网关主战场）

默认路由表：`/api → api`（catch-all path）、`api.localhost / → api`（host 分组）、`/ → web`（根兜底）。

1. `supertask doctor` 确认 nginx/caddy/apache 探测状态（缺失给平台安装指引，不代装）。
2. `/gateway` 页：从服务生成草稿 → diff 确认 → 应用 → 「本机校验」→ 启动。
3. 浏览器访问 `http://127.0.0.1:8080/`（web 页面）与 `http://127.0.0.1:8080/api/echo`（看 `X-Forwarded-*` 是否透传）；`http://api.localhost:8080/` 验证 host 分组（浏览器对 `*.localhost` 直连 127.0.0.1）。
4. 三引擎切换：`/gateway` 页把 kind 换成 `caddy`（可开 `tls: internal`，浏览器访问 `https://localhost:8080`，首次点「信任本机 CA」）或 `apache`（注意 apache 不转发 WebSocket）。
5. CLI 联动：`supertask up --wait healthy` 应先服务后网关；`supertask status --json` 出现 `gateway` 行；`supertask down` 网关一并清场（无进程残留）。

### compose-demo/（容器）

- `redis`（6379）与 `whoami`（8083，`traefik/whoami`）两个 compose 服务；容器页可见容器 ID/状态。
- `docker.builds` 的 `demo-node`：运行页/容器页触发构建，产物 tag `demo-node:local`（Dockerfile 构建零依赖 Node 服务）。

## 注意

- SuperTask 对 spring/node 服务分别注入 `SERVER_PORT` / `PORT` 环境变量，样例服务全部跟随该端口，可直接测试「改端口后健康检查跟着变」。
- 端口若与本机已占用端口冲突，直接在 SuperTask 里改端口（运行页一键改端口）即可。
- 测试产生的运行时文件都在各工作区 `.supertask/` 下，可整目录删除重置。
