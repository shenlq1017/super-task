# SuperTask 1.2 实现计划

> 日期：2026-08-28  
> 状态：phase 1 完成；phase 2 core 侧完成（工具链 provider/resolver/安装 + 网络策略）。mirror/registry 运行时注入与 /env UI 未做。进度见 [2026-08-28-v1-2-progress.md](2026-08-28-v1-2-progress.md)  
> 功能规格真源：[2026-08-27-v1-2-feature-spec.md](2026-08-27-v1-2-feature-spec.md)  
> 上位：repository conventions · [YAML 规范](../spec/yaml.md) · [IPC 契约](../spec/ipc.md)

本文把规格 **§18 交付顺序** 拆成可执行任务：点名文件、测试和完成标准。行为细节、错误码语义、安全边界以功能规格为准，不在此重复。

## 一句话

先把 1.2 的类型、YAML/app.json 兼容层和 IPC 数据结构落地，再按工具链 → 端口/密钥 → 日志 → 指标 → profile → jar → 真机验收的顺序接行为。本阶段只解析和持久化，不安装工具、不杀外部 PID、不写 secret 值。

## 约束（贯穿各 phase）

- 业务只进 `crates/supertask-core`；Tauri command 闭包只做 IPC 适配。
- YAML 结构化保存必须带 `base_hash`，冲突返回 `YAML_CONFLICT`。
- 未实现的命令返回 `FEATURE_SOON`，禁止假成功。`toolchain.install` 在 install provider 落地前保持 SOON。
- `launch: jar` 必须能 round-trip；真正启动放到 phase 7。此前 `runtime.startOne` / `plan_service` 返回 `LAUNCH_UNSUPPORTED`。
- YAML 继续 `version: 1`，IPC protocol 继续 `1`。1.0/1.1 文件必须能解析；新字段缺省即默认值；结构化保存不得丢掉未知 / reserved 字段。
- `log_retention` 是**顶层**字段，不要嵌进 `logging`（旧 `LoggingSpec` 结构化保存会丢掉新字段）。
- 密钥值不得进入 yaml、app data、日志、事件、遥测。

## Phase 1 — 公共模型与兼容层（本轮）

规格 §18.1。只做模型、轻量校验、持久化与 schema 测试。

### 任务 1.1 ErrorCode 1.2

- **文件：** `crates/supertask-core/src/error.rs`
- **做：** 按规格 §14.1 增加稳定码，serde `SCREAMING_SNAKE_CASE`。`PORT_DUP`、`LAUNCH_UNSUPPORTED` 已存在，不要改名。
- **测试：** 每个新码序列化为规格字符串。
- **完成标准：** 码表与 §14.1 一一对应；旧码行为不变。

### 任务 1.2 Typed YAML

- **文件：** `crates/supertask-core/src/spec/file.rs`、`validate.rs`、`mod.rs`；调用方字面量 `scan.rs`、`merge.rs`
- **做：**
  - `toolchain` / `secrets` / `profiles` 从 `Value` 改为 typed struct；未知键走 flatten `extra`
  - 顶层 typed：`network`、`log_retention`
  - `ServiceSpec.build_args: Vec<String>`（从 extra 提升）
  - 轻量校验：工具版本字符集、代理/镜像 URL 只允许 http(s) 且无 userinfo、profile id / 数量、group 长度、required 只存合法 key 名
  - `launch: jar` 允许解析；非法 launch 仍 `LAUNCH_UNSUPPORTED`
- **不在本任务：** dotenv 执行、代理注入、profile overlay 运行时、jar 启动
- **测试：** `crates/supertask-core/tests/spec_yaml.rs` + 模块单测（见「本轮测试」）
- **完成标准：** 1.0/1.1 YAML 仍解析；1.2 段 round-trip；未知字段不丢；secret required 只有名字

### 任务 1.3 AppData v2

- **文件：** `crates/supertask-core/src/appdata.rs`
- **做：** 按规格 §12.2 升到 version 2；flatten extra 保留未知键；v1 加载时迁移；迁移写入失败则用内存中的升级结果（新字段默认值），**不覆盖**旧文件
- **禁止写入：** secret 值、日志正文、指标历史、Git 凭据
- **测试：** v1 → v2 不丢未知键；写入失败旧文件仍在
- **完成标准：** 默认 `toolchainManager=auto`、`network.proxyMode=off`、`noProxy` 含 loopback、通知/指标默认开

### 任务 1.4 IPC / 事件类型

- **文件：** `crates/supertask-core/src/ipc/mod.rs`、新建 `ipc/v12.rs`
- **做：** protocol 保持 1；`st.metrics` 负载；§13 各命令的 input/output **只建数据结构**，不挂 handler
- **完成标准：** `toolchain.install` 仍在 `SOON_COMMANDS`（phase 2 才实现）

### 任务 1.5 本轮测试与文档

- **测试命令：** `cargo test -p supertask-core`
- **本轮覆盖规格 §17.1 中属于模型的部分：**
  - 1.0/1.1 YAML（含 extra / reserved）经 1.2 typed 解析后 round-trip
  - 1.2 YAML：toolchain / secrets / network / profiles / log_retention / group / env_file / `launch: jar` / build_args 能解析，未知字段 round-trip
  - `secrets.required` 只存名字不存值
  - 代理 URL 拒绝 userinfo 与非 http(s)
  - app.json v1 升 v2 不丢未知键
  - 新 ErrorCode 序列化字符串
- **文档：** 更新 `repository conventions`「当前阶段」；本计划；**不**改功能规格正文
- **完成标准：** 既有测试全绿；无 UI、无 provider、无安装

## Phase 2 — 工具链与网络

规格 §18.2、§4、§7。依赖 phase 1 的 toolchain / network 模型。

### 任务 2.1 Provider 与 resolver

- **文件：** 新建 crates/supertask-core/src/toolchain/ ；扩展 probe.rs
- **做：** auto 选择顺序（规格 §4.2）；固定 argv；默认不请求管理员；失败保留旧工具
- **测试：** 选择顺序、版本非法、权限失败、fake runner argv
- **完成标准：** 安装成功后重新 probe；解析失败 MISSING_TOOL；toolchain.install 从 SOON 改为 live

### 任务 2.2–2.3

网络策略（新建 network.rs：off/system/custom，health 绕过代理）与 /env 安装页数据。workspace 覆盖 app 默认。

## Phase 3–8（摘要，细节以规格为准）

3. **端口与 secrets：** PortInspector、ports.assign、dotenv 子集、required 只返回 key 名。不杀外部 PID。
4. **日志与通知：** 顶层 log_retention 轮转/清理、search/export、crash reason。
5. **指标与 runtime：** Job accounting、st.metrics sampler、预留 building。不改坏 1.0 状态机。
6. **profile/group：** overlay 只覆盖 env/enabled/port；切换忙则 PROFILE_SWITCH_BUSY。
7. **Spring jar：** package + 唯一 artifact + java -jar。
8. **Windows 集成验收：** fake provider、真机 PATH、Playwright、1.0/1.1 回归。

## 建议下一轮

**Phase 2 工具链 + 网络。**

## 文档债

- docs/spec/yaml.md 仍把 secrets/toolchain/profiles 标 reserved、launch: jar 标解析拒绝。
- docs/spec/ipc.md 按 phase 增补，不要一次写完。
