# 路线图九方向推进现状与后续改进计划

> 日期：2026-09-06
> 基线提交：`f71518d`（定时备份与保留策略交付记录）
> 当前分支：`main`
> 当前状态：前序切片已分批提交；方向二 A「运行中进程原地接管」在工作树中执行到中段，尚未提交。
> 本文是现状快照与后续执行清单，不替代 `docs/ROADMAP.md`（方向目标）或
> `docs/ROADMAP-NEXT-SLICES.md`（切片顺位）。

---

## 1. 结论摘要

### 1.1 方向九是否按 ROADMAP 完成

**按当前 ROADMAP 目标，方向九已完成可交付范围。**

已完成的可交付项：

- 扩展就绪审计：`docs/inventory/2026-09-06-ecosystem-extension-readiness.md`。
- 模板导入 / 导出：模板 zip 导入本地库、从本地或内置模板导出可分享 zip，导入导出可往返。
- kind、spec、错误码、权限和 preview/apply 边界已有稳定性审计结论。

仍未做的三类能力不是遗漏，而是路线图明确的暂缓项：

- 插件 / 自定义 kind：抽象尚未稳定，暂缓。
- WSL2 后端：依赖 Windows 主场景和平台差异收敛，暂缓。
- 团队环境基线与漂移检测：云的定位尚未拍板，暂缓。
- 模板远端分发 / 社区市场：导入导出已形成分发载荷单元，但真实社区需求尚未出现，暂缓。

因此不应把「方向九暂缓项未实现」误判为方向九未完成；后续只有出现真实分发需求时，才追加下载、sha256 校验和远端载荷入口，不提前建设社区市场或索引服务。

### 1.2 本轮已经提交的切片

截至 `f71518d`，以下 7 个切片已经完成代码、契约 / 文档同步并提交：

| 切片 | 方向 | 交付内容 | 主要提交 |
|---|---:|---|---|
| D | 一 | `health.type: log` + `pattern`；LogHub 增量扫描、水位、粘性命中、spec 校验 | `8951b6a`, `b6c9463` |
| T / M2 | 八 | tag 发布新增 macOS aarch64 / x86_64 DMG、Linux x86_64 AppImage / deb，统一 draft Release | `0905945` |
| C | 二 | Procfile preview/apply，每行转 generic 服务，shell 语法保守跳过，`.env` 用 env_file 引用 | `e15da4b`, `2f57d21`, `3210845` |
| F | 三 | needs 卡片「安装并钉扎」，复用 `toolchain.install persist + base_hash` 写回 `toolchain.*` | `01de5ca`, `0089a8a` |
| J | 四 | cloudflared quick tunnel URL 从日志提取到运行时快照和运行页服务卡片 | `99f58d6`, `76eb3cb`, `1502f2a` |
| R | 七 | MCP `supertask_env_snapshot`，聚合主机、工具链、needs、服务诊断上下文 | `efcf625`, `ed58f7f` |
| O | 六 | `data.volumes.*.backup` 定时自动快照、份数 / 天数 / 字节保留策略 | `fff27f9`, `f71518d` |

这些交付已回落到 `CHANGELOG.md`、`docs/spec/`、`docs/ROADMAP.md` 和
`docs/ROADMAP-NEXT-SLICES.md`。

### 1.3 当前执行到一半的切片

当前唯一处于工作树中的切片是 **A：运行中进程原地接管**。工作树状态如下：

- 未提交文件 8 个：
  - `crates/supertask-core/src/engine.rs`
  - `crates/supertask-core/src/ipc/v17.rs`
  - `crates/supertask-core/src/proc/windows.rs`
  - `src-tauri/src/commands.rs`
  - `src-tauri/src/lib.rs`
  - `frontend/src/ipc/api.ts`
  - `frontend/src/ipc/mock.ts`
  - `frontend/src/ipc/protocol.ts`
- 当前差异规模约为 360 行新增、58 行删除。
- 已实现的中段：
  - `WindowsJob::attach_pid`：`OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE)` 后调用 `AssignProcessToJobObject`。
  - `Engine::adopt_attach`：Unix 返回 `PLATFORM_UNSUPPORTED`，Windows 复用端口 + 工作目录 + 程序类型三维归属复核。
  - `workspace.adoptAttach` Tauri 命令及 IPC 名称、DTO、mock 入口。
  - attached pid watcher 和统一进程退出收场路径。
  - Windows 真实 Job 测试，以及引擎守卫路径测试。
