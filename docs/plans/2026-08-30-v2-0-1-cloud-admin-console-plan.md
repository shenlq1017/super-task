# SuperTask 云管理控制台（v2.0.1）— 实施计划与执行记录

> 日期：2026-08-30 · 状态：**Phase 1–5 全部完成。**
> 规格与拍板：[v2.0.1 spec](2026-08-30-v2-0-1-cloud-admin-console-spec.md)；
> 服务端契约真源：[cloud-server.md](../spec/cloud-server.md) §8。
> 批准计划在会话内（`glacial-gorge-asp`），本文是它的落地版并附执行记录。

## 阶段与产物

### Phase 1 — 服务端授权基座 ✅

| 产物 | 内容 |
|---|---|
| `migrations/0002_admin.sql` | `accounts.role TEXT NOT NULL DEFAULT 'user'` + `accounts_role` 索引 |
| `src/state.rs` | `SqliteConnectOptions::from_str(..).foreign_keys(true).create_if_missing(true)`；`:memory:` 单连接、文件库 8 连接 |
| `src/config.rs` | `admin_email` / `admin_password`（both-or-neither，只设其一即 `from_env` 失败）、`console_dir`（默认 `cloud-console/dist`）、`admin_configured()`、`console_ready()` |
| `src/main.rs` | seed 之后 `admin::bootstrap_admin`，启用时 `warn!` 提示 `/admin/`；非 loopback 告警文案追加「管理控制台随服务一起暴露」 |
| `src/error.rs` | `AdminForbidden` → 403 `ADMIN_FORBIDDEN`、`AdminNotConfigured` → 403 `ADMIN_NOT_CONFIGURED`（不进 `CLOUD_*`） |
| `src/admin.rs` | `require_admin` = `auth::account_from_bearer` → 角色判定；`ROLE_USER` / `ROLE_ADMIN` 常量 |

### Phase 2 — 管理 API + 静态服务 ✅

- `src/admin.rs` 数据层：`list`（邮箱/id 子串搜索 + 用量聚合 + `limit` 默认 100 上限 500）、
  `create`、`set_role`、`set_disabled`、`set_password`、`delete`、`bootstrap_admin`、`me`、
  `status`、`admin_login`、`admin_refresh`；自我保护在同一事务内查 `enabled_admin_count`。
- `src/admin_http.rs`：十条 handler；`create` 返 201 + 行，`set_password` / `delete` 返 204，
  路径 id 过 `entities::valid_id`；写操作 `tracing::info!(actor, action, target)`。
- `src/lib.rs`：`client_api`（permissive CORS）与 `admin_api`（`CorsLayer::new()`，默认拒绝跨域）
  分开构建后 `merge`；`/admin` → `/admin/` 重定向 + `index.html` + `{*asset}` 手写静态服务
  （相对路径组件白名单 + canonical 二次校验 + 缺 `index.html` 时的构建提示页）。
- `tests/admin.rs` 八条集成测试。
- **零新 crate**：未加 `rust-embed`，也不需要计划里提的 `tower-http` `fs` feature。
  `Cargo.toml` 只有结构调整：加 tokio `fs` feature（静态文件读取），`tower` 移到
  `[dev-dependencies]`（只有测试用 `ServiceExt`）。

### Phase 3 — `cloud-console/` 脚手架与登录 ✅

独立 Vite 7 + React 19 + TS 5.8 + Tailwind 4 + radix-ui 工程（**不引入 npm workspaces**）。
`vite.config.ts` 用 `base: "/admin/"`、`emptyOutDir: false`（沙箱的安全删除会拦截 vite 清空
`dist` 的 trash 操作，且 `index.html` 按哈希引用资源，不需要清空）、`server.port 1430` 与
`/admin/api` → `127.0.0.1:8787` 代理。
`src/lib/api.ts`：typed fetch + `sessionStorage` 会话 + 单飞刷新（并发 401 合并成一趟 refresh）+
最多一次重放。`src/index.css` 复刻 `frontend/` 设计令牌（浅色单一主题）。
`src/providers/auth.tsx` + `src/app.tsx`（hash 路由，路由守卫用字面 `<Route>` 而非返回
`<Route>` 的包装组件——react-router v7 的 `createRoutesFromChildren` 不穿透自定义组件）。
`login-page.tsx`：状态探针 → 未配置引导时显示安装提示；口令显隐；`describeError()` 映射三个管理码。

