# Cloudflare Tunnel（命名隧道）

固定域名/固定 URL 的 Cloudflare 隧道。token 只经 `.env.tunnel`（env_file）注入
进程环境——不写入 supertask.yaml、日志或事件，也不进 Git（建议把 `.env.tunnel`
加入 `.gitignore`）。

## 使用

1. 本机需已安装 `cloudflared`
   （[官方下载](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/)，
   或 `winget install Cloudflare.cloudflared`）。
2. 在 [Cloudflare Zero Trust](https://one.dash.cloudflare.com/) → Networks → Tunnels
   创建隧道（Public Hostname 页把域名指向 `http://localhost:<目标端口>`），
   复制隧道 token。
3. 把 token 粘贴进本工作区 `.env.tunnel` 的 `TUNNEL_TOKEN=` 之后，保存。
4. 启动 `tunnel` 服务；连接状态在「运行 → 日志」中查看。
5. 网络抖动导致进程退出时，`restart: on-failure` 会自动拉起。
