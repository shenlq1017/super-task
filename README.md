# SuperTask

**一份 `supertask.yaml`,一键拉起、一键收场你机器上的所有服务。**

本地优先 · Tauri 2 + Rust(不是 Electron)· Windows 优先,macOS / Linux 可用

> 推送到 GitHub 后,在标题下方补一行徽章:
> `https://github.com/<你的用户名>/super-task/actions/workflows/ci.yml` 的 badge。

---

AI 时代带来了一个意想不到的副作用:
**越来越多的服务,不是你启动的。**

AI 助手为了「帮你验证」,拉起了一堆服务;
你关掉窗口,服务还在跑。
端口被占着,内存被吃着,而你甚至不知道怎么把它停下来。

老问题也一个都没少:

- 后端是 Spring Boot 多模块,而你是前端——想把栈拉起来,先背一套 Maven 咒语;
- 新项目交接,README 只写了「本地跑一下就行」,端口撞了没人负责;
- 启动命令散落在五个终端里,明天你就会忘记谁是谁。

**SuperTask 就是为「快速启动服务」而生的。**
服务和代码早就躺在你的机器上,SuperTask 只是把它们的启动方式
记进一份 `supertask.yaml`——从此一键或一条命令拉起整个栈:
按顺序、等健康、聚日志;收工时,一键清掉整棵进程树。

*(截图待补:运行页全景)*

## 30 秒上手

把「Spring 后端 + Node 前端」的启动方式记一次:

```yaml
version: 1
name: my-stack
services:
  user-api:
    kind: spring-boot
    module: user-api
    port: 8081
    health:
      type: http
      http: http://127.0.0.1:8081/actuator/health
  web:
    kind: node
    dir: web
    port: 5173
    script: dev
    depends_on:
      - user-api
scripts:
  build:
    cmds:
      - mvn -q clean package -DskipTests
```

然后三选一(或者都用):

| 方式 | 怎么做 |
|------|--------|
| 桌面 | 打开工作区 → 指向目录 → 「全部启动」 |
| 命令行 | `supertask up --wait healthy` |
| AI 编辑器 | MCP 一行配置:`{ "mcpServers": { "supertask": { "command": "supertask", "args": ["mcp"] } } }` |

## 起得来 — 一键拉起既有项目

- 指向目录,或一条 `supertask up`:整个栈按依赖顺序启动,等健康检查通过才算起来;失败自动清场,不留半截栈。
- 启动方式在 yaml 里声明一次,直接进 git——新人入职第二天不用再问人。
- 懒得写:扫描 `pom.xml` / `package.json` 生成草稿,或者干脆让它读项目的 README。
- 六种服务通吃:Spring Boot(Maven 多模块)、Node、Python、Go、Docker Compose,以及任何叫得上名字的程序。
- 启动顺序、端口、健康检查全在文件里——每次都一样地起来。
- 不懂对方的工具链也没关系:`doctor` 告诉你缺什么,JDK / Maven / Node 可经 mise / winget 装好——前端也能一键拉起后端。

## 看得见 — 每个服务,一目了然

- 聚合日志、可搜索、可导出;不用再在终端之间来回切。
- 状态、CPU / 内存、端口占用尽收眼底;端口撞了,一键改完写回 yaml。
- 一键打开 Cursor / IDEA / VS Code,顺手看 git 分支与状态。

## 停得掉 — 走的时候,不留残渣

- 一键杀整棵进程树,子进程也不例外;`down` 之后,端口是真的释放了。
- 让 Cursor / Claude 通过 MCP 管理服务:编辑器一关,服务随之清场——AI 时代,不该有孤儿进程。
- CLI 同样干脆:`supertask up / down / status / logs`,退出码透传,能直接进 CI 流水线。

## 带得走 — 工作区跟着你跑

- 导出 / 导入 zip,换台机器拉起来接着干。
- 网关配置(nginx / caddy / apache)也从这份 yaml 生成,带本机校验与一键 HTTPS。
- 云同步可选:密钥默认**不**上云——想同步,得你自己勾选。
- AI 辅助也可选:解释日志、补健康检查,用你自己的 Key,只提建议不动手。

## 和你现在用的比

| | 几个终端 | docker compose | Shell / 任务脚本 | AI 自己拉 |
|---|---|---|---|---|
| 按依赖顺序启动 | 自己记 | ⚠️ 只排序,不等就绪 | 手写 | 指望不上 |
| 健康通过才算起来 | 手动 curl | ⚠️ 能配,Java 栈不好写 | 手写 | ❌ |
| 聚合日志与状态 | 散在 N 个窗口 | `docker logs` 逐个看 | ❌ | ❌ |
| 一键停止、无残留 | 逐个杀,Windows 常杀不掉子进程 | ✅ | 手写 | ❌ 工具关了服务还在 |
| Spring Boot 热重载 / 调试 | ✅ | 得先容器化,Windows 卷挂载慢 | ✅ | ✅ |
| 启动知识进仓库 | ❌ 在人脑里 | ✅ compose.yml | ✅ 在脚本里 | ❌ |

