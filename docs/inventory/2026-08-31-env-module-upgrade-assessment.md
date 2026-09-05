# /env（环境·探测）页多角度评估与深化方向

> 2026-08-31。dated 分析文档（调研类，非 living）。范围：**仅 `/env` 单页**（工具链探测/安装/钉扎 + 网络镜像代理），不含 `/docker`、`/gateway`。事实均带 file:line 证据，立实施项前以代码为准复核。
>
> 红线核查（结论：未发现违反）：安装全部是显式按钮（不代装 ✓）；UI 不拼 cmdline ✓；yaml 保存带 `base_hash` ✓；soon 不假成功 ✓。

## 0. 结论摘要

`/env` 的后端链路完成度高于前端表达：**探测→安装→升级→钉扎→网络注入**五环已通（1.2/1.4/1.7 三次迭代），但页面停在"工具卡片网格 + 一张网络表单"的第一版形态。最值得做的三件事：

1. **版本选择列表**——后端 `WINGET_PACKAGES` 是硬编码白名单（`toolchain/manifest.rs:42-68`：java 仅 21/17/11、python 仅 3.13/3.12/3.11、go 仅 1.23/1.22），前端却让用户**自由手输版本**（`env-page.tsx:447-453`），输错即 `TOOLCHAIN_VERSION_INVALID`。数据已在后端，改成下拉是低风险高回报。
2. **环境诊断/就绪度**——工作区 `toolchain` 钉扎版本与实测版本**不做比对**（`requiredBadge` 只展示不告警，`env-page.tsx:408`）；两管理器全缺时安装按钮静默禁用、无"先装 mise"指引（违背设计真源「错误说人话」，见 §3.1）。
3. **两级网络闭环**——后端 `AppNetwork` app 级默认已建好（`appdata.rs:30-40`、`network.rs:21-30` 八字段齐全、`commands.rs:563-568` 合并已通），但**没有任何本地 UI 能写它**（`network.save` 是有常量无注册的死代码，`ipc/mod.rs:65`）；无工作区时 `/env` 网络能力为零。

## 1. 现状盘点（功能矩阵）

| 环节 | 状态 | 证据 |
|---|---|---|
| 探测（9 工具 + 3 网关） | ✅ 并行探测、4s 超时、平台目录兜底；❌ **无缓存**，进页即全量重探 | `probe.rs`；`env-page.tsx:82-85` |
| 安装 / 升级 | ✅ mise 优先 winget 兜底、operation 进度、失败错误码下一步；❌ 版本手输、无可选列表、无卸载、管理器全缺时无指引 | `env-page.tsx:132-149`、`toolchain/` |
| 钉扎 | ✅ 显式"固定"写回 `toolchain.*`（带 `base_hash`）；❌ `wsTc.manager` 只读展示、无 UI 可改 | `env-page.tsx:170-172,439-443` |
| env_delta 注入 | ⚠️ 仅显式 `toolchain.manager: mise` 才注入，**失败静默回退** | `launcher.rs:479` 附近 |
| 网络（代理+镜像） | ✅ 工作区级 7 字段表单、脏守卫、`yaml.saveForm` 保存；❌ `no_proxy` 有字段无 UI（`protocol.ts:402`）；❌ 无 app 级（全局）入口 | `env-page.tsx:236-357` |
| 生效环境快照 | ⚠️ `env.effective` 已实现（含来源层），仅运行页抽屉消费（`run-page.tsx:526`），`/env` 无查看面 | `ipc/mod.rs:108` |
| mock 模式 | ❌ `toolchain.install` 拒绝 python/go（`mock.ts:1247-1250`），与前端 `CORE_TOOLS` 不一致，mock 下无法演示 1.7 链路 | `mock.ts` |

## 2. 角度 A：功能完整性（缺口清单）

| # | 缺口 | 影响 | 成本 |
|---|---|---|---|
| F1 | 版本可选列表缺失（手输 vs 后端白名单结构性矛盾） | 安装失败率高、体验差 | 低中（数据源现成：`WINGET_PACKAGES` + `mise ls-remote`） |
| F2 | 无卸载（`mise uninstall` 对称缺失） | 低频 | 低 |
| F3 | 无多版本枚举/切换（探测只认 PATH 单版本） | 有 mise 却用不上多版本能力 | 中（限 env_delta 方案则到点） |
| F4 | `no_proxy` 无 UI | 字段闲置；企业代理场景刚需 | 极低 |
| F5 | app 级（全局）网络默认无 UI；无工作区时网络能力为零 | 两级合并只做了一半 | 低中 |
| F6 | 工作区 `toolchain.manager` 无选择 UI（只能改 yaml） | 与 env_delta 门槛联动，隐性知识 | 低 |
| F7 | `/env` 无生效环境快照查看面（工作区级） | 注入链是黑盒 | 低中（后端已就绪） |
| F8 | 诊断缺失：钉扎≠实测不告警、缺项无汇总、镜像/代理连通性不自检 | 用户不知道"环境到底对不对" | 中 |
| F9 | mise/winget 全缺时无安装指引（安装按钮禁用即终点） | 违背「错误说人话」 | 极低 |

