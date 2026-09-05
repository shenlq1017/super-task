# v2.0 实现计划（云：账号 / 同步 / 一键迁移 / 发布工程收口）

> 2026-08-29。依据：[2026-08-29-v2-0-feature-spec.md](2026-08-29-v2-0-feature-spec.md)（2026-08-29 用户指令视为立项拍板）。
> 状态：**客户端 Phase 1–8 的自动化范围已落地（FakeCloudProvider 路线，CI 零真实网络）；自托管参考服务的本地 HTTP/API 自动化范围也已落地。Phase 0 发布工程（C1/C2）与 Phase 10 真机验收仍待真机/密钥，正式端点运营方仍待拍板。**
> 实测基线：core 370 个单测 / cli 20 全绿；四语 locale parity 944 keys；前端 build 通过；server 有 3 个本地 in-process API 集成测试。

## 实现期偏差与决策备案（2026-08-29）

1. **拍板 0.4 未决项**：服务端运营方仍开放——内置占位端点（`cloud.supertask.local.example`），客户端全量经 provider trait 可自托管；真机验收（场景 2/4/5/9）依赖真实端点。
2. **v1.7 Phase 8 验收未关闭**（用户指令直接进 v2.0）：验收专项（inv-4 B 类）继续挂账。
3. **Phase 3.2/3.3 部分**：vault crypto（argon2id + XChaCha20-Poly1305 + AAD）与 round-trip/篡改测试已落地；yaml `secrets.sync: true` 字段增量与 vault 打包/写回编排**待补**（`CLOUD_ENCRYPT_REQUIRED` 码已备）。
4. **CLI `cloud sync` 首版为只读预览**（云端实体/本地跟踪/冲突计数），落盘同步在桌面端——避免 CLI 在无人选择落盘目录时产生半状态（cloud.md 已注明）。
5. **template 拉取落盘挂起**：推送（本地→云）已可用，拉取写回 local 模板目录 pending（报告可见，随 cloud.md 迭代）。
6. **welcome「从云端恢复」入口与 settings 遥测/端点 UI 已补齐**（IPC `cloud.telemetry.set` 与 `cloud.endpoint.set` 已接线）。
7. HTTP 层经 `HttpExecutor` trait 注入（ureq 仅生产路径）；错误映射矩阵用本地 fake 单测。
8. **自托管 server 状态**：`crates/supertask-cloud-server` 已加入 workspace；配置、Argon2 auth/token 数据层、entity 数据层、SQLite migration、HTTP router/handler、`/healthz`、配额/遥测端点和本地 in-process API 集成测试已落地。正式 HTTPS 部署、运营归属和真机验收仍未完成。server 约束单列于 [docs/spec/cloud-server.md](../spec/cloud-server.md)。
9. **端点设置**：`CloudHandle` 已具备端点校验/重载，`cloud.endpoint.set` 已注册为 Tauri IPC；浏览器 mock 保留 `supported: false, local_only: true` 降级口径。
10. **客户端 HTTP 认证**：已接入 401 → refresh 一次 → 原请求重放一次，并在刷新成功后先持久化新 token；刷新失败清理 session。真实端点真机验收仍待完成。
> 执行约定：先读 `project tooling/executing-plans-0.1.0\SKILL.md`；前端任务点名 skill；CLI 构建用 `CARGO_TARGET_DIR=target-cli` 防与桌面 dev 产物撞名。

## 基线与每期回归

- 参照基线（v1.7 收尾目标）：core ≈ 430+ / cli 20 全绿；四语 parity ≈ 880。kickoff 实测后回填。
- 每 Phase 收尾必跑：`cargo test -p supertask-core`；`CARGO_TARGET_DIR=target-cli cargo test -p supertask-cli`；`frontend/` 内 `npm run build`；i18n parity 脚本。
- 当前实测基线：core 370 个单测、cli 20 个测试、server 3 个 in-process API 测试；前端 locale parity 944 keys。
- **CI 零真实网络**：所有云测试走 FakeCloudProvider；HttpCloudProvider 的错误映射用本地 HTTP 测试双（或 trait 层 fake），不访问外网。

