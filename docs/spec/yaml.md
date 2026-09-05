# SuperTask YAML 规范

> 文件：`supertask.yaml`（或 `supertask.yml`，二者不可并存）  
> 格式版本：`version: 1`  
> 状态：1.0 实现中。带 **1.0** 的字段本版要解析、校验、可启动。带 **reserved** 的字段必须能读、能写回、不能当未知垃圾丢掉；本版不执行。

本文是 YAML 真源。Rust 类型与 JSON Schema 必须跟本文对齐。

---

## 1. 文件与编码

| 规则 | 值 |
|------|-----|
| 位置 | 工作区根目录 |
| 编码 | UTF-8（无 BOM 优先；有 BOM 要能读） |
| 大小上限 | **1 MiB** |
| 服务数量上限 | **64**（1.0） |
| 单个 env map | **256** 键 |
| 单个脚本 `cmds` | **32** 条 |
| 并存 | 同时存在 `.yaml` 与 `.yml` → 错误 `YAML_DUP_FILE` |

`root` 只允许 `"."`（相对本文件）。禁止 `..`、盘符、UNC。真正绝对路径由引擎在打开工作区时绑定，不写进文件。

---

## 2. 设计原则

1. **一等公民是 `services`（长期进程）**，`scripts` 是一次性任务。  
2. **未知顶层键进 `extensions` 等价物**：具名 reserved 段用具名字段；再未知的进 `x-*` 或 flatten extra。  
3. **密钥不进本文件。** `secrets` 段只描述「去哪读」，不写密码。  
4. **kind 向前兼容：** 不认识的 `kind` 仍能打开文件，不能启动（`KIND_UNSUPPORTED`）。  
5. **表单保存**可丢注释和键序；**原文保存**按字节写盘。引擎结构化写回必须保留具名 reserved 段。

---

## 3. 顶层字段

| 字段 | 1.0 | 类型 | 说明 |
|------|-----|------|------|
| `version` | 必填 | uint | 必须为 `1`。`2+` 若可部分解读则警告 `SPEC_NEWER`，缺关键字段则拒绝 |
| `kind` | 可选 | string | 缺省为空（引擎不注入默认值；仅「扫描项目生成配置」会写 `workspace`）。reserved：`fragment`（多文件拼接，1.x） |
| `name` | 可选 | string | 显示名，默认取目录名。最长 80 |
| `description` | reserved | string | 工作区说明 |
| `root` | 可选 | string | 仅 `"."` |
| `env` | 1.0 | map\<string,string\> | 工作区环境；值一律当字符串 |
| `secrets` | reserved | object | 见 §7，1.2 phase 3 起读取 |
| `profiles` | reserved | object | 见 §7，1.2 phase 6 起生效 |
| `services` | 必填 | map | 至少一个服务 |
| `scripts` | 1.0 | map | 可空 |
| `toolchain` | 1.2 | object | typed：`manager`/`java`/`maven`/`node`/`package_manager`，见 1.2 规格 §4 |
| `network` | 1.2 | object | typed：`proxy`（off/system/custom）/`maven.mirror`/`npm.registry`/`python.index_url`（1.7）/`go.goproxy`（1.7），见 1.2 规格 §7 |
| `log_retention` | 1.2 | object | **顶层**保留策略，不要嵌进 `logging` |
| `templates` | reserved | object | 1.1 来源模板元数据 |
| `git` | reserved | object | 1.1 |
| `docker` | 1.3 | object | typed：`compose_file`/`project_name`/`builds`，见 1.3 规格 §5.1 |
| `gateway` | 1.6 | object | typed：`kind`/`enabled`/`port`/`bin`/`tls`/`routes`，见 §7.1 |
| `cloud` | reserved | object | 2.0 |
| `ai` | reserved | object | 2.1 |
| `logging` | 1.0 | object | 工作区级日志限额，见 §8 |
| `x-*` | extra | any | 厂商/试验字段，必须 round-trip |

服务 key、脚本 key：`^[A-Za-z][A-Za-z0-9_-]{0,63}$`。用作日志文件名，禁止路径分隔符。

---

## 4. `services.*`

### 4.1 通用（所有 kind）