一句话:**像终端和脚本一样跑宿主机裸进程,但有 compose 的声明、顺序与清场语义**。
容器只是六种服务之一,不是前提;AI 起的服务,也有人负责收场。

## 示例工作区

[`examples/`](examples/) 里有四个开箱即用的工作区,桌面端「打开工作区」指向对应目录即可:

| 目录 | 覆盖场景 | 前置要求 |
|------|----------|----------|
| `spring-multi/` | Maven 多模块:拓扑启动、健康检查、改端口、脚本、构建 jar | JDK 17+、Maven |
| `node-demo/` | Node 基线:启停、日志、健康、脚本(零依赖,离线可跑) | Node |
| `gateway-demo/` | nginx / caddy / apache 网关、路由、本机 HTTPS | Node;网关引擎按需 |
| `compose-demo/` | compose 起停、镜像构建 | Docker Desktop |

## 构建与开发

正式安装包还在路上(发布工程是路线图的一部分)。在那之前,从源码跑起来比你想的快:

```bash
git clone https://github.com/<你的用户名>/super-task && cd super-task
npm install            # 前端依赖
npm run tauri dev      # 桌面应用(纯前端调试用 npm run dev,走 mock IPC)
```

CLI 与测试:

```bash
cargo build -p supertask-cli    # bin: supertask
supertask doctor                # 看看你的机器还缺什么
cargo test -p supertask-core    # 引擎测试
```

要求:Rust stable、Node 20+,以及你想跑的服务各自需要的东西
(比如跑 Spring 需要 JDK 17 + Maven)。Tauri 平台依赖见
[官方文档](https://tauri.app/start/prerequisites/)。

测试基线:引擎 455 / CLI 20 / 云服务 14,
CI 在 Windows、macOS、Linux 三平台跑。

## 云参考服务(自托管)

仓库包含独立的参考服务端 `crates/supertask-cloud-server`,
用于协议联调与自托管:账号、配额、管理面与自带控制台。
正式 HTTPS 部署尚未完成,生产使用请先自行评估。

```bash
cargo run -p supertask-cloud-server        # 默认 127.0.0.1:8787
npm run console:dev                        # 控制台开发模式
```

完整配置与边界见 [docs/spec/cloud-server.md](docs/spec/cloud-server.md);
Windows 下一键拉起两端:`start-cloud.ps1`。

## 路线图

| 版本 | 主题 | 状态 |
|------|------|------|
| 1.0 – 1.7 | 能跑 / 能带走 / 能养活 / 能装箱 / 能出门 / 能搬家 / 能对外 / 能扩 | ✅ 已落地 |
| 2.0 – 2.1 | 能上云(账号、同步、迁移)/ 能读文档(README 导入、AI) | 🚧 功能已实现,真机验收与正式部署进行中 |
| 2.2 | 能长:插件(自定义 kind)、WSL2 | 🌱 规划中 |

## 文档

- YAML 规范:[docs/spec/yaml.md](docs/spec/yaml.md)
- IPC 契约:[docs/spec/ipc.md](docs/spec/ipc.md)
- 架构:[docs/spec/architecture.md](docs/spec/architecture.md)
- CLI 与 MCP:[docs/spec/cli.md](docs/spec/cli.md)
- 云客户端协议:[docs/spec/cloud.md](docs/spec/cloud.md) · 云参考服务:[docs/spec/cloud-server.md](docs/spec/cloud-server.md)
- 系统盘点(现状真源):[docs/inventory/](docs/inventory/)
- 给 agent 的贡献指南:[AGENTS.md](AGENTS.md)

## 参与

- 报错、提需求:Issue;改代码:PR。
- 动手前建议先读 [AGENTS.md](AGENTS.md) 和 [docs/inventory/](docs/inventory/),那里记着系统的现状与欠账。

## 致谢

- 网关路由配置模型借鉴 [nginxconfig.io](https://github.com/digitalocean/nginxconfig.io)(DigitalOcean,MIT),按本机开发场景裁剪,未引入其代码。
- 网关页交互思路借鉴 [nginxWebUI](https://github.com/cym1102/nginxWebUI)(GPL),未引入其代码。

## License

⚠️ 仓库还没有 LICENSE 文件——正式发布前需要选定许可证并补上。