### Phase 4 — 账号管理 UI ✅

`accounts-page.tsx`：防抖搜索（首屏立即加载，之后 250ms）+ 表格（角色/状态 chip、实体数与字节
用量）+ 行内动作 + `create-account-dialog` + `password-dialog` + `confirm-dialog` 二次确认 +
自身行守卫（对自己的停用/降级/删除入口按 actor id 隐藏）。
`labels.ts`：zh / en 双语约 90 键，`navigator.language` 选择，不引 i18next。
变体按 AGENTS.md：新建 `default`、保存 `success`、停用 `warn`、删除 `destructive`、次操作
`outline`；全部 `cursor-pointer` + hover 有底色变化。

### Phase 5 — 文档与 CI ✅

- `.github/workflows/ci.yml`：新增 `cloud` job（`cargo test -p supertask-cloud-server` +
  `npm ci && npm run build`，`cache-dependency-path: cloud-console/package-lock.json`）。
  **云服务端测试此前完全不在 CI**，这次一并补上。
- 根 `package.json`：`console:dev`、`build:console`。
- `docs/spec/cloud-server.md`：§1 去掉「网页控制台」非目标、§2.1/§2.3 模块与前端边界、
  §3.1 三条环境变量 + 引导纪律、§3.2 `foreign_keys` 与 pool、§4 `role` 列与三条存储约束、
  新增 §8 管理面契约、§9 安全要求第 6 条、§10 验收清单。
- `docs/plans/2026-08-29-v2-0-feature-spec.md` §3：「移动端 / 网页控制台」拆开，网页控制台标注
  为 2026-08-30 范围变更并指向 v2.0.1 规格。
- 本组新文档 + `AGENTS.md` 当期阶段与文档地图 + inv-1 / inv-4 回写。

## 执行记录

### 验证结果

- `cargo test -p supertask-cloud-server`：**14 全绿**（`admin` 单测 3 + `tests/admin.rs` 8 +
  `tests/api.rs` 3）。
- `cargo clippy -p supertask-cloud-server --all-targets`：本特性文件零告警（`auth.rs:36` 一条
  既有告警不属于本次改动，已用 `git diff` 核对；CI 不跑 clippy）。
- `npm run build:console`（仓库根脚本，内部 `tsc && vite build`）：通过，产物 342.85 kB JS /
  33.23 kB CSS，路径为 `/admin/assets/*`，与服务端静态 handler 对齐。
- 本地 curl（`127.0.0.1:8799`，临时库 `target/console-smoke/cloud.db`）：`/healthz` ok；
  `/admin/api/status` → `{"admin_available":true,"console_ready":true}`；`/admin/` 200
  `text/html`；JS/CSS 200 且 MIME 正确；`curl --path-as-is /admin/../Cargo.toml` → 404；
  管理员登录签发 token。
- Playwright（headless chromium，11 步走查全通过）：落地重定向 → 错口令告警 → 登录 → 新建
  `demo<时间戳>@supertask.local`（后缀避免重复跑撞邮箱）→ 升为 admin → 停用 `demo` → 自身守卫
  按钮不可见 → 删除 → 退出登录 → 深链 `#/accounts` 未登录被弹回登录页 → 全程无 console error。
- **客户端契约未变的直接证据**：`demo` 账号 `/auth/login` 200 → 控制台停用 → 再打
  `/auth/login` 得 401 `{"code":"CLOUD_AUTH_FAILED","error":"认证失败","message":"认证失败"}`。
  `crates/supertask-core` 与 `src-tauri` 一行未改。

### 实现期发现并修掉的三个真 bug

1. **停用的管理员删不掉** —— 见 spec §8.5。回归测试
   `deleting_a_disabled_admin_is_allowed_while_one_stays_enabled`。
2. **控制台日期显示 1970** —— epoch 秒当毫秒。`formatTime` 改 `new Date(epochSeconds * 1000)`，
   并在 `AccountRow.created_at` 上注明单位是秒。
