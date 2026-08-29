# SuperTask 云参考服务（2.0）

> 状态：**自托管参考服务的 HTTP router/API、SQLite migration、seed、配额和遥测最小实现已落地；尚未进行正式生产部署与真机运营验收。**
> `crates/supertask-cloud-server` 已加入 Cargo workspace，集成测试通过本地 in-process router + 临时 SQLite 运行，CI/测试不访问公网。
> 本文区分「协议要求」「当前代码事实」和「生产部署限制」，不表示已有官方运营端点或生产级部署配置。
>
> 客户端协议真源：[cloud.md](cloud.md)。客户端端点默认为占位值
> `https://cloud.supertask.local.example`；官方运营方尚未拍板。本文描述的本地参考服务默认使用
> `127.0.0.1:8787`，两者不是同一个默认值。

## 1. 范围与非目标

参考服务用于本地开发、自托管协议联调和客户端真机验收准备，覆盖：

- email + password 登录、短期 access token、refresh token 轮换；
- 按账号隔离的实体列表、读取、乐观并发写入和删除；
- 实体数量与 JSON data 字节数配额；
- 遥测批次接收的最小接口（客户端默认不发送）；
- SQLite 持久化和开发 seed 账号。

不在服务端本期范围内：远程启停、实时协同、OAuth、网页控制台、移动端、日志/指标上云、
客户端密码找回，以及官方生产端点和 TLS 证书运营。生产部署应放在 HTTPS 反向代理后；参考
服务自身不承担正式证书托管。

## 2. 当前代码边界

### 2.1 workspace 与 crate

- crate：`crates/supertask-cloud-server`，二进制名：`supertask-cloud-server`。
- 根 `Cargo.toml` 已将其列为 workspace member。
- 依赖方向独立于 `supertask-core` 的引擎/桌面模块；服务端 DTO 与客户端通过 JSON 契约对齐。
- 已有模块：`lib.rs`、`config.rs`、`auth.rs`、`entities.rs`、`state.rs`、`error.rs`、`http.rs`、
  `quota.rs`、`telemetry.rs` 和 `migrations/0001_init.sql`；`tests/api.rs` 覆盖本地 router 的
  health/login/refresh/auth、entity CRUD/rev 冲突、配额和 telemetry 主路径。
- 二进制启动流程读取配置、执行 migration、按需 seed、绑定 listener，并注册 `/healthz`、认证、
  entities、quota 和 telemetry 路由；当前已可用于本地联调，但仍不等于生产部署或官方端点。

### 2.2 客户端配套边界

- `supertask-core` 的 `HttpCloudProvider` 使用 `ureq + rustls`，端点可传入；错误映射和
  transport fake 测试不访问公网。
- `src-tauri` 的 `CloudHandle` 已有端点校验/重载能力；`cloud.endpoint.set` 已注册为 Tauri IPC，
  成功后持久化 `AppData.cloud_endpoint` 并切换后续 provider。浏览器 mock 保留
  `supported: false, local_only: true` 降级口径。
- 客户端已接入「401 → refresh 一次 → 重放一次」：刷新 token 先保存，再重放原操作；刷新失败
  清理本地 session 并返回 `CLOUD_AUTH_FAILED`。

## 3. 配置与本地启动约束

### 3.1 环境变量

| 变量 | 默认值/必填 | 语义 |
|---|---|---|
| `SUPERTASK_BIND` | `127.0.0.1:8787` | 监听地址；默认只监听 loopback |
| `SUPERTASK_DATABASE_URL` | `sqlite://supertask-cloud.db` | SQLx SQLite URL；`:memory:` 仅用于进程内测试 |
| `SUPERTASK_DEV_SEED` | 未设置（关闭） | `1` 或 `true` 时启用开发 seed |
| `SUPERTASK_SEED_EMAIL` | `demo@supertask.local` | seed 账号邮箱；会规范化为小写 |
| `SUPERTASK_SEED_PASSWORD` | **无默认值** | 开启 seed 时必须在运行时注入；本文和仓库不提供密码 |
| `RUST_LOG` | 由 tracing subscriber 环境决定 | 日志过滤配置；不得把 token、密码、Authorization 或实体明文写入日志 |

