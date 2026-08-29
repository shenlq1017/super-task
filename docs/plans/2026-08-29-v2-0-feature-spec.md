# v2.0 功能规格：云（账号 / 同步 / 一键迁移）

> 2026-08-29。状态：**客户端自动化范围与自托管参考服务本地 HTTP/API 已落地；正式 HTTPS 部署、真机验收和正式端点运营方仍待完成/拍板。**
> 实现计划：[2026-08-29-v2-0-implementation-plan.md](2026-08-29-v2-0-implementation-plan.md)。
> 一句话：**本地优先不变，云是可选增强——账号登录、工作区/模板/设置按实体同步（密钥默认不上云，勾选才走端到端加密）、新机器一键迁移（拉账号 + 工具链差量安装），顺手关掉发布工程欠账（签名 / updater 真端点 / 安装包）。**

---

## 1. 背景与版本序列

- 1.x 功能面（1.0–1.7：工作区 / 启停 / 日志 / 配置 / 环境 / 容器 / 网关 / CLI / MCP / 导出包 / 横向扩展）落地后，roadmap 2.0 主题是「能上云」：账号、同步、一键迁移（原需求 8 / 9）。
- **版本序列与前置**：v1.7 → 验收专项（inv-4 B1–B6 真机矩阵 + C1/C2 发布工程）→ **v2.0** → v2.1 → v2.2。v1.8 预留给横向扩展的真机反馈迭代（如 python package_manager 探测，v1.7 规格 §15），不阻塞 2.x 主线。
- 本版本另承担「产品出门」职责：账号体系上线意味着自动更新与分发必须从占位变真实（C1/C2 在 Phase 0 关闭，inv-4 C 类清账）。

## 2. 原则

1. **本地优先**：未登录 / 离线 / 云故障时，全部既有功能零变化；云命令失败只报错，永不阻塞本地操作。
2. **密钥永远本地优先**（roadmap 原则 4）：默认不同步；显式勾选 + 端到端加密后才上云；服务端只见密文。
3. **遥测默认关**（roadmap「全程」）：开启也只发白名单枚举事件。
4. **云是备份与搬运工，不是控制面**：同步只落盘，绝不自动启动服务（与 1.5 import「只落盘不启动」语义一致）；无远程启停。
5. **协议可自托管**：API 契约公开（docs/spec/cloud.md 为真源），端点可配置；官方端点是默认值不是依赖。仓库的 `supertask-cloud-server` 是已可本地联调的参考服务，不能替代已验收的官方生产服务。

## 3. 目标与非目标

**目标**：

1. 账号登录 / 登出 / 会话保持（email + 密码，token 刷新）；
2. 实体同步：工作区（yaml）、模板（local 来源）、应用设置白名单——修订 / 冲突模型明确，无静默覆盖；
3. 密钥同步（opt-in，E2E 加密 vault）；
4. 一键迁移向导（登录 → 选实体 → 落盘 → 工具链差量安装）；
5. 遥测（opt-in，最小事件集）；
6. 发布工程收口（C1 签名与 updater 真端点、C2 安装包与 release 流水线）；
7. CLI `cloud` 子命令（与桌面共享会话）。

**非目标**：

- 远程控制服务启停（云不是控制面）；
- 实时协同 / 多人共享工作区（实体级备份同步，非 CRDT、非实时）；
- 密钥的免 passphrase 跨设备解密（passphrase 用户自管，丢失不可恢复——文档化并 UI 明示）；
- OAuth 第三方登录（后续候选；本期 email+密码 + 可自托管）；
- 日志 / 指标 / 进程数据上云；
- 移动端 / 网页控制台；
- `supertask.ai` 密钥上云（v2.1 的 AI key 永不入 vault，见 v2.1 规格 §4.1）。

## 4. 总体架构