- 尚未完成的闭环：
  - 运行页「接管」用户入口尚未接线；当前只有协议 / API / mock 中段。
  - `docs/spec/ipc.md` §10.16、`CHANGELOG.md`、`ROADMAP.md`、`ROADMAP-NEXT-SLICES.md` 尚未同步 A 的最终交付口径。
  - A 的全量回归、前端构建、三平台 CI 还没有在最终代码上重新跑完。
  - 当前格式检查失败，必须先执行 `cargo fmt --all` 并重新检查。

**A 不能按当前工作树直接提交。** 原因不是功能方向错误，而是接管动作触及
Windows `kill-on-close` 进程安全边界，必须先关闭下面列出的竞态风险并补齐用户可达入口。

---

## 2. 九个方向的完成矩阵

状态含义：

- **已交付**：本轮或既有基线已完成，且已回落到路线图 / CHANGELOG / spec。
- **执行中**：已有未提交代码，但尚未达到可提交验收标准。
- **剩余**：路线图中仍有计划，但当前尚未开始。
- **暂缓**：路线图明确不投入，不能按普通欠账处理。

| 方向 | 当前状态 | 已交付 / 基线 | 仍需推进或暂缓 |
|---|---|---|---|
| 一 服务监管与自愈 | **部分已交付** | restart 策略、崩溃通知、日志模式就绪判定已交付 | D2：服务级钩子、级联重启、失败保持与手动重试 |
| 二 纳管任意来源 | **A 执行中** | 孤儿进程纳管、Procfile 导入已交付 | A 原地接管先收尾；随后 M 隧道模板并入、B kind 推断；compose 导入仍是高价值欠账 |
| 三 环境供给 | **部分已交付** | needs 四态 resolve、mise / winget 来源、安装并钉扎已交付 | G compose / 容器作为 needs 来源；E 归档供给执行器；项目级版本隔离 |
| 四 网络与身份 | **部分已交付** | 隧道模板、WebSocket、CORS、多域名、重写 / 重定向 / 静态站点、隧道 URL 提取已交付 | M 模板并入现有工作区；H hosts、I 私有 CA 为高成本；K 真机网关验收；L apache 预检 |
| 五 主机与服务可观测性 | **基本完成** | 主机指标 MCP、服务资源归因、系统信息、历史趋势、体检报告已交付 | 仅剩服务 `version:` spec 扩展，价值低、排位中低 |
| 六 数据与备份 | **部分已交付** | 数据卷离线快照 / 恢复、定时备份与 auto 保留策略已交付 | N 数据库感知备份；P 外部目录导出 / 跨工作区导入；外部目录备份 |
| 七 AI 原生运行时 | **部分已交付** | 错误聚合、就绪等待、统一脱敏、环境快照 MCP 已交付 | Q MCP 环境供给（依赖 E）；S AI 审计与回放 |
| 八 多平台可用 | **部分已交付** | M1 三平台 CI；M2 三平台 Release 产物已接线 | V 三平台真机冒烟；U macOS 签名公证、W Linux AppImage 自动更新暂缓；X 差异收敛随 V 驱动 |
| 九 长期与生态 | **按 ROADMAP 完成可交付范围** | 扩展就绪审计、模板导入 / 导出已交付 | 插件 / 自定义 kind、WSL2、团队基线、模板远端分发均为明确暂缓 |

---

## 3. A 原地接管的阻塞项与安全要求

### 3.1 已验证的能力

当前针对 A 的验证结果：

| 验证项 | 结果 | 说明 |
|---|---|---|
| `cargo test -p supertask-core --lib adopt_attach` | 通过 | 1 项引擎守卫路径测试通过，覆盖服务不存在、无 port、状态竞态前置、无监听端口 |
| `cargo test -p supertask-core --lib proc::windows` | 通过 | 2 项 Windows Job 测试通过，包含真实外部 `ping` 进程 attach 后由 Job 终止 |
| `cargo check -p supertask` | 通过 | Tauri 壳能编译，命令已注册 |
| `cargo fmt --all -- --check` | **未通过** | A 当前存在 rustfmt 差异，不能提交 |
| core 全量回归 | 需重跑 | 631 项全绿是在 A 改动之前；A 改完后只跑了定向测试 |
| 前端构建 | 需重跑 | A 改完后尚未对最终工作树重新执行 |
| 三平台 CI | 未执行 | 当前工作树未提交，尚未进入 CI |

### 3.2 必须先修的进程安全竞态