开发 shell 只应在运行时注入 seed 密码，不把它写入脚本、命令历史、`.env`、migration、
测试输出或提交文件。例如先在当前 shell 设置 `SUPERTASK_DEV_SEED=1`、邮箱和密码，再启动
服务；密码值不属于本仓库文档内容。

启用 seed 但未设置非空 `SUPERTASK_SEED_PASSWORD` 时，配置解析必须失败；禁止生成默认密码。
seed 以邮箱幂等 upsert，密码以 Argon2 PHC hash 保存，每次 hash 使用随机 salt。

### 3.2 绑定与数据库

- 默认 bind 必须保持 loopback，适合本机客户端联调。
- 配置非 loopback 时，启动日志会明确提示服务已暴露到 loopback 之外；部署者仍必须把服务放在 HTTPS
  反代和访问控制之后。当前实现不是生产级 TLS/访问控制配置，不能直接暴露公网。
- SQLite 父目录应由启动流程创建；生产数据库不得指向测试临时目录。
- `AppState` 通过 SQLx pool 共享配置；当前 pool 上限为 8 个连接，并执行内置 migration。
- 不使用全局可变业务状态；账号、token、实体和遥测摘要均按数据库账号隔离。

本地参考服务的开发启动方式为：

```text
cargo run -p supertask-cloud-server
```

启动流程会读取环境变量、执行 SQLx migration、按需创建 seed 账号并监听配置地址；默认监听
`127.0.0.1:8787`。客户端仍需通过 `cloud.endpoint.set` 或对应设置选择本地 URL；正式端点
不应被本地地址替换。

## 4. 数据库与存储约束

`migrations/0001_init.sql` 当前定义以下表：

| 表 | 当前字段/约束 | 用途 |
|---|---|---|
| `accounts` | `id` 主键、`email` unique、`password_hash`、`created_at`、`disabled` | 账号和 Argon2 密码 hash |
| `access_tokens` | `token_hash` 主键、`account_id`、`device_id`、`expires_at` | 短期 access token 的 hash |
| `refresh_tokens` | `token_hash` 主键、`account_id`、`device_id`、`expires_at`、`revoked_at` | 可撤销、可轮换 refresh token 的 hash |
| `entities` | `(account_id,id)` 主键、`type`、`rev`、`updated_at`、`updated_by`、`data`、`byte_size` | 账号隔离的 JSON 实体 |
| `telemetry_batches` | 自增 `id`、`account_id`、`received_at`、`event_count` | 有界遥测批次摘要 |

约束：

1. 服务端数据库只保存密码 hash、token hash 和实体 JSON；access/refresh 原文只在登录/刷新
   响应和客户端会话容器中短暂存在。
2. `entities.data` 是 JSON 文本；`byte_size` 固定按 UTF-8 JSON data 字节数计，不含 envelope、
   SQLite 索引和表结构。
3. `(account_id,id)` 是实体隔离边界；同一个 id 在不同账号下互不相见。
4. 实体 `id` 由客户端提供，不由服务端另开 create 端点或重新分配。它必须是稳定、不可从
   URL 语义推断业务含义的 opaque id；服务端只做格式/长度校验，不把它当本地路径。
5. 当前参考 schema 没有 `accounts.updated_at` 等计划外字段；新增字段需另开 migration，
   不得在文档中预先当作已存在。
6. 服务端不保存本地路径、工作区进程信息、服务日志、YAML 之外的客户端环境变量、密码或
   token。遥测只允许白名单事件及必要的有界摘要。

默认配额由当前 `Config` 固定为 **100 个实体、10,000,000 字节**。服务端实现可以在未来
把配额移入部署配置，但必须保持 `/quota` 字段和超限事务回滚语义。

