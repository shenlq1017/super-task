# SuperTask v1 功能规划与技术选型

> 日期：2026-08-25  
> 状态：技术选型已确认。1.0 细节以 [v1-0-feature-spec.md](2026-08-25-v1-0-feature-spec.md) 为准。  
> 工作名：**SuperTask**  
> 完整路线：[2026-08-25-product-roadmap.md](2026-08-25-product-roadmap.md)  
> UI 占位：[2026-08-25-ui-extensibility.md](2026-08-25-ui-extensibility.md)  
> 栈评估：[../research/2026-08-25-stack-evaluation.md](../research/2026-08-25-stack-evaluation.md)

## 一句话

Windows 上的轻量桌面工作台：用一份 YAML 描述并可视化启停 **Spring Boot 多模块 + Node** 服务，能改端口和环境变量，能看日志和运行状态。

不做 1.0 的云 IDE、不做 Nix、不做 Docker 管理器。那些是后路，不是骨架。

## 产品定位

| 别人 | 我们 |
|------|------|
| DevPod：进容器里开发 | 在本机把已有项目跑起来 |
| Taskfile：跑任务 | 监管长期服务，任务只是配菜 |
| Dockge：管 compose | 先管 JVM/Node 进程，compose 以后再说 |
| IDEA Run Config：绑 IDE | 工作区级、可带走的 YAML |
| Runme：跑 README | 以后当导入器 |

对标手感：Dockge 的干净界面 + IDEA 对 Spring 模块的理解。

## 非功能（1.0）

- **平台**：Windows 10/11 优先（WebView2）。macOS/Linux 结构预留，不承诺 1.0。
- **体积**：安装包目标 < 20MB；空闲内存远低于 Electron 桌面应用。
- **性能**：UI 操作（起停、切日志）体感 < 200ms；日志流不卡死 UI。
- **安全**：默认只操作本机用户指定的项目目录；不上传代码；不写系统服务。
- **可靠性**：点停止必须杀掉整棵进程树（Maven/npm 的子进程算在内）。数据丢失可接受（本地工具）；错误日志不能丢。

## 1.0 功能（做透）

### 工作区

- 添加已有目录为 Workspace
- 读取 / 生成 `supertask.yaml`
- 列表：服务名、类型（spring-boot / node）、端口、状态（stopped / starting / running / unhealthy / exited）
- 一键打开源码目录（资源管理器）

### Spring Boot 多模块

- 扫描根 `pom.xml`，列出带 `spring-boot-maven-plugin` 或可执行模块的子模块
- 启动方式 1.0 只支持一种：**`mvn -pl <module> spring-boot:run`**（单模块省略 `-pl`；不要默认 `-am`；可改 extra args）
- **`package` 再跑 jar 放到 1.x**，1.0 不做
- 每模块独立 `SERVER_PORT` / `SPRING_PROFILES_ACTIVE` / 自定义 env
- 健康检查：TCP 端口，可选 HTTP（默认猜 `/actuator/health`，可关）

### Node

- 扫描 `package.json`（workspace 根或子目录）
- 启动：`npm/pnpm/yarn run <script>`，1.0 默认 `dev`，可改
- 识别 lockfile 决定包管理器；手动覆盖
- 环境变量里改 `PORT`

### YAML 脚本

- `services`：长期进程（一等公民）
- `scripts`：一次性任务（bootstrap、build），可在 UI 点运行，输出进同一日志区
- UI 可编辑表单（端口、env），也可打开原始 YAML

### 运行时

- 启动 / 停止 / 重启单个或全部（按 `depends_on` 排序）
- 实时日志（stdout/stderr 合并，可按服务过滤）
- 状态来自：进程是否在 + 健康检查
- 工作区级 env + 服务级 env，后者覆盖前者

### 探测（不安装）

- 显示 `java -version`、`mvn -v`、`node -v`、包管理器版本
- 缺失时明确报错，并告诉人去装，不在 1.0 里代装

## 1.0 明确不做

云账号、同步、一键迁移、AI、README 生成服务、nginx/apache、Docker 镜像打包、Python/Go/C++、Nix、完整工具链安装、多机器。

Git clone、官方模板：标成 **1.1**。1.0 导航里要有占位页，不要等 1.1 再加菜单。

## YAML 草图