| 字段 | 1.0 | 类型 | 默认 | 说明 |
|------|-----|------|------|------|
| `kind` | 必填 | string | | 见 §4.2 |
| `service` | 1.3 | string | | `kind: compose` 专用：compose 文件内的服务名；其余 kind 经 extra round-trip |
| `enabled` | 1.0 | bool | true | false 则启动全部时跳过 |
| `group` | 1.7 | string | | 运行页分组（原 reserved）；运行页按 YAML 首现序聚类，未分组排最后 |
| `labels` | reserved | map | {} | 任意标注 |
| `port` | 1.0 | uint16 | | 主端口；1–65535。无端口的服务可省略（健康只能 `none`） |
| `ports` | reserved | uint16[] | | 附加端口，1.2 |
| `env` | 1.0 | map | {} | 覆盖工作区 `env` |
| `env_file` | reserved | string[] | | 1.2，相对 root，受路径沙箱 |
| `depends_on` | 1.0 | string[] | [] | 服务 id。成环 → `CYCLE`，禁止启动 |
| `depends_on_ex` | reserved | object[] | | `{service, condition}`，1.2。1.0 忽略执行 |
| `grace_secs` | 1.0 | uint | kind 默认 | starting 内健康失败不升为 unhealthy |
| `health` | 1.0 | object | 见下 | |
| `restart` | 2.2 | string | `never` | `never` \| `on-failure` \| `always`。进程**意外退出**（非手动停止）后自动重启：`on-failure` 仅退出码 ≠ 0，`always` 含 0 正常退出。compose 不允许（重启由 compose 文件自管）；不设 `unless-stopped`（引擎生命周期即应用会话，会话内与 `always` 行为一致） |
| `max_retries` | 2.2 | uint | 5 | 自动重启次数上限（1..=100），仅 `restart` ≠ `never` 时可设，否则 `SPEC_INVALID`。1s 起指数退避（16s 封顶）；耗尽后停在 Exited 并写 `last_error`（自动重启 N 次后放弃）。手动停止取消待执行的重启；手动启动重置计数。自动重启按上次启动计划原样重放（不重建、不复检工具）；构建期退出不自动重建；应用重启后需手动启动 |
| `extra_args` | 1.0 | string[] | [] | 追加到启动 argv，**不当 shell 字符串** |
| `cwd` | reserved | string | kind 决定 | 覆盖工作目录，必须在工作区内 |
| `launch` | 1.0 | string | 见 kind | spring：`run` / `jar`（1.2）；其它 `LAUNCH_UNSUPPORTED` |
| `build_tool` | 1.4 | string | 按构建文件探测 | `maven` \| `gradle`；见 §4.3 |
| `logging` | 1.0 | object | 继承工作区 | 可覆盖 max_bytes / ring_lines |
| `resources` | reserved | object | | CPU/内存提示，1.2 |
| `x-*` | extra | any | | round-trip |

**环境合并（启动时）：** 进程从当前用户环境起步 → 叠工作区 `env` → 叠服务 `env`。然后：

- `kind: spring-boot` 且最终没有 `SERVER_PORT` 且有 `port` → 注入 `SERVER_PORT={port}`  
- `kind: node` / `python` / `go` 且没有 `PORT` 且有 `port` → 注入 `PORT={port}`（1.7 起 python/go 加入；`generic` 无生态约定不注入）  

再叠加**网络注入（1.7，优先级最低，显式 env 永远赢）**：`npm_config_registry` / `PIP_INDEX_URL` / `GOPROXY` / `MAVEN_ARGS`（maven mirror 生成 `.supertask/maven-settings.xml`）与代理键；详见 1.7 规格 §7。

表单改端口：同时改 `port` 与上述对应键（若存在）。

### 4.2 `kind`

| kind | 首次版本 | 行为 |
|------|----------|----------|
| `spring-boot` | 1.0 | 可启动 |
| `node` | 1.0 | 可启动 |
| `compose` | 1.3 | 可启动（1.3）：`docker compose up -d --no-deps <service>`；`service` 必填、注入类字段（`env`/`env_file`/`extra_args`/`build_args`/`jvm_args`/`cwd`/`restart`/`module`/`dir`/`package_manager`/`launch`）非法即 `SPEC_INVALID`；grace 默认 60s、health 默认 `tcp(port)`；详见 1.3 规格 §5 |
| `python` | 1.7 | 可启动（2026-08-29 自 2.2 提前）：见 §4.6 |
| `go` | 1.7 | 可启动（同上提前）：见 §4.6 |
| `generic` | 1.7 | 可启动（argv 通用进程）：见 §4.6。UI 不提供拼 cmdline 入口 |
| 其它字符串 | — | 当未知 kind，不可启动，写回原字符串 |