```
┌─ 前端：/cloud 页 + welcome「从云端恢复」+ settings ┐
│ 壳层：cloud.* IPC（8 条，§11）                     │
│ core：cloud/ 模块（新）                            │
│   provider.rs   trait CloudProvider                │
│     ├ HttpCloudProvider（ureq + rustls，真端点）   │
│     └ FakeCloudProvider（内存实现，测试/CI）       │
│   session.rs    token 存储 / 刷新 / 设备 id        │
│   sync.rs       实体模型 / rev / 冲突 / 推拉引擎   │
│   crypto.rs     vault 端到端加密                   │
│   migrate.rs    恢复向导数据流 / 工具链差量        │
│   telemetry.rs  opt-in 事件批量上报                │
└──────────── HTTPS（契约：docs/spec/cloud.md）──────┘
               同步服务（可独立部署 / 自托管）
```

- 依赖注入沿 `GitRunner` / `ValidateRunner` / `FakeRunner` 先例：core 不写死网络实现，单测与 CI 全走 `FakeCloudProvider`，**CI 零真实网络**。
- 本地状态：`%APPDATA%/SuperTask/cloud/`（`session.json`、`state.json`）。工作区 ↔ 云端实体的映射写进本地工作区注册（appdata recents 条目增 `cloud_id` 字段）。

## 5. 账号与会话

- **登录**：email + 密码 → 服务端返回 access_token（短时效）+ refresh_token（长时效）。密码只出现在登录请求体，不落盘、不进日志。
- **会话存储**：appdata `cloud/session.json`。Windows 上优先 DPAPI（`CryptProtectData`）静态加密——`windows` crate feature 增补在实现期核查（Phase 1 任务）；不可用则回退受限 ACL 明文并在 cloud.md 标注。**CLI 与桌面共享同一会话文件**（§14）。
- **刷新**：请求遇 401 → 自动 refresh 一次 → 重放；refresh 失效 → `CLOUD_AUTH_FAILED` 并转登出态。
- **设备标识**：`sha256(hostname + 首次运行时间戳)` 十六进制（复用 sha2，零新依赖）。用于实体 `updated_by` 展示与排障，不用于追踪。
- **登出**：清 session.json（保留 state.json 与全部本地数据）；服务端可另设设备吊销（服务端职责，不在客户端范围）。

## 6. 同步模型

### 6.1 实体

| type | 内容 | id | 说明 |
|------|------|----|------|
| workspace | `{name, yaml 文本, group?}` | 服务端分配 uuid | 只同步 yaml 与名称，**不同步本地路径**；拉取时用户选目标目录 |
| template | 模板目录文件集 | 模板 id（沿 1.1 模板升级的 id 概念） | 仅 local 来源模板；builtin 不上云 |
| settings | 应用设置白名单 JSON | 固定 `"app-settings"` | language、通知开关、网络 app 默认（proxy 模式 / mirror 等）；**不含任何路径与密钥** |
| secrets.vault | E2E 密文 blob | 固定 `"vault"` | 见 §7 |
| kind | 自定义 kind 插件包 | kind id | **v2.2 增**（协议前向兼容规则生效，见 §10） |

### 6.2 修订与冲突

- 服务端 per-entity 单调递增 rev（轻量 MVCC）：PUT 带 `base_rev`，不匹配 → 409。
- 本地 `state.json` 记每个实体的 `base_rev` + `last_synced_hash`。
- **dirty 判定**：当前内容 hash ≠ `last_synced_hash`。
- **同步算法**（`cloud.sync` 单命令完成，两阶段）：
  1. pull：服务端 rev > 本地 base_rev 的实体进入「待应用」；
  2. push：本地 dirty 实体逐个 PUT（base_rev = 本地记录值）；
  3. 冲突：本地 dirty 且服务端 rev 也已前进 → 409 → 记入冲突列表，**两端内容都保留**，绝不静默覆盖；
  4. 冲突解决（`cloud.resolve`）：keep-local / keep-server / keep-both（both 对 workspace 生成「副本」实体，副本命名规则实现期定）。
