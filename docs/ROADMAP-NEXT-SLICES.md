# Roadmap 后续切片 · 逐项调整计划

> 2026-09-06 全面盘点：汇总九个方向各自的「缺漏剩余项」，按当前基线（方向八 M1 三平台 CI
> 全绿、方向九模板导入/导出已交付）逐项调整优先级并给出执行计划。
> 每条 = 调整后优先级 + 理由 + 目标/验收雏形/待细化；开工前按
> `docs/ROADMAP-EXECUTION-PROMPTS.md` 的通用执行协议补充具体设计。
> 交付后把条目改写为剩余范围或移除（维护约定同 `docs/ROADMAP.md` §14）。

## 优先级总览（调整后）

> 2026-09-06 更新：首顺位 D（日志模式就绪判定）已交付并移出（进 `CHANGELOG.md`）；
> 方向一剩余三项（钩子 / 级联重启 / 失败保持）补录为打包切片 D2；
> T（M2 三平台 release 产物）、C（Procfile 导入）、F（needs 钉扎写回）、
> J（隧道公网 URL 提取）、R（环境快照上下文）、O（工作区定时备份）、
> A（运行中进程原地接管）均已交付并移出。

| 顺位 | 切片 | 方向 | 调整后优先级 | 调整理由 |
|---|---|---|---|---|
| 1 | A 运行中进程原地接管 | 二 | ✅ 已交付 | 见 §方向二 A（2026-09-06，契约 ipc.md §10.16 增补） |
| 2 | M 隧道模板并入现有工作区 | 四 | 中 | 复用 preview/apply；与 C 同批可做 |
| 3 | G compose 作为 needs 来源 | 三 | 中 | 依赖方向二数据复用边界拍板 |
| 4 | E 归档供给执行器 | 三 | 中 | 打开能力上限但工程量大（下载器/校验/隔离） |
| 5 | V M4 三平台真机冒烟 | 八 | 中 | 需真机环境；K 网关行为清单并入本项 |
| 6 | D2 钩子 / 级联重启 / 失败保持 | 一 | 中 | ROADMAP 主表剩余 ★★ 项，打包成一个切片 |
| 7 | B kind 智能推断 / X M6 差异收敛 / L apache 预检 | 二/八/四 | 中低 | 顺手级或依赖前项 |
| 8 | Q MCP 环境供给 | 七 | 中低 | 硬依赖 E 归档执行器落地 |
| 9 | S AI 审计回放 / N DB 感知备份 / H hosts / I 私有 CA / P 快照导出 | 七/六/四/六 | 低 | 高成本或主场景外 |
| 10 | U M3 签名公证 / W M5 AppImage / Y 模板远端分发 | 八/八/九 | 暂缓 | 需外部账号/市场；前置未齐 |

---

## 方向一：服务监管与自愈

> 已交付：`restart` 策略与自动重启监管、崩溃通知、**日志模式就绪判定**
> （`health.type: log` + `pattern`，2026-09-06，进 `CHANGELOG.md` 与 yaml.md §4.5）。
> 以下为剩余项。

### D2. 生命周期钩子 / 级联重启 / 失败保持（打包切片） —— 优先级：中

- **目标**：服务级 `pre_start` / `post_start` 钩子（独立超时，`pre_start` 失败阻断）；
  修好上游后一键重启下游传递闭包（拓扑数据已有）；可选的失败保持与手动重试策略
  （默认保持现有清场行为）。
- **已有材料**：`graph.rs` 拓扑与 `start_order`、工作区级 `scripts` 执行先例、
  restart 策略状态机。
- **验收雏形**：钩子在正确时机执行且超时可配；级联重启只影响传递闭包并按依赖序；
  失败保持开启时失败服务保留现场等待手动重试，关闭时行为与现状一致。
- **待细化**：钩子字段矩阵（spec/校验/编辑器）、级联重启 IPC 形态、失败保持与
  现有 restart 策略的组合语义。

---