## 5. 认证 API 契约

### `POST /auth/login`

请求：

```json
{"email":"user@example.com","password":"<runtime value>"}
```

成功响应：

```json
{
  "account_id": "acct-...",
  "email": "user@example.com",
  "access_token": "<opaque>",
  "refresh_token": "<opaque>",
  "expires_in_secs": 900
}
```

要求：邮箱和密码为空或认证失败统一 HTTP 401；不得区分「邮箱不存在」和「密码错误」。当前
认证模块的 access token 时长为 900 秒，refresh token 时长为 30 天。登录日志不得记录密码、
token、完整邮箱或 Authorization。

### `POST /auth/refresh`

请求：`{"refresh_token":"<opaque>"}`。

- 只接受未撤销且未过期的 refresh token；失败统一 HTTP 401。
- 成功后撤销旧 refresh token，签发新的 access/refresh token 对，并返回同一响应形状。
- 客户端遇到任何需要认证的请求 HTTP 401 时，只允许 refresh 一次，然后**只重放原请求一次**。
- refresh 失败时客户端清理本地 session，返回 `CLOUD_AUTH_FAILED`；不得无限重试或循环刷新。
- 登录和 refresh 本身不再递归 refresh。

当前 `auth.rs` 已实现 token hash、过期和 refresh rotation 的数据层语义；HTTP router 已调用
对应认证 handler。服务端响应使用 `{error, code, message}` 最小错误对象，客户端仍按 HTTP 状态
映射稳定 `CLOUD_*` 错误码。

本期不增加 `POST /auth/logout`：客户端 `cloud.logout` 清理本地会话并保留本地数据/同步状态；
服务端 token 吊销 API 如需增加，必须另行扩展协议。

## 6. 实体 API 契约

所有实体 API 除登录外都要求 `Authorization: Bearer <access_token>`。实体信封为：

```json
{
  "id": "client-owned-opaque-id",
  "type": "workspace",
  "rev": 1,
  "updated_at": 1710000000,
  "updated_by": "device-id-or-opaque-client-id",
  "data": {"name":"demo","yaml":"version: 1\n"}
}
```

### id 与 type

- `id` 使用客户端提供的稳定 opaque id；服务端以 URL `/entities/:id` 的 id 为准。
- 参考实现的格式护栏：非空、最多 128 个字节，只允许 ASCII 字母/数字、`.`、`-`、`_`；拒绝
  路径分隔符、控制字符和路径穿越语义。
- `type` 是实体类型字符串；已知类型包括 `workspace`、`template`、`settings`、`secrets.vault`，其中
  `secrets.vault` 必须保留点号拼写。参考服务仅校验非空、长度和安全 ASCII 字符，并保存未知 type，
  以保持未来 kind 扩展的前向兼容。
- 客户端列表解析逐项处理未知 type，加入 `skipped` 并报告，其他合法实体继续同步。

### `GET /entities?type=`

无 `type` 返回当前账号的全部实体；有 `type` 只返回该类型。响应是实体信封数组。服务端按
账号过滤，不得返回其他账号的数据。非法的查询参数格式由服务端返回 400；未知但格式合法的
实体 type 可被保存和查询，是否能被客户端应用由客户端前向兼容规则决定。

### `GET /entities/:id`

按账号和 opaque id 查找；不存在 HTTP 404。响应返回完整实体信封。

### `PUT /entities/:id`

请求体必须包含 `type`、`data`、`base_rev`，可选 `updated_by`：

```json
{
  "type": "workspace",
  "data": {"name":"demo","yaml":"version: 1\n"},
  "base_rev": 0,
  "updated_by": "device-id"
}
```

