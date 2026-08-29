# SuperTask CLI 与 MCP（1.5 用户文档）

> 规格真源：[1.5 功能规格](../plans/2026-08-29-v1-5-feature-spec.md) §4–§5  
> bin：`supertask`（crate `crates/supertask-cli`）。业务与桌面端同一 `supertask-core`，错误码同一张表。

## 工作区解析

所有命令按以下顺序定位工作区（必须含 `supertask.yaml`，否则 `NO_WORKSPACE`）：

1. `-w/--workspace <dir>`
2. 环境变量 `SUPERTASK_WORKSPACE`
3. 从 cwd 向上逐级搜索

## 命令

| 命令 | 取锁 | 说明 |
|------|------|------|
| `supertask up [ids…] [--wait healthy\|started\|none] [--wait-timeout S] [-- cmd…]` | ✅ | 拓扑启动 → 等待（默认 healthy，300s 超时）→ **启动网关（1.6）** → 交互聚合日志或 `--` 包装（退出码透传）。失败/超时/信号 → 停止全部；网关启动失败同样清场并以 `GATEWAY_*` 退出码 1 结束（未配置网关则静默跳过；`--wait` 不等待网关健康） |
| `supertask down [ids…]` | ✅ | 停止全部/所选（含网关）；他人持锁 → `WORKSPACE_LOCKED` |
| `supertask restart [ids…]` | ✅ | 停止再启动（网关运行中时一并重启） |
| `supertask status [--json]` | ❌ | 服务端口监听状态 + 网关行（kind/port/state/routes 数，1.6）+ 锁持有者（owner/pid） |
| `supertask logs [id] [--lines N] [--grep P]` | ❌ | 历史日志尾部/检索 |
| `supertask script run <id>` / `script cancel` | ✅ | 运行（等待结束，返回退出码）/取消；cmds 只来自 YAML |
| `supertask export [-o FILE] [--with-secrets]` | ❌ | 导出 zip（manifest + supertask.yaml；`--with-secrets` 含声明密钥文件明文） |
| `supertask import <pkg> [--dest DIR]` | ❌ | 只落盘不启动；目标已有 yaml 拒绝 |
| `supertask doctor` | ❌ | 工具链 + docker + 网关三引擎（nginx/caddy/apache）探测摘要 |
| `supertask mcp` | 惰性 | stdio MCP 服务器（见下） |
| `supertask version` | ❌ | 版本与协议 |

全局参数：`--json`（机器可读 `{ok, data | error:{code,message,details}}`，错误码与 IPC 同表）、`--no-color`（保留开关，当前输出为纯文本）。

## 退出码

`0` 成功；`1` 运行错误（健康超时 `HEALTH_TIMEOUT`、`WORKSPACE_LOCKED`、`MISSING_TOOL` 等）；`2` 用法错误。

## CI 用法

```yaml
- run: supertask up --wait healthy -- mvn verify
# 服务全部健康后运行 mvn verify；子命令退出码原样透传；结束后无残留进程
```

## 工作区所有权（engine.lock）

- 打开工作区的第一个可变动作会获取 `<root>/.supertask/engine.lock`（pid + holder + 时间戳）。
- 同一工作区同时只有一个存活进程 owner：桌面 / CLI / MCP 互相拒绝（`WORKSPACE_LOCKED`，details 带 holder 与 pid）。
- 只读命令（status / logs / doctor / export）不取锁，owner 运行期间也能用。
- 持有进程崩溃后锁视为 stale，下次自动接管。`.supertask/` 建议整体加入 `.gitignore`。

## MCP 接入（Cursor / Claude 等编辑器）

```json
{ "mcpServers": { "supertask": { "command": "supertask", "args": ["mcp"] } } }
```

- 仅 stdio 本地传输、tools only，无网络监听。
- 工具：`supertask_status`、`supertask_start`、`supertask_stop`、`supertask_restart`、`supertask_logs`、`supertask_run_script`、`supertask_cancel_script`。
- **断开即清场**：编辑器退出/重载会关闭 stdio，MCP 进程停止全部服务、释放锁并退出（防孤儿优先）。可变工具描述中已明示。
- 桌面已打开同一工作区时，可变工具返回 `WORKSPACE_LOCKED`（details 带 holder/pid），只读工具仍可用。

## 开发者备忘

- CLI bin 与桌面 dev 产物同名 `supertask.exe`；桌面 dev 进程运行时用 `CARGO_TARGET_DIR=target-cli cargo build -p supertask-cli` 隔离构建（安装版为 `SuperTask.exe`，不受影响）。
- 带色输出预留 `--no-color`；后续按实现计划用 `anstyle` + `anstream`。

## 网关（1.6）

CLI 只消费 yaml 真源，不提供 `gateway` 子命令（编辑路由是 GUI 语义，桌面 `/gateway` 页提供路由编辑 / diff 确认 / 本机校验 / caddy HTTPS 与信任）：

- `supertask up`：服务健康后自动启动 `gateway:` 段配置的网关（`enabled: true` 且配置有效）；网关失败 → 停止全部 + 退出 1 + stderr 错误码（`GATEWAY_*`）。`--wait` 不等待网关健康（网关不阻塞流水线主目标）。
- `supertask down` / `restart`：清场/重启包含网关（restart 仅网关运行中时一并重启）。
- `supertask status --json`：`gateway` 行（kind / port / state / routes 数；未配置为 null）。
- `supertask doctor`：nginx / caddy / apache 三引擎探测行（只探测，不代装；缺失时按平台给安装指引）。
- 生成物：`<root>/.supertask/gateway/`（nginx.conf / Caddyfile / httpd.conf），随每次启动/应用重新生成——磁盘产物是缓存，不是编辑对象。

### 开源致谢

- 路由配置模型借鉴 [nginxconfig.io](https://github.com/digitalocean/nginxconfig.io)（DigitalOcean，MIT）的「domain → path 路由 → https 选项」抽象与产物组织，按本机开发场景大幅裁剪（无 certbot/HSTS/生产安全头），未引入其代码或依赖。
- `/gateway` 页面的可视化闭环（分块卡片 + 表格式增删 + 生成→校验→状态）借鉴 [nginxWebUI](https://github.com/cym1102/nginxWebUI)（cym1102，GPL）的信息架构与交互思路，未引入其代码与安全模型。
