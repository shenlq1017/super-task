# Cloudflare Tunnel（快速隧道）

把本机任意已监听端口暴露为临时公网 URL（`https://<随机子域>.trycloudflare.com`），
用于 webhook / 支付回调 / OAuth 回调调试，或把本机服务给同事演示。

## 使用

1. 本机需已安装 `cloudflared`
   （[官方下载](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/)，
   或 `winget install Cloudflare.cloudflared`）。
2. 创建工作区时填「目标服务端口」——本机已监听的任意端口，
   可以是其他 SuperTask 工作区里服务的端口（SuperTask 服务都在本机回环上）。
3. 启动 `tunnel` 服务，在「运行 → 日志」里查看分配的公网 URL。
4. 网络抖动导致进程退出时，`restart: on-failure` 会自动拉起（URL 会变化）。

## 需要固定域名？

quick tunnel 的 URL 每次启动都会变化。需要固定域名请使用命名隧道：
用「Cloudflare Tunnel（命名隧道）」模板创建工作区，把 token 填入 `.env.tunnel`。
token 只经 env_file 注入进程环境，不写入 supertask.yaml、日志或事件。
