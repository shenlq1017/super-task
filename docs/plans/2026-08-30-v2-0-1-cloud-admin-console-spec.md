# SuperTask 云管理控制台（v2.0.1）— 功能规格

> 日期：2026-08-30 · 状态：**已实现并本地验收（服务端 14 项测试全绿 + Playwright 真机走查通过）；
> 正式 HTTPS 部署与运营仍未完成。**
> 服务端契约真源：[cloud-server.md](../spec/cloud-server.md) §8。本文记「为什么这样切」和
> 「验收口径」，不重复端点表。

## 1. 问题

v2.0 交付了 `crates/supertask-cloud-server`，但它**只有面向桌面客户端的 8 条 API**。整张
`accounts` 表只有 `id / email / password_hash / created_at / disabled`，没有角色、没有运营端点、
没有静态页面能力。

结果是：一个自托管运营方只能靠 SQL 手工查账号、手工改 `disabled`、手工删数据，新账号只能靠启动
时的 `SUPERTASK_DEV_SEED` 种一个。repository conventions 里「服务端运营方/端点未拍板」「正式 HTTPS 部署与真机
验收未完成」两条欠账，卡点之一正是**没有管理入口**。

## 2. 目标与非目标

**目标**：让运营方在浏览器里完成账号生命周期与角色管理，不需要读 SQL、不需要重编二进制。

**非目标（本期明确不做）**：会话/设备列表与吊销、实体内容浏览（含 `data` 明文查看与读取留痕）、
每账号配额覆盖、遥测聚合、管理员审计表、批量导入导出账号、2FA、密码找回、多页分页精修；
桌面端不新增任何云管理 IPC。

## 3. 三项拍板

| 决策点 | 结论 | 理由 |
|---|---|---|
| 形态 | **服务端自带 Web 控制台**（`http://127.0.0.1:8787/admin/`） | 桌面端代码零改动；运营方不需要装 SuperTask 才能管云；自托管场景下服务端本来就是必装件 |
| 前端技术 | **新建 Vite + React 19 + TS + Tailwind 4 + shadcn（radix-nova）子项目 `cloud-console/`** | 与 `frontend/` 同一视觉与同一套约定，零学习成本；独立工程不污染桌面端构建 |
| 范围 | **仅账号生命周期 + 角色** | 这两件没有 UI 就只能裸写 SQL；其余项都能靠 SQL 或后续版本补 |

## 4. 架构决策

1. **角色放在 `accounts.role`，不建独立 admins 表。** 复用已有的 Argon2 口令哈希与 token
   签发/轮换/过期链路，管理面只需在既有 bearer 校验后追加一次角色判定；一个账号可以同时是普通
   用户（自己的实体照常在客户端同步）或纯运营。
2. **管理员引导沿用 seed 的「无默认密码」纪律。** `SUPERTASK_ADMIN_EMAIL` +
   `SUPERTASK_ADMIN_PASSWORD` 两者任一缺失 → 管理面整体不可用；只设其一 → 配置解析失败。
   禁止生成默认口令。
3. **管理面走独立登录端点** `POST /admin/api/login`，不复用公开 `/auth/login`：一来避免向未认证
   方泄露「某邮箱是否管理员」，二来整个 `/admin` 子树可以统一收紧 CORS。
4. **`/admin` 子树单独收紧 CORS，客户端 API 的 permissive 保持不变。** 避免回归；控制台与 API
   同源，浏览器不发预检，dev 走 vite proxy 也是同源。管理员 token 存 `sessionStorage`，
   跨标签页不驻留。
5. **静态资源走文件系统，不嵌进二进制。** `SUPERTASK_CONSOLE_DIR`（默认 `cloud-console/dist`）。
   理由：不引入 `rust-embed` 造成的「编译顺序依赖构建产物」，CI 不必先编 dist 才能 `cargo build`，
   运营方换前端包不用重编 Rust。控制台用 hash 路由，`/admin/` 命中 `index.html` 即可，无需 SPA
   fallback。
6. **顺手修掉一个真实隐患：外键级联只在单个池连接生效。** `PRAGMA foreign_keys` 在 SQLite 下是
   **每连接**生效的，而文件库池上限 8；原实现只在池上执行了一次 pragma 语句，其余连接上
   `ON DELETE CASCADE` 可能不触发——而「删除账号」正依赖它清四张子表。改为
   `SqliteConnectOptions::foreign_keys(true)`。不修就是删号留孤儿 token 和实体。
7. **零新 crate。** 不加 `rust-embed`，也不需要 `tower-http` 的 `fs` feature（见偏差备案 1）。
   `Cargo.toml` 只有两处结构调整：静态文件读取用到 tokio 的 `fs` feature，`tower` 移到
   `[dev-dependencies]`（只有集成测试用 `ServiceExt::oneshot`，生产二进制不再直接依赖它）。
   前端不加 `shadcn` CLI 依赖、`@fontsource`、`react-i18next`。

## 5. 授权模型

- `require_admin` = bearer 校验 → 查 `role` → 非 `admin` 返回 403 `ADMIN_FORBIDDEN`。每个受保护
  handler 第一行都调用，无例外。
- 角色判定**只发生在调用方已证明账号所有权之后**：口令错误与 `/auth/login` 完全同形（401，不可
  分辨），凭证正确但角色不够才 403。管理面因此不会变成邮箱枚举侧信道。
