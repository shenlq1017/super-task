# Roadmap 后续切片建议（待细化）

> 汇总各方向切片交付后留下的「下一步最小切片」建议，供下一轮开工时选择。
> 本文只记目标与验收雏形，**不排期、不写版本号**；每条的具体设计、字段与测试清单
> 在该切片开工前按 `docs/ROADMAP-EXECUTION-PROMPTS.md` 的通用执行协议统一补充完善。
> 新切片交付后，把对应条目改写为剩余范围或移除（维护约定同 `docs/ROADMAP.md` §14）。

---

## 方向二：纳管任意来源

> 已交付：孤儿进程纳管 dry-run 预览与确认写回（generic 忠实复刻原命令，
> `docs/spec/ipc.md` §10.16）。以下为剩余切片。

### A. 运行中进程原地接管（免重启纳入引擎监管）

- **目标**：纳管后的服务无需重启，运行中的外部进程即受引擎监管（统一启停 /
  进程树清理），而不是靠「重开工作区识别为外部实例」衔接。
- **已有材料**：Windows Job Object（`proc/windows.rs`）、`DETACHED` 会话内接管
  注册表（`engine.rs`）、`stop_one` 外部分支的归属复核 + `kill_foreign_by_pid`。
- **验收雏形**：纳管后不杀进程、不重启，运行页该服务从「外部 · 仅监控」转为受管
  状态；停止走树杀；重启走引擎完整链路。attach 失败可诊断并可回退到现有外部实例语义。
- **待细化**：Windows `AssignProcessToJobObject` attach 的权限边界与失败面；
  Unix 无 attach 等价物的降级语义（pid 会话级跟踪 vs 明确不支持）；attach 后
  `Slot` 生命周期与 `managed` 翻转时机；`ipc.md` §6 状态机增补。

### B. 专用 kind 智能推断（仅当证据充分）

- **目标**：纳管预览在证据充分时给出可切换的一等公民 kind 建议（仍默认 generic
  忠实复刻），减少用户手工升级成本。
- **验收雏形**：如 node 进程 + cwd 下 `package.json` 有匹配 script → 提示可转为
  `kind: node`；证据不足不出建议、绝不静默改写草稿；切换后的字段（dir/script/
  package_manager）全部可解释。
- **待细化**：每种 kind 的证据矩阵与置信度展示（复用 merge.rs `FieldMeta` 模式）；
  java 场景 `java -jar` 与 `mvn spring-boot:run` 的区分口径。

### C. Procfile 导入

- **目标**：`Procfile`（Overmind / Foreman / Heroku 生态）每行 `name: command`
  转成一个 generic 服务，撬动 Overmind 用户池；成本最小（格式极简）。
- **已有材料**：本次建立的 preview/apply 模板（Taskfile 导入同构：`preview(root,
  current)` 纯函数 → 勾选 → `apply` 合并 → `save_form`）；`.env` 读取已有
  `secrets::parse_dotenv`。
- **验收雏形**：`import.procfilePreview` / `import.procfileApply`（或并入统一导入
  面）；端口/名称冲突表达与幂等语义与 §10.16 同口径；`.env` 不回显敏感值。
- **待细化**：错误码（沿用 `*_NOT_FOUND` / `*_INVALID` 惯例）；是否与 devcontainer /
  `.env` 导入合并成一个「来源导入」切片。

---

## 方向一：服务监管与自愈（上轮遗留）

> 已交付：`restart` 策略与自动重启监管、崩溃通知。以下为表中剩余项。

### D. 日志模式就绪判定

- **目标**：健康/就绪判定新增 `log-pattern` 来源（匹配如 `Started ... in ...
  seconds`），Spring Boot 主打场景不再只靠 tcp 猜。
- **验收雏形**：配置的服务只在满足就绪条件后报告 ready；超时 / 进程退出 / 日志
  未匹配 / 配置错误均有稳定结果；既有 `none/tcp/http` 与 restart 策略行为不变。
- **待细化**：日志源接入点（`LogHub` 已有环形缓冲）、匹配窗口与重复日志幂等、
  非 UTF-8 / 大日志输入、yaml 字段与 schema。

---

## 方向三：环境供给

> 已交付：声明式 needs 的 resolve-only dry-run 与 mise/winget 供给接入（docs/spec/ipc.md §10.17）。以下为剩余切片。

### E. 归档供给执行器（免安装中间件下载/校验/解压）

- **目标**：让 archive 状态从「可供给性报告」变成可执行供给（下载官方 zip/单文件
  → sha256 校验 → 解压到 app data 工作区隔离目录 → PATH 注入/解析）。
