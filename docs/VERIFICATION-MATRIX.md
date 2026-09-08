# SuperTask 验证矩阵（路线图切片验收手册）

> 从 `docs/ROADMAP.md`（九方向全部候选点）整理：每个功能点的**状态 + 如何验证**
> （自动化命令 / 手动步骤 / 通过标准）。交付状态以 `docs/ROADMAP-NEXT-SLICES.md`
> 为准，行为口径以 `docs/spec/` 为准。
>
> - 常驻门禁（每次提交/PR）：见 §0。
> - 已交付切片的回归验证：见 §1（自动化为主）。
> - 未交付/暂缓项的验收方法（开工即用）：见 §2。
> - 真机冒烟与发布验证：见 §3、§4。
>
> 最后更新：2026-09-06 · 基线 `b04cc3c`（Actions v5）

---

## 0. 验证分层与常驻门禁

| 层级 | 含义 | 成本 |
|---|---|---|
| L0 静态 | `cargo fmt`、前端 `tsc`（含在 build 内） | 秒级 |
| L1 离线单测 | `cargo test` 三 crate（全 fake/offline，零真实网络） | 分钟级 |
| L2 本机真机 | 本机 Windows：CLI 实二进制冒烟、桌面应用手动走查 | 分钟级 |
| L3 三平台 CI | GitHub Actions：三平台矩阵 + 前端 + 云（push main / PR 自动触发） | 约 4 分钟 |
| L4 发布 | tag → draft Release → CNB 镜像 → 更新链路 | 发版时 |

**常驻门禁命令**（与 `ci.yml` 同口径，提交前本地必过）：

```bash
cargo fmt --all -- --check
cargo test -p supertask-core      # 约 665 项
cargo test -p supertask-cli       # 25 项
cargo test -p supertask-cloud-server  # 16 项
cargo check -p supertask          # Tauri 壳
npm --prefix frontend run build   # tsc + vite
```

**运行时冒烟**（等价 CI 的 `cli version smoke`，且覆盖引擎真实加载）：

```bash
$env:CARGO_TARGET_DIR = "target-cli"; cargo build -p supertask-cli
.\target-cli\debug\supertask.exe --version   # supertask 0.2.0
.\target-cli\debug\supertask.exe version     # protocol/engine 版本
.\target-cli\debug\supertask.exe -w examples\node-demo status  # 真实开 yaml + 快照
.\target-cli\debug\supertask.exe doctor      # 真机工具链探测摘要（只读）
```

**已知 CI 脆弱点**（已修一例，见 `0bc1780`）：断言 temp 路径的测试必须按
`canonicalize + strip_verbatim` 口径比较，否则在 CI macOS（`/var` 软链）与
Windows（`RUNNER~1` 短文件名）上失败、本地通过。新测试一律遵守。

---

## 1. 已交付切片回归矩阵

> 目标：改动不破坏已交付行为。每项 = 自动化锚点 + 手动抽查 + 通过标准。

### 方向一：服务监管与自愈

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| `restart` 策略 + 重试上限 | `cargo test -p supertask-core --lib restart`（预算耗尽转放弃等） | 示例服务配 `restart: always`，kill 进程看自动拉起与 `restart_attempt` 徽标 | 重试序号递增、用尽后 `last_error` 给放弃原因、不无限重启 |
| 崩溃通知 | 引擎 `exit_reason=crash` 单测 + 前端 crash-notifier | 起服务后 taskkill，看 Toast/系统通知可点击跳转 | 非 stop 退出必弹通知，不含日志原文与密钥 |
| 日志模式就绪判定 `health.type: log` | `health::log_ready*` + 引擎水位/粘性测试（yaml.md §4.5） | Spring Boot 服务配 `pattern: "Started .* in .* seconds"`，看 Starting→Running 翻转 | 旧日志不误命中（水位）、命中后不回退（粘性）、非法正则加载期拒绝 |

### 方向二：纳管任意来源

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| 孤儿进程纳管 preview/apply | `adopt::` 22 项（草稿推导/脱敏/matched/幂等） | 发现页「纳管进程」→ 预览勾选 → 写回，yaml 含 `origin: adopted` | 只增所选、不杀进程、命令行脱敏 |
| Procfile 导入 | `procfile::` 13 项 | 配置页「导入 Procfile」：shell 语法行置灰跳过、`.env` 挂引用 | 宁可少导不错导；重复 apply 幂等 |
| 原地接管 adoptAttach | `adopt_attach_guard_paths`、`adopt_attach_guard_blocks_lifecycle`、`proc::windows` 3 项（含暂存 drop 不误杀）| Windows：停止态+port 服务点「接管运行中进程」→ 确认 → 受管 Running，停止杀树 | 归属不符拒绝且目标进程存活；Unix 明确 `PLATFORM_UNSUPPORTED` |