- **与本地 base_hash（YAML_CONFLICT）的关系**：云 rev 管云端副本，磁盘 base_hash 管本地编辑，互不替代。拉取落盘按「外部写入」处理并同步更新 base_hash；目标文件已有本地未同步修改 → 走冲突流程，不覆盖。
- **打开中的工作区**（引擎持有 / 锁定，1.5 `WORKSPACE_LOCKED`）：拉取挂起该实体并提示；关闭工作区后重试可应用。同步绝不在服务运行中写该工作区 yaml。

### 6.3 触发方式

- 手动：/cloud「立即同步」+ 命令面板 + CLI `supertask cloud sync`。
- 自动（默认开、可关，仅登录后生效）：应用启动 30s 后 + 每 15 分钟；静默执行 pull+push，冲突只计数不弹窗。

## 7. 密钥同步（opt-in + E2E）

- yaml secrets 声明处新增可选 `sync: true`（默认 false，yaml.md 增量）；仅 backend=local/file 可勾选（env 后端是引用，无内容可同步）。
- **vault**：所有勾选 secret 打包 JSON → XChaCha20-Poly1305 加密（key = argon2id(passphrase, salt)）→ base64 实体。nonce 每次随机；AAD = 账号 id；salt 存实体元数据（明文，无泄密风险）。
- **passphrase**：在 /cloud 设置；本地缓存经 DPAPI 包裹（同 session 口径）；**丢失 = 云端 vault 不可恢复**，设置时 UI 明示。
- 拉取端：输入 passphrase → 解包 → 写回 secrets 存储（**不写进 yaml 明文**）；与目标已有 secret 同名 → keep-both 生成后缀副本，不覆盖。
- 未设 passphrase 但存在勾选 secret 的实体 → `CLOUD_ENCRYPT_REQUIRED`。

## 8. 一键迁移

向导入口：welcome「从云端恢复」+ /cloud「迁移」卡。步骤：

1. 登录；
2. 展示实体清单（工作区 / 模板 / 设置 / 密钥可选勾选）；
3. 为工作区选落盘根目录（统一或逐个；复用 1.5 import 的落盘语义：目标已有 yaml 拒绝并提示，不覆盖）；
4. （可选）输入 passphrase 恢复密钥；
5. **工具链差量**：对每个工作区 `ToolchainSpec` 钉扎 × 当前 `ToolchainProbe` 逐项比对 → 缺失清单 → 一键安装（**复用 1.2 mise/winget 安装链，显式点击，不代装原则不变**）；未钉扎项跳过；版本不匹配给 warning 不自动升降级；
6. 完成页「打开首个工作区」。

## 9. 遥测（默认关）

- 事件枚举（**全集**，白名单外不存在）：`app_start` / `app_stop` / `feature_open(feature_id)` / `service_start(kind)`。无路径、无名称、无 yaml 内容。
- 批量上报：每 24h 或退出时一次。
- 关闭 = 完全 no-op（**零网络请求**，有单测断言）。
- settings 开关默认关；开关状态本身不在同步白名单内（避免设备间互相改隐私设置）。

## 10. 服务端协议（摘要；真源为 docs/spec/cloud.md）

- REST / JSON over HTTPS：
  `POST /auth/login`、`POST /auth/refresh`、`GET /entities?type=`、`GET /entities/:id`、`PUT /entities/:id`（base_rev 乐观并发 → 409）、`DELETE /entities/:id`、`POST /telemetry/batch`。