- **已有材料**：`needs.rs` 的 `ARCHIVE_CATALOG` 与平台键、`toolchain/runner.rs`
  SpawnSpec/FakeRunner 注入模式、`pkg.rs` 的 zip 读写与 zip-slip/sha256 先例、
  `network::tool_env` 代理注入。
- **验收雏形**：相同目录+平台得到确定性下载计划；下载/解压可被 fake transport 全
  离线测试；安装目录不出沙箱；中断后重试状态一致；凭据/代理不进日志。
- **待细化**：传输 trait 与 fake 注入点、sha256 清单托管方式（内置 vs 远端化）、
  解压后如何进入服务 PATH/launcher 解析、错误码（沿用 vs 新增 `ARCHIVE_*`）。

### F. needs 安装与钉扎写回一体化（persist）

- **目标**：installable 项安装成功后，可把钉扎版本写回 `toolchain.*`，让
  「声明式需求」收敛为显式钉扎（与 /env 版本选择器同一落点）；本切片 resolve
  侧已按 `persist: false` 只读，写回是纯增量。
- **已有材料**：env 页 needs 安装流（`needsOps` / pending 状态机）、
  `toolchain.install` 的 persist 参数与 base_hash 乐观锁、
  `persist_toolchain_version`（commands.rs，含 npm→package_manager 映射）。
- **验收雏形**：安装成功且用户确认钉扎 → `toolchain.*` 写回、needs 重新解析为
  satisfied；`YAML_CONFLICT` 时安装结果保留、仅写回失败（§4.3 既有语义）；
  不确认钉扎时行为与本切片完全一致。
- **待细化**：UI 确认点（默认钉 vs 显式勾选）、已满足条目在 needs 段的表述
  （保留原样 vs 标注来源）、是否新增独立 `needs.apply` IPC 还是继续复用
  `toolchain.install`。

### G. compose / 运行中容器作为 needs 的「已存在」来源

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

> 已交付：隧道纳管模板（cloudflared 快速/命名、frpc，generic 纳管 + env_file 凭据）、
> 网关三形态路由（代理 / 重定向 / 静态站点）与 strip_prefix、route 级 CORS、
> 多域名别名、apache WebSocket（docs/spec/yaml.md §7.1）。以下为剩余候选
> （均为涉及平台权限的高成本项，开工前先做权限边界调研）。

### H. hosts 文件管理（本机 DNS 最小切片）

- **目标**：`*.localhost` 之外的真实域名（如 `api.myapp.dev`）在本机可解析：
  工作区声明域名 → 引擎生成 hosts 条目 → 管理员权限写入系统 hosts →
  关闭工作区时清理自己写入的段。
- **验收雏形**：写入带 SuperTask 标记段（幂等、可区分、可整体清理）；无管理员
  权限时明确报错并给手动指引；多工作区域名冲突可检测；不碰用户手工条目。
- **待细化**：Windows hosts 提权方式（以 UAC 提权子进程写入 vs 引导用户手动）、
  与路由 `host:` 字段的联动（校验提示域名未解析）、是否会污染 hosts 的安全审查。

### I. 私有 CA 与证书签发（caddy 之外的第二条路）

- **目标**：不依赖 caddy internal CA 的证书能力：内置私有 CA（根证书生成 +
  信任引导），为 nginx / apache 渲染出 `ssl_certificate` 路径与 SAN 证书，
  覆盖 `tls: internal` 在三引擎的一致语义。
- **已有材料**：`GatewayTls::Internal` 已是 spec 字段（当前仅 caddy 生效）；
  rcgen 纯 Rust 签发可离线测试；`gateway.trust` 的用户确认先例。
- **验收雏形**：`tls: internal` + kind: nginx/apache 时产物含证书路径且证书
  对声明的 host（含多域名别名）有效；CA 私钥不出 `.supertask/` 沙箱、不进日志；
  根证书信任沿用 trust 确认模式；过期/缺失自动重签。
- **待细化**：CA 密钥的存放位置与权限、Windows 信任库写入方式、
  与 caddy internal CA 的并存策略（避免两套根证书）。

以下为三形态路由与隧道切片交付时留下的**待改进**（来自本轮已知的边界，
成本小、可独立开工）：

### J. 隧道就绪信息：公网 URL 提取到服务卡片

- **现状**：quick tunnel 的公网 URL（`*.trycloudflare.com`）只在服务日志里，
  用户要翻日志才能拿到。
- **改进**：generic 服务的日志管道识别隧道分配 URL（cloudflared 的
  `https://<子域>.trycloudflare.com` 行），提取后进运行页服务卡片/状态提示；
  只读展示、不回显 token。