## 方向二：纳管任意来源

> 已交付：孤儿进程纳管 dry-run 预览与确认写回（generic 忠实复刻原命令，
> `docs/spec/ipc.md` §10.16）。以下为剩余切片。

### A. 运行中进程原地接管（免重启纳入引擎监管） —— ✅ 已交付（2026-09-06）

落地口径：`workspace.adoptAttach`（`{ workspace_id, service_id }` → `{ service_id,
pid, warnings[] }`，Windows 专用，Unix → `PLATFORM_UNSUPPORTED`）。归属复核复用
纳管同一三维判定（端口 + 工作目录 + 程序类型），归属不符一律拒绝；attach 占位
guard 互斥同服务 start/stop/restart/二次 attach；暂存 Job（无 kill-on-close）
失败路径释放不误杀，转正只在提交前一刻生效；attached 服务 restart 压 `never`、
退出码未知记 `-1`、接管前历史日志不可见。运行页服务详情（停止态 + 声明 port +
非 compose）提供「接管运行中进程」按钮与确认文案，成功 toast 带 pid。
契约见 `docs/spec/ipc.md` §10.16 增补。剩余：专用 kind 智能推断（B）、compose
导入等仍按原顺位推进。

### B. 专用 kind 智能推断（仅当证据充分） —— 优先级：中低

- **目标**：纳管预览在证据充分时给出可切换的一等公民 kind 建议（仍默认 generic
  忠实复刻），减少用户手工升级成本。
- **验收雏形**：如 node 进程 + cwd 下 `package.json` 有匹配 script → 提示可转为
  `kind: node`；证据不足不出建议、绝不静默改写草稿；切换后的字段（dir/script/
  package_manager）全部可解释。
- **待细化**：每种 kind 的证据矩阵与置信度展示（复用 merge.rs `FieldMeta` 模式）；
  java 场景 `java -jar` 与 `mvn spring-boot:run` 的区分口径。

### C. Procfile 导入 —— ✅ 已交付（2026-09-06）

已按「忠实优先」落地：每行转 `kind: generic` 服务（sh 风格引号感知拆词 →
program/args）；含 shell 语法（`$`、管道、重定向、组合、通配符等）的命令**跳过
不导入**（generic 不经 shell，插值/操作符无法忠实表达，与 Taskfile「按原文导入」
不同——scripts 走 `bash -c`，服务没有等价落点）；`.env` 存在时挂 `env_file`
引用不回显；`labels` 记 `origin/imported-from`；新增错误码 `PROCFILE_NOT_FOUND`
/ `PROCFILE_INVALID`。契约见 `docs/spec/ipc.md` §10.19。

---

## 方向三：环境供给

> 已交付：声明式 needs 的 resolve-only dry-run 与 mise/winget 供给接入（ipc.md §10.17）。
> 以下为剩余切片。

### E. 归档供给执行器（免安装中间件下载/校验/解压） —— 优先级：中

- **目标**：让 archive 状态从「可供给性报告」变成可执行供给（下载官方 zip/单文件
  → sha256 校验 → 解压到 app data 工作区隔离目录 → PATH 注入/解析）。
- **已有材料**：`needs.rs` 的 `ARCHIVE_CATALOG` 与平台键、`toolchain/runner.rs`
  SpawnSpec/FakeRunner 注入模式、模板包的 zip 读写与 zip-slip 先例（template.rs）、
  `network::tool_env` 代理注入。
- **验收雏形**：相同目录+平台得到确定性下载计划；下载/解压可被 fake transport 全
  离线测试；安装目录不出沙箱；中断后重试状态一致；凭据/代理不进日志。
- **待细化**：传输 trait 与 fake 注入点、sha256 清单托管方式（内置 vs 远端化）、
  解压后如何进入服务 PATH/launcher 解析、错误码（沿用 vs 新增 `ARCHIVE_*`）。

### F. needs 安装与钉扎写回一体化（persist） —— ✅ 已交付（2026-09-06）