### 方向三：环境供给

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| needs 四态 resolve + 安装钉扎 | `needs::`（语法/矩阵/四态）+ persist 写回测试 | 环境页 needs 卡片：检查→安装/安装并钉扎→翻转 satisfied | `YAML_CONFLICT` 仅写回失败、安装保留 |
| compose/容器来源 | `needs::` 10 项 + engine `needs_resolve_sees_compose_postgres` / `degrades_without_docker` | 工作区声明 `postgres@16` + 本机有对应容器/声明，看 satisfied 与 reason | 运行/停止/声明/缺席三态可区分；docker 不可用不阻塞 |
| 归档供给执行器 | `archive::` 8 项 + `needs::` 3 项（已安装优先）| 环境页 archive 行「下载安装」（minio 约 100MB，手动按需）→ 重跑 resolve 翻转 | sha256 不符删分片；失败不落半成品；mysql/postgres 明确拒绝无校验供给 |

### 方向四：网络与身份

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| 隧道模板 + URL 提取 | `tunnel` 6 项 | cloudflared quick tunnel 服务启动后看运行页 URL 徽标可点击 | URL 粘性至重启、重启清零；token 不回显 |
| 网关三形态/CORS/重写 | golden 单测锁字节 | 网关页 preview 与应用（行为级见 §3 真机清单 K） | 三引擎渲染一致；apache 含 `upgrade=websocket` |
| 模板并入现有工作区 | `template::tests::merge` 5 项 | 模板页对已打开工作区点「并入当前工作区」→ 预览勾选 → 写回 | 冲突与已存在文件跳过不覆盖；重复 apply 全跳过 |

### 方向五：主机与服务可观测性

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| 主机指标 MCP / 资源归因 / 系统信息 / 趋势 / 体检 | `metrics`/`host_metrics`/`system_info` 单测 + `supertask_host_metrics` MCP 测试 | 监控页五卡片展示；MCP 调 `supertask_host_metrics` | 无采样字段为 null 不伪造 0；指标不落盘 |

### 方向六：数据与备份

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| 数据卷快照/恢复 | `snapshot::` 12 项 + `SNAPSHOT_BUSY` 守护 | 工作区页创建快照 → 改文件 → 预览（remove_count）→ 恢复 | 损坏包拒绝且不动目标目录；中断可回滚 |
| 定时备份与保留 | `backup` 2 项（保留矩阵/tick） | 卷配 `backup: {interval_mins: 5}`，等 5 分钟看 auto 快照出现 | 只作用 auto 快照、最新一份永不清除 |

### 方向七：AI 原生运行时

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| 错误聚合/就绪等待/统一脱敏 | CLI `mcp::` 25 项 + `ai::sanitize` 单测 | MCP 调 `supertask_errors` / `supertask_wait_ready` | 密钥与进程输出脱敏；AI 一次拿到就绪+错误摘要 |
| 环境快照 MCP | `env_snapshot_dispatch_returns_bounded_context`（CI 修过路径口径）| 同上 `supertask_env_snapshot` | 大小有界、全脱敏、只读不改状态 |

### 方向八：多平台可用（已交付部分）

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| M1 三平台 CI 矩阵 | Actions `core (windows/macos/ubuntu)` 全绿即证 | `gh run list --branch main` | 三平台同提交全绿 |
| M2 三平台发布产物 | `release.yml`（tag 触发，dry-run 不轻易测；审 job 定义） | 发版时见 §4 | DMG/AppImage/deb 进同一 draft Release；不写 `latest.json` |

### 方向九：长期与生态（已交付部分）

| 功能 | 自动化 | 手动抽查 | 通过标准 |
|---|---|---|---|
| 模板导入/导出 | `template::` import/export 往返单测 | 模板页导入 zip → 导出 → 再导入往返 | 不安全路径/超限拒绝；同 id 拒收；失败不落半成品 |

---

## 2. 未交付项验收矩阵（开工即用）

### D2. 生命周期钩子 / 级联重启 / 失败保持（优先级：中）

- **钩子**：单测（pre_start 失败阻断/超时 `HOOK_FAILED`/post_start 脱离不阻塞就绪/
  超时杀进程）+ 手动（配 `hooks.pre_start: {run, timeout_secs}` 起服务，看阻断与日志行）。
- **级联重启**：单测（传递闭包计算/按依赖序停启/只影响闭包）+ 手动（详情页「级联重启」，
  上游修好后下游按拓扑重启；`restartOne --cascade` CLI 同步）。
- **失败保持**：单测（`on_fail: keep` 跳过监管、手动 start 清标记、与 restart 组合 keep 优先）
  + 手动（kill 进程看 Exited +「失败保持」徽标 +「重试」按钮，关闭行为与现状一致）。