- **待细化**：提取规则的挂点（LogHub 管道 vs 每次快照时正则）、重启换 URL 的
  呈现、frpc 的 `远程地址` 文案组装。

### K. 网关三形态真机冒烟（CORS / WebSocket / 静态）

- **现状**：三引擎渲染由 11 份 golden 锁字节，但 CORS 白名单回显、preflight 204、
  apache `upgrade=websocket`、caddy `uri strip_prefix` 剥空路径等**运行时行为**
  只过了本机 `nginx -t` / `caddy validate` / `httpd -t` 级校验，未做真实请求验收。
- **改进**：按方向八真机冒烟的口径，补一组网关行为清单（每引擎：
  代理透传 / 剥前缀 / 重定向 / 静态索引 / CORS 命中与未命中 / preflight /
  WebSocket 回显），汇入平台验收；发现差异回修渲染。
- **待细化**：与方向八 M 系列共用环境；caddy `handle` 互斥顺序的实测确认。

### L. apache 版本预检（upgrade=websocket 需 ≥2.4.47）

- **现状**：旧版 Apache（<2.4.47）会在 `httpd -t` 阶段报 unknown parameter，
  错误可见但要读 stderr 才能定位；`probe.rs` 已能拿版本字符串（`httpd -v`）。
- **改进**：探测到 apache 且版本 < 2.4.47 时，在 `toolchain.probe` 的 gateway
  apache 项加能力标注（或 validate 输出附 warning），UI 提示升级指引。
- **待细化**：版本解析的健壮性（Apache Lounge / 发行版后缀）、警告挂点选
  probe 还是 validate。

### M. 隧道模板并入现有工作区（替代独立工作区）

- **现状**：模板创建的是独立工作区（target_port 指向本机端口，跨工作区可用
  但体验割裂）；把隧道服务加进**现有**工作区目前只能手改 yaml。
- **改进**：复用方向二导入 preview/apply 的模板（`preview(root, current)` →
  勾选 → `apply`），支持「向当前工作区添加模板服务/服务块」；与孤儿进程
  纳管、Taskfile 导入共用同一写回与冲突语义。
- **待细化**：块模板（blocks）在此入口的呈现、端口占位 `{{port}}` 的分配
  交互、与 needs/toolchain 段的叠加规则。

---

## 方向五：主机与服务可观测性

> 已交付：主机指标 MCP 暴露（`supertask_host_metrics`：只读、无参数、脱敏——除 `platform`
> 枚举值外无字符串字段，不暴露 IP/路径/进程/环境信息；复用 `system.metrics` 采样不持久化，
> 缺失字段为 null 不伪造为 0；契约见 cli.md MCP 清单）。
>
> 已交付：按服务归因的资源占用（监控页「服务资源占用」卡片 + 运行页服务卡片 CPU 徽标；
> 复用既有 `metrics.snapshot` per-service 采样，零新采样面；内存降序、null 为「—」、
> compose/外部纳管行注明口径）。
>
> 已交付：系统信息面板（监控页静态卡片 + 只读 `system.info`：平台 API 直采、零新增依赖、
> 取不到为 null；纯静态只读，与动态指标分列，不混入 MCP 工具）。
>
> 已交付：指标历史趋势（监控页「历史趋势」卡片：CPU + 内存压力跨页面存活，状态栏与
> 监控页既有轮询按采样时间戳去重合并进有界环形缓冲（~1h @1 Hz），零新采样面；
> 不落盘——「不持久化」约定不变，跨重启持久化保留为显式扩展选项）。
>
> 已交付：一键体检报告（监控页「体检报告」卡片：工具链 / 网关引擎 / Docker 三节只读
> 探测 + Markdown 导出，复用既有探测面零新采样，口径同 CLI `supertask doctor`）。

（方向五剩余小项：服务版本标注——勘误后仅剩「版本」无声明来源，需 spec `version:`
字段扩展，见 ROADMAP §7 表。）

---

## 方向六：数据与备份

> 已交付：服务绑定数据快照/恢复最小闭环（spec `data:` 段、离线文件快照 zip+manifest+sha256、
> stash 回滚式恢复、工作区页「数据快照」卡片；docs/spec/ipc.md §10.18、yaml.md §7.3）。
> 以下为剩余切片。

### N. 数据库感知备份（pg_dump / mysqldump / 在线一致性）

- **目标**：对「服务关联库」做数据库感知的备份：识别数据来源（本机 postgres/mysql
  等）后调用对应 dump 工具产出逻辑备份，恢复按库语义导入；ROADMAP §8 剩余最小集中的
  「seed、常用查询」同源。