落地口径：needs 卡片 installable 行提供「安装」与「安装并钉扎」两个显式动作
（默认不钉，行为与此前完全一致）；「安装并钉扎」= 同一 `toolchain.install` 带
`persist: true` + `base_hash`（复用既有 persist_toolchain_version，零后端改动、
零新增错误码），成功后写回 `toolchain.*`（npm/pnpm/yarn 写 `package_manager`）、
重新 resolve 翻转 satisfied；`YAML_CONFLICT` 时安装保留、仅写回失败（§4.3）。
契约增补见 `docs/spec/ipc.md` §10.17。

### G. compose / 运行中容器作为 needs 的「已存在」来源 —— 优先级：中

- **目标**：needs 解析除本机 PATH/安装枚举外，识别 compose 栈或运行中容器
  提供的中间件（如栈内已有 postgres:16 容器 → satisfied，来源标注 compose），
  兑现 ROADMAP「needs: postgres:16 → 自动发现本机 / compose / 可安装」的完整链路。
- **已有材料**：`docker/` 模块（probe_docker / ps / images）、方向二发现与纳管
  的进程/来源识别、graph 拓扑。
- **验收雏形**：compose 工作区声明 `needs: [postgres@16]` 且栈内存在匹配镜像的
  service → satisfied（来源=compose）；容器存在但未启动与不存在可区分；
  判定逻辑离线 fake 覆盖，docker 不可用时不阻塞其余条目解析。
- **待细化**：镜像 tag ↔ 版本前缀的匹配口径、compose service 与 needs id 的
  映射规则、容器来源 satisfied 是否要求服务已在拓扑中纳管、与方向二纳管
  数据的复用边界。

---

## 方向四：网络与身份

> 已交付：隧道纳管模板、网关三形态路由与 strip_prefix、route 级 CORS、多域名别名、
> apache WebSocket（yaml.md §7.1）；方向九已交付模板导入/导出。以下为剩余候选
> （H/I 为涉及平台权限的高成本项，开工前先做权限边界调研）。

### H. hosts 文件管理（本机 DNS 最小切片） —— 优先级：低（高成本权限项）

- **目标**：`*.localhost` 之外的真实域名在本机可解析：工作区声明域名 → 引擎生成
  hosts 条目 → 管理员权限写入系统 hosts → 关闭工作区时清理自己写入的段。
- **验收雏形**：写入带 SuperTask 标记段（幂等、可区分、可整体清理）；无管理员
  权限时明确报错并给手动指引；多工作区域名冲突可检测；不碰用户手工条目。
- **待细化**：Windows hosts 提权方式（UAC 提权子进程 vs 引导手动）、与路由
  `host:` 字段联动、安全审查。

### I. 私有 CA 与证书签发 —— 优先级：低（高成本权限项）

- **目标**：不依赖 caddy internal CA 的证书能力：内置私有 CA（根证书生成 +
  信任引导），为 nginx / apache 渲染证书路径与 SAN 证书，覆盖 `tls: internal`
  三引擎一致语义。
- **已有材料**：`GatewayTls::Internal` 已是 spec 字段（当前仅 caddy 生效）；
  rcgen 纯 Rust 签发可离线测试；`gateway.trust` 的用户确认先例。
- **验收雏形**：`tls: internal` + nginx/apache 产物含证书路径且对声明 host 有效；
  CA 私钥不出 `.supertask/` 沙箱、不进日志；根证书信任沿用 trust 确认模式；
  过期/缺失自动重签。
- **待细化**：CA 密钥存放与权限、Windows 信任库写入方式、与 caddy internal CA
  的并存策略。

### J. 隧道就绪信息：公网 URL 提取到服务卡片 —— ✅ 已交付（2026-09-06）

