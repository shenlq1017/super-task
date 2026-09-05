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

（后续方向切片交付后在此追加）
