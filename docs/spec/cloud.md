# SuperTask 云协议（2.0）

> 状态：v2.0 客户端自动化范围已实现（FakeCloudProvider 全覆盖，CI 零真实网络）；真实 HTTP
> 客户端已接入 refresh/replay 一次和端点设置 IPC，服务端本地 router/API 及 in-process 集成测试
> 已落地；正式端点运营、HTTPS 部署和真机验收仍未完成。
> **服务端实现与运营归属 = 开放问题 #1（v2.0 规格 §18）**：仓库已有
> `crates/supertask-cloud-server` 参考服务 crate，默认 loopback + SQLite + 可选开发 seed。端点可
> 自托管，内置客户端占位端点 `https://cloud.supertask.local.example` 待运营方拍板替换。本文为协议真源。

## 1. 原则（v2.0 规格 §2 摘要）

1. 本地优先：未登录/离线/云故障零影响。
2. 密钥默认不上云：当前客户端没有任何上传/下载 `secrets.vault` 实体的代码路径（同步绑定仅覆盖 settings / template / workspace），vault 从不出本机；vault 的 E2E 加密能力（argon2id + XChaCha20-Poly1305，AAD=账号 id）已在客户端实现，供未来启用。协议中的 `sync: true` 勾选开关为设计预留，当前代码不存在该配置项。
3. 遥测默认关；关闭 = 零网络请求（有单测断言）。
4. 云是备份与搬运工：只落盘，绝不自动启动服务；无远程启停。

## 2. REST 端点

| 方法 | 路径 | 说明 |
|------|------|------|
| GET | `/healthz` | 探活；客户端 `cloud.status` / 同步前用其探测可达性 |
| POST | `/auth/login` | `{email, password}` → `{account_id, email, access_token, refresh_token, expires_in_secs}` |
| POST | `/auth/refresh` | `{refresh_token}` → 同上；服务端只接受未撤销且未过期 token，成功后撤销旧 refresh token 并轮换新 token |
| GET | `/entities?type=` | 实体列表 |
| GET | `/entities/:id` | 单实体（客户端仅冲突回退时调用） |
| PUT | `/entities/:id` | `{type, data, base_rev, updated_by?}`；客户端提供稳定 opaque id；rev 不匹配 → **409** |
| DELETE | `/entities/:id` | 删除；服务端恒返回 **204** |
| GET | `/quota` | `{entities, entities_max, bytes, bytes_max, by_type?}`（`by_type` 为各实体类型用量明细，可缺省） |
| POST | `/telemetry/batch` | 需 `Authorization: Bearer` 认证；`{events: [...]}`（白名单事件全集：app_start/app_stop/feature_open/service_start） |

## 3. 实体信封

`{id, type, rev, updated_at, updated_by(device), data}`；`id` 由客户端提供，是账号范围内稳定的
opaque id，不由服务端另行分配；服务端只做格式/长度护栏，不把它解释为本地路径。`type ∈
workspace | template | settings | secrets.vault`（协议保留未来类型）。参考服务仅做安全字符、长度和非空校验，保存未知 type；对其他
查询/PUT type 仍要求格式合法；客户端列表解析逐项进行，未知 type 加入 `skipped` 并报告，不能令整批响应失败。`data`：明文 JSON 或 `{blob, salt}` 密文（vault：argon2id(passphrase, salt) →
key；XChaCha20-Poly1305；AAD=账号 id；nonce 随机）。

- workspace：`{name, yaml}`——**不同步本地路径**；拉取时用户选目标目录；目标已有 yaml 拒绝（pkg/import 语义）。
- template：`{id, files: {rel: content}}`（仅 local 来源；builtin 不上云）。**当前实现限制：模板为仅推送方向**——拉取落盘尚未实现（客户端 template 绑定的写路径恒返回未应用），pull 阶段模板会被跳过并计入待处理。
- settings：白名单 JSON（locale / 通知开关 / 网络 app 默认），固定 id `app-settings`；不含路径与密钥。
- secrets.vault：固定 id `vault`；passphrase 用户自管，**丢失不可恢复**（UI 明示）。

## 4. 修订与冲突

服务端按账号隔离实体；`(account_id, id)` 是唯一键。实体 id 由客户端提供，为稳定 opaque id；
服务端不另设 create/id 分配端点，PUT URL 中的 `:id` 是权威 id。参考服务的护栏为非空、最多
128 字节，仅 ASCII 字母/数字、`.`、`-`、`_`；具体服务可收紧但不能把 id 当路径或业务命令解析。
参考服务对 type 只执行非空、长度和安全 ASCII 字符校验，并保存未知 type 字符串以保持前向兼容；
客户端列表解析逐项处理未知 type，加入 `skipped` 并报告，不能令整批响应失败。

PUT 请求体必须为 `{type, data, base_rev, updated_by?}`。新建要求 `base_rev=0` 并产生 `rev=1`；
更新要求 `base_rev == 当前 rev`，成功后 rev 单调递增；不匹配返回 **409** 且绝不覆盖服务端内容。
`type` 必须非空且不超过参考服务 64 字节，只允许安全 ASCII 字符。写入时服务端在同一事务中更新
`type/data/rev/updated_at/updated_by/byte_size`；实体数量或 data UTF-8 字节配额超限返回 429
（`CLOUD_QUOTA_EXCEEDED`）并回滚。

本地 `%APPDATA%/SuperTask/cloud/state.json` 记 `{entities: {id: {type, base_rev, last_synced_hash, local_path?}}, conflicts, last_synced_ms}`。
dirty = 当前内容 hash ≠ last_synced_hash。同步两阶段：pull（远端 rev 更新 → 应用；本地 dirty → 冲突）
→ push（dirty PUT；409 → 冲突）。冲突两端内容都保留；`cloud.resolve` 三选一：
keep-local / keep-server / keep-both（both = 本地内容推送为 `<id>-copy` 副本实体，服务端版本落本地）。
打开中的工作区（引擎持有）：该实体挂起（pending），关闭后重试。

## 5. HTTP → 错误码映射

401/403 → `CLOUD_AUTH_FAILED`；409 → `CLOUD_SYNC_CONFLICT`；413/429 → `CLOUD_QUOTA_EXCEEDED`（当前参考服务端实际只返回 429，客户端映射保留 413 兼容）；
其余非 2xx / 解析失败 → `CLOUD_PROTOCOL_ERROR`；连接失败/超时 → `CLOUD_OFFLINE`。
401 → 客户端仅 refresh 一次，并只重放触发 401 的原请求一次（login/refresh 本身不重放）；refresh
失效 → 清理本地 session，返回 `CLOUD_AUTH_FAILED` 并转登出态。禁止无限重试或循环 refresh。
服务端 refresh 必须只接受未撤销且未过期 token，成功后撤销旧 refresh token 并签发新 token 对。

## 6. 会话与设备

会话文件 `%APPDATA%/SuperTask/cloud/session.json`：Windows DPAPI 静态加密（`encrypted: true` + hex payload），
DPAPI 不可用回退明文（本机口径记录于 1.5 惯例——登出即清）。设备 id = sha256(hostname+首启时间) 前 16 hex。
CLI 与桌面共享会话；登录只发生在桌面端（`cloud login` 无 CLI 命令）。

## 7. keep-both 命名（§18.4 决议）

副本实体 id 固定为 `<origin-id>-copy`；当前实现不做 `-copy2`、`-copy3` 递增——若该 id 在服务端已存在，PUT（base_rev=0）会按 409 冲突处理。workspace 副本不改写 name（保持原 YAML 内容原样落盘/上传）。