- 契约：yaml.md 字段矩阵 + `HOOK_FAILED` 码表 + schema；UI 文案四语言。

### B. kind 智能推断（中低）/ X. 差异收敛（中低，随 V）

- B：单测（证据矩阵每种 kind 正/反例、证据不足不出建议）+ 手动（纳管预览出现可切换建议，
  默认仍 generic，切换后字段可解释）。
- X：不单独立项，由 V 真机结果逐条驱动收敛，每条附平台与复现步骤。

### L. apache 版本预检（中低）

- 单测（版本解析健壮性：`2.4.6` < `2.4.47` 数值比较、不可读回退）+ 手动（装 apache 2.4.6x
  看 probe/validate 警告 + UI 升级指引）。

### Q. MCP 环境供给（中低，E 已就绪）

- 单测（`ensure_tool`/`ensure_service` 确认语义、fake transport 全离线）+ 手动（MCP 客户端
  下发「跑起这个项目」，观察确认→安装→启动闭环与审计痕迹）。
- 前置确认交互设计必须先定（MCP 无 UI），否则不开工。

### S / N / H / I / P（低）

- S：先定存储/保留/可逆边界；验审计列表与回滚。
- N：先定 dump 凭据（env_file）与在线一致性口径；验逻辑备份/恢复与缺工具可诊断错误。
- H：先做 UAC 权限边界设计；验标记段幂等写入/清理、不碰手工条目、无权限时给手动指引。
- I：先定 CA 密钥存放与信任库写入方式；验证书三引擎生效、过期自动重签、私钥不出沙箱。
- P：复用模板导入安全口径 + restorePreview 覆盖保护；验导出→导入往返与 format 兼容声明。

### V. 三平台真机冒烟 + K（中，需真机）—— 详见 §3

### U / W / Y（暂缓，不验收，只列前置）

- U：需 Apple Developer 账号（$99/年），CI-only 签名路线已由 dbx 验证可行。
- W：等 M2 产物用户反馈 + 更新签名策略。
- Y：等真实社区需求；导入/导出契约即分发载荷，不提前建市场。

### ROADMAP 其余未编号候选（方向三/五/六，优先级中低）

- 工具链自举 / 免安装中间件归档（10–20 个）/ 项目级版本隔离 / 工具清单远端化 /
  邮件捕获与对象存储纳管 / compose 与 devcontainer/`.env` 导入 / 反向导出矩阵 /
  服务版本标注：开工时按本矩阵体例（自动化锚点 + 手动步骤 + 通过标准）补验收行，
  先定字段/权限/口径再写码（H/I 类）。

---

## 3. 真机冒烟清单（V + K，Windows / macOS / Linux 各跑一遍）

每项记（通过/失败 + 版本/截图/日志片段），失败项转 X 差异收敛条目：

1. 起停：示例工作区 `up` 全绿、`down` 全停（CLI + 桌面各一遍）。
2. 进程树清理：启动含子进程的服务后停止，确认无孤儿进程残留。
3. 健康检查：tcp/http/log 三型各一起就绪翻转。
4. 日志：运行页实时流 + 历史检索 + 8KiB 截断行。
5. 端口回收：`down` 后端口释放，可立即重起（orckit 做不到的 Windows 独家项）。
6. 通知与托盘：崩溃通知可点击跳转；托盘常驻/退出行为。
7. K 网关行为（每引擎 nginx/caddy/apache）：代理透传 / strip_prefix（尤其剥空路径）/
   重定向 / 静态索引 / CORS 命中与未命中 / preflight 204 / WebSocket 回显——真实请求，
   不只看渲染文本。

---

## 4. 发布验证清单（发版时）

1. 统一升版（workspace + frontend + tauri.conf + lock）→ CHANGELOG 版本段落。
2. `git tag vX.Y.Z` → push → `release.yml` 跑完：Windows NSIS/MSI + macOS DMG×2 +
   Linux AppImage/deb 进同一 draft Release。
3. mirror-to-cnb 成功；`latest.json` 仍只含 Windows（M2 约束）。
4. 应用内「设置 → 检查更新」：国内走 CNB、海外走 GitHub（双端点顺序）。
5. draft 转正式发布。

---

## 5. 维护约定

- 新切片开工：先在本矩阵 §2 补验收行（自动化锚点 + 手动步骤 + 通过标准），再写码。
- 交付后：本矩阵对应行改写为 §1 体例（附测试名与契约位置），`ROADMAP-NEXT-SLICES.md`
  同步移出。
- 真机能力永不用编译替代；权限/密钥类先定边界再实现（H/I/S/N 前置）。
