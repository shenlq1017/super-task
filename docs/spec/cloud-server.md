# SuperTask 云参考服务（2.0）

> 状态：**自托管参考服务的 HTTP router/API、SQLite migration、seed、配额和遥测最小实现已落地；
> 账号管理控制台（`/admin/api/*` 管理面 + 自带 Web 前端）已落地；尚未进行正式生产部署与真机运营验收。**
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
- SQLite 持久化和开发 seed 账号；
- 面向运营方的**账号管理控制台**：`/admin/api/*` 管理面 + 同进程托管的 Web 前端，覆盖账号
  生命周期（建号、改密、停用/启用、删除）与角色（`user` / `admin`）。

不在服务端本期范围内：远程启停、实时协同、OAuth、移动端、日志/指标上云、客户端密码找回，
以及官方生产端点和 TLS 证书运营。控制台侧本期也不做：会话/设备吊销、实体内容浏览、每账号
配额覆盖、遥测聚合和管理员审计表。生产部署应放在 HTTPS 反向代理后；参考服务自身不承担正式
证书托管。

## 2. 当前代码边界

### 2.1 workspace 与 crate

- crate：`crates/supertask-cloud-server`，二进制名：`supertask-cloud-server`。
- 根 `Cargo.toml` 已将其列为 workspace member。
- 依赖方向独立于 `supertask-core` 的引擎/桌面模块；服务端 DTO 与客户端通过 JSON 契约对齐。
- 已有模块：`lib.rs`、`config.rs`、`auth.rs`、`admin.rs`、`admin_http.rs`、`entities.rs`、
  `state.rs`、`error.rs`、`http.rs`、`quota.rs`、`telemetry.rs`，migration
  `migrations/0001_init.sql` 与 `migrations/0002_admin.sql`；`tests/api.rs` 覆盖本地 router 的
  health/login/refresh/auth、entity CRUD/rev 冲突、配额和 telemetry 主路径；`tests/admin.rs`
  覆盖管理面授权、自我保护、级联删除和 role 越界。
- 二进制启动流程读取配置、执行 migration、按需 seed、按需幂等引导管理员（`admin::bootstrap_admin`）、
  绑定 listener，并注册 `/healthz`、认证、entities、quota、telemetry、`/admin/api/*` 与
  `/admin/` 静态控制台路由；当前已可用于本地联调，但仍不等于生产部署或官方端点。

### 2.2 客户端配套边界

- `supertask-core` 的 `HttpCloudProvider` 使用 `ureq + rustls`，端点可传入；错误映射和
  transport fake 测试不访问公网。
- `src-tauri` 的 `CloudHandle` 已有端点校验/重载能力；`cloud.endpoint.set` 已注册为 Tauri IPC，
  成功后持久化 `AppData.cloud_endpoint` 并切换后续 provider。浏览器 mock 保留
  `supported: false, local_only: true` 降级口径。
- 客户端已接入「401 → refresh 一次 → 重放一次」：刷新 token 先保存，再重放原操作；刷新失败
  清理本地 session 并返回 `CLOUD_AUTH_FAILED`。
- 本期管理面**零客户端改动**：`crates/supertask-core/src/cloud/`、`src-tauri/src/cloud.rs` 和
  `frontend/` 均未修改，客户端 HTTP→`CLOUD_*` 错误映射不受影响。

### 2.3 控制台前端边界

- 前端是仓库根下的独立子项目 `cloud-console/`（Vite 8 + React 19 + Tailwind 4 + shadcn /
  radix-ui），**不参与 Cargo 编译、不嵌进二进制**，构建产物 `cloud-console/dist` 由服务端
  按文件系统读取；换前端包不需要重编 Rust。
- 独立 `package.json`，刻意不引入 npm workspaces，`frontend/` 与 CI 的
  `cache-dependency-path` 互不影响。
- 控制台使用 hash 路由和 `base: "/admin/"`，因此 `/admin/` 命中 `index.html` 即可，无需 SPA
  fallback；管理员 token 存 `sessionStorage`（不写 `localStorage`），并沿用客户端的
  「401 → refresh 一次 → 只重放一次」纪律。
- 开发期 `vite.config.ts` 把 `/admin/api` 代理到服务端，浏览器侧同源，不触发 CORS。dev server
  固定 `host: "127.0.0.1"`：vite 默认按 `localhost` 解析，Windows 上落到 `::1`，文档里的
  `127.0.0.1:1430` 就连不上。**不要改成 `true`**——那会把管理面暴露到局域网。
