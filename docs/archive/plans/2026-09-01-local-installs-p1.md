# 本机已装环境枚举（/env 多版本探测）— P1

> 2026-09-01。承接 `docs/inventory/2026-08-31-env-module-upgrade-assessment.md` 的 **F3（多版本枚举）/ E4（多版本切换）**。本文记录已实施的 P1（只读枚举）与刻意推迟到 P2 的生效接线。事实带 file:line。

## 0. 目标与边界

把 `/env` 从「每工具只认 PATH 上单版本」升级为「枚举本机全部已装安装（IDEA 式）」。**红线（全部满足）**：

- 只读：不写任何配置、不改 PATH、不改写 `NVM_SYMLINK`；
- **绝不调用 `nvm.exe`**（输出本地化、交互脆弱；文件扫描确定性）；**绝不 `nvm use`**（全局副作用 + UAC）；
- 安装 provider 固定 argv 的既有约束不变（discover 与安装链路正交）。

## 1. 候选源（对齐 IDEA / VS Code java 插件，自研）

**Java**（`toolchain/discover.rs: discover_java`）：

- Windows 注册表（`java_registry_homes`）：`HKLM\SOFTWARE\JavaSoft\JDK`（9+）、`JavaSoft\Java Development Kit`（JDK8）、`WOW6432Node\...\Java Development Kit`（32 位）、`Eclipse Adoptium\JDK`、`Microsoft\JDK`；枚举子键后读 `JavaHome`（复用 `resolver::read_reg_value`）。
- `JAVA_HOME`。
- 目录扫描（`java_dir_candidates`）：`%ProgramFiles%\{Java,Eclipse Adoptium,Amazon Corretto,Zulu,BellSoft}`、`%USERPROFILE%\.jdks`（IDEA 下载目录）；Unix：`/usr/lib/jvm`、`/Library/Java/JavaVirtualMachines`（含 `Contents/Home` 布局）、`~/.sdkman/candidates/java`。
- 每个 home `bin/java -version` spawn 验证（复用 `probe::version_of`，4s 超时），半截安装丢弃；目录名不假设版本语义（`jdk-1.8`/`jdk-25`/`temurin-21` 都收）。
- **同版本去重 JDK 优先 JRE**（`dedup_java_same_version`：`jdk-1.8` 与 `jre-1.8` 都报 `1.8.0_371`，保留含 `javac` 的那个）。

**Node**（`discover_node`）：

- nvm-windows：`NVM_HOME`（`settings.txt` 的 `root:` 优先，缺省回退 `NVM_HOME` 本身）扫 `v*` 目录，目录名即版本（`nvm_version_from_dir_name`，首段须为数字防 `vapp` 混入），仅查 `node(.exe)`/`bin/node` 存在性，不再 spawn。
- Unix：`~/.nvm/versions/node`。
- PATH 兜底：非 nvm 的 standalone node（`source=Directory`，active）。

## 2. active 判定（两层）

1. discover 内：node 经 `NVM_SYMLINK` 实链目标（canonicalize 穿透 junction/symlink）比对（`nvm_symlink_target` + `same_dir`）。
2. `probe::mark_active_by_probe_path` 回填（bundle 阶段，工具探测完成后）：PATH 命中的可执行文件目录落在某 home 内 → active；**shim 兜底**——Oracle `javapath` 是 hardlink，目录归属判不出来，退化为「probe 已验证版本号 == 安装版本号」且该工具尚无 active 才标记。

## 3. 数据结构与 IPC（additive）

- `ToolchainProbe.installs: Vec<DiscoveredInstall>`（`probe.rs`，`#[serde(default)]`）；`DiscoveredInstall { tool, version, home, source, active }`；`InstallSource`（registry/directory/env_var/nvm_dir，snake_case）+ `ToolKind` 加 lowercase serde。
- `probe_bundle()` 三路并行（工具探测 / provider 可用性 / 安装枚举），总耗时 ≈ 最慢单项（实测本机 13 项 ≈ 亚秒，60s TTL 缓存内）。
- `toolchain.probe` 的 `ToolchainProbeOut` flatten 透传 `installs`；`app.load` 预填的初始探测不含 installs（走 `toolchain.probe` 补）。旧前端忽略新字段。
- 前端：`protocol.ts` `DiscoveredInstall`；`env-page.tsx` `ToolCard` 折叠区（`installs.length > 1` 才显示，默认收起；每行 版本 + active 徽标 + 来源徽标 + 路径），**纯展示**；mock 样例 + 四语 `pages.env.{installsTitle,installActive,src*}` keys。

## 4. 测试

- `toolchain/discover.rs` 9 单测（目录名版本解析、版本段比较、nvm 目录扫描含 Unix bin 布局、settings.txt root、reg 子键解析纯函数、去重）+ 真机形状冒烟（不假设具体装了什么）。
- 全 workspace `cargo test` 绿；`cargo clippy` 新代码零告警（`toolchain/discover.rs` 0、`probe.rs` 仅余既有 `require_tools_for_kind:421`）。
- 前端 `tsc --noEmit` + `vite build` 通过。

## 5. 生效接线（P2，已完成）

> 2026-09-02 落地。原「刻意推迟」项已实现并验证，`/env` 的枚举与生效闭环交付。

- **launcher 生效**（`launcher.rs`）：`apply_pinned_version_env(toolchain, service_env, kind, installs, env)`——服务 env 的 `SUPERTASK_*_VERSION` 优先于工作区旧 pin；按 pin 用 `version_matches`（**段边界前缀**：全等或 `want+'.'` 前缀，防 `2` 匹配 `24`）解析到已装 install，`version_bin_dir` 前插 `<home>/bin`（node 在 Windows 用 `<home>`）到**子进程** PATH，java 额外设 `JAVA_HOME`（防 mvn 与运行时双 JDK）。进程级、可并存、**不改全局**（不碰 `NVM_SYMLINK`/用户 PATH/`nvm use`）。
- **engine 接线**（`engine.rs::spawn_service`）：仅 `SpawnerKind::Real` 且存在工作区或服务级版本选择时，取 `installs` 注入。
- **前端选用**（`env-page.tsx`）：ToolCard 折叠列表每行「选用」按钮（`canPin` 门控）→ `apiYamlSaveForm` 写 `toolchain[node|java]=version` + reload + 重探测；镜像 `version_matches` 前缀标记「已选用」行（绿底 + 徽标）。
- **运行详情切换**（`run-page.tsx`）：环境 Tab 只把本机已装版本按主版本去重后放入下拉，不再单独展示安装列表；切换写入当前 `services.<id>.env` 的 `SUPERTASK_*_VERSION`，启动时服务级选择优先于工作区钉扎，因此不同服务可独立使用不同版本。
- **验证**：launcher 32 单测（含 4 个 P2 pin 测试）+ 全 workspace 455 全绿；真机 E2E `pin java=17 → 子进程实际 java 17.0.7`；前端 mock CDP 驱动点击 node 22.17.1「选用」→ 成功 toast「已选用 22.17.1」；`tsc --noEmit` + `vite build` 通过。
- **边界提醒**：钉扎写的是已装 install 的**全版本**（如 `24.19.0`），与现有 pin 写 probe 版本一致，均靠 `version_matches` 前缀解析，无歧义。
- **仍未覆盖**：`require_tools_for_kind` 包管理器（pnpm/yarn）跟 node 版本走、fnm/volta/`mise ls` 多版本枚举、macOS `java_home -V`——留待后续。

（观察清单见下）