当前 `adopt_attach_windows` 的流程是：

1. 锁内读取 Slot 状态和服务配置；
2. 锁外执行进程发现与归属复核；
3. 创建 `kill-on-close` Job 并 attach 外部 pid；
4. 再次加锁确认 Slot 仍是 `Stopped`，然后把 Job 放入 Slot。

第 4 步如果发现用户在第 2～3 步之间已经启动 / 停止 / 改变了服务状态，当前实现会返回错误；但此时外部 pid 已经被挂进临时 Job，临时 Job 离开作用域可能触发 `kill-on-close`，**存在把本来仍在运行的目标进程意外杀掉的风险**。

提交前必须改成不会在失败路径释放一个仍持有目标进程的 kill-on-close Job。建议顺序：

1. 将 Slot 置为明确的 attach-in-progress 状态或引入 per-slot attach guard，阻止同服务的 start / stop / restart 进入临界区。
2. 在真正 attach 前重新核对 pid、端口、工作目录和进程类型；发现复用或归属变化即不 attach。
3. attach 与 Slot 提交必须成为一个不可被普通生命周期操作插入的临界区。
4. 任一失败路径都必须保证：目标进程仍由原归属管理，或者 Job 的清理行为是显式且可解释的；不能依赖临时 `Job` 析构的隐式杀进程。
5. 增加一个并发 / 失败回滚测试，明确验证 attach 失败不会杀掉目标进程。

### 3.3 A 的语义边界要固定

- **Windows**：同用户进程通常可取得所需访问权；受保护进程、权限不足、已属于不兼容 Job 等失败，返回可诊断错误并保持外部实例语义。
- **Unix**：没有与 Windows Job Object 等价的安全 attach 语义，继续返回 `PLATFORM_UNSUPPORTED`，不伪造「已接管」。
- **停止 / 关闭**：成功接管后，服务进入引擎监管，停止和关闭会按 Job 的树清理语义终止被接管进程；UI 必须在确认文案中明确这一点。
- **重启策略**：当前实现把 attached 服务的 restart 压成 `never`，避免没有 `RestartPlan` 时误触发自动重启。这个限制要写进契约并在 UI 告知，或者在实现完整启动计划后再恢复 restart 语义。
- **退出码**：attached pid 没有 `Child::wait()`，watcher 当前使用 `-1` 表示未知退出码；IPC / 诊断文案要区分「退出码未知」，不能把 `-1` 伪装成真实进程退出码。
- **日志**：原地接管没有 stdout / stderr 管道，接管前历史日志不能自动补入 LogHub；日志能力必须在契约中说明为「后续日志 / 文件日志可见」，不能宣称等同于引擎启动的服务。

---

## 4. 后续待改进的额外计划

以下顺序是在不提交 A 半成品的前提下调整的执行顺序。每项都要完成代码、spec、测试、文档回落后才能从本表移出。

### P0-A：收尾并安全交付原地接管

**目标**：纳管后的运行中进程不重启、不被误杀，立即从外部实例转为受管服务；停止 / 关闭整树清理；失败安全回退。

**工作包**：

1. 修复 attach 与 Slot 提交竞态，增加 attach-in-progress / 失败回滚测试。
2. 补 `run-page` 接管按钮、确认文案、成功 / 失败 toast；按钮仅在外部运行且服务声明了可验证 `port` 时出现。
3. 补 `ipc.md` §6 / §10.16 的 `workspace.adoptAttach`、Windows / Unix 差异、日志与 restart 限制。
4. 补 CHANGELOG、ROADMAP、NEXT-SLICES 的交付回落。
5. 执行 fmt、core 全量、CLI、Tauri、前端构建，再交给三平台 CI。

**验收**：attach 成功后服务卡片 `managed=true`；不重启目标 pid；停止会杀树；attach 失败目标进程仍活且服务保持外部语义；Unix 明确返回不支持。

### P1-M：隧道模板并入现有工作区

**目标**：模板块可以通过 preview/apply 添加到当前工作区，不要求另建工作区。

**重点**：模板服务块选择、`{{port}}` 占位分配、与现有 services / needs / toolchain 合并、端口冲突、base_hash、重复导入幂等。

**验收**：纯预览不落盘；确认后只改所选块；冲突不覆盖；生成的 YAML 可重新加载并通过校验；既有 `templates.create` 行为不变。

### P1-G：compose / 运行中容器作为 needs 来源

**目标**：`needs: postgres@16` 能识别当前 compose 栈或运行中容器提供的中间件，并区分已运行、存在但未运行和不存在。