落地口径：引擎 `push_line` 管道识别 cloudflared quick tunnel 分配行
（`https://<子域>.trycloudflare.com`，子串快筛 + 正则提取），粘性存 Slot
（重启清零），随 `RuntimeSnapshot` 下发（`ServiceRuntimeView.tunnel_url`，
additive 缺省不序列化）；运行页服务卡片只读徽标点击即开。frpc 远程地址
日志形态不稳定，不做推测式提取（待真实日志证据）。契约见 ipc.md §6。

### K. 网关三形态真机冒烟 —— 优先级：中（并入方向八 V 一起做）

- **现状**：三引擎渲染由 golden 锁字节，但 CORS 回显、preflight 204、
  apache `upgrade=websocket`、strip_prefix 剥空路径等**运行时行为**未做真实请求验收。
- **改进**：补网关行为清单（每引擎：代理透传 / 剥前缀 / 重定向 / 静态索引 /
  CORS 命中与未命中 / preflight / WebSocket 回显），汇入平台验收。

### L. apache 版本预检（upgrade=websocket 需 ≥2.4.47） —— 优先级：中低

- **改进**：探测到 apache 且版本 < 2.4.47 时，在 `toolchain.probe` 的 gateway
  apache 项加能力标注（或 validate 输出附 warning），UI 提示升级指引。
- **待细化**：版本解析健壮性、警告挂点选 probe 还是 validate。

### M. 隧道模板并入现有工作区（替代独立工作区） —— 优先级：中

- **改进**：复用 preview/apply 模板，支持「向当前工作区添加模板服务/服务块」；
  与孤儿进程纳管、Taskfile 导入共用同一写回与冲突语义。方向九模板导入/导出
  交付后，「模板 → 现有工作区」是同一落点的自然延伸。
- **待细化**：块模板呈现、`{{port}}` 占位分配交互、与 needs/toolchain 段叠加规则。

---

## 方向五：主机与服务可观测性

> 已交付：主机指标 MCP 暴露、按服务归因资源占用、系统信息面板、指标历史趋势、
> 一键体检报告（全部零新增采样面；详见 CHANGELOG）。

剩余仅小项：**服务版本标注**——勘误后仅剩「版本」无声明来源，需 spec `version:`
字段扩展（字段矩阵：schema/校验/编辑器/测试），价值 ★ 成本中，排位中低。

---

## 方向六：数据与备份

> 已交付：服务绑定数据快照/恢复最小闭环（spec `data:` 段、离线 zip+manifest+sha256、
> stash 回滚式恢复、工作区页卡片；ipc.md §10.18、yaml.md §7.3）。以下为剩余切片。

### N. 数据库感知备份（pg_dump / mysqldump / 在线一致性） —— 优先级：低

- **目标**：识别数据来源后调用对应 dump 工具产出逻辑备份，恢复按库语义导入。
- **验收雏形**：绑定本机 postgres 服务的工作区可一键逻辑备份/恢复；dump 工具缺失
  给可诊断错误（`MissingTool` 口径）；在线一致性口径明确（不伪造）。
- **待细化**：数据来源识别、dump 凭据来源（env_file，不进日志）、与文件快照
  混存/互斥、大库超时语义。

### O. 工作区定时备份与保留策略 —— ✅ 已交付（2026-09-06）

落地口径：每卷 `data.volumes.*.backup`（`interval_mins` 5..=43200 + `max_count`/
`max_age_days`/`max_total_bytes`，越界 `DATA_INVALID`）；引擎 open 启动常驻调度线程
（tick 30s，配置变化下个 tick 生效），到期自动创建快照（note = `auto`，dataList
可见），绑定服务运行中该轮跳过；保留策略只作用于 auto 快照、**最新一份从不清除**；
重新打开以既有最新 auto 快照为基线不重复补拍；close/detach 停调度，写入为
tmp+rename 原子操作。契约见 yaml.md §7.3 与 ipc.md §10.18。

### P. 快照导出到外部目录 / 跨工作区导入 —— 优先级：低

- **目标**：单个快照 zip 导出到用户指定目录，并允许从外部快照导入同名卷——
  快照升级为可搬运资产。方向九模板导入/导出的安全口径可直接复用。