### 4.3 `kind: spring-boot`

| 字段 | 1.0 | 说明 |
|------|-----|------|
| `module` | 必填 | Maven：传给 `mvn -pl`，如 `user-service`；Gradle：模块目录相对路径（嵌套项目 `a/b`），argv 转为 `:a:b` 项目路径 |
| `build_tool` | 默认探测 | 1.4：`maven` \| `gradle`。**显式指定跳过探测**；非法值 `SPEC_INVALID` |
| `launch` | 默认 `run` | `run`（bootRun / spring-boot:run）或 `jar`（1.2：bootJar/package → `java -jar`） |
| `jvm_args` | reserved | 1.x |

**构建工具探测（1.4 §5.1）**：module 目录（单模块工程为 root）有 `build.gradle` / `build.gradle.kts` → gradle；有 `pom.xml` → maven；**两者并存 → `BUILD_TOOL_AMBIGUOUS`**（打开时警告 + 启动硬错误）；都没有 → 打开警告，启动按工具缺失（`MISSING_TOOL`）处理。

**Maven 路径**：命令 `mvn.cmd -pl <module> spring-boot:run` + `extra_args`；`module` 为 `"."`（单模块工程）时省略 `-pl`，只跑 `spring-boot:run`。**多模块 reactor**（cwd 处 `pom.xml` 的 `<modules>` 含该 module）时：启动前自动执行 `mvn -pl <module> -am install -Dmaven.test.skip=true`（仅装上游兄弟模块）；`spring-boot:run` **不加 `-am`**（否则会在聚合父 POM 上执行 run → 无 main class）。run 命令自动追加 `-Dmaven.test.skip=true`（用户 `extra_args` 已写则不再注入）。亦可手动跑 `scripts.bootstrap`。

**Gradle 路径（1.4）**：命令 `gradlew[.bat] [:module:]bootRun` + `extra_args`；`module` 为 `"."` 时省略任务路径前缀，直接 `bootRun`。Gradle 自身解析跨模块任务依赖，无 `-pl`/`-am` 问题。执行优先 wrapper：root（或 module 目录）存在 `gradlew`（Unix）/ `gradlew.bat`（Windows）则用 wrapper（Unix 无执行位时经 `sh gradlew` 执行并警告一次）；否则用 PATH 的 `gradle`；都无 → `GRADLE_WRAPPER_MISSING`，建议 `gradle wrapper --gradle-version <x>`，不代装。`launch: jar` → `gradlew [:module:]bootJar`（默认不加 `-DskipTests` 等价物），artifact 识别在 `module/build/libs`，排除 `*-plain.jar` / `*-sources.jar` / `*-javadoc.jar`，零候选 `ARTIFACT_MISSING`、多候选 `JAR_AMBIGUOUS`，复用 1.2 jar 规则。

工作目录：工作区 root。

默认 `grace_secs`: **45**。默认 `health.type`: **tcp**（连配置端口，通配符监听归一化为回环）。
需要 HTTP 探测（如 actuator）须显式写 `health.type: http`；未装 actuator 的应用打
`/actuator/health` 会 404，把运行中的服务误判为不健康。

### 4.4 `kind: node`

| 字段 | 1.0 | 说明 |
|------|-----|------|
| `dir` | 必填 | 相对 root 的前端目录，沙箱校验 |
| `package_manager` | 可选 | `npm` \| `pnpm` \| `yarn` \| `bun`；省略则按 lockfile / `packageManager` 探测 |
| `script` | 可选 | 默认 `dev`，否则 `start`；都没有则不能启动 |

工作目录：`root/dir`。  
命令：`<pm>.cmd run <script>`（Bun 为 `bun run <script>`）；若有 `extra_args` 则 `--` 再追加。

默认 `grace_secs`: **15**。默认 `health.type`: **tcp**（连 `127.0.0.1:port`）。

### 4.6 `kind: python` / `go` / `generic`（1.7）

字段合法性矩阵（非法组合 `SPEC_INVALID`；未知 kind 仍 `KIND_UNSUPPORTED`）：

