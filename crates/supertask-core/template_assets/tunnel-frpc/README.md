# frp 客户端（frpc）

把本机端口经 frps 服务器（自建或服务商提供）暴露为远端端口。
auth.token 只经 `.env.frp`（env_file）注入——`frpc.toml` 用 frp 的
`{{ .Envs.FRP_TOKEN }}` 配置模板引用，不写死凭据；`.env.frp` 也建议加入
`.gitignore`。

## 使用

1. 本机需已安装 `frpc`（frp ≥ 0.52，[GitHub Releases](https://github.com/fatedier/frp/releases)）。
2. 创建工作区时填四个参数：frps 地址 / frps 端口 / 远端暴露端口 / 目标服务端口
   （目标端口可以是其他 SuperTask 工作区里服务的端口）。
3. 把与 frps 约定的 token 填进 `.env.frp` 的 `FRP_TOKEN=` 之后，保存。
4. 启动 `tunnel` 服务，连接状态在「运行 → 日志」中查看；
   外部通过 `frps地址:远端端口` 访问本机目标端口。
5. 网络抖动导致进程退出时，`restart: on-failure` 会自动拉起。

## 自定义代理类型

`frpc.toml` 是 frp 官方配置，可直接扩展 `type = "http"` / `stcp` 等代理类型，
改完在 SuperTask 重启 `tunnel` 服务即可。