- 根目录 `start-cloud.ps1` 是本地一键入口（Windows）：服务端与控制台 dev 各占一个窗口。窗口必须
  用 `cmd /c` 包装——`cargo.exe` 是 rustup shim，直接 `Start-Process` 到新控制台窗口会以 code 1
  退出。管理员邮箱/口令只从环境或**不回显的交互输入**取，脚本与日志不落盘；退出时按进程树
  `taskkill /T` 清理，关掉任一窗口即两端一起停。

## 3. 配置与本地启动约束

### 3.1 环境变量

| 变量 | 默认值/必填 | 语义 |
|---|---|---|
| `SUPERTASK_BIND` | `127.0.0.1:8787` | 监听地址；默认只监听 loopback |
| `SUPERTASK_DATABASE_URL` | `sqlite://supertask-cloud.db` | SQLx SQLite URL；`:memory:` 仅用于进程内测试 |
| `SUPERTASK_DEV_SEED` | 未设置（关闭） | `1` 或 `true` 时启用开发 seed |
| `SUPERTASK_SEED_EMAIL` | `demo@supertask.local` | seed 账号邮箱；会规范化为小写 |
| `SUPERTASK_SEED_PASSWORD` | **无默认值** | 开启 seed 时必须在运行时注入；本文和仓库不提供密码 |
| `SUPERTASK_ADMIN_EMAIL` | 未设置（管理面关闭） | 管理员邮箱；会规范化为小写；必须与 `SUPERTASK_ADMIN_PASSWORD` 同时设置 |
| `SUPERTASK_ADMIN_PASSWORD` | **无默认值** | 管理员口令（≥12 字符）；只能与邮箱一起注入 |
| `SUPERTASK_CONSOLE_DIR` | `cloud-console/dist` | 控制台静态资源目录；缺失时 `/admin/` 返回构建提示页 |
| `RUST_LOG` | 由 tracing subscriber 环境决定 | 日志过滤配置；不得把 token、密码、Authorization 或实体明文写入日志 |

开发 shell 只应在运行时注入 seed 密码，不把它写入脚本、命令历史、`.env`、migration、
测试输出或提交文件。例如先在当前 shell 设置 `SUPERTASK_DEV_SEED=1`、邮箱和密码，再启动
服务；密码值不属于本仓库文档内容。

启用 seed 但未设置非空 `SUPERTASK_SEED_PASSWORD` 时，配置解析必须失败；禁止生成默认密码。
seed 以邮箱幂等 upsert，密码以 Argon2 PHC hash 保存，每次 hash 使用随机 salt。

管理员引导沿用同一条「无默认密码」纪律：`SUPERTASK_ADMIN_EMAIL` 与
`SUPERTASK_ADMIN_PASSWORD` 必须**同时**存在，只设其一即 `Config::from_env` 失败，禁止补默认
口令。引导是幂等 upsert，每次启动都会把该邮箱刷成 `disabled=0` + `role=admin` 并重设口令，
因此换口令只需改环境变量重启。两者都不设置时管理面不会静默放行：`GET /admin/api/status` 报
`admin_available: false`，持有效会员凭证尝试管理登录会拿到 403 `ADMIN_NOT_CONFIGURED`，无凭证
探测仍按普通认证失败返回 401。

### 3.2 绑定与数据库

- 默认 bind 必须保持 loopback，适合本机客户端联调。
- 配置非 loopback 时，启动日志会明确提示服务已暴露到 loopback 之外；部署者仍必须把服务放在 HTTPS
  反代和访问控制之后。当前实现不是生产级 TLS/访问控制配置，不能直接暴露公网。
- SQLite 父目录应由启动流程创建；生产数据库不得指向测试临时目录。
- `AppState` 通过 SQLx pool 共享配置；文件库 pool 上限为 8 个连接，`PRAGMA foreign_keys` 在
  SQLite 下是**每连接**生效的，因此必须配在 `SqliteConnectOptions::foreign_keys(true)` 上，
  不能只对某个连接执行一次语句——否则其余池连接上删除账号不会级联清掉 token/实体/遥测。
  `:memory:` 走单连接（每个池连接会拿到各自独立的内存库）。