| 字段 | spring-boot | node | compose | python | go | generic |
|------|:---:|:---:|:---:|:---:|:---:|:---:|
| `module` | 必填 | ✗ | ✗ | 与 entry 恰一（`python -m`） | ✗ | ✗ |
| `dir` | ✗ | 必填 | ✗ | 必填 | 可选（缺省 `.`） | 可选（缺省 `.`） |
| `entry` | ✗ | ✗ | ✗ | 必填（与 module 恰一），相对 dir | ✗ | ✗ |
| `package` | ✗ | ✗ | ✗ | ✗ | 可选（缺省 `.`；裸路径归一为 `./x`），相对 dir | ✗ |
| `program` + `args` | ✗ | ✗ | ✗ | ✗ | ✗ | program 必填、args 可选 |
| `extra_args` | ✓ | ✓ | ✗ | ✓ | ✓（传给被运行程序） | ✓ |
| `launch`/`build_tool`/`jvm_args`/`package_manager`/`script`/`service` | 按上文 | | ✗ | ✗ | ✗ | ✗ |

**python 启动**：`python <entry>` 或 `python -m <module>` + extra_args；工作目录 `dir`。
解释器解析顺序：`dir/.venv` → `dir/venv` → root `.venv`/`venv` → PATH（`.venv` 与 `venv` 并存取 `.venv`）。Windows venv 用 `Scripts/python.exe`，Unix 用 `bin/python`。`entry` 文件不存在 → `ENTRY_NOT_FOUND`（启动硬错误）。

**go 启动**：`go run <package>` + extra_args；工作目录 `dir`。`package` 目录不存在 → `PACKAGE_NOT_FOUND`。go build flags 不支持（需要时用 scripts）。`go build` 产物运行模式后排。

**generic 启动**：`<program> [args…]` + extra_args；工作目录 `dir`。`program` 含路径分隔符 = 工作区内相对路径（相对 `dir`，禁止 `..`/绝对路径），规划期解析为绝对路径并检查存在；纯名字走 PATH（PATHEXT），未命中 `MISSING_TOOL`。**UI 不提供拼 cmdline 入口**（已拍板 14）。

默认 `grace_secs`：python **15**、go **60**（冷编译宽限）、generic **15**。默认 `health.type`：**tcp**（有 `port` 时；无 port 允许 `none`）。

### 4.5 `health`

```yaml
health:
  type: none | tcp | http
  http: http://127.0.0.1:8080/actuator/health   # type=http
  interval_secs: 2
  timeout_secs: 2
```

| type | 成功 | 缺 port |
|------|------|---------|
| `none` | 进程还在即 running | 允许 |
| `tcp` | TCP connect 成功 | 非法 |
| `http` | GET 且 **2xx**（503 失败） | 非法 |

探测目标只打 `127.0.0.1` / `localhost`，**禁止**对非本机做健康检查（1.0 安全）。`http` URL 若 host 不是 loopback → `HEALTH_HOST_FORBIDDEN`。`interval_secs` / `timeout_secs` 缺省均为 **2** 秒；`health.type` 缺省等价 `none`（进程存活即视为 running，除非 kind 自带 tcp 默认，见 §4.3–§4.6）。

不走系统 HTTP 代理。

---

## 5. `scripts.*`

| 字段 | 1.0 | 说明 |
|------|-----|------|
| `desc` | 可选 | UI 说明 |
| `cmds` | 必填 | 非空字符串数组，**顺序执行**，第一条非 0 退出则停 |
| `cwd` | 可选 | 默认 workspace root，必须在沙箱内 |
| `env` | 可选 | 叠在工作区 env 上 |
| `timeout_secs` | 可选 | 默认 1800，整段脚本 |
| `depends_on` | reserved | 脚本间依赖，1.x |

1.0 同一工作区同时只跑一个脚本。  
`cmds` 来自 **yaml 文件**（用户仓库，视为可信），不接受 IPC 传入任意命令串。Windows 上每条 cmd 经 `cmd.exe /C`（工作区可信边界）。前端只能传 **script id**。

---

## 6. `logging`（工作区或服务）

| 字段 | 默认 | 上限 |
|------|------|------|
| `max_bytes` | 10485760（10 MiB） | 64 MiB |
| `ring_lines` | 2000 | 20000 |
| `retain_tail_bytes` | 2 MiB（文件超限后留下的尾） | — |