3. **新建对话框无法提交** —— `busy` 误含 `creating`。改成 `state === "loading" || busyId !== null`。

### 自测过程中订正的两处测试脚本假警报

- 断言了 `demo2@supertask.local`，实际 seed 邮箱是 `demo@supertask.local`。
- 深链检查用 URL 断言，但同文档 hash 导航不产生网络活动，`networkidle` 立即返回导致竞态。
  改用 `probe_guard.py` 独立验证渲染内容（冷启动 `#/accounts` / `#/login` / 裸 `/admin/` 三种
  入口都渲染登录页且没有账号表格），确认守卫本身正确，再把断言换成渲染内容。

### 计划内被推翻的一处预期

`/admin/api/accounts/../../etc/role` 预期 400、实测 405：axum 的 PathNorm 在路由前就把字面
`../` 规范化掉了，请求根本到不了 handler。改测百分号编码穿越（`%2e%2e%2f`）和 `bad!id`——
这两条会真正到达 `valid_id` 并返回 400。**这是框架行为，不是护栏漏判**，已写进
[cloud-server.md](../spec/cloud-server.md) §8。

## 后续：本地一键启动脚本（2026-08-30 用户追加）

根目录新增 `start-cloud.ps1`：一个命令拉起云服务端与控制台 dev，各自独立窗口，就绪后打印入口并
（默认）打开浏览器；管理员邮箱/口令只从环境或**不回显**的 `Read-Host -AsSecureString` 取，脚本
正文零凭据。选项 `-ServerBind` / `-AdminEmail` / `-NoBrowser`。

实测证据（`-ServerBind 127.0.0.1:8791 -NoBrowser`，Windows PowerShell 5.1）：

- `/healthz` → `{"status":"ok"}`；`/admin/api/status` → `admin_available=true console_ready=true`。
- `http://127.0.0.1:1430/admin/` → 200；经 vite 代理 `POST /admin/api/login` → 200 拿到 token，
  再 `GET /admin/api/accounts` → 200 且管理员账号 `role=admin` 与 `role=user` 账号并列。
- 关掉控制台窗口后：服务端 `healthz` 连接被拒，`supertask-cloud-server` / `cargo` / vite node
  进程数归零 —— 两棵进程树一起清干净。

两处非显然的坑（都写进了 cloud-server.md §2.3 与 AGENTS.md）：

1. **`Start-Process cargo` 在新控制台窗口里以 code 1 退出**。`cargo.exe` 是 rustup shim，同样的
   命令经 `cmd /c` 包装后正常监听。脚本因此统一用 `cmd /c "title … & <cmd> & echo. & pause"`，
   顺带让报错退出的服务把日志停在窗口里可读，而不是闪退。
2. **vite dev 默认绑到 `::1`**。`host: false` 时 vite 按 `localhost` 解析，Windows 上优先 IPv6，
   于是文档里的 `http://127.0.0.1:1430/admin/` 一直连不上、脚本卡在 120 秒就绪超时。改
   `cloud-console/vite.config.ts` 为 `host: "127.0.0.1"`（仍然只监听 loopback，不给局域网开口子）；
   `npm run build:console` 产物未变（342.85 kB JS / 33.23 kB CSS）。

残余行为：服务崩溃时它的窗口停在「请按任意键继续」，脚本的守护循环因此继续等待——需要人按键或
关窗才触发整体清理。这是「日志看得见」换来的代价，已记在脚本文档串里。

## 遗留

| 项 | 状态 |
|---|---|
| 桌面端 `#/cloud` 真机确认「被停用账号登录报认证失败」 | 协议层已用 curl 证毕；GUI 那一半属人工矩阵，未做 |
| 正式 HTTPS 部署 / 运营端点 / 备份策略 | v2.0 既有欠账（inv-4 D1），本期不消化 |
| 控制台退出登录后服务端 refresh token 仍有效 30 天 | 会话吊销明确排除在范围外，已在 spec §7 记为残余风险 |
| `SUPERTASK_SEED_EMAIL` 不做小写规范化（`admin_email` 做） | 既有行为，未动；写口令类文档时以代码为准 |