- **已有材料**：`snapshot.rs` 的 manifest/条目校验与恢复管线、`data.volumes` 绑定、
  needs 的工具探测（可判 pg_dump/mysqldump 可用性）。
- **验收雏形**：绑定本机 postgres 服务的工作区可一键逻辑备份/恢复；dump 工具缺失
  时给出可诊断错误（复用 `MissingTool` 口径）；在线一致性口径明确（不伪造）。
- **待细化**：数据来源识别（needs id ↔ 服务 kind/镜像）、dump 凭据来源（env_file，
  不进日志）、备份与文件快照的混存/互斥、大库超时语义。

### O. 工作区定时备份与保留策略

- **目标**：`data.volumes` 快照的定时自动创建与保留上限（条数/总字节/天数），
  兑现 ROADMAP「与快照能力共用一套归档机制」。
- **已有材料**：`snapshot.rs` 全套原语（create/list/delete、上限检查）、
  `LogRetentionSpec` 的保留策略字段先例、`.supertask` 运行时目录。
- **验收雏形**：声明保留策略后，超限快照自动清理且从不清除最新一份；定时触发点
  明确（引擎常驻事件循环）；关闭工作区不产生半截快照。
- **待细化**：spec 字段形态（`data.retention` vs 每卷字段）、定时器挂点与间隔来源、
  触发时绑定服务运行的跳过语义、UI 呈现位置。

### P. 快照导出到外部目录 / 跨工作区导入

- **目标**：把单个快照 zip 导出到用户指定目录（ROADMAP §8「备份到外部目录」），
  并允许从外部快照文件导入到同名卷——快照从「工作区内部状态」升级为可搬运资产。
- **已有材料**：`snapshot.rs` 的 manifest 格式与校验、导出包 `pkg.rs` 的选目录交互、
  工作区页既有卡片模式。
- **验收雏形**：导出的 zip 可在另一工作区同名卷导入且整包校验通过；目标卷声明与
  快照 manifest 的 volume_id 不一致时给出明确错误或改名建议。
- **待细化**：导入时的覆盖保护（复用 restorePreview 口径）、format 演进的兼容声明。

---

## 方向七：AI 原生运行时

> 已交付：MCP 错误聚合与就绪等待（`supertask_errors` / `supertask_wait_ready`，outcome
> 四态可区分 reached/failed/stopped/timeout，超时是结果不是错误）与全工具出口统一脱敏
> （core `ai::sanitize::Redactor`：声明密钥值替换 + 敏感行整行掩码；cli.md MCP 节）。
> 以下为剩余切片。

### Q. MCP 环境供给能力（ensure_tool / ensure_service）

- **目标**：Agent 说一句「跑起这个项目」即可补齐依赖：MCP 工具封装 needs resolve 的
  「可安装 / 可归档供给」态，经确认语义安装工具链并启动服务（ROADMAP §9「MCP 环境供给能力」）。
- **已有材料**：`needs.rs` 四态解析（ipc.md §10.17）、`toolchain` 的 mise/winget 安装、
  `supertask_wait_ready` 的就绪闭环、出口统一脱敏。
- **验收雏形**：缺工具的工作区经 MCP 一次补齐并就绪；安装动作有确认/取消语义，
  失败不清场（与 needs resolve 口径一致）；输出全程脱敏。
- **待细化**：确认交互（MCP 无 UI，确认语义怎么落）、归档供给下载器落地、
  安装凭据与代理的口径。

### R. 环境快照上下文（结构化输出给 AI）

- **目标**：把工具版本、端口、健康、错误摘要聚合成一个「环境快照」结构化返回，
  让 AI 不靠多次调用拼凑上下文（ROADMAP §9「环境快照上下文」）。
- **已有材料**：`Engine::diagnostics()` 聚合、`probe` 工具链探测、`host_metrics` 采样、
  `metrics` 服务指标。
- **验收雏形**：一次调用返回可直接进 prompt 的结构化上下文（全脱敏、大小有界、
  缺采样字段可区分）。
- **待细化**：字段矩阵与大小上限、与 `supertask_status` / `supertask_errors` 的取舍或合并。

### S. AI 操作审计与回放

- **目标**：可查看「AI 这段时间动了什么」并回滚（ROADMAP §9「AI 操作审计与回放」）。
- **已有材料**：`operation.rs` 长操作记录（无 list API，需补）、事件总线、
  `snapshot.rs` 数据卷快照（可作回滚手段）。
- **验收雏形**：MCP 会话的可变操作有按序审计记录（脱敏）；回放/回滚有确认与预览。
- **待细化**：审计存储位置与保留策略、operation list API、可逆操作边界。

---

（后续方向切片交付后在此追加）