- **待细化**：导入覆盖保护（复用 restorePreview 口径）、format 演进兼容声明。

---

## 方向七：AI 原生运行时

> 已交付：MCP 错误聚合与就绪等待（`supertask_errors` / `supertask_wait_ready`）与
> 全工具出口统一脱敏（core `ai::sanitize::Redactor`）。以下为剩余切片。

### Q. MCP 环境供给能力（ensure_tool / ensure_service） —— 优先级：中低（硬依赖 E）

- **目标**：Agent 说一句「跑起这个项目」即可补齐依赖：MCP 工具封装 needs resolve
  的可安装/可归档供给态，经确认语义安装并启动。
- **待细化**：确认交互（MCP 无 UI，确认语义怎么落）、归档供给下载器落地（=切片 E）、
  安装凭据与代理口径。

### R. 环境快照上下文（结构化输出给 AI） —— ✅ 已交付（2026-09-06）

落地口径：MCP 新工具 `supertask_env_snapshot`（无参数），聚合 `Engine::diagnostics`
（就绪分账 + 错误摘要，去日志摘录）+ `toolchain_probe` 缓存（版本摘要**不带路径**）
+ `needs_resolve`（四态，reason 截断 ≤200 字符）+ spec 钉扎/声明 + 主机指标为一次
结构化返回；大小有界、出口统一脱敏、缺采样字段为 null。取锁类只读（不改服务状态）。
与 `supertask_status` / `supertask_errors` 分工写进 cli.md MCP 清单与工具描述。
MCP 工具 10 → 11。

### S. AI 操作审计与回放 —— 优先级：低

- **目标**：可查看「AI 这段时间动了什么」并回滚。
- **已有材料**：`operation.rs` 长操作记录（无 list API，需补）、事件总线、
  `snapshot.rs` 数据卷快照（可作回滚手段）。
- **待细化**：审计存储与保留策略、operation list API、可逆操作边界。

---

## 方向八：多平台可用

> 已交付：M1 CI 矩阵恢复三平台并修复全部暴露的平台问题（2026-09-05，CI 全绿）；
> **M2 release 平台产物**（2026-09-06：macOS aarch64/x86_64 DMG + Linux x86_64
> AppImage/deb 进同一 draft Release，不写 latest.json 保住 Windows 更新链路，
> ubuntu-22.04 构建兼顾 glibc 兼容面；进 `CHANGELOG.md`）。
> 以下对应本地规划 M3–M6（P1→P2）。

### U. M3 macOS 签名与公证 —— 优先级：暂缓（需 Apple Developer 账号）

### V. M4 三平台真机冒烟（六项） —— 优先级：中（需真机环境）

- 起停、进程树清理、健康检查、日志、端口回收、通知与托盘；**并入方向四 K 的
  网关行为清单**（CORS / WebSocket / 静态三形态真实请求验收）。

### W. M5 Linux 打包分发（AppImage 自动更新通道） —— 优先级：暂缓（依赖 T 落地反馈）

### X. M6 平台差异收敛 —— 优先级：中低（随 M4 冒烟结果驱动）

- 路径约定、权限模型、caddy/nginx 安装方式与命令行差异；`proc/unix.rs` 已有
  Linux cgroups 与 macOS 回退分支。

---

## 方向九：长期与生态

> 已交付：扩展就绪审计（docs/inventory/2026-09-06）+ 模板导入/导出（本地库写入路径
> 与可分享 zip 包）。插件/自定义 kind、WSL2、团队基线维持路线图暂缓判定。

### Y. 模板远端分发 / 社区市场 —— 优先级：暂缓

- **前置**：导入/导出契约已是分发载荷单元；待真实社区需求出现后加
  「下载 + sha256」前置步骤即可，契约不推翻。
- **不做的**：社区市场界面、远端索引服务——单人项目阶段是沉没成本。

---

（后续方向切片交付后在此追加；执行顺序见顶部「优先级总览」。）
