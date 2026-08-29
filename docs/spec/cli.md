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
| `supertask up [ids…] [--wait healthy\|started\|none] [--wait-timeout S] [-- cmd…]` | ✅ | 拓扑启动 → 等待（默认 healthy，300s 超时）→ 交互聚合日志或 `--` 包装（退出码透传）。失败/超时/信号 → 停止全部 |
| `supertask down [ids…]` | ✅ | 停止全部/所选；他人持锁 → `WORKSPACE_LOCKED` |
| `supertask restart [ids…]` | ✅ | 停止再启动 |
| `supertask status [--json]` | ❌ | 服务端口监听状态 + 锁持有者（owner/pid） |
| `supertask logs [id] [--lines N] [--grep P]` | ❌ | 历史日志尾部/检索 |
| `supertask script run <id>` / `script cancel` | ✅ | 运行（等待结束，返回退出码）/取消；cmds 只来自 YAML |
| `supertask export [-o FILE] [--with-secrets]` | ❌ | 导出 zip（manifest + supertask.yaml；`--with-secrets` 含声明密钥文件明文） |
| `supertask import <pkg> [--dest DIR]` | ❌ | 只落盘不启动；目标已有 yaml 拒绝 |
| `supertask doctor` | ❌ | 工具链 + docker 探测摘要 |
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