- 实体信封：`{id, type, rev, updated_at, updated_by(device), data | encrypted_blob}`。
- HTTP → 错误码映射：401/403 → `CLOUD_AUTH_FAILED`；409 → `CLOUD_SYNC_CONFLICT`；413/429 → `CLOUD_QUOTA_EXCEEDED`；其余非 2xx / 无法解析 → `CLOUD_PROTOCOL_ERROR`；连接失败 / 超时 → `CLOUD_OFFLINE`。
- 配额：按账号实体数 + 总字节数；超限 `CLOUD_QUOTA_EXCEEDED`（/cloud 展示用量）。
- **前向兼容**：未知 entity type → 客户端 skip 并在状态中报告（不报错）——v2.2 的 kind 实体依赖此规则；未知 API 版本 → `CLOUD_PROTOCOL_ERROR`。
- 服务端运营归属仍是**开放问题 #1**（§18）：客户端 FakeCloudProvider 与仓库参考 server 的本地自动化范围已完成，但场景 2/4/5/9 的真实端点验收必须在运营方拍板并完成正式 HTTPS 部署后进行。详见 [cloud-server.md](../spec/cloud-server.md)。

## 11. IPC 契约增量（ipc.md 增 §10.12）

| 命令 | 入参 | 出参 / 要点 |
|------|------|-------------|
| cloud.login | `{email, password}` | 会话建立；失败 `CLOUD_AUTH_FAILED` |
| cloud.logout | — | 清会话，保留本地数据 |
| cloud.status | — | 登录态 / 账号 / 设备 / 最近同步时间 / 冲突数 / 配额用量 |
| cloud.sync | — | 执行 §6.2 算法；返回冲突列表（若有） |
| cloud.resolve | `{entity_id, choice}` | choice ∈ local / server / both |
| cloud.migrate.plan | — | 实体清单 + 工具链差量（§8 步骤 5） |
| cloud.migrate.apply | `{workspaces:[{entity_id, dir}], include_templates, include_settings, passphrase?}` | 落盘 + 安装触发（安装结果逐项返回） |
| cloud.telemetry.set | `{enabled}` | 默认 false |

- `features.rs`：cloud → `Live`（since 2.0）；`SOON_COMMANDS` 移除 `cloud.login` / `cloud.sync`（features.rs:51-55）。
- 同步为短命令非流式，**不新增事件流**（不做进度事件；向导安装进度复用既有 operation 事件桥，实现期核对）。

## 12. 错误码汇总

| 码 | 场景 |
|----|------|
| `CLOUD_NOT_LOGGED_IN` | 未登录调用需会话的云命令 |
| `CLOUD_AUTH_FAILED` | 登录失败 / refresh 失效 |
| `CLOUD_OFFLINE` | 网络不可达 / 超时 |
| `CLOUD_SYNC_CONFLICT` | 存在待解决冲突（sync 返回冲突列表时的汇总码） |
| `CLOUD_ENCRYPT_REQUIRED` | 勾选密钥同步但未设 passphrase |
| `CLOUD_QUOTA_EXCEEDED` | 超配额（413/429） |
| `CLOUD_PROTOCOL_ERROR` | 服务端异常响应 / 协议版本不识别 |

## 13. 前端

- **/cloud**（导航「扩展」组，soon → live）：登录卡（email/password 表单）→ 登录后：账号与会话卡（设备名、登出）/ 同步中心（实体列表 + 状态 + 冲突三选一）/ 迁移卡 / 配额展示。
- **welcome**：与「导入工作区包」并列新增「从云端恢复」路径（首启 onboarding 不动本地导入）。
- **settings**：遥测开关（默认关，文案明示发什么）、passphrase 管理、云端点配置（高级，默认官方）。
- 浏览器 dev 模式：前端 mock IPC 增 mock provider（含冲突旋钮，对齐 mock dirty 态的确定性规则先例）。
- 命令面板：「立即同步」「打开云页」；无死链。
- 按钮语义按 AGENTS.md 约定：登录/立即同步 = default，解决冲突 = outline，登出 = outline、清除云端数据（若有）= destructive。

## 14. CLI