- 新建要求 `base_rev=0`，服务端写入 `rev=1`。
- 更新要求 `base_rev` 等于当前实体 rev；不匹配返回 HTTP 409，绝不覆盖当前内容。
- 成功更新 rev 单调递增，并返回完整信封。
- `type` 非空且不超过参考实现的 64 字节护栏，只允许安全 ASCII 字符；未知但格式合法的 type
  会被保存并在列表中返回，客户端负责逐项 skip 无法应用的未来类型。
- 写入应在单事务中完成实体、rev、`updated_at`、`updated_by` 和 `byte_size` 更新；超配额时
  整体回滚并返回 HTTP 413 或 429。
- `data` 必须是合法 JSON；vault 数据不在服务端解密，只按 `{blob,salt}` 密文载荷保存。

### `DELETE /entities/:id`

按账号删除实体，幂等返回 204 或 200；不得删除其他账号同 id 的实体。

## 7. 配额与遥测

### `GET /quota`

成功响应：

```json
{"entities":0,"entities_max":100,"bytes":0,"bytes_max":10000000}
```

配额按账号统计实体数量和 `data` UTF-8 字节数。PUT 任一限额超出时返回 413/429，客户端映射
为 `CLOUD_QUOTA_EXCEEDED`。

### `POST /telemetry/batch`

请求只允许客户端白名单事件：

- `app_start`
- `app_stop`
- `feature_open`（带 feature id）
- `service_start`（带 kind）

服务端必须限制 body 和 batch 数量，拒绝或逐项跳过未知事件；不得接受路径、工作区名称、
YAML 内容、命令行、环境变量值、密码、token 或 Authorization。客户端遥测默认关闭，关闭时
必须零请求。参考 schema 只保留有界批次摘要，生产存储策略仍待运营方决定。

## 8. 错误、日志和安全边界

客户端错误映射保持 [cloud.md](cloud.md)：401/403 → `CLOUD_AUTH_FAILED`，409 →
`CLOUD_SYNC_CONFLICT`，413/429 → `CLOUD_QUOTA_EXCEEDED`，其他非 2xx/解析失败 →
`CLOUD_PROTOCOL_ERROR`，传输失败/超时 → `CLOUD_OFFLINE`。

服务端当前 `AppError` 的响应形状是最小的 `{ "error": "...", "code": "...", "message": "..." }`；
客户端仍以 HTTP 状态和稳定 `CLOUD_*` code 为主，不依赖服务端回显的敏感请求字段。服务端内部
错误只记录服务端日志中的错误摘要，不向客户端返回数据库细节。

最低安全要求：

1. 密码只用于登录请求和 Argon2 校验，永不落盘为明文、永不进入日志/telemetry。
2. 数据库只存 token hash；日志不得记录 token、Authorization 或实体明文。
3. 所有读写均以 bearer token 得到的 account id 为边界；禁止客户端传 account id 选择租户。
4. 默认 loopback；生产使用 HTTPS 反代、访问控制、备份和数据库权限隔离。
5. 服务端不启动客户端工作区服务，也不执行实体中的命令或 YAML。

## 9. 健康检查与验收状态

`GET /healthz` 不要求登录，仅用于本地启动等待和测试，返回 `{ "status": "ok" }`，不泄露账号、
配置或 seed 密码。当前已由 router 注册。

当前服务端自动化验收清单：

- [x] axum router 与 REST handler；
- [x] `/healthz`、优雅关闭和 migration 启动链；
- [x] 临时 SQLite API 集成测试，不绑定固定端口、不访问公网；
- [x] seed 幂等、账号隔离、token 过期/撤销/rotation；
- [x] PUT type + opaque id + rev/409 + 配额事务回滚；
- [x] telemetry 白名单、body 限制和敏感字段拒绝；
- [x] 客户端 401 refresh/replay 一次自动化测试；
- [x] 参考服务放行格式合法的未知 type，并由客户端逐项 skip；
- [ ] 正式端点运营、HTTPS 部署和 v2.0 真机验收。

相关实现进度见 [v2.0 implementation plan](../plans/2026-08-29-v2-0-implementation-plan.md)。
