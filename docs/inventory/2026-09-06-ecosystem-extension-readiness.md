# 生态扩展就绪审计（方向九 · 首切片）

> 结论基于代码与 spec 证据，不是主观判断。每条证据带 file:line。
> 审计对象：docs/ROADMAP.md §11 四个候选项的「契约是否足以支撑扩展」。
> 日期：2026-09-06 · 基线：fa2f290（方向八 M1 三平台 CI 全绿之后）

## 一、核心契约稳定性结论

| 契约面 | 证据 | 结论 |
|---|---|---|
| kind 开放字符串 | `docs/spec/yaml.md:117`：未知 kind 不可启动、写回原字符串；AGENTS.md 规则 6 | **稳定且加法式**——新 kind 只增不改，旧文件零迁移 |
| 工作区 spec 前向兼容 | `docs/spec/yaml.md:269`：所有段 `additionalProperties: true`，新字段不破坏旧读者 | **稳定**——数据级扩展（模板包携带的 yaml 片段）天然兼容 |
| 错误码 | `docs/spec/ipc.md` §7 码表 + 前缀纪律（AGENTS.md 规则 4）；`TEMPLATE_INVALID` / `TEMPLATE_WRITE` / `TEMPLATE_ID_CONFLICT` 等已成族（ipc.md:406-407、523） | **稳定且可加法扩展**——模板导入/导出零新增码即可表达 |
| 权限模型 | 健康只打 loopback；路径过 sandbox；前端无 shell/fs（AGENTS.md 规则 2/3）；模板清单 files 路径校验（template.rs:185-195） | **稳定**——模板导入是 appdata 内的数据复制，不新增权限面 |
| 导入器 preview/apply 模式 | importer/（Taskfile、readme）与 merge.rs 的 preview→confirm→apply，已被孤儿纳管（ipc.md §10.16）、数据快照（§10.18）复用三次 | **模式已验证三次**，可继续复用 |
| 模板子系统 | 「目录 + template.yaml 清单」统一模型；builtin 编译期嵌入（template.rs:26）+ local 库（appdata）；id 冲突保护（template.rs:506-509）；params/blocks 校验（template.rs:295、203） | **模型成型**，缺的只是库的写入路径（见下） |

## 二、逐候选项判定

### 1. 插件 / 自定义 kind —— 维持「不做」
- 路线图判定不变：抽象未收敛前固化 ABI 会把错误抽象变成契约（ROADMAP §11）。
- 审计补充证据：`features.rs` 13 页全部 live、六种 kind 全部可启动，启动器矩阵刚随
  三平台恢复而稳定（launcher 的 maven/node 程序名平台化，2026-09-06 e9be887）——
  此刻抽象仍在收敛期（如 launch 字段仅支持 `run`，template.rs:347-351）。
- **重启条件**：kind 字段矩阵与 launch 语义稳定一个发布周期后再评估。

### 2. WSL2 后端 —— 维持「不做」
- 前置门槛未变：三平台 CI 刚建立（方向八 M1），M4 真机冒烟未做。
- 审计补充：proc 层已有 unix 分支（`proc/unix.rs`，M1 期间确认 Linux cgroups 与
  macOS 回退），但 WSL2 是「Windows 宿主里的 Linux 工具链」，归属平台推进专项
  （M6 差异收敛之后），不属生态切片。

### 3. 团队环境基线与漂移检测 —— 维持「不做」
- 云的重新定位（官方服务 vs 团队同步协议）未拍板（ROADMAP §11）；云端仍为
  自托管参考实现（AGENTS.md 状态速览）。投入即沉没成本。

### 4. 模板生态 —— **唯一解锁，本切片实现最小原型**
- 现状：模板「消费侧」完整（list/create/preview，内置 7+ 套 + 组合块向导），
  「供给侧」缺失——本地库（`%APPDATA%/SuperTask/templates/<id>/`）没有任何写入路径：
  外部模板进不来，本地模板出不去。ipc.md:506 已定义 `source: "builtin" | "local"`
  且模板页已按来源区分（templates-page.tsx:213），UI 侧挂点现成。
- 契约缺口小且自包含：补 `templates.import`（zip → 本地库）与 `templates.export`
  （本地/内置 → 可分享 zip）两个命令；错误码零新增（复用 TEMPLATE_* +
  `TARGET_NOT_EMPTY` + `NOT_FOUND`）。
- 风险与对策：外部 zip 是唯一不受信输入 → 条目数/字节上限（对齐 snapshot.rs:21-23
  的快照口径，模板规模取 2000 条 / 64 MiB）、条目路径安全规则（对齐 snapshot.rs:460）、
  id 字符集显式校验（`parse_manifest` 不校验 id 字符集，template.rs:168 只查 id==目录名
  ——导入的 id 直接成为目录名，必须先过 `[A-Za-z0-9_-]` 单段规则）、清单⇄文件
  双向一致性（多文件拒收）、与内置/现有本地 id 冲突拒收、staging 目录 + 原子改名
  失败即清理。
- 退出/回滚路径：原型失败不影响既有契约——两个新命令独立于 list/create/preview，
  删除命令与本地库目录即完全退出；不污染核心 kind、IPC 码表或配置兼容性。

## 三、未决问题（影响范围 + 后续验证方式）

1. **分享格式的版本演进**：清单 schema 变化（如新增 params 目标）时，旧导入器对
   新包的处置。影响面：`templates.import` 一处。验证方式：包内 `version` 字段 +
   parse 严格失败（`TEMPLATE_INVALID`，`invalid` 标记模型兜底，template.rs:62-64），
   不做自动升级；出现真实社区包后再定前向策略。
2. **probe_one_in 破坏候选不 fallthrough**（方向八 M1 期间 WSL 发现）：影响工具探测
   显示，不影响本切片。验证方式：待真实多 shim 环境反馈后按需改。
3. **模板远端分发**（URL / 市场）：本切片明确不做；`templates.import` 的 zip 单元
   即未来的分发载荷，届时仅加「下载 + sha256」前置步骤，契约不推翻。