- `supertask cloud status | sync | logout`：**共享 appdata 会话文件**（CLI 不处理密码；登录只发生在桌面端）。未登录 → `CLOUD_NOT_LOGGED_IN` + 人话提示「请在桌面端登录」。
- `--json` 错误码与 IPC 同表（沿 1.5 约定）。

## 15. Phase 划分（概览）

0 发布工程收口与拍板 → 1 cloud 骨架（provider / session / fake）→ 2 同步引擎 → 3 密钥 E2E → 4 一键迁移 → 5 遥测 → 6 壳层 IPC + feature 转 live → 7 前端 → 8 CLI → 9 文档闭环 + 全量回归 → 10 验收（fake 全链路 CI + 真机 + Playwright）。任务级拆解见实现计划。

## 16. 验收标准（场景矩阵）

1. 未登录：全部既有功能零变化；/cloud 显示登录引导；命令面板无云动作死链。
2. 登录成功：/cloud 显示账号与设备信息；access 过期自动刷新重放成功；登出后本地一切可用、数据完整。
3. 网络断开：`cloud.sync` 返回 `CLOUD_OFFLINE` 人话错误；本地启停/日志/配置不受任何影响。
4. 推送：新建工作区 A 并修改 yaml → sync → FakeCloudProvider 收到实体且 rev+1；local 模板同步后第二会话可见。
5. 拉取：第二设备（隔离 appdata 的同账号 fake）登录 → sync → 工作区 A 的 yaml 落盘到所选目录，**不自动启动**。
6. 冲突：两端修改同一实体 → sync 返回冲突列表 → 三选一解决后两端一致；全程无静默覆盖（fake 侧断言旧值未被覆盖）。
7. 密钥默认不同步：含 secrets 的 yaml 同步后云端无 vault 实体；勾选 `sync: true` 且设 passphrase 后 vault 为密文（fake 侧断言 blob 不可读、明文不出现在任何请求）。
8. passphrase 丢失路径：清空本地凭据后仅凭账号无法恢复 vault（文档化行为 + 设置时 UI 警示存在）。
9. 一键迁移向导：全新 appdata → 登录 → 选工作区+模板 → 工具链差量（fake probe missing → FakeRunner 安装成功）→ 打开工作区可用。
10. 工作区打开（锁定）时拉取：该实体挂起 + 提示（`WORKSPACE_LOCKED` 交互一致）；关闭工作区后重试成功。
11. 遥测：默认关 → 零网络请求（断言）；开启后仅枚举事件、无路径 / yaml 内容（请求体断言）。
12. CLI：`supertask cloud status/sync/logout` 读共享会话；未登录人话报错；`--json` 错误码与 IPC 同表。

## 17. 安全与隐私清单

- 密码不落盘、不进日志；token 静态加密（DPAPI 优先）且登出即清。
- 默认零上传：未登录 / 未开自动同步时无云请求；遥测关闭零请求（单测断言）。
- 上传内容审计面：yaml（用户可见）、local 模板、设置白名单、（勾选后）密文 vault。**无本地路径**——workspace 实体设计上不含路径。
- 健康检查 / 服务流量与云零关系（云不是代理）。

## 18. 风险与开放问题

1. **服务端运营方**（最大开放问题）：官方端点谁部署 / 维护 / 付费；协议先行 + fake 使客户端可独立交付，但场景 2/4/5/9 的真机验收需要真实端点——**kickoff 前拍板**（候选：自建最小服务 / 延后真机验收项 / 先只交付自托管文档）。
2. token 静态加密的 DPAPI feature 名核查（实现期，Phase 1）。
3. 自动同步默认开的取舍：15min 频率对自托管小服务端的影响——保守方案是自动同步默认关；实现期随开放问题 1 一并拍板。
4. keep-both 副本命名与去重规则（实现期定，写入 cloud.md）。
5. 迁移差量在多工作区多钉扎时安装时长较长：UI 需进度与逐项取消（复用 operation 事件）。