## 3. 角度 B：页面设计性

### 3.1 与设计真源（`docs/archive/plans/2026-08-26-ui-design-1.0-2.1.md`）对照

| 真源要求 | 现状 |
|---|---|
| `/env` 顶上复用 **ProbeBar**（§2.5） | 未实现；前端无任何 ProbeBar 组件（各页自绘探测卡） |
| statusbar 30px 含「工具链探针」（§2） | 未实现；无状态栏 |
| 设置页「网络代理」分组（§2.6，1.2 占位） | 未落；全局网络默认至今无家（应归位此处或 /env 全局 tab，并回改真源） |
| 「错误说人话…给出可执行指引」（§3.4） | 部分：安装失败有错误码下一步（§15.1）✅；管理器缺失引导 ❌ |
| 「表单保存丢注释 → 保存前 toast 说明」（§2.4） | NetworkCard 仅保存后报成功，无保存前提示（`env-page.tsx:252-266`） |

### 3.2 信息架构

- 一页混两域（工具链 + 网络），卡片风格不一致：工具卡带图标态/操作区，Gradle 是扁平信息条（`env-page.tsx:203-220`），网络是表单卡——三套视觉语言。页面继续加重（如加快照/诊断）前应先分区或分 tab。
- **无就绪度汇总**：打开页面只能逐卡扫，缺"5 项中 3 项就绪、2 项缺"的头部概览。
- 无工作区时的空态只有一行 `pinHint`（`env-page.tsx:222-226`）；NetworkCard 整个禁用（保存按钮 `disabled={!hasWs}`），"为什么不能配"没有解释。

### 3.3 交互细节

- **管理器选择是页级全局状态却渲染在每张卡里**（`env-page.tsx:65` 定义、`190-192` 逐卡传入）：改一张卡影响所有卡，无视觉提示，易误读为卡片私有。应提升为页头统一控件或改为逐卡状态。
- 代理模式 `off/system/custom` 选项为英文裸值且未翻译（`env-page.tsx:289-297`）；`off` 时 HTTP/HTTPS 输入仍可编辑（应条件禁用或收起）。
- 代理/镜像 URL 无任何校验（拼错到启动时才暴露）。
- `requiredBadge` 对 npm/pnpm/yarn 显示的是**工具名而非版本号**（`env-page.tsx:126-128` 返回 key），语义含混（实为"包管理器选择"）。
- 初始态用 `as unknown as ToolchainProbeOut` 强转并置 `managers: null`（`env-page.tsx:61`）：类型谎言 + 首帧管理器徽标与真实探测不一致。
- 重探按钮无"上次探测时间"；探测结果不共享（docker/gateway 页各自再探）。

### 3.4 可达性与文案

- aria-label 覆盖较好（版本输入、管理器下拉）✅；图标按钮有 title ✅。
- 四语 locale `pages.env.*` keys 齐全；但 `off/system/custom`、busy 态动词等细节需过一遍一致性。

## 4. 角度 C：债务收口

| # | 债务 | 处置 | 成本 |
|---|---|---|---|
| D1 | probe 无缓存：进页全量重探、operation 成功后再全探（`env-page.tsx:99`），9 工具×版本命令 4s 超时 | core 加 TTL 缓存 + 失效点（install/upgrade/钉扎后失效）；`docker.probe` 已有缓存先例（`engine.rs:1761`） | 中 |
| D2 | `network.save` 死代码（常量 `ipc/mod.rs:65` + `NetworkSaveInput`，src-tauri 无注册） | 删除；或实现为 app 级网络保存命令（配合 F5） | 极低/低 |
| D3 | `env-page.tsx:61` 强转 + `managers: null` | 改为可空的干净类型 | 极低 |
| D4 | mock install 缺 python/go（`mock.ts:1247-1250`） | 补白名单 + 默认版本 | 极低 |
| D5 | env_delta 静默回退（`launcher.rs:479` 附近） | 回退可见化（启动日志/事件提示），或按 §0-3 默认化 | 低 |