路径：`{workspace}/.supertask/logs/{serviceId}.log`，脚本为 `.supertask/logs/scripts/{scriptId}.log`。

---

## 7. Reserved 顶层段（只存不跑的版本段落）

引擎必须：反序列化、原样出现在结构化写回（若文件里有）。缺省省略。

> 1.2 起部分段已转 typed 并执行：`toolchain`（版本钉扎 + manager）、`network`（代理/镜像）。`secrets` / `profiles` / `docker` 等仍按版本路线只存不跑；已转 typed 的段未知键走 flatten extra round-trip。

```yaml
secrets:
  backend: local          # local | env | file
  file: .env.local        # 相对 root，1.2 才读

profiles:
  active: local
  items:
    local: {}

toolchain:
  java: "21"
  maven: "3.9"
  node: "20"
  manager: mise           # 1.2

templates:
  id: spring-node-basic
  version: "1"

git:
  default_remote: origin

docker:
  compose_file: compose.yaml

gateway:
  kind: nginx             # 1.6 起 typed，见 §7.1
  routes: []

cloud:
  workspace_id: ""

ai:
  enabled: false
```

JSON Schema 对这些段用 `additionalProperties: true`，避免 1.1 加字段时 1.0 读失败。

### 7.1 `gateway`（1.6 转 typed）

网关是一等能力：路由是意图，SuperTask 把意图编译成对应反代引擎的配置文件，
校验后像服务一样托管。`gateway: {}`（1.0 reserved 空段）语义不变：读回仍在、
视为未配置（`GATEWAY_NOT_CONFIGURED`），旧文件零迁移。

```yaml
gateway:
  kind: nginx              # nginx | caddy | apache（必填；缺 kind = 未配置）
  enabled: true            # 缺省 true；false = 配置保留但不启动
  port: 8080               # 监听端口，缺省 8080，只允许 1024–65535 且不撞服务端口
  bin: null                # 可选：反代二进制显式路径（探测的最终 fallback）
  tls: off                 # 仅 caddy 生效：off | internal（本机 CA HTTPS）
  routes: []               # 路由列表：
  # - host: api.localhost  #   可空 = 全匹配（catch-all）
  #   path: /api           #   必填，以 / 开头的前缀；'/' 为根
  #   target: user-api     #   服务 id（生成时解析为其当前 port）
  #   # 或 upstream: 127.0.0.1:9000  # 显式上游，与 target 互斥
  x-experiment: keep       # 未知键 flatten extra round-trip
```

校验（打开工作区时 warning，apply/start 时硬错误 `GATEWAY_ROUTE_INVALID`）：
kind 枚举；port 1024–65535 且不与任一服务 port 重复；`(host, path)` 不重复；
path 以 `/` 开头；host 为空或合法 hostname（含 `*.localhost` 形式子域）；
target/upstream 恰一且 target 服务存在并有 `port`（或 `ports` 首个）。
配置产物生成到 `.supertask/gateway/`（磁盘产物是缓存，不是编辑对象）。

---

## 8. 校验顺序

1. 文件大小、编码、dup 文件  
2. YAML 语法  
3. `version`  
4. id 字符集、服务数  
5. 每服务 kind 特有必填  
6. `depends_on` 引用存在  
7. 端口重复 → **警告** `PORT_DUP`（1.0 不阻断；1.2 阻断）  
8. 成环只在 **启动** 时硬失败（打开文件仍允许，便于人改）  

`enabled: false` 的服务仍参与引用检查（别人可以 depends_on 它，启动时再报）。

---

## 9. 最小合法例子

```yaml
version: 1
name: mall
env:
  SPRING_PROFILES_ACTIVE: local
services:
  user-api:
    kind: spring-boot
    module: user-service
    port: 8081
    depends_on: []
  web:
    kind: node
    dir: web
    port: 5173
    depends_on: [user-api]
scripts:
  bootstrap:
    desc: 安装依赖
    cmds:
      - mvn -q -pl user-service -am install -Dmaven.test.skip=true
      - pnpm --dir web install
gateway: {}
```

`gateway: {}` 必须能读回仍在。

---

## 10. JSON Schema

机器可读副本：[supertask.schema.json](supertask.schema.json)。编辑器可 `$schema` 引用。以本文冲突时以本文为准，改 schema。