- 配置非 loopback 时，启动告警会同时点明「管理控制台随服务一起暴露」，部署者必须把
  `/admin/` 与客户端 API 一起放在 HTTPS 反代和访问控制之后。
- 不使用全局可变业务状态；账号、token、实体和遥测摘要均按数据库账号隔离。

本地参考服务的开发启动方式为：

```text
cargo run -p supertask-cloud-server
```

启动流程会读取环境变量、执行 SQLx migration、按需创建 seed 账号并监听配置地址；默认监听
`127.0.0.1:8787`。客户端仍需通过 `cloud.endpoint.set` 或对应设置选择本地 URL；正式端点
不应被本地地址替换。

要带管理控制台启动，先在仓库根构建前端，再在**当前 shell 运行时**注入管理员邮箱与口令
（口令值不属于本仓库文档内容，不要写进脚本或 `.env`）：

```text
npm run build:console
SUPERTASK_ADMIN_EMAIL=<operator email> SUPERTASK_ADMIN_PASSWORD=<runtime value> \
  cargo run -p supertask-cloud-server
```

然后浏览器打开 `http://127.0.0.1:8787/admin/`。控制台 dev 模式（`npm run console:dev`，
默认 `127.0.0.1:1430` 并把 `/admin/api` 代理到 `8787`）同样可用，无需先构建。

## 4. 数据库与存储约束

`migrations/0001_init.sql` 当前定义以下表；`migrations/0002_admin.sql` 给 `accounts` 追加
`role TEXT NOT NULL DEFAULT 'user'` 与 `accounts_role` 索引：

| 表 | 当前字段/约束 | 用途 |
|---|---|---|
| `accounts` | `id` 主键、`email` unique、`password_hash`、`created_at`、`disabled`、`role`（`user`/`admin`） | 账号、Argon2 密码 hash 与角色 |
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
7. `role` 只接受 `user` / `admin` 两个字面量，服务端硬校验，越界值返回 400；既有账号由
   migration 的 `DEFAULT 'user'` 落位，不存在第三种角色或隐式超级账号。
8. 账号 `id` 由规范化（trim + 小写）后的邮箱稳定派生（`acct-` + 哈希前 24 位十六进制），
   不用随机串；同一邮箱在 seed、引导和管理面建号三条路径上得到同一 id。
9. 删除账号依赖 `access_tokens` / `refresh_tokens` / `entities` / `telemetry_batches` 的
   `ON DELETE CASCADE`；`tests/admin.rs` 在删号后逐表 `COUNT(*)` 断言归零，作为
   `foreign_keys(true)` 的回归防线。

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
  整体回滚并返回 HTTP 429（`CLOUD_QUOTA_EXCEEDED`；当前实现不返回 413）。
- `data` 必须是合法 JSON；vault 数据不在服务端解密，只按 `{blob,salt}` 密文载荷保存。

### `DELETE /entities/:id`

按账号删除实体，恒返回 204（幂等：重复删除同样 204）；不得删除其他账号同 id 的实体。

## 7. 配额与遥测

### `GET /quota`

成功响应：

```json
{"entities":0,"entities_max":100,"bytes":0,"bytes_max":10000000}
```

配额按账号统计实体数量和 `data` UTF-8 字节数。PUT 任一限额超出时返回 413/429，客户端映射
为 `CLOUD_QUOTA_EXCEEDED`。

### `POST /telemetry/batch`

需要 `Authorization: Bearer <access_token>` 认证（服务端从 bearer 解出账号后按账号记账）。
请求只允许客户端白名单事件：

- `app_start`
- `app_stop`
- `feature_open`（带 feature id）
- `service_start`（带 kind）

服务端必须限制 body 和 batch 数量，拒绝或逐项跳过未知事件；不得接受路径、工作区名称、
YAML 内容、命令行、环境变量值、密码、token 或 Authorization。客户端遥测默认关闭，关闭时
必须零请求。参考 schema 只保留有界批次摘要，生产存储策略仍待运营方决定。

## 8. 管理控制台 API 契约（v2.0.1）

管理面走独立子树 `/admin/api/*`，与客户端 API 分开登录：一来避免向未认证方泄露「某邮箱是否
管理员」，二来整个 `/admin` 子树可以单独收紧 CORS。桌面客户端永不调用 `/admin`。

