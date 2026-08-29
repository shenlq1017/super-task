# SuperTask 1.6 功能规格

> 日期：2026-08-29  
> 状态：草案——默认决策已给出（§16），待确认后转「待实现」（前置：1.5 已交付，自动化范围全落地）  
> 上位文档：[产品路线](2026-08-25-product-roadmap.md) · [1.4 功能规格](2026-08-28-v1-4-feature-spec.md) · [1.5 功能规格](2026-08-29-v1-5-feature-spec.md) · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md) · [引擎架构](../spec/architecture.md)

本文把路线中的 1.6「能对外」收到可实现、可测试、可交付的粒度。1.6 的主题是**网关**：把 `gateway:`（自 1.0 起 reserved 的顶层段）变成一等能力——**路由模型 → 反代配置生成 → 本机校验 → 引擎托管进程 → 可视化页面**。覆盖三家反代引擎：**nginx**（核心，一等公民）、**Caddy**（开发用一键本机 HTTPS）、**Apache**（简化支持，最小反代集，为后续 PHP 场景铺路）。

一句话：服务的端口是事实，路由是意图；SuperTask 负责把意图编译成对应反代引擎的配置文件，校验后像服务一样托管它。

## 1. 目标与边界

### 1.1 产品目标

1. **能对外**：一个监听端口，按 host/path 把请求转发到工作区内的服务（或显式 upstream），前端通过网关访问后端不再记端口。
2. **能可信**：生成的配置必须先过本机校验（`nginx -t` / `caddy validate` / `httpd -t`）才能启动；校验失败原文透出，不静默带病运行。
3. **能托管**：网关进程享受与 service 同级的生命周期——进程树终止无残留、日志批次、TCP 健康、指标、端口占用排除、`stop_all`/CLI `down` 纳入清场。
4. **能看见**：`/gateway` 从 ComingSoon 转为 live 页面：路由可视化（host/path → 目标服务 → 上游存活）、配置预览、校验结果、启停操作、Caddy HTTPS 状态与信任指引。
5. **能 HTTPS**：`kind: caddy` 时借助 Caddy 内置 local CA 为 `localhost` 提供浏览器信任的本机 HTTPS（开发场景，零公网依赖）。

### 1.2 版本范围

| 能力 | 1.6 行为 |
|------|----------|
| 反代引擎 | nginx（全量路由能力）、caddy（同路由能力 + 本机 HTTPS）、apache（同路由能力，最小 mod_proxy 集） |
| YAML | 顶层 `gateway:` 段转正为 typed 配置（§4）；`version: 1` 不变 |
| 路由模型 | host（可空=全匹配）+ path 前缀 → target（service id）或 upstream（显式 host:port） |
| 配置产物 | 生成到 `<root>/.supertask/gateway/`（运行时产物，随 `.supertask/` 一起 gitignore） |
| 校验 | spawn CLI：`nginx -t -c … -p … -e stderr` / `caddy validate --config … --adapter caddyfile` / `httpd -t -f …`；退出码 + stderr 原文 |
| 托管 | engine 新增 gateway 托管 slot（`Arc<dyn ProcessTree>`，非 services 成员）；`up`/`down`/`stop_all`/锁/清场全纳入 |
| 探测 | `toolchain.probe` 输出 `gateway: { nginx, caddy, apache }`（found/version/path）；不代装 |
| IPC | protocol 1 不变；新增 `gateway.*` 命令组（§8），`gateway.apply` 从 SOON_COMMANDS 转正 |
| 平台 | 三平台（沿用 1.4 matrix）；CLI `up`/`status`/`down` 纳入网关 |
| 前端 | `/gateway` live；路由编辑 + 应用走 diff 确认；四语 |

### 1.3 明确不做