---

## Phase 0 · 发布工程收口与前置拍板

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 0.1 | C1：签名密钥生成与安全托管；updater endpoint / pubkey 替换 `REPLACE_AT_RELEASE` 占位 | `src-tauri/tauri.conf.json`、CI secrets | 真机升级链一次（旧版→新版）验证通过；inv-4 C1 销账 |
| 0.2 | C2：安装包（Tauri bundler NSIS + MSI）+ 三平台 release 流水线（artifact 签名） | CI workflow、bundler 配置 | 三平台产物可安装可启动；inv-4 C2 销账 |
| 0.3 | 验收专项确认：B1–B6 已按 v1.7 Phase 8 矩阵关闭，结果回写 inv-4 | `docs/inventory/…-inv-4-debts.md` | 清单化销账 |
| 0.4 | **拍板（阻塞后续）**：服务端运营方 / 官方端点；自动同步默认开 or 关（spec §18.1/18.3） | spec §18 回填 | 决策记入 repository conventions 已拍板 |
| 0.5 | 阶段切换回改：roadmap 状态、repository conventions 当前阶段、inv-1 交付表开 2.0 行 | 三处文档 | 一致 |

## Phase 1 · core：cloud 骨架（provider / 协议 / 会话）

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 1.1 | `cloud/provider.rs`：trait CloudProvider（login / refresh / list / pull / push / delete）+ 实体信封类型（serde，对齐 spec §10） | 新模块 `crates/supertask-core/src/cloud/` | 协议类型 round-trip 单测 |
| 1.2 | `FakeCloudProvider`：内存实现 + 旋钮（注入 409 冲突 / 401 过期 / 429 配额 / 离线） | 同上 | 每旋钮一测 |
| 1.3 | `cloud/session.rs`：token 存取（appdata `cloud/session.json`）、refresh 接口/登出清理；设备 id = sha256(hostname+首启时间) | appdata 模块复用 | 刷新/失效/登出单测；统一 401 refresh/replay 仍待接线 |
| 1.4 | token 静态加密：Windows DPAPI（核查 `windows` crate 所需 feature 并增补；不可用回退受限 ACL + 文档标注） | `Cargo.toml`、cloud/session.rs | 加密往返 + 回退路径单测 |
| 1.5 | `HttpCloudProvider`：ureq + rustls；HTTP → CLOUD_* 错误映射（spec §10 表） | 新依赖 `ureq` | 映射矩阵单测（本地测试双） |

## Phase 2 · core：同步引擎

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 2.1 | `cloud/sync.rs`：本地状态（appdata `cloud/state.json`：per-entity base_rev + last_synced_hash）、dirty 判定、pull→push 两阶段、冲突收集 | cloud/ 新文件 | dirty/干净/两端改/409 单测（全 fake） |
| 2.2 | workspace 实体适配：本地注册条目 `cloud_id` 映射；拉取落盘复用 pkg/import 语义（目标已有 yaml 拒绝；只落盘不启动）；落盘后 base_hash 按外部写入更新 | appdata recents 结构、`pkg.rs` 复用 | 落盘/拒绝/无自动启动单测 |
| 2.3 | 打开中工作区挂起：引擎持有 → 实体 pending + 状态可见；关闭后可重试 | engine 状态查询、`WORKSPACE_LOCKED` 口径 | 挂起/重试单测 |
| 2.4 | template 实体适配：`%APPDATA%/SuperTask/templates/` 目录 ↔ 实体 | template 模块复用 | 往返单测 |
| 2.5 | settings 实体适配：白名单键（language/通知/网络 app 默认）；漏键忽略并警告 | appdata 设置结构 | 白名单/漏键单测 |
| 2.6 | `cloud.resolve`：local / server / both（workspace 副本命名规则：`<name> (copy N)`，记入 cloud.md） | sync.rs | 三选一单测 |
| 2.7 | 自动同步调度：启动 30s + 每 15min（仅登录后；设置可关）——壳层驱动或 core 驱动实现期定，倾向壳层定时调 cloud.sync | 壳层 state.rs | 调度逻辑单测（注入时钟） |