| 方法 | 路径 | 认证 | 语义 |
|---|---|---|---|
| GET | `/admin/api/status` | 无 | `{admin_available,console_ready}` 安装探针，不含任何账号数据 |
| POST | `/admin/api/login` | 无 | `{email,password}` → 与 `/auth/login` 同形状的 `LoginResponse`；非管理员 403 |
| POST | `/admin/api/refresh` | `refresh_token` | 复用 refresh rotation，重新签发后再校验一次角色 |
| GET | `/admin/api/me` | Bearer | `{account_id,email,role}` |
| GET | `/admin/api/accounts?query=&limit=&offset=` | Bearer | 账号行数组，含 `entity_count` / `entity_bytes` 用量聚合；默认 `limit=100`、上限 500，按 `created_at DESC` |
| POST | `/admin/api/accounts` | Bearer | `{email,password,role?}` → 201 + 账号行 |
| PUT | `/admin/api/accounts/{id}/role` | Bearer | `{role}` → 账号行 |
| PUT | `/admin/api/accounts/{id}/disabled` | Bearer | `{disabled}` → 账号行 |
| PUT | `/admin/api/accounts/{id}/password` | Bearer | `{password}` → 204，不回显任何内容 |
| DELETE | `/admin/api/accounts/{id}` | Bearer | 204，级联清 token/实体/遥测 |

### 授权与错误码

- 每个受保护 handler 第一行都调用 `admin::require_admin` = bearer 校验 → 查 `role` →
  非 `admin` 则 `ADMIN_FORBIDDEN`。没有例外，也没有「只在登录时判一次角色」。
- 角色判定只发生在调用方**已证明账号所有权之后**：`/admin/api/login` 的口令错误与
  `/auth/login` 完全同形（401 `CLOUD_AUTH_FAILED`，不可分辨），只有凭证正确但角色不够才
  返回 403。因此「不是管理员」不会变成邮箱枚举侧信道。
- 管理码 `ADMIN_FORBIDDEN` / `ADMIN_NOT_CONFIGURED` 走 `AppError` 既有的
  `{error,code,message}` 形状，但**不占用客户端 `CLOUD_*` 命名空间**，桌面 provider 的
  HTTP→错误码映射保持不变。
- `disabled` 账号的 bearer 视为认证失败（401）；停用后 `/auth/login` 同样返回 401。

### 自我保护与输入护栏

- 口令最低 12 字符、上限 1 KiB；邮箱必须含 `@` 与带点的域，且拒绝空白/控制字符和
  `"` `,` `;` `:` `%`（`%` 会让管理台的子串搜索歧义化）。角色只接受 `user` / `admin`。
- 一律 400 + 中文 message 的拒绝：不能停用/降级/删除**当前登录的自己**；不能让最后一个
  **启用的**管理员被降级、停用或删除——但已停用的管理员可以正常删除（它不在启用计数内）。
  已停用账号需先启用再改角色。`enabled_admin_count` 在同一事务内查。
- 改密只换登录口令：已签发的 token 仍然有效，直到自然过期。会话吊销本期不做，控制台的
  「退出登录」也只清本地 `sessionStorage`。
- 账号 id 复用实体侧的 `valid_id` 安全 ASCII 护栏，路径 id 越界返回 400，未知 id 返回 404。
  注意 axum 会在路由前规范化字面 `../`，因此这类请求可能表现为 405；百分号编码的穿越
  （`%2e%2e%2f`）会到达 handler 并被护栏拒绝。

### 日志与静态资源

- 每次管理写操作 `tracing::info!` 记 `actor=<account_id> action=<...> target=<account_id>`，
  不含邮箱、口令、token。本期只到日志级，不建审计表。
- 客户端 API 保留 `CorsLayer::permissive()`（避免回归），`/admin/api` 用不设 `allow_origin`
  的 `CorsLayer::new()`，即默认拒绝跨域；控制台与 API 同源，浏览器不发预检。
- `/admin/` 由本进程按文件系统读 `SUPERTASK_CONSOLE_DIR`（默认 `cloud-console/dist`），
  只接受纯相对路径组件并二次校验 canonical 路径仍在目录内，越界一律 404；`index.html`
  缺失时返回一个提示执行 `npm run build:console` 的内联页面，而不是裸 404。
  刻意不用 `ServeDir`：`/admin/{*asset}` 捕获会与 `/admin/api/*` 冲突。

## 9. 错误、日志和安全边界