## 5. 角度 D：深化（功能纵深，均通过到点判定）

- **S1 版本选择列表**（最高优先）：下拉 = `WINGET_PACKAGES` 白名单（`manifest.rs:42-68`）∪ `mise ls-remote <tool>`（mise 可用时），`lts` 别名保留。IPC 加 `toolchain.versions`（core 新函数，走 ToolRunner 接缝）。根治 `TOOLCHAIN_VERSION_INVALID`。
- **S2 环境诊断**：① 钉扎版本 × 实测版本比对（不匹配 → 告警徽标 + 升级/重装动作）；② 缺项汇总连现有安装按钮；③ 镜像/代理连通性只读自检（守「能探测就别装」；健康检查已有绕代理先例 `strip_proxy_vars`）。
- **S3 就绪度概览**：页头汇总条（就绪/缺失/版本冲突计数）+ 按当前工作区 required 过滤的"本工作区需要"视图。
- **S4 app 级网络默认**：设置页新增「网络」分组（归位设计真源 §2.6 的 1.2 占位），读写 `AppNetwork`；`/env` NetworkCard 标注"未填则继承全局"。
- **S5 工作区 manager 选择 UI**：`toolchain.manager` auto/mise/winget 下拉（写回走 `yaml.saveForm`），与 env_delta 注入策略联动展示。
- **S6 env_delta 默认化**：有钉扎即注入，去掉"显式 mise"门槛（1.7 接线补强）。
- **S7 生效环境快照查看面**：`/env` 底部或运行页已有的组件复用，展示注入键 + 来源层（`env.effective` 后端现成）。

## 6. 角度 E：升级（结构与产品定位）

- **E1 环境就绪度引导流**：打开工作区时按 `toolchain` 要求 × probe 出"缺项清单 → 显式安装"引导（不违反不代装）。把 `/env` 从"被动查询页"升级为"主动对齐流"，是产品差异化（对标语义是"环境对齐"而非"容器管理"）。成本中高，涉 welcome/workspaces 流程。
- **E2 ProbeBar 组件化 + 状态栏探针**：把探测结果做成共享组件/全局状态（顺带解决"各页重复探测"），落设计真源 §2/§2.5 欠账。依赖 D1 缓存。
- **E3 页面分区重构**：工具链 / 网络 / 诊断三区或 tab，承接 S2/S3/S7 落地前的信息架构前置。
- **E4 多版本切换**（观察）：仅走 env_delta 方案（`mise which tool@ver`）；.mise.toml 路线属 2.2 导出，勿越线。

## 7. 优先级建议（三档）

1. **立即做**（低成本、高回报/地基）：S1 版本选择列表 · D1 probe 缓存 · D3+D4+D2 微修与死代码包 · F4 no_proxy UI + 代理模式交互修正（off 禁用输入、选项翻译）· F9 管理器缺失指引。
   理由：S1 数据现成直接消灭一类失败；D1 是 S2/E2 及 docker/gateway 页的共同前提；其余均半天内量级。
2. **下一版**：S2 环境诊断 · S3 就绪度概览 · S4 app 级网络默认 · S5 manager 选择 · S6 env_delta 默认化 · E1 就绪度引导流。
   理由：价值最高的一批，但各自成本中，且依赖第一档的缓存/列表地基。
3. **观察**：E2 ProbeBar/状态栏 · E3 分区重构（随 S2/S3/S7 决定） · E4 多版本切换 · F2 卸载 · S7 快照查看面（可随 S2 顺带）。
   理由：依赖页面形态稳定 / 低频 / 与 2.2 边界（.mise.toml）待划线。

## 8. 立项须知（测试接缝与基线）

- 接缝：`toolchain/` 的 ToolRunner + FakeRunner（S1 版本查询可注入）；`probe.rs` 内联单测（D1）；`network.rs` ×11 单测（S4）；`engine.rs` 有 `env_effective_snapshot_sources_and_lifecycle` 先例（S7）；前端 mock 模式四态开关先例（`docker-page` 的 `st:mockDockerMode`）。
- 基线：`cargo test -p supertask-core`（455）、locale 1067 keys、CLI 20、server 14。
- 流程：立项写 dated feature-spec 入 `docs/archive/plans/`；交付/欠账变化回改 `docs/inventory/` 对应 inv 文档；设计真源偏离项（ProbeBar/设置页网络分组）在立项时回改真源或明确废止。
