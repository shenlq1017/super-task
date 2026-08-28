# 技术方向综合评估（2026-08-25，第二轮）

> 问题：前端 React+TS 是否更合适；后台 Go 还是 Rust；壳层在总体分析后再推荐。  
> 约束：轻量、Windows 优先、以后跨平台、Docker/云/AI/网关都会来、1.0 只要 Spring `run` + Node。  
> 结论先看第 5 节。

## 1. 产品三年长什么样

把用户原清单展开后，本机代码真正要「自己写」的只有几块：

| 能力 | 实现方式 |
|------|----------|
| 进程树、日志泵、健康检查 | 必须自研 |
| 打开目录、托盘、自动更新 | 桌面壳插件 |
| Git / Docker / Maven / mise / nginx | **调系统 CLI**，不要嵌 SDK |
| 云同步、账号、AI | HTTPS 客户端 |
| README 生成 YAML | 前端 + 一次 LLM HTTP |
| 本机反代 | 1.6 再决定：生成 nginx 配置 vs 内嵌小代理 |

所以语言之争 **不是**「谁的 Docker SDK 更好」（我们不链 Docker SDK），而是：

1. 桌面壳生态谁能撑 5 年  
2. 进程监管在 Windows 上谁更不容易写砸  
3. 前端与 AI 生态、招人、组件库  
4. 1.0 会不会被壳层绑死，导致 1.3 要重写  

禁止的组合：**Tauri 壳 + Go sidecar**。1.0 多一个守护进程，日志、生命周期、签名全双份，不懒。选了壳就选同语言引擎。

## 2. 前端：React+TS vs Vue 3

上一稿选 Vue 是因为「够用」。这一轮按「主流产品」重评。

| | React + TypeScript | Vue 3 + TypeScript |
|--|--------------------|--------------------|
| 桌面/独立产品默认模板 | Tauri / Electron 官方与社区几乎都是 React | 能用，例子少 |
| 精美 UI 组件 | shadcn/ui、Radix、TanStack 是 2026 默认拼装 | Naive / shadcn-vue 可用，生态窄一圈 |
| Agent 写代码质量 | React+TS 训练语料最多 | 够用，复杂桌面状态管理例子更少 |
| 状态（多服务日志流） | Zustand / TanStack Query 很熟 | Pinia 也熟 |
| 招人 / 以后外包 | 明显更易 | 国内也熟，但桌面岗仍是 React 简历多 |
| 包体 | 差不多，都是 Vite SPA | 差不多 |

**结论：前端改 React + TypeScript。**  
不是 Vue 不好，是这个产品要跟 Tauri、shadcn、以后的云控制台同一套前端习惯。Vue 不再作为 1.0 选项。

UI 库：Vite + React 18/19 + TS + **shadcn/ui**（复制组件，不锁死设计系统）。不要上 Next.js（这是桌面 WebView，不是 SSR）。

## 3. 后台：Go vs Rust

只比较「进程引擎 + 以后 CLI 包装」，不比 Web 框架。

### Go 更合适的时刻

- 团队从 Java/Node 过来，1.0 要最快出进程树  
- 以后大量「云原生 API」（Docker Engine API、K8s）——但我们规划是 CLI，这条红利用不上  
- 标准库 `os/exec`、`x/sys/windows` Job Object 文档和帖子最多  

### Rust 更合适的时刻

- 和 **Tauri 2** 同进程，零 sidecar  
- 长期跑着泵日志、文件监视、并发健康检查，内存安全有意义  
- mise、ripgrep、大量现代 CLI 是 Rust；以后若要内嵌而不是 spawn，同语言  
- Tauri 插件（updater、dialog、fs、shell、notification）是这个产品 1.1–2.0 会真用到的  

### 会痛的地方（两边都有）

| 痛点 | Go | Rust |
|------|----|------|
| Windows 杀 Maven 进程树 | Job Object，资料多 | `windows-rs` Job Object，要仔细写，但做得到 |
| 1.0 速度 | 更快 | 慢 2–4 周量级（类型与异步） |
| 以后内嵌反向代理 | 有 Caddy 模块可参考 | pingora / actix，也能做；1.6 也可只生成 nginx conf |
| 招人 | 国内后端更熟 | 桌面+系统方向在涨 |