```yaml
version: 1
name: mall
root: .

env:
  SPRING_PROFILES_ACTIVE: local

services:
  user-api:
    kind: spring-boot
    module: user-service
    port: 8081
    health:
      http: http://127.0.0.1:8081/actuator/health
    env:
      SERVER_PORT: "8081"
    depends_on: []

  web:
    kind: node
    dir: web
    package_manager: pnpm
    script: dev
    port: 5173
    env:
      PORT: "5173"
      VITE_API_URL: http://127.0.0.1:8081
    depends_on: [user-api]

scripts:
  bootstrap:
    desc: 安装依赖
    cmds:
      - mvn -q -DskipTests install
      - pnpm --dir web install
```

1.0 解析器要小：未知字段忽略并警告，不要做成通用工作流引擎。

## 技术选型（推荐）

详见 ADR-0002 与 stack-evaluation。摘要：

| 层 | 推荐 | 为什么 | 备选 |
|----|------|--------|------|
| 桌面壳 | **Tauri 2** | 轻量、React 资料多、更新/权限/跨平台是三年桌面基建 | 拒绝 Rust 时整栈改 Wails 2 |
| 引擎 | **Rust** | 必须和 Tauri 同语言；后期 git/docker 用 CLI，Go SDK 用不上 | 不要 Tauri+Go sidecar |
| UI | **React + TS + Vite + shadcn/ui** | 桌面主流拼装；不要 Next.js | Vue 不再作 1.0 选项 |
| 配置 | 自有 YAML | 未知字段保留，方便 1.x 加段 | 不要 1.0 双格式 |
| 本地状态 | 工作区目录下 `.supertask/` | 日志文件 + UI 状态 | 以后再 SQLite |
| 日志 | 每服务滚动文件 + UI 环形缓冲 | 简单 | 不要 1.0 上 ELK |
| 工具链（1.2） | **mise** + winget | 不要自研安装器 | |

不推荐 Electron：和轻量化直接冲突。

### 架构（1.0）

```
┌─────────────────────────────────────────┐
│  React + TS  (WebView2 / Tauri)         │
│  AppShell 导航全占位 + 运行/日志/配置   │
└──────────────────┬──────────────────────┘
                   │ Tauri invoke / event
┌──────────────────▼──────────────────────┐
│  Rust                                    │
│  workspace  │ yaml  │ detector           │
│  supervisor │ health│ probe(jdk/node)    │
│  windows Job Object 杀进程树             │
└─────────────────────────────────────────┘
         │ spawn / pipe
         ▼
   mvn / java / node 子进程
```

Windows 必须用 **Job Object**（或等价的进程树杀法）。只 `Kill()` 父进程会留下一堆孤儿 JVM，这是本产品的正确性底线，不是优化。

## 关键决策（ADR 摘要）

完整条目见 `docs/adr/0002-tauri-rust-react.md`（0001 已作废）。

1. 本机进程优先，容器后置  
2. Tauri 2 + Rust + React+TS，不 Electron、不 sidecar  
3. 自有 YAML，未知字段保留；导入器以后再做  
4. 1.0 只探测工具链，不安装  
5. 1.0 按 2.0 信息架构占位导航  

细路线见 `docs/plans/2026-08-25-product-roadmap.md`，此处不重复。

## 1.0 成功标准

1. 能打开一个真实的多模块 Spring Boot + 前端 Node 仓库，生成或手写 YAML 后全部启动。  
2. 改某个模块端口，重启后日志和健康检查跟新端口走。  
3. 点停止后，任务管理器里看不到残留 `java`/`node`。  
4. 日志按服务可看、可滚动。  
5. 未装 Maven/Node 时，UI 说人话，不假装启动成功。
6. 左侧导航含模板/环境/Git/容器/网关/云/AI 占位，不是只有三个按钮的临时壳。

## 风险

| 风险 | 缓解 |
|------|------|
| Maven 进程树杀不干净 | 1.0 第一个可运行检查就测 Job Object |
| Gradle / 非标准目录 | 1.0 只承诺 Maven；Gradle 列为 1.x |
| pnpm workspace 复杂 | 1.0 按 `dir` + script，不解析 pnpm 图 |
| WebView2 缺失（极少） | 启动时检测，引导安装 Evergreen |
| 用户其实更想要 Docker | 1.3 再加；1.0 文案写清「本机进程」 |

## 建议的下一步

技术选型已确认（ADR-0002）。1.0 功能已深化为规格文档。下一份：**1.0 实现计划**，然后脚手架。
