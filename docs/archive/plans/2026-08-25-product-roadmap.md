# SuperTask 产品路线

> 2026-08-25。覆盖用户原 17 条，并补上缺口。版本是规划用的，不是承诺的发版日。  
> 工作名：**SuperTask**。配置文件：`supertask.yaml`。

## 产品一句话

本机优先的可视化工作台：把「环境、脚本、进程、网关、容器、云、AI」收成一个工作区，1.0 先把 **Windows 上的 Spring Boot 多模块 + Node** 跑稳。

## 原需求落点

| # | 原需求 | 首次进入 | 备注 |
|---|--------|----------|------|
| 1 | 精美可视化 | 1.0 | 壳与导航一次成型，后期加页不拆房 |
| 2 | 简洁操作 + YAML 脚本 | 1.0 | `services` + `scripts` |
| 3 | 轻量 | 全程 | Tauri，禁 Electron |
| 4 | env / 端口灵活 | 1.0 | 1.2 补端口占用冲突 |
| 5 | 日志追踪 | 1.0 | 1.2 搜索/导出 |
| 6 | 运行状态 | 1.0 | 1.2 CPU/内存 |
| 7 | 跨平台，Windows 先 | 1.0 Win / 1.4 macOS+Linux | |
| 8 | 云、账号、同步 | 2.0 | 1.0 导航占位 |
| 9 | 一键迁移 | 2.0 | 1.5 先做离线导出包 |
| 10 | 模板快速创建 | 1.1 | 1.0 占位页 |
| 11 | AI | 2.1 | 1.0 占位 |
| 12 | README 生成部署 | 2.1 | 导入器，输出 YAML |
| 13 | nginx / apache | 1.6 | 1.0 网关占位 |
| 14 | 环境一键构建/升级 | 1.2 | 外包 mise/winget；1.0 只探测 |
| 15 | 打开源码目录 | 1.0 | 1.1 打开 IDE |
| 16 | Docker 镜像打包 | 1.3 | 先 compose 再 build |
| 17 | Git 拉取 | 1.1 | clone / pull / 状态 |

Spring：**1.0 只 `spring-boot:run`；1.x 再 `package` + 跑 jar。**

## 补上的缺口（原清单没有、产品会缺）

这些不做成 1.0，但路线里要有名字，否则 2.0 会漏。

| 缺口 | 为什么要 | 版本 |
|------|----------|------|
| 应用壳占位导航 | 后期加页不拆布局 | 1.0 |
| 命令面板 | 功能变多后的统一入口 | 1.0 骨架，1.2 填满 |
| 系统托盘 / 开机启动 | 桌面工具基本盘 | 1.1 |
| 自动更新 | 否则 1.1 开始无法交付 | 1.1 |
| 端口占用检测 + 一键改端口 | Windows 上比 env 更常炸 | 1.2 |
| 密钥与 `.env.local`（不进 git） | 不能把密码写进 yaml | 1.2 |
| 打开 Cursor / IDEA / VS Code | 「打开目录」的下一步 | 1.1 |
| Git 状态（脏、分支）不只 pull | 迁移和协作需要 | 1.1 |
| 工作区导出 zip | 迁移的离线版 | 1.5 |
| 从 pom / package.json 生成 YAML | 1.0 已做无文件时的扫描草稿；1.1 是模板/向导 | 1.0 / 1.1 |
| 导入 compose / Taskfile | 存量项目 | 1.3 / 1.4 |
| Gradle 模块 | 只做 Maven 会挡一半 Java | 1.4 |
| 服务分组 / profile（local、test） | 多模块很快会乱 | 1.2 |
| 崩溃通知 | 状态灯不够 | 1.2 |
| 日志搜索、导出、保留策略 | 追踪要能翻历史 | 1.2 |
| 进程 CPU/内存 | 状态追踪的下一层 | 1.2 |
| Redis/MySQL 等 sidecar | Spring 项目现实依赖 | 1.3（可先 compose） |
| 公司 HTTP 代理 / npm/maven 镜像 | 国内环境 | 1.2 |
| 中英 UI | 目标用户中文，以后英文 | 1.0 中文；1.4 i18n |
| CLI `supertask up` | CI 与「不要 GUI」 | 1.5 |
| MCP | 给 Cursor/Agent 起停服务 | 1.5 |
| 插件 / 自定义 kind | 避免核心膨胀 | 2.2 |
| WSL2 后端 | Windows 上跑 Linux 工具链 | 2.2 |
| 遥测默认关 | 云账号前不要偷数据 | 全程 |

不做进路线（除非以后单独立项）：做成 DevToys 式万能工具箱、做成 Codespaces、自研 Nix。

## 版本地图