- 管理错误码 `ADMIN_FORBIDDEN` / `ADMIN_NOT_CONFIGURED` **不占用客户端 `CLOUD_*` 命名空间**，
  桌面 provider 的 HTTP→错误码映射零改动。
- 自我保护：不能停用/降级/删除当前登录的自己；不能让最后一个**启用的**管理员消失。已停用的管理员
  仍可删除（它不在启用计数内）——这条是实现期真机走查发现的 bug 的修法，见偏差备案 5。

## 6. 前端设计要点

- 镜像 `frontend/` 的约定但不复用它的路由/provider：设计令牌同源（品牌紫 `#5E6AD2`、底色
  `#F7F8F8`、`--line-strong`、`--r-sm`），UI 组件同源风格。
- `api.ts` 承担「401 → refresh 一次 → 只重放一次」，并把并发 401 合并成单趟刷新；刷新失败清理
  会话。与客户端云协议同一条纪律。
- 两个页面：`login-page`（含 `ADMIN_NOT_CONFIGURED` 安装态提示与口令显隐）、`accounts-page`
  （搜索 + 表格 + 新建对话框 + 行内动作 + 改密对话框）。
- 危险操作（删除、停用、降级）一律二次确认；变体按 repository conventions 表：新建 `default`、保存 `success`、
  停用 `warn`、删除 `destructive`、次操作 `outline`；全部 `cursor-pointer` + hover 有底色变化。
- 文案在 `src/lib/labels.ts`，zh-CN / en-US 双语用 `navigator.language` 选择，不引 i18n 依赖。

## 7. 验收口径

自动化：`cargo test -p supertask-cloud-server`（in-process router + `:memory:`，不占端口、
不访问公网）与 `cd cloud-console && npm run build`。必须覆盖的负向路径见
[cloud-server.md](../spec/cloud-server.md) §10 清单。

真机（浏览器）：启动带管理员的服务端 → 开 `/admin/` → 登录 → 新建账号 → 升为 admin → 停用
seed 账号 → 用被停用的账号打 `/auth/login` 确认 401 → 删除该账号 → 重启服务端确认状态落盘。
桌面端 `#/cloud` 那一半（确认 UI 报「认证失败」）仍属人工矩阵，见 §10 清单未勾项。

诚实边界：控制台「退出登录」只清本地会话——服务端 refresh token 在 30 天有效期内仍然可用，因为
会话吊销本期被明确排除。这在有物理接触的攻击模型下是一个真实残余风险，必须在部署文档里点明。

## 8. 偏差备案（相对批准计划）

实现期的偏离与真机发现的 bug，全部以代码事实为准，不回填计划正文：

1. **`tower_http::ServeDir` 换成手写资源 handler。** `/admin/{*asset}` 捕获会与
   `/admin/api/*` 抢路由；`ServeDir` 挂在 `nest_service("/admin")` 下同样会让
   `/admin/api/...` 先被静态层吃掉。手写版只做相对路径组件白名单 + canonical 路径二次校验，
   并且省掉了一个 feature 依赖。
2. **多了一条 `GET /admin/api/status`（计划写九端点，实交付十条）。** 未认证的安装探针，让控制台
   能把「还没引导管理员」和「口令错了」分开显示，不返回任何账号数据。
3. **`AppError::Forbidden(&'static str)` 落成两个无参变体 `AdminForbidden` / `AdminNotConfigured`。**
   code 必须是稳定字面量而不是调用方传的字符串，message 统一中文，与既有
   `{error,code,message}` 形状一致。
4. **UI 组件从 `frontend/src/components/ui/` 直接复制同源版本，未跑 `npx shadcn add`。** 控制台
   没有 `shadcn` 依赖，跑 CLI 会把它拉进 `package.json`，并需要联网取 registry。角色选择用
   `--r-sm` chip 而非 `select`，因此连 select 组件都不需要。
5. **停用的管理员删不掉（真机走查发现）。** `delete()` 拿 `enabled_admin_count()` 比较时没有排除
   「目标自己已被停用」，于是 root（启用）+ demo（停用管理员）时计数为 1，删除被拒。修成
   `&& !disabled`，并加专门回归测试。
6. **控制台日期全显示 1970（真机走查发现）。** `auth::now()` 用 `as_secs` 存 epoch **秒**，
   `formatTime` 当成毫秒乘了。修转换并在 `AccountRow.created_at` 上注明单位。
7. **新建对话框永远无法提交（真机走查发现）。** `busy` 把 `creating`（对话框自身开关）算进去了，
   提交按钮永久 disabled。改成只看加载态与行内忙 id。

## 9. 影响面

- 动：`crates/supertask-cloud-server/`（新增 `admin.rs`、`admin_http.rs`、`migrations/0002_admin.sql`、
  `tests/admin.rs`；改 `lib.rs`、`main.rs`、`config.rs`、`state.rs`、`error.rs`）、
  `cloud-console/`（新）、根 `package.json`、`.github/workflows/ci.yml`、本文档组。
- 不动：`crates/supertask-core/src/cloud/*`、`src-tauri/src/cloud.rs`、`frontend/**`、
  `docs/spec/ipc.md`（控制台不经 IPC）。
- 附带收掉一个无关缺口：**云服务端测试此前完全不在 CI**。新增 `cloud` job 同时跑
  `cargo test -p supertask-cloud-server` 与控制台构建。

实施分期与执行记录见 [v2.0.1 implementation plan](2026-08-30-v2-0-1-cloud-admin-console-plan.md)。