- **热重载**（`nginx -s reload` / caddy admin API）：路由变更 = 重新生成配置 + 重启网关 slot。热重载留 1.7+。
- **公网证书 / ACME / Let's Encrypt**：1.6 的 HTTPS 只有 Caddy internal CA（localhost 开发场景）。
- **负载均衡 / 多 upstream / upstream 探活 / 被动健康检查**：一条路由一个目标。
- **任意指令透传**：不提供 `extra_directives` 之类的原文拼接口（注入面大、校验失去意义）。用户的高级需求自己写配置文件，SuperTask 不拦。
- **Apache 深度定制**：LoadModule 集合、MPM、PHP-FPM（`mod_proxy_fcgi`）、mod_php 均不做——1.6 的 apache 只是最小反代集；PHP 场景是后续版本（预留见 §5.3）。
- **多网关实例 / 远程网关 / 非 loopback 监听**：一个工作区一个网关，只监听 `127.0.0.1`（caddy 为 `https://localhost`）。对外暴露不是本机工作台的场景。
- **hosts 文件管理**：`api.localhost` 这类子域依赖浏览器对 `*.localhost` 的内建解析（Chrome/Edge/Firefox 均直连 127.0.0.1），不改系统 hosts。
- **网关配置的 MCP 工具**：1.5 工具集冻结为 7 个起停/观察工具；Agent 需要网关时走 CLI `supertask up`。
- **导入已有 nginx/caddy/apache 配置**：只生成，不解析存量配置文件。
- **nginxWebUI 式的实例管理闭环**（conf 文件可手动编辑、配置历史版本回滚、独立证书管理块、参数模板注入、负载均衡为一等实体、远程/集群同步）：本机托管 +「yaml 是真源、生成物是缓存」的模型下这些是「nginx 实例运维工具」的职能，与「路由是编辑对象」冲突，1.6 不做、留 1.7+。取舍与借鉴界限见 §10。
- 云同步、账号（2.0）；AI（2.1）。

YAML `version: 1`、IPC protocol 1、app data v3 均不变。Windows 既有场景零回归仍是合入门槛。

## 2. 用户场景与成功标准

### 2.1 反代聚合访问（nginx）

1. 工作区有 `user-api`(8081)、`web`(5173)。用户在 `/gateway` 页选择 `kind: nginx`、监听 `8080`，添加两条路由：`/api → user-api`、`/ → web`。
2. 点「应用」：前端展示生成的 `nginx.conf` diff 确认 → 写回 yaml（`base_hash` 冲突 → `YAML_CONFLICT`）→ 生成配置到 `.supertask/gateway/` → `nginx -t` 校验 → 网关启动。
3. 浏览器访问 `http://127.0.0.1:8080/api/...` 命中 user-api；`:8080/` 命中 web 前端。路由表里两条路由的上游存活点为绿。
4. user-api 改端口（1.2 改端口写回）后再次启动网关：生成的 `proxy_pass` 指向新端口，无需手改任何 nginx 配置。

### 2.2 本机 HTTPS（caddy）

1. 用户选 `kind: caddy`、端口 `8443`，路由同上。生成的 Caddyfile 站点地址为 `https://localhost:8443`，`tls internal`。
2. 首次启动后页面提示「Caddy 根证书尚未信任」；点「信任」弹出确认（说明将修改系统信任库）→ 执行 `caddy trust` → 状态转绿。
3. 浏览器访问 `https://localhost:8443` 无警告，证书由 `Caddy Local Authority` 签发。

### 2.3 简化 Apache

1. 用户机器上有 XAMPP 的 httpd。选 `kind: apache`，路由同 2.1。生成的 `httpd.conf` 自包含（LoadModule 最小集 + ProxyPass）。
2. `httpd -t` 校验通过后启动；停止后无残留 `httpd.exe`/`httpd` 进程（进程树终止）。
3. 机器没有 httpd 时：`GATEWAY_BINARY_MISSING`，页面给出安装指引（不代装）。

### 2.4 生命周期与清场

1. 网关运行中，用户停掉全部服务再关窗口：网关随 `stop_all`/引擎退出终止，`tasklist`/`pgrep` 无 `nginx`/`caddy`/`httpd` 残留。
2. CLI：`supertask up` 在服务健康后启动网关；网关启动失败 → 停止全部、退出 1、stderr 列 `GATEWAY_*` 错误码；`supertask down` 停网关；`supertask status --json` 显示网关行。
3. 网关自身端口参与端口检查：网关运行中检查自己的监听端口 → 提示可用（当前由网关占用），沿用 1.2 的自身排除语义。
4. 桌面、CLI、MCP 三入口互斥沿用 1.5 锁：网关是引擎状态的一部分，锁语义不变。

### 2.5 路由校验错误说人话

1. 路由 target 填了不存在的服务 → 应用时 `GATEWAY_ROUTE_INVALID`，details 指明第几条路由、什么原因。
2. 路由目标服务没有配置 port → 同上（生成时无法解析 upstream）。
3. `nginx -t` 失败（如 8080 被外部占用——Windows 版 `nginx -t` 会真实 bind 监听端口）→ `GATEWAY_CONFIG_INVALID`，details 带 `[emerg]` 原文，页面红条展示。