客户端错误映射保持 [cloud.md](cloud.md)：401/403 → `CLOUD_AUTH_FAILED`，409 →
`CLOUD_SYNC_CONFLICT`，413/429 → `CLOUD_QUOTA_EXCEEDED`（当前服务端实现只返回 429，客户端
映射保留 413 兼容），其他非 2xx/解析失败 →
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
6. 唯一允许「指名别的账号」的入口是 `/admin/api/*`，且必须先过 `require_admin` 的角色证明；
   客户端 API 仍一律以 bearer 得到的 account id 为边界。

## 9.1 客户端友好增量（参考服务）

参考服务在保持既有客户端契约（列表仍为实体信封数组、`/quota` 既有字段、`CLOUD_*` 错误码）的前提下，增加了下列**可选用**字段与端点，旧客户端可忽略：

- `GET /healthz`：除 `status` 外返回 `db`（`ok`/`error`）、`now_ms`、`version`；DB 探测失败时 HTTP 503 且 `status=degraded`。
- 实体信封：顶层增加派生字段 `name`（优先 `data.name`，其次 `data.title`，否则回退为 `id`），**仍保留完整 `data`**。
- `PUT` 修订冲突（HTTP 409 / `CLOUD_SYNC_CONFLICT`）：错误体可带 `current`（当前实体信封），便于客户端合并。
- `updated_by`：请求体缺省或空时，回退使用 `x-device-id` 请求头（再缺省为 `server-device`）。
- `GET /quota`：增加 `by_type: [{ type, entities, bytes }, ...]`（按 type 聚合）。
- `GET /telemetry/policy`（需认证）：声明默认关闭、白名单事件、批次上限与 `retention: "counts_only"`（服务端只存批次计数，不存事件载荷）。
- `POST /telemetry/batch`：成功改为 HTTP 200 + `{ "accepted": N }`（不再是 204）。

## 10. 健康检查与验收状态

`GET /healthz` 不要求登录，仅用于本地启动等待和测试，返回 `{ "status": "ok" }`，不泄露账号、
配置或 seed 密码。当前已由 router 注册。

当前服务端自动化验收清单（`cargo test -p supertask-cloud-server`，16 项全绿：3 单测 +
8 管理面集成 + 5 客户端 API 集成）：

- [x] axum router 与 REST handler；
- [x] `/healthz`、优雅关闭和 migration 启动链；
- [x] 临时 SQLite API 集成测试，不绑定固定端口、不访问公网；
- [x] seed 幂等、账号隔离、token 过期/撤销/rotation；
- [x] PUT type + opaque id + rev/409 + 配额事务回滚；
- [x] telemetry 白名单、body 限制和敏感字段拒绝；
- [x] 客户端 401 refresh/replay 一次自动化测试；
- [x] 参考服务放行格式合法的未知 type，并由客户端逐项 skip；
- [x] 会员凭证打管理端点 403 `ADMIN_FORBIDDEN`、未认证 401、无管理员时 403
  `ADMIN_NOT_CONFIGURED`，且管理登录的口令错误与 `/auth/login` 不可分辨；
- [x] 建号 → 登录 → 改密 → 停用（停用后 `/auth/login` 返 401）→ 删除全生命周期；
- [x] 自我保护：对自己 disable/demote/delete 全 400，最后一个启用管理员降级被拒，
  已停用的管理员仍可删除；
- [x] 删号后逐表 `COUNT(*)` 断言 token/实体/遥测归零（守住 `foreign_keys(true)`）；
- [x] `role` 字面量越界与编码穿越路径被拒；
- [x] `/admin/` 静态资源命中、缺失时回落构建提示页、`..` 越界不出目录；
- [x] CI 新增 `cloud` job：服务端测试 + 控制台构建；
- [ ] 正式端点运营、HTTPS 部署和 v2.0 真机验收；
- [ ] 桌面端 GUI 真机确认「被控制台停用的账号在 `#/cloud` 登录报认证失败」（协议层已用
  curl 验证：登录 200 → 停用 → 401 `CLOUD_AUTH_FAILED`）。

相关实现进度见 [v2.0 implementation plan](../archive/plans/2026-08-29-v2-0-implementation-plan.md)。
管理控制台的规格与分期实施记录见
[v2.0.1 cloud admin console spec](../archive/plans/2026-08-30-v2-0-1-cloud-admin-console-spec.md) 与
[v2.0.1 implementation plan](../archive/plans/2026-08-30-v2-0-1-cloud-admin-console-plan.md)。
