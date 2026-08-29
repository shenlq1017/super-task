# v2.0 云参考服务与客户端验收记录

> 日期：2026-08-29  
> 范围：本地参考服务、core cloud 客户端、Tauri cloud IPC、浏览器 mock 云页。  
> 网络边界：自动化测试与浏览器验证只访问本地进程；未访问公网。

## 自动化结果

| 检查 | 结果 |
|---|---|
| `cargo check --workspace --offline` | 通过 |
| `cargo test -p supertask-cloud-server --locked --offline` | 3 个集成测试通过 |
| `cargo test -p supertask-core --locked --offline` | 370 个单测通过；集成测试通过；1 个既有 ignored |
| `cargo test -p supertask --offline` | 通过（lib/main/doc tests，无失败） |
| `CARGO_TARGET_DIR=target-cli cargo test -p supertask-cli --offline` | 20 个测试通过 |
| `cd frontend && npm run build` | 通过（tsc + vite build） |
| `cd frontend && node scripts/gen-zh-hant.mjs` | 944 keys；en-US/ja-JP/zh-TW parity OK；0 issues |
| `cargo fmt --manifest-path crates/supertask-cloud-server/Cargo.toml -- --check` | 通过 |

## 本地参考服务

使用运行时注入的 seed 密码启动，不把密码写入仓库：

```text
SUPERTASK_DEV_SEED=1 SUPERTASK_SEED_EMAIL=demo@supertask.local SUPERTASK_SEED_PASSWORD=<runtime-secret> cargo run -p supertask-cloud-server
```

已验证：

- `GET http://127.0.0.1:8787/healthz` 返回 `{"status":"ok"}`；
- 登录返回 account id、email、access/refresh token 和 `expires_in_secs`，服务端日志不含密码/token；
- 客户端提供的稳定实体 id 可创建，错误 `base_rev` 返回 409 `CLOUD_SYNC_CONFLICT`；
- refresh 成功轮换 refresh token，旧 refresh token 再用返回 401；
- 新 access token 可读取账号隔离的 quota；
- 测试完成后已删除临时 SQLite 数据库和 token 输出文件。

## 浏览器 mock 黑盒

Playwright Chromium headless，390×844、reduced motion：

1. `/cloud` 未登录页可见；
2. 非法邮箱显示 inline error；
3. 密码显示/隐藏按钮切换 `type`；
4. 登录成功后显示账号会话卡；
5. 同步显示冲突，`保留本地` 后冲突从列表移除；
6. 端点高级设置保存成功；
7. welcome 提供“从云端恢复”入口，settings 提供端点和遥测开关；
8. 页面未向公网发起请求。

## 尚未关闭的人工/部署项

- 正式服务端运营方、生产 HTTPS 端点、证书和反向代理部署；
- Windows 真机上的 DPAPI、Tauri WebView、真实双设备冲突与迁移矩阵；
- `secrets.sync: true` 的 YAML 字段与 vault 打包编排；
- passphrase 专项管理 UI；
- 发布工程 C1/C2（签名、updater 端点和安装包）。