## 3. 总体架构

```text
supertask-core
    ├─ spec/          gateway 段 typed 模型 + 校验（§4）
    ├─ gateway/       新模块：
    │    ├─ model     Route/GatewayConf 中间表示（与引擎无关）
    │    ├─ render    IR → nginx.conf / Caddyfile / httpd.conf（纯函数 + golden 测试）
    │    ├─ probe     三家二进制探测（PATH → 平台已知位置 → gateway.bin）
    │    └─ validate  spawn CLI 校验 + stderr/退出码归一化
    ├─ engine.rs      GatewaySlot 托管（spawn/stop/健康/日志/指标/端口排除/stop_all）
    └─ ipc/           gateway.* 命令类型（v16.rs）

src-tauri            gateway.* Tauri 命令（薄适配）
supertask-cli        up/down/status 纳入网关（复用 engine，零新逻辑）
frontend             /gateway live 页（路由编辑/预览/校验/启停/HTTPS）
```

- 网关**不是** services 成员：不参与 `depends_on` 拓扑、不参与 profile 切换、不被服务启停连带；它是引擎的平级托管对象（`GatewaySlot`），与 service slot 共享 `ProcessTree`、日志批次（source=`gateway`）、健康探测（TCP `gateway.port`）、指标与端口排除机制。
- 生成 → 校验 → 启动三段分离：`render` 是纯函数（可 golden 测试）；`validate` 只 spawn 只读命令；启动前必须校验通过（`gateway.start`/`up` 内部先跑同一条校验链）。
- 配置模型借鉴 [nginxconfig.io](https://github.com/digitalocean/nginxconfig.io)（MIT）的「domain → path 路由 → https 选项」抽象，按本机开发场景裁剪（无 certbot/dhparam/HSTS/生产安全头）；借鉴概念与命名，不引入其代码与依赖。

## 4. 路由模型与 YAML

### 4.1 typed 结构（yaml.md 同步更新）

```yaml
gateway:
  kind: nginx              # nginx | caddy | apache（必填；缺 gateway 段或 kind = 未配置）
  enabled: true            # 缺省 true；false = 配置保留但不启动（up/stop_all 均忽略）
  port: 8080               # 监听端口，缺省 8080
  bin: null                # 可选：二进制显式路径（探测的最终 fallback）
  routes: []               # 路由列表，见下
  tls: off                 # 仅 caddy 生效：off | internal（internal = 本机 CA HTTPS）

routes:
  - host: api.localhost    # 可空 = 全匹配（catch-all）；非空须为合法 hostname
    path: /api             # 必填，以 / 开头的前缀；'/' 为根
    target: user-api       # 服务 id → 生成时解析为该服务当前 port
    # 或 upstream: 127.0.0.1:9000   # 显式上游（与 target 互斥，二选一必填）
```

- `gateway: {}`（1.0 起 reserved 空段）语义不变：读回仍在、视为未配置（`GATEWAY_NOT_CONFIGURED`），旧文件零迁移。
- 校验（打开工作区时 warning，应用/启动时硬错误）：
  - kind 合法；port 1024–65535；routes 里 `(host, path)` 不得重复。
  - target 必须是已存在的 service id 且该服务有 `port`（或 ports）；target 与 upstream 恰有一个。
  - path 必须以 `/` 开头；host 为空或合法 hostname（含 `*.localhost` 形式子域）。
  - 违规 → `GATEWAY_ROUTE_INVALID`（details 带路由序号与原因）。
- 上游地址解析：target 服务生成时取其 `port`；监听地址选择复用 1.2 端口表——IPv4-only → `127.0.0.1:<port>`，仅 IPv6 监听 → `[::1]:<port>`，双栈/未运行 → `127.0.0.1:<port>`（页面在「仅 IPv6」时标注）。

### 4.2 与既有机制的关系

- 改端口（1.2 `ports.assign`）写回 yaml 后，下次网关启动/应用时路由自动指向新端口（配置生成永远以当前 yaml 为真源）。
- 网关监听端口参与 `ports.inspect`：网关 slot 的进程树在自身排除集合内（复用 1.2 `OwnRuntime` 语义）；网关停止时该端口照常判占用。
- profile（1.2）：gateway 段不进 profile overlay；profile 切换不影响网关配置。

## 5. 配置生成（render）

产物目录 `<root>/.supertask/gateway/`：`nginx.conf` / `Caddyfile` / `httpd.conf`。自包含（不 include 工作区外文件），随启动重新生成（磁盘产物是缓存，不是编辑对象）。

### 5.1 nginx

- 启动：`nginx -c <abs conf> -p <prefix> -e stderr`（prefix = `.supertask/gateway/`，nginx 相对路径基准）；校验同 argv 加 `-t`。
- 生成要点：`worker_processes 1`；`daemon off`（前台，进程树托管必需）；`pid`/`error_log`/`access_log` 指向 `.supertask/gateway/`；按 host 分组 server 块（空 host = default_server）；`listen 127.0.0.1:<port>`；location 按 path 前缀（最长优先）→ `proxy_pass http://<upstream>` + `proxy_set_header Host/X-Forwarded-For/X-Forwarded-Proto/X-Real-IP`；`proxy_http_version 1.1` + `Upgrade`/`Connection` 头透传（Vite HMR WebSocket 可用）；无匹配路由 → 404 server 块。
- **Windows 已知行为**：Windows 版 `nginx -t` 会真实 bind 监听端口，端口被外部占用时校验失败——这正是我们要的（提前暴露冲突），错误映射为 `GATEWAY_CONFIG_INVALID` 并在文案提示「端口被占用」。

### 5.2 Caddy

- 启动：`caddy run --config <abs Caddyfile> --adapter caddyfile`（前台）；校验：`caddy validate --config … --adapter caddyfile`。
- 生成要点：全局 `admin off`（避免 2019 端口冲突；停止走进程树，不经 admin API）；按 host 分组站点块；`tls: internal` 时站点地址 `https://localhost:<port>` 且块内 `tls internal`，`tls: off` 时 `http://localhost:<port>`；路由 matcher 为 path 前缀 → `reverse_proxy <upstream>`（Caddy 自动透传 Host 与 WebSocket 升级）。
- 信任：`caddy trust` 只由用户在页面显式确认后执行（修改系统信任库，需用户知情）；`caddy untrust` 同理由页面提供。首次启动后页面探测信任状态（`localhost` 证书已签发 + 询问用户是否信任过的启发式：执行 `caddy trust --help` 类轻探不做，改为展示证书路径与指引——实现计划细化，以不静默改系统为红线）。

### 5.3 Apache（简化集）

- 启动：Unix `httpd -DFOREGROUND -f <abs conf>`；Windows `httpd.exe -f <abs conf>`（无 -DFOREGROUND，父子进程由 Job Object 收编）。校验：`httpd -t -f <abs conf>`。
- 生成要点：自包含最小 `httpd.conf`——`ServerName localhost`、`Listen 127.0.0.1:<port>`、`ErrorLog`/`CustomLog` 指向 `.supertask/gateway/`、最小 `LoadModule` 集（`mpm`、`mod_proxy`、`mod_proxy_http`、`mod_headers`、`mod_log_config` 等，按三平台发行版默认模块路径约定生成；路径探测不到 → 校验失败原文透出）；按 host 分组 `<VirtualHost>`；`ProxyRequests Off` + `ProxyPreserveHost On` + `ProxyPass /path http://<upstream>/path` + `ProxyPassReverse`；WebSocket 转发（`mod_proxy_wstunnel`）不做，页面在 apache + 需 WS 的路由上提示降级。
- PHP 预留：路由模型不增加 php 字段；后续版本若做 PHP，走 `mod_proxy_fcgi` + `php.localhost` 站点，本规格不留半成品字段。

### 5.4 产物与运行时目录

```
<root>/.supertask/
    engine.lock        # 1.5
    logs/…             # 网关日志 source=gateway 也在此
    gateway/
        nginx.conf | Caddyfile | httpd.conf
        nginx-{access,error}.log / caddy 的 stdout 由日志泵接管
```

`.supertask/` 整体 gitignore（1.5 已建议，文档再确认）。

## 6. 校验与探测

### 6.1 校验链（启动/应用前置）

1. 路由静态校验（§4.1，core 纯逻辑）→ `GATEWAY_ROUTE_INVALID`。
2. 二进制探测（§6.2）→ `GATEWAY_BINARY_MISSING`（details 带引擎名与建议安装方式，不代装）。
3. 生成配置 → spawn 校验命令 → 非零退出 → `GATEWAY_CONFIG_INVALID`（details 带 stdout/stderr 原文，前端红条完整展示）。
4. 通过后才允许 spawn 网关进程；spawn 失败/立即退出 → `GATEWAY_START_FAILED`。

校验是只读命令（不常驻），超时 10s；三平台同一链路。`gateway.validate` 单独暴露给页面「校验」按钮（对磁盘上当前生成物或临时生成物执行，不启动）。

### 6.2 二进制探测

- 解析顺序：`gateway.bin`（显式）→ PATH → 平台已知位置（Windows：常见 zip 解压目录不做注册表扫描，只查 PATH 与显式 bin；macOS：`/opt/homebrew/bin`、`/usr/local/bin`；Linux：`/usr/sbin`（nginx/httpd 常见）、`/usr/bin`、snap bin）。
- 版本：`nginx -v`（注意输在 stderr）、`caddy version`、`httpd -v`。
- `toolchain.probe` 输出增 `gateway: { nginx: {found,version,path}, caddy: {…}, apache: {…} }`（结构对齐 1.4 的 `gradle` 项）。
- **不提供安装 provider**：能探测就别安装（路线原则）；缺失时页面给平台对应的一句指引（winget/brew/apt 包名）。

## 7. 引擎托管生命周期

- `GatewaySlot`：`Arc<dyn ProcessTree>` + RtState（复用 service 状态机：Starting→Running/Unhealthy→Exited）+ 日志泵（source=`gateway`，走 `st.logs` 批次；GBK/UTF-8 解码与 ANSI 剥离复用既有逻辑）+ TCP 健康（`127.0.0.1:gateway.port`，双栈回退同 1.2）+ 指标（进程树聚合）。
- 启动：`engine.gateway_start()` = 校验链 → render → spawn → Starting；健康达标 → Running。启动前若 target 服务未运行：**不阻塞**（网关不拉起上游，转发目标不达是上游的事），但页面路由表显示上游灰点；`up`（CLI/命令面板「全部启动」）顺序为先服务后网关。
- 停止：进程树终止（SIGTERM/宽限/SIGKILL；Windows Job Object），与 service 完全同语义；`stop_all`/`down`/引擎退出（含 CLI `up` 结束清场、MCP 断连清场）一律包含网关。
- 重启/应用：`gateway.apply`（§8）在网关运行中 = 重写 yaml → 重新生成 → 重启 slot；未运行 = 只落盘不启动。
- 锁：网关属引擎状态，1.5 锁语义零变化。
- 端口：网关 slot 进程树计入 `ports_inspect` 的自身排除（`OwnRuntime`）；网关监听端口与服务端口重复在路由校验时即拒绝（`GATEWAY_ROUTE_INVALID`：gateway.port 不得与任一服务 port 相同）。

## 8. IPC 契约增量

protocol 1 不变，新增命令组（ipc.md 增 §10.10）：

```text
gateway.status    { workspace_id } → { configured, kind, enabled, port, state,
                                       pid?, routes: [{host, path, target?, upstream?,
                                       target_port?, upstream_alive?}],
                                       conf_path?, trusted? (caddy) }
gateway.preview   { workspace_id, gateway: GatewayConf? } → { files: [{name, content}] }
                                        # 传配置则渲染草稿，不传用当前 yaml；纯内存，不落盘
gateway.validate  { workspace_id, gateway?: GatewayConf } → { ok, message?, stderr? }
gateway.apply     { workspace_id, gateway: GatewayConf, base_hash }
                                  → { spec, hash, restarted: bool, warnings }
                                        # 写 yaml（save_form 语义，YAML_CONFLICT 冲突）
                                        # + 重新生成 + 运行中则重启
gateway.start     { workspace_id } → { accepted }
gateway.stop      { workspace_id } → { accepted }
gateway.restart   { workspace_id } → { accepted }
gateway.trust     { workspace_id } → { accepted }   # 仅 caddy：spawn `caddy trust`（UI 需先确认）
```

- `gateway.status` 只读不取锁之外的写路径（沿用 1.5 语义：命令本身在持锁引擎上调用，无新锁语义）。
- `gateway.trust` 修改系统信任库：IPC 层照常暴露（本地单用户、无网络面），**UI 层强制确认对话框**；CLI 不提供 trust 子命令（不代用户改系统）。
- `session.hello`：`gateway` feature `soon → live`（since 1.6）；`gateway.apply` 移出 SOON_COMMANDS。
- 其余命令无结构变化。

## 9. 错误与安全要求

### 9.1 新增稳定错误码

| code | 触发条件 |
|------|----------|
| `GATEWAY_NOT_CONFIGURED` | 无 gateway 段 / 无 kind / enabled false 时执行启动类命令 |
| `GATEWAY_ROUTE_INVALID` | 路由静态校验失败（target 不存在/无端口、path/host 非法、重复、与网关端口冲突） |
| `GATEWAY_BINARY_MISSING` | 反代二进制未找到（details 带引擎名与平台指引） |
| `GATEWAY_CONFIG_INVALID` | 本机校验失败（details 带工具 stderr 原文） |
| `GATEWAY_START_FAILED` | 校验通过但 spawn 失败/进程立即退出 |

其余复用现有码（`YAML_CONFLICT`、`PORT_SCAN_FAILED`、`JOB_KILL`、`WORKSPACE_LOCKED`、`HEALTH_TIMEOUT` 等）。

### 9.2 安全边界

- 只监听 loopback（nginx/apache `127.0.0.1`；caddy `localhost`）——本机工作台不对局域网开面。要做对外暴露的用户自己写配置。
- 生成器只输出白名单指令集（§5），无用户原文注入点；`upstream` 值经 host:port 语法校验（拒绝 URL、userinfo、scheme）。
- 校验与启动只 spawn 固定 argv（`-c/-p/-e`、`--config --adapter`、`-t -f`），不透传 shell；`gateway.bin` 是路径值，经沙箱同样的 canonicalize 规则校验存在即可执行。
- `caddy trust/untrust` 修改系统信任库：仅 UI 显式确认后执行；失败原文透出；不做任何静默信任。
- 网关日志进批次存储，脱敏规则与 service 日志一致；生成物中的 upstream 不含密钥（路由模型无凭据字段，需要鉴权的头由用户服务自己处理）。
- 不扫描注册表/不猜测安装路径：探测只有 PATH + 已知位置 + 显式 bin 三层。

## 10. 开源复用清单（选型纪律沿用 1.4 §6.3 / 1.5 §10）

| 用途 | 结论 | 说明 |
|------|------|------|
| 配置模型借鉴 | [nginxconfig.io](https://github.com/digitalocean/nginxconfig.io)（MIT，DigitalOcean） | 借鉴其 domain→routing→https 抽象与产物组织（「生成一份正确默认值的单站点配置」）；**不引代码不引依赖**（Vue 前端项目，无可复用 Rust/TS 库）；用户文档致谢 |
| 可视化闭环借鉴 | [nginxWebUI](https://github.com/cym1102/nginxWebUI)（Java + layui，GPL 声明，作者 cym1102） | 借鉴其「**配置分块 CRUD → 生成 conf → 校验 → 覆盖/重载 → 状态/日志**」的交互闭环与信息架构（http 参数 / 反向代理 / 负载均衡 / 证书 各自成块 + 表格式增删行 + 「从常用配置起步」）；open 版 GPL 与封闭技术栈（solon+sqlite、root 运行、远程 web 管理）与我们的本机桌面模型不兼容，**只借交互思路与术语，不引代码、不引其安全模型（README 自陈 root 运行风险高）**；用户文档致谢 |
| nginx 配置生成/解析 crate | **不引** | crates.io 无维护良好的生成器（`nginx-config` 停维护 8 年；`nginx-discovery`/`nginx_lint_parser` 是面向分析的解析器，与生成无关）。生成走自研 typed 模型 + 字符串渲染（golden 测试锁定）；真值校验 = spawn `nginx -t`，与所有解析 crate 的自述边界一致 |
| caddy / apache 交互 | spawn CLI | `caddy validate/fmt/trust`、`httpd -t` 均为官方命令行，无 SDK 需求 |
| 进程树 / 日志 / 健康 / 指标 | 全部复用现有 core | `proc/`、log 批次、health、metrics、ports——网关 slot 是新用户不是新机制 |
| 前端可视化 | 自研（沿用现有组件体系） | 路由/分组用现有 shadcn 组件与 Linear 浅色 token；借鉴 nginxWebUI「分块卡片 + 表格 CRUD」的信息架构；不嵌第三方图库（无图算法需求，表即图） |

结论：**1.6 零新增 crate 依赖**（core 与前端均是）。

**两个开源对照的定位差异**（决定「借什么、不借什么」，也是 §1.3 边界的依据）：

| | nginxconfig.io | nginxWebUI | SuperTask 1.6 |
|---|---|---|---|
| 定位 | 一次性生成器 | 运行中实例的持续运维台 | 本机多服务工作台的网关托管 |
| 编辑对象 | 表单 → 一份 conf | conf 文件（可手改 + 历史回滚） | yaml（gateway 段），生成物是缓存 |
| 拓扑 | 单网站在线 | nginx 实例（可集群） | 本机服务（service id 即 upstream） |
| 证书 | Let's Encrypt 生产 | acme.sh + DNS 自动续签 | Caddy internal CA（localhost 开发） |
| 运行面 | 浏览器端纯前端，无后台 | root + web 远程 + sqlite | 桌面本地、loopback、无鉴权面 |

我们取**nginxWebUI 的可视化信息架构与闭环节奏**（分块、表格式增删、生成→校验→状态），去掉它的**实例运维面**（conf 手改、回滚、证书块、负载均衡实体、集群同步、root/web 安全模型）。

## 11. 前端范围（/gateway live）

页面结构（Linear 浅色，卡片分区，交互对齐运行页/配置页既有模式）：

1. **总览卡**：反代引擎（kind 选择器，三家 + 「未配置」空态）、监听端口、运行状态 dot + 状态文案、启停/重启/校验按钮、`bin` 覆盖提示（探测未命中但显式路径可用时显示来源）。
2. **路由卡（核心交互）**：表格式编辑（借鉴 nginxWebUI「分块卡片 + 表格 CRUD」，按 `host` 分组折叠呈现——每个非空 host 为一个分组标题，空 host 集中到「全匹配」组，组内为 location 行）——每行 path、target（服务下拉，带端口徽标）/ upstream（切换开关切手动地址）、上游存活 dot（运行中且端口监听=绿）、删除；「添加路由」「从服务生成草稿」（有 port 的服务各生成一条 `/服务id` 路由，一键填充）；底部「应用变更」→ `gateway.preview` 弹 diff 确认（复用 1.1 合并向导的 diff 样式）→ `gateway.apply`。
3. **配置预览卡**：`gateway.preview` 结果只读代码块（等宽、横向滚动、复制按钮）；校验按钮与结果行（ok 绿条 / stderr 红条原文）。
4. **HTTPS 卡（仅 caddy）**：`tls` 开关（off/internal）、证书状态（internal CA 签发的 localhost 证书路径）、「信任本机 CA」按钮 + 风险确认对话框（明示修改系统信任库）→ `gateway.trust`。
5. **工具链卡**：nginx/caddy/apache 三行探测结果（found/version/path + 缺失时平台安装指引，不代装）。
6. **空态**：gateway 未配置 → 引导卡（一句话说明 + 选引擎 + 「从服务生成路由草稿」进入编辑）。

- 路由变更未保存时离开页面/切工作区 → 脏确认（对齐配置页行为）。
- `gateway.apply` 的 `YAML_CONFLICT` 走既有冲突文案；`GATEWAY_*` 四语词条入 `errors.*`；页面文案四语 parity（1.4 校验规则）。
- 运行页/命令面板增加「启动网关/停止网关」入口（注册表驱动，不破坏壳层纪律）。
- mock：浏览器 mock 提供 demo 网关（nginx + 3 条路由 + 假探测结果 + 假 preview 文本），交互全可走。

## 12. CLI / MCP 联动

- `supertask up`：服务全部达到等待条件后启动网关（enabled 且配置有效）；网关失败 → `stop_all` + 退出 1 + stderr 错误码。`--wait` 不等待网关健康（网关不阻塞流水线主目标）。
- `supertask down` / `restart`：纳入网关；`status`（人读表 + `--json`）增加网关行（kind/port/state/routes 数）；`doctor` 增网关探测摘要。
- MCP：工具集不变；`supertask_status` 快照附带网关行（只读，天然覆盖）。
- CLI 不提供 `gateway` 子命令（编辑路由是 GUI 语义；CLI 只消费 yaml 真源）。

## 13. 非功能要求

- `gateway.preview` / `render` 纯函数，p95 < 10ms（无 IO）；`gateway.validate` 受外部进程支配（超时 10s 上限）；`gateway.status` 走快照，无额外扫描。
- 网关 slot 不改变引擎热路径：未配置 gateway 的工作区（绝大多数存量用户）零行为变化、零额外开销（`Option<GatewaySlot>` 空开销路径）。
- 三平台 CI 全绿为合并门槛；Windows 零回归。
- 配置产物不进遥测（本就无遥测）；`caddy trust` 结果不进日志明细。

## 14. 测试与验收

### 14.1 Core 单元测试

- 路由静态校验逐条：target 缺失/无端口、path 非法、host 非法、(host,path) 重复、与网关端口冲突、target/upstream 互斥。
- render golden 测试：三家引擎 ×（单路由/多 host 分组/空 host catch-all/IPv6 上游/caddy tls on/off/apache LoadModule 集）快照锁定。
- 探测：PATH 命中/未命中/显式 bin 优先；版本解析（nginx stderr 输出）。
- 校验链错误映射：非零退出 → `GATEWAY_CONFIG_INVALID` 带 stderr；超时；二进制缺失。

### 14.2 集成测试（桩进程纪律沿用）

- fake 反代桩（脚本桩监听 TCP，模拟配置合法/非法）：`gateway.start` 全链（render→validate→spawn→健康）、`apply` 运行中重启、`stop_all` 含网关、退出无残留。
- 校验桩（exit 0/1 + stderr 固定文案）驱动错误映射。
- 引擎层：gateway 与服务端口互查排除（1.2 语义回归）。

### 14.3 前端与 CLI

- `npm run build` + 四语 parity 校验 + mock 交互手测清单（空态→草稿→diff→应用→校验→启停）。
- CLI 集成：`up` 含网关（桩）、失败清场、`status --json` 网关行结构快照。
- Playwright（真机验收期）：网关页中文用例一条（草稿→应用→状态）。

### 14.4 真机验收矩阵

| 场景 | Windows | macOS | Linux |
|------|---------|-------|-------|
| nginx 反代真实 Spring+Node 工作区 | ✅ | ✅ | ✅ |
| caddy `https://localhost` 无警告 + trust/untrust | ✅ | ✅ | ✅ |
| apache（XAMPP/发行版包）最小反代 | ✅（XAMPP） | ✅（brew） | ✅（apt） |
| 网关启停无残留（三引擎） | ✅ | ✅ | ✅ |
| CLI `up --wait healthy -- cmd` 含网关 | ✅ | ✅ | ✅ |
| 1.0–1.5 回归抽样 | 全量 | 抽样 | 抽样 |

## 15. 交付顺序

1. **路由模型与校验**：spec typed 模型 + 错误码 + 静态校验（地基，零依赖）。
2. **render 三家**：IR + nginx/Caddyfile/httpd.conf 渲染 + golden 测试（纯函数，可与 1 并行启动）。
3. **探测与校验链**：probe 扩展 + spawn 校验命令 + 错误映射。
4. **引擎托管**：GatewaySlot 全生命周期 + stop_all/CLI 纳入。
5. **IPC + 壳层**：gateway.* 命令 + Tauri 适配 + feature 转 live。
6. **前端网关页**：状态/路由编辑/diff 应用/预览/HTTPS/工具链 + mock + 四语。
7. **CI 与真机验收**：matrix 扩展网关桩用例 + §14.4 矩阵。

依赖：2、3 依赖 1；4 依赖 2、3；5 依赖 4；6 依赖 5（mock 可先行）；7 收口。

## 16. 默认决策（本稿建议，待确认）

- 网关建模 = 顶层 `gateway:` 段 + 引擎托管 slot（非 services 成员，不进拓扑/profile）——已与用户确认。
- 三家引擎都做：nginx 全量一等；caddy 专注本机 HTTPS（`tls internal`，仅 localhost）；apache 最小反代集、PHP 明确延后。
- 路由变更不热重载：apply = 重写 yaml + 重新生成 + 重启 slot。
- 上游引用 service id（一等公民），显式 `upstream` 兜底外部目标；生成时按监听表选 v4/v6 回环地址。
- 只监听 loopback；不做对外暴露；不做指令透传。
- `caddy trust` 仅 UI 显式确认后执行；CLI 不提供。
- 配置生成零第三方 crate（无成熟库，golden 测试锁定）；模型借鉴 nginxconfig.io（MIT 致谢，不引代码）。
- 新错误码五枚（§9.1）；YAML `version: 1`、protocol 1、app data v3 不变；`gateway: {}` 旧语义零迁移。
- 工具链只探测不代装（nginx/caddy/apache 无安装 provider）。