**长期选 Rust**，前提是壳也选 Tauri。  
若壳选 Wails，后台必须 Go，否则又回到双语言。

## 4. 壳层：Electron / Wails / Tauri / 无壳

### Electron

- 前端 React 最熟、自动更新最成熟  
- 安装包 50–150MB+，空闲内存高  
- 和「轻量化」冲突  

**否决。** 除非哪天要嵌 Chromium 级终端/PDF，现在没有这个需求。

### 无壳（Go/Rust 听 localhost，系统浏览器打开）

- 出原型最快  
- 不像产品：托盘、文件对话框、自动更新、协议关联都要后补  
- 1.0 要「精美可视化桌面」，这条只适合内部狗食，不当正式壳  

**否决为正式 1.0。**

### Wails v2 + Go

- WebView2，包体小，和 Go 一体  
- 进程引擎用 Go 很顺  
- 生态、插件、自动更新、社区体量明显小于 Tauri  
- Wails v3 在 2026 仍偏新，跟 v2 不连续的风险要承担  

适合：**认定后台是 Go、并且接受桌面生态是二流** 的团队。

### Tauri 2 + Rust

- 同样 WebView2，包体 5–15MB 量级  
- 2026 轻量桌面的默认答案：updater、签名、权限模型、跨 Windows/macOS/Linux、插件齐  
- 官方与社区模板就是 React + TS  
- 以后若真要移动端（不一定要），同一套壳  

代价：引擎用 Rust，1.0 比 Wails 慢一截；Windows Job Object 要认真写测试。

### 对照（和本产品有关的行）

| 标准 | Electron | Wails 2 | Tauri 2 |
|------|----------|---------|---------|
| 轻量 | 差 | 好 | 好 |
| React+TS 资料 | 最好 | 一般 | 好 |
| 自动更新 / 签名 | 最熟 | 一般 | 好，且在变好 |
| 和引擎同语言 | Node（又重了） | Go | Rust |
| 5 年生态赌谁 | 仍在，但与轻量相反 | 不确定 | 目前最像赢家 |
| Windows 1.0 | 熟 | 熟 | 熟（WebView2） |
| Docker/Git 后期 | spawn 即可 | spawn | spawn |

## 5. 最终推荐（一套，不要拼盘）

**Tauri 2 + Rust 引擎 + React + TypeScript + Vite + shadcn/ui**

理由，按权重：

1. **壳和引擎必须同语言** — 否则 1.3 前后要重写或加 sidecar。  
2. **前端已定为 React+TS** — 与 Tauri 默认路径重合，与 Wails 不重合。  
3. **后期能力大多是 spawn CLI** — Go 的 Docker SDK 优势用不上，不必为它选 Wails。  
4. **轻量 + 跨平台 + 自动更新** — 三年后还在卖的是桌面产品，不是 Go 微服务。Tauri 更像这条路的基础设施。  
5. **1.0 范围小**（YAML、spawn `mvn`/`node`、日志、Job Object）— Rust 写得完，不需要 Go 的「先出活再重构」。

备选（仅当明确拒绝 Rust）：**Wails v2 + Go + React+TS**。不要 Vue，不要 Electron，不要 Tauri+Go。

## 6. 1.0 引擎边界（Rust 里写什么）

只这些：

- 读/写 `supertask.yaml`（未知字段保留，方便以后扩展）  
- spawn + 管道 + Windows Job Object  
- TCP/HTTP 健康检查  
- 工作区 `.supertask/` 日志滚动  
- 探测 `java`/`mvn`/`node`  
- 打开资源管理器  

Git、Docker、mise、nginx、云、AI：**命令表 + 以后再接**，1.0 不要预埋空实现文件成灾，但 UI 要占位（见 UI 扩展规划）。

## 7. 来源

- 上一轮桌面对比：`docs/research/2026-08-25-landscape.md`  
- Tauri / Wails / Electron 2026 公开对比文（同 landscape 列表）  
- 本产品路线：`docs/plans/2026-08-25-product-roadmap.md`