## Phase 3 · core：密钥 E2E

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 3.1 | `cloud/crypto.rs`：argon2id(passphrase, salt) → key；XChaCha20-Poly1305 加解密 vault（AAD=账号 id，nonce 随机） | 新依赖 `argon2`、`chacha20poly1305` | 加密往返 / 错误 passphrase / 篡改检测单测 |
| 3.2 | yaml secrets 声明 `sync: true` 增量（默认 false；仅 local/file 后端可勾选，env 拒绝并 SPEC_INVALID） | `spec/file.rs`、`spec/validate.rs`、yaml.md | round-trip + 校验矩阵单测 |
| 3.3 | vault 打包/解包：勾选 secret 集合 ↔ 加密实体；拉取写回 secrets 存储（同名 keep-both 后缀副本）；`supertask.ai` id 硬排除 | secrets 模块复用 | 打包/解包/同名/排除单测 |
| 3.4 | passphrase 管理：设置 / DPAPI 包裹缓存 / 未设 + 有勾选 → `CLOUD_ENCRYPT_REQUIRED` | cloud/、appdata | 三态单测 |

## Phase 4 · core：一键迁移

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 4.1 | `cloud/migrate.rs`：restore plan（实体清单 + 工具链差量：ToolchainSpec 钉扎 × probe 比对；未钉扎跳过；版本不符 warning） | `toolchain/probe.rs` 复用 | 差量矩阵单测（fake probe：found/missing/version-mismatch） |
| 4.2 | apply 编排：实体拉取落盘（复用 Phase 2 适配）+ 缺失工具逐项走既有安装链（mise→winget→重解析）；进度/取消复用 operation 事件桥 | `toolchain/install.rs` 复用、壳层事件 | 编排单测（FakeRunner）+ 取消路径 |

## Phase 5 · core：遥测

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 5.1 | `cloud/telemetry.rs`：事件枚举（app_start/app_stop/feature_open/service_start(kind)）、批量缓冲、上报 | cloud/ | 事件形状单测 |
| 5.2 | 关闭 = no-op：`enabled=false` 时**零网络调用**（fake 计数断言）；批量节奏（24h/退出） | cloud/、壳层生命周期 | 零请求断言单测 |

## Phase 6 · 壳层：IPC + feature 转 live

| # | 任务 | 触点 | 验收 / 测试 |
|---|------|------|-------------|
| 6.1 | `cloud.*` 九条命令（spec §11 表 + endpoint.set）薄适配 | `src-tauri/src/commands.rs`、lib.rs | 已接线；端点校验/持久化/重载已覆盖 |
| 6.2 | features.rs：cloud → Live(2.0)；SOON_COMMANDS 移除 cloud.login/cloud.sync | `features.rs:32,51-55` | 已完成并更新 features |
| 6.3 | 自动同步定时器挂载（若 Phase 2.7 定壳层） | state.rs | 未确认完整接线；不作为当前已交付能力 |

## Phase 7 · 前端

> Skills：`vercel-react-best-practices`、`vercel-composition-patterns`（provider/组合）、`ui-styling`（token/对比度）；7.5 审查用 `web-design-guidelines`。按钮语义按 repository conventions 约定表。

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 7.1 | /cloud 页（soon→live）：登录卡 / 账号会话卡 / 同步中心（实体列表+状态+冲突三选一）/ 迁移卡 / 配额 | `pages/cloud-page.tsx` 新建、registry 无需改（cloud 已在扩展组） | 已完成 mock/自动化范围；真实端点未验收 |
| 7.2 | welcome「从云端恢复」入口（与本地导入并列；未登录时点击引导登录或去 /cloud） | `welcome-page.tsx` | 已完成 |
| 7.3 | settings：遥测开关（默认关）+ passphrase 管理 + 端点高级配置 | `settings-page.tsx` | 遥测/端点 UI 已完成；passphrase 管理仍待补；浏览器 mock 保留 local-only 降级 |
| 7.4 | mock provider（浏览器 dev）：含冲突/离线旋钮的确定性规则 | 前端 mock IPC | mock 双路径可演示 |
| 7.5 | 四语 keys（cloud/welcome/settings 增量）+ 页面审查 | `i18n/locales/*` | parity 通过；审查过 |