```
1.0  骨架：跑起来 + 壳占位
1.1  带走：模板 / git / 开 IDE / 更新
1.2  省事：工具链 / 端口 / 密钥 / 日志增强
1.3  容器：compose + 镜像
1.4  平台：macOS、Linux、Gradle、i18n
1.5  可搬：导出包、CLI、MCP
1.6  网关：nginx/apache/caddy 模板
1.7  横向：Python/Go/generic kind、镜像接线、分组、通知、入口归位
2.0  云：账号、同步、一键迁移
2.1  智能：AI、README 导入
2.2  生态：插件、WSL、更多语言
```

### 1.0 — 能跑（Windows）

- 工作区、无 yaml 时扫描生成草稿、Spring 多模块 `run`、Node、依赖启动  
- 端口/env、日志、状态、打开文件夹、工具链探测  
- **完整导航壳**：模板/环境/容器/网关/Git/云/AI 为占位  
- 命令面板骨架（只搜已启用命令）  
- UI 中文  
- 细则：[2026-08-25-v1-0-feature-spec.md](2026-08-25-v1-0-feature-spec.md)

### 1.1 — 能开始

- 官方模板（Spring 多模块 + Node 最少两套）  
- `git clone` / `git pull`，显示分支  
- 打开 Cursor / IDEA / VS Code / 资源管理器  
- 托盘、自动更新  
- 扫描向导升级（可 merge），不是第一次生成 yaml  

### 1.2 — 能养活

- mise/winget 安装/升级 JDK、Maven、Node  
- 端口占用、一键换端口并写回 YAML  
- `.env.local` + 密钥不进 git  
- Maven/npm 镜像、系统代理  
- 日志搜索/导出、崩溃通知、CPU/内存  
- profile / 分组  
- Spring `package` + 跑 jar（1.x，不进 1.0）  

### 1.3 — 能装箱

- `kind: compose`  
- 镜像 build/tag  
- 用 compose 起 Redis/MySQL sidecar  

### 1.4 — 能出门

- macOS、Linux  
- Gradle 多模块  
- UI 中英  
- 导入 Taskfile（映射成 scripts）  

### 1.5 — 能搬家（离线）

- 工作区导出/导入 zip（yaml + 密钥指引 + 工具链清单，不含整个 `node_modules`）  
- CLI：`supertask up/down/logs`  
- MCP：list/start/stop/logs  

### 1.6 — 能对外

- nginx / apache 配置生成与本机校验  
- 可选 Caddy 一键本机 HTTPS（开发用）  
- 服务端口 → 反代路由可视化  

### 1.7 — 能扩（横向，2026-08-29 拍板提前）

- Python / Go / generic 三服务 kind（端到端：探测/安装/扫描/启动/网关/CLI/MCP）  
- 镜像与代理运行时接线（npm registry / pip index / GOPROXY / maven settings.xml）  
- 服务分组 UI、崩溃通知（清 1.2 欠账 A1–A3）  
- 工作区包入口归位 `/workspaces`、导航五组重排  
- 细则：[2026-08-29-v1-7-feature-spec.md](2026-08-29-v1-7-feature-spec.md)

### 2.0 — 能上云

- 账号登录  
- 工作区/模板/密钥策略同步（密钥默认不同步，要明确勾选）  
- 一键迁移：导出包 + 目标机拉账号 + 对工具链差量安装  
- 发布工程收口（签名 / updater 真端点 / 安装包，清 inv-4 C1–C2）；遥测默认关（opt-in 最小事件集）  
- 细则：[2026-08-29-v2-0-feature-spec.md](2026-08-29-v2-0-feature-spec.md)（规划稿）  

### 2.1 — 能读文档

- 读 README/脚本，生成 `supertask.yaml` 草稿（人确认后写入；**确定性规则引擎**，非 LLM）  
- AI：解释日志、改端口、补健康检查；默认走用户自己的 API Key（OpenAI 兼容端点；只建议不自动应用）  
- 细则：[2026-08-29-v2-1-feature-spec.md](2026-08-29-v2-1-feature-spec.md)（规划稿；与 2.0 无硬依赖，排序为产品决策）  

### 2.2 — 能长

- 插件（自定义 kind）：**数据化 manifest 插件**（argv/字段/探测/扫描描述式，零代码执行），可经云同步分享  
- WSL2：`runtime: wsl`（实验开关，默认关）  
- 仍不自研 Nix；需要可复现时导出 devcontainer / mise.toml（纯函数渲染 + CLI `export --format`）  
- 细则：[2026-08-29-v2-2-feature-spec.md](2026-08-29-v2-2-feature-spec.md)（规划稿）  

（Python / Go 服务 kind 已于 2026-08-29 拍板提前至 **1.7** 落地；「更多语言」由 1.7 的 `generic` kind 兜底。）

## 原则

1. **先进程，后环境，再容器，再云，再 AI。** 倒过来全是演示。  
2. **能 spawn 就不内嵌。** git/docker/mise/nginx 都是 CLI。  
3. **1.0 就按 2.0 的信息架构占位**，不要先做「只有三个按钮的小工具」再重构导航。  
4. **密钥永远本地优先。** 云同步默认不同步 secret。
