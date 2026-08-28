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
| `kind` | 可选 | string | 默认 `workspace`。reserved：`fragment`（多文件拼接，1.x） |
| `name` | 可选 | string | 显示名，默认取目录名。最长 80 |
| `description` | reserved | string | 工作区说明 |
| `root` | 可选 | string | 仅 `"."` |
| `env` | 1.0 | map\<string,string\> | 工作区环境；值一律当字符串 |
| `secrets` | reserved | object | 见 §7，1.2 phase 3 起读取 |
| `profiles` | reserved | object | 见 §7，1.2 phase 6 起生效 |
| `services` | 必填 | map | 至少一个服务 |
| `scripts` | 1.0 | map | 可空 |
| `toolchain` | 1.2 | object | typed：`manager`/`java`/`maven`/`node`/`package_manager`，见 1.2 规格 §4 |
| `network` | 1.2 | object | typed：`proxy`（off/system/custom）/`maven.mirror`/`npm.registry`，见 1.2 规格 §7 |
| `log_retention` | 1.2 | object | **顶层**保留策略，不要嵌进 `logging` |
| `templates` | reserved | object | 1.1 来源模板元数据 |
| `git` | reserved | object | 1.1 |
| `docker` | reserved | object | 1.3 |
| `gateway` | reserved | object | 1.6 |
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
| `group` | reserved | string | | 1.2 UI 分组 |
| `labels` | reserved | map | {} | 任意标注 |
| `port` | 1.0 | uint16 | | 主端口；1–65535。无端口的服务可省略（健康只能 `none`） |
| `ports` | reserved | uint16[] | | 附加端口，1.2 |
| `env` | 1.0 | map | {} | 覆盖工作区 `env` |
| `env_file` | reserved | string[] | | 1.2，相对 root，受路径沙箱 |
| `depends_on` | 1.0 | string[] | [] | 服务 id。成环 → `CYCLE`，禁止启动 |
| `depends_on_ex` | reserved | object[] | | `{service, condition}`，1.2。1.0 忽略执行 |
| `grace_secs` | 1.0 | uint | kind 默认 | starting 内健康失败不升为 unhealthy |
| `health` | 1.0 | object | 见下 | |
| `restart` | reserved | string | `never` | `never` \| `on-failure` \| `always`，1.2 |
| `extra_args` | 1.0 | string[] | [] | 追加到启动 argv，**不当 shell 字符串** |
| `cwd` | reserved | string | kind 决定 | 覆盖工作目录，必须在工作区内 |
| `launch` | 1.0 | string | 见 kind | spring：仅 `run`；`jar` → `LAUNCH_UNSUPPORTED` |
| `logging` | 1.0 | object | 继承工作区 | 可覆盖 max_bytes / ring_lines |
| `resources` | reserved | object | | CPU/内存提示，1.2 |
| `x-*` | extra | any | | round-trip |

**环境合并（启动时）：** 进程从当前用户环境起步 → 叠工作区 `env` → 叠服务 `env`。然后：

- `kind: spring-boot` 且最终没有 `SERVER_PORT` 且有 `port` → 注入 `SERVER_PORT={port}`  
- `kind: node` 且没有 `PORT` 且有 `port` → 注入 `PORT={port}`  

表单改端口：同时改 `port` 与上述对应键（若存在）。

### 4.2 `kind`

| kind | 首次版本 | 行为 |
|------|----------|----------|
| `spring-boot` | 1.0 | 可启动 |
| `node` | 1.0 | 可启动 |
| `compose` | 1.3 | 可解析（`service` 必填、注入类字段非法）；启动 1.3 phase 3 接入，此前 `LAUNCH` 类启动返回 `KIND_UNSUPPORTED` |
| `python` | 2.2 | 同上 |
| `go` | 2.2 | 同上 |
| `generic` | 1.x | argv 通用进程，1.0 不可启动 |
| 其它字符串 | — | 当未知 kind，不可启动，写回原字符串 |

### 4.3 `kind: spring-boot`

| 字段 | 1.0 | 说明 |
|------|-----|------|
| `module` | 必填 | 传给 `mvn -pl`，如 `user-service` 或 `:user-service` |
| `launch` | 默认 `run` | 只允许 `run`。`jar` 留给 1.x |
| `jvm_args` | reserved | 1.x |

工作目录：工作区 root。  
命令：`mvn.cmd -pl <module> spring-boot:run` + `extra_args`。  
`module` 为 `"."`（单模块工程）时省略 `-pl`，只跑 `spring-boot:run`。  
不要默认加 `-am`：Maven 会把 `spring-boot:run` 套到 reactor 里每一个项目（含没有该插件的聚合 POM），启动失败。需要 also-make 时写进 `extra_args`，或先跑 `scripts.bootstrap`（`mvn install`）。

默认 `grace_secs`: **45**。默认 `health.type`: **tcp**（连配置端口，通配符监听归一化为回环）。
需要 HTTP 探测（如 actuator）须显式写 `health.type: http`；未装 actuator 的应用打
`/actuator/health` 会 404，把运行中的服务误判为不健康。

### 4.4 `kind: node`

| 字段 | 1.0 | 说明 |
|------|-----|------|
| `dir` | 必填 | 相对 root 的前端目录，沙箱校验 |
| `package_manager` | 可选 | `npm` \| `pnpm` \| `yarn`；省略则按 lockfile / `packageManager` 探测 |
| `script` | 可选 | 默认 `dev`，否则 `start`；都没有则不能启动 |

工作目录：`root/dir`。  
命令：`<pm>.cmd run <script>`；若有 `extra_args` 则 `--` 再追加。

默认 `grace_secs`: **15**。默认 `health.type`: **tcp**（连 `127.0.0.1:port`）。

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

探测目标只打 `127.0.0.1` / `localhost`，**禁止**对非本机做健康检查（1.0 安全）。`http` URL 若 host 不是 loopback → `HEALTH_HOST_FORBIDDEN`。

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
  kind: nginx             # nginx | apache | caddy
  routes: []

cloud:
  workspace_id: ""

ai:
  enabled: false
```

JSON Schema 对这些段用 `additionalProperties: true`，避免 1.1 加字段时 1.0 读失败。

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
      - mvn -q -DskipTests install
      - pnpm --dir web install
gateway: {}
```

`gateway: {}` 必须能读回仍在。

---

## 10. JSON Schema

机器可读副本：[supertask.schema.json](supertask.schema.json)。编辑器可 `$schema` 引用。以本文冲突时以本文为准，改 schema。