## Phase 8 · CLI

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 8.1 | `supertask cloud status/sync/logout`：共享 appdata 会话；未登录人话报错；`--json` 同表 | `crates/supertask-cli` | 已完成；`cloud sync` 当前仅只读预览 |
| 8.2 | cli.md：cloud 命令一节 | `docs/spec/cli.md` | 已完成并补充只读预览边界 |

## Phase 9 · 文档闭环 + 全量回归

| # | 任务 | 触点 | 验收 |
|---|------|------|------|
| 9.1 | `docs/spec/cloud.md` + `docs/spec/cloud-server.md`：客户端协议与参考服务边界（PUT type/opaque id/rev/refresh/unknown type/storage/seed） | docs/spec/ | 已完成；记录正式运营/HTTPS 未验收 |
| 9.2 | yaml.md：secrets `sync: true` 增量 | `docs/spec/yaml.md` | 仍待实现/文档同步，不能宣称完成 |
| 9.3 | ipc.md §10.12（九命令 + 端点命令 + 错误码七枚 + 零新事件声明）；architecture.md（cloud 模块一节、本地优先原则入架构原则） | 对应文档 | IPC 已同步；architecture cloud 小节仍待补 |
| 9.4 | repository conventions 当前阶段 + 规范真源更新；inv-1 交付表、inv-2 server 事实、inv-3 cloud UI、inv-4 欠账回改 | living 文档 | 已完成；盘点明确区分客户端与 server |
| 9.5 | 全量回归四连 + 基线核对 | — | 当前自动化基线已记录于 verification；正式端点与真机项仍开放 |

## Phase 10 · 验收

| # | 任务 | 验收 |
|---|------|------|
| 10.1 | fake + local server 全链路（CI）：spec §16 场景 1–12 中除真机项外自动化 | 已完成可重复自动化；server API 使用 in-process 临时 SQLite |
| 10.2 | 真机：登录/刷新/离线/推送拉取/冲突/迁移差量（真实 mise/winget 一次）；双设备或双 appdata 隔离模拟 | 待验收；记录进 `docs/verification/2026-xx-xx-v2-0-acceptance.md` |
| 10.3 | Playwright（skill：`webapp-testing`）：登录→同步→冲突→迁移向导主链路（mock provider） | 待用例入库 |

## 依赖与并行

- Phase 0 前置阻塞全部（0.4 拍板阻塞 1.5 真端点配置，不阻塞 fake 开发）。
- 串行主干：1 → 2 → 3 / 4（3、4 依赖 2 的实体模型，3 与 4 可并行）。
- Phase 5 独立可并行；6 依赖 1–5；7 依赖 6；8 依赖 6；9 → 10。
- 每期独立可合入；未完成的云命令不挂 IPC（沿「soon 返回 FEATURE_SOON，禁止假成功」）。

## 复用清单（新依赖三条，均有理由）

| 依赖 | 用途 | 理由 |
|------|------|------|
| `ureq` 2.x（rustls，阻塞式） | 云协议 HTTP 客户端 | core 零 http 能力（updater 是壳层 Tauri 插件）；reqwest 重（tokio 全家）；ureq 小且活跃 |
| `argon2` | vault KDF | RustCrypto 系，活跃维护；自研 KDF 禁止 |
| `chacha20poly1305` | vault 对称加密 | 同上；XChaCha20-Poly1305 是文件加密惯例 |

其余零新依赖：sha2（设备 id）复用；FakeCloudProvider 沿 FakeRunner/GitRunner 注入先例；导出/落盘语义复用 pkg.rs；安装链复用 toolchain/install.rs。与 1.5 复用核查惯例一致，结论实现期回填本文件。