**重点**：镜像 tag 与版本前缀的确定性匹配、compose service 映射、Docker 不可用时的降级、FakeDockerRunner 离线测试。

### P1-E：归档供给执行器

**目标**：archive 状态从只读可供给性报告变成可执行下载、sha256 校验、解压和隔离目录供给。

**重点**：传输 trait + fake transport、清单托管、zip-slip / 沙箱、断点 / 重试一致性、PATH 或 launcher 解析、代理与凭据脱敏。完成后才能推进 Q MCP 环境供给。

### P1-D2：生命周期钩子、级联重启、失败保持

**目标**：补齐方向一剩余的三个 ★★ 欠账，但不破坏现有 restart 策略。

**重点**：spec 字段矩阵、钩子独立超时、pre_start 阻断、post_start 时机、依赖传递闭包重启、失败保持与手动 retry 的组合语义。

### P2-L：apache 能力预检

**目标**：探测 apache 版本低于 2.4.47 时，明确提示其不具备 WebSocket upgrade 能力。

**重点**：版本解析、warning 挂点（probe 或 validate）、UI 提示、无 apache 或版本不可读时的 null / warning 口径。

### P2-V：三平台真机冒烟

**目标**：在 Windows、macOS、Linux 真机验证起停、进程树、健康、日志、端口回收、通知 / 托盘，并纳入网关真实请求清单。

**前置**：M2 发布任务能在 tag 上产出；需要实际 runner / 设备与手动验收记录。不能用 CI 编译替代真机验收。

### P3-B / X：顺手项

- B：kind 智能推断只在证据充分时给建议，默认仍为 generic。
- X：平台差异收敛由 V 的真实结果驱动，不提前凭假设改平台分支。

### P4-Q / S / N / H / I / P

- Q：MCP `ensure_tool` / `ensure_service`，依赖 E。
- S：AI 操作审计与回放，先明确存储、保留和可逆操作边界。
- N：数据库感知备份，先定数据源、dump 凭据和在线一致性口径。
- H：hosts 管理，先做 UAC / 权限边界设计。
- I：私有 CA，先定密钥存储、信任库写入和 caddy 并存策略。
- P：快照外部目录导出 / 跨工作区导入，复用模板导入的安全口径。

### 暂缓项

- U：macOS 签名与公证，需要 Apple Developer 账号。
- W：Linux AppImage 自动更新通道，依赖 M2 产物反馈与更新签名策略。
- Y：模板远端分发 / 社区市场，等待真实需求。
- 插件 / 自定义 kind、WSL2、团队环境基线，继续遵守方向九审计结论。

---

## 5. 维护规则

1. A 在完成安全竞态修复、用户入口、spec、回归和 CI 前，不得从「执行中」改成「已交付」。
2. 任何切片只要改变 YAML、IPC、运行时状态或安全边界，必须同步 `docs/spec/`；只改路线图文字不算完成。
3. 提交前至少保留以下证据：聚焦测试、全量相关测试、fmt / build、必要时三平台 CI；真机能力不能用编译替代。
4. 交付后从 `ROADMAP-NEXT-SLICES.md` 移出或改写为剩余范围，并在 `ROADMAP.md`、`CHANGELOG.md` 和本 inventory 留下可追溯记录。
5. 对外部进程、hosts、证书、Job Object 和自动备份等不可逆或高权限操作，优先记录失败回滚路径，再补 UI 文案。

---

## 6. 关键证据索引

- 路线图目标：`docs/ROADMAP.md:110-339`
- 当前切片顺位：`docs/ROADMAP-NEXT-SLICES.md:1-321`
- 方向九扩展审计：`docs/inventory/2026-09-06-ecosystem-extension-readiness.md`
- A 当前引擎入口：`crates/supertask-core/src/engine.rs:1000-1118`
- A 当前 Windows Job attach：`crates/supertask-core/src/proc/windows.rs:95-122`
- A 当前 IPC 输出：`crates/supertask-core/src/ipc/v17.rs:50-56`
- A 当前 Tauri 命令：`src-tauri/src/commands.rs:1548-1555`
- 已交付 Procfile 契约：`docs/spec/ipc.md:1060-1120`（§10.19）
- 已交付环境快照 MCP：`docs/spec/cli.md:77-90` 附近
- 已交付定时备份契约：`docs/spec/yaml.md:377-425` 与 `docs/spec/ipc.md:1050-1065`
