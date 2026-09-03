import type {
  AppLoadOut,
  GitStatus,
  HelloOut,
  LogLine,
  LogSource,
  OpState,
  Prefs,
  RuntimeSnapshot,
  ScanMergeItem,
  ScanPreviewOut,
  ReadmePreviewOut,
  ServiceSpec,
  ScriptRuntimeView,
  ServiceRuntimeView,
  ForeignService,
  SuperTaskFile,
  TaskfileImportItem,
  TaskfilePreviewOut,
  TemplateSummary,
  ToolchainProbe,
  ToolchainProbeOut,
  ManagerAvailability,
  WorkspaceOpenOut,
  YamlView,
  YamlSaveOut,
  RtState,
  GatewayConf,
  GatewayStatusOut,
  ToolProbe,
  CloudStatusOut,
  CloudSyncOut,
  CloudMigratePlanOut,
  CloudMigrateApplyOut,
  AiConfigOut,
  AiStatusOut,
  AiTask,
  AiTemplate,
  ServiceMetrics,
} from "./protocol";
import { PROTOCOL, cmd, event } from "./protocol";

// ---------------------------------------------------------------------------
// In-memory demo workspace so the UI is fully interactive in a plain browser
// (vite) without Tauri. Mirrors a Spring Boot + Node stack like the
// knife4j-demo-openapi3 project the integration tests target.
// ---------------------------------------------------------------------------

const DEMO_ROOT = "C:/path/to/your/workspace";

function demoSpec(): SuperTaskFile {
  return {
    version: 1,
    kind: "supertask.workspace",
    name: "knife4j-demo-openapi3",
    description: "Knife4j OpenAPI3 演示工作区（mock）",
    root: DEMO_ROOT,
    env: { SPRING_PROFILES_ACTIVE: "dev" },
    services: {
      gateway: {
        kind: "spring-boot",
        enabled: true,
        group: "backend",
        labels: { module: "gateway" },
        port: 8080,
        ports: [8080],
        env: { SERVER_PORT: "8080" },
        env_file: [],
        depends_on: ["auth-service", "order-service"],
        grace_secs: 30,
        health: { type: "http", http: "http://localhost:8080/actuator/health", interval_secs: 5, timeout_secs: 3 },
        restart: null,
        extra_args: [],
        cwd: null,
        launch: null,
        module: "knife4j-gateway",
        jvm_args: ["-Xmx512m"],
        dir: null,
        package_manager: null,
        script: null,
        logging: null,
      },
      "auth-service": {
        kind: "spring-boot",
        enabled: true,
        group: "backend",
        labels: {},
        port: 8081,
        ports: [8081],
        env: { SERVER_PORT: "8081" },
        env_file: [],
        depends_on: [],
        grace_secs: 30,
        health: { type: "http", http: "http://localhost:8081/actuator/health", interval_secs: 5, timeout_secs: 3 },
        restart: null,
        extra_args: [],
        cwd: null,
        launch: null,
        module: "knife4j-auth",
        jvm_args: ["-Xmx512m"],
        dir: null,
        package_manager: null,
        script: null,
        logging: null,
      },
      "order-service": {
        kind: "spring-boot",
        enabled: true,
        group: "backend",
        labels: {},
        port: 8082,
        ports: [8082],
        env: { SERVER_PORT: "8082" },
        env_file: [],
        depends_on: ["auth-service"],
        grace_secs: 30,
        health: { type: "http", http: "http://localhost:8082/actuator/health", interval_secs: 5, timeout_secs: 3 },
        restart: null,
        extra_args: [],
        cwd: null,
        launch: null,
        module: "knife4j-order",
        jvm_args: ["-Xmx512m"],
        dir: null,
        package_manager: null,
        script: null,
        logging: null,
      },
      "web-console": {
        kind: "node",
        enabled: true,
        group: "frontend",
        labels: {},
        port: 3000,
        ports: [3000],
        env: { PORT: "3000" },
        env_file: [],
        depends_on: [],
        grace_secs: 10,
        health: { type: "http", http: "http://localhost:3000/", interval_secs: 5, timeout_secs: 3 },
        restart: null,
        extra_args: [],
        cwd: null,
        launch: "dev",
        module: null,
        jvm_args: [],
        dir: null,
        package_manager: "npm",
        script: "vite",
        logging: null,
      },
      "docs-site": {
        kind: "node",
        enabled: false,
        group: "frontend",
        labels: {},
        port: 4100,
        ports: [4100],
        env: { PORT: "4100" },
        env_file: [],
        depends_on: [],
        grace_secs: 10,
        health: { type: "http", http: "http://localhost:4100/", interval_secs: 5, timeout_secs: 3 },
        restart: null,
        extra_args: [],
        cwd: null,
        launch: "start",
        module: null,
        jvm_args: [],
        dir: null,
        package_manager: "npm",
        script: "serve",
        logging: null,
      },
      // 1.3 kind: compose sidecar：redis 运行中、mysql 已退出（覆盖两种容器态展示）
      redis: {
        kind: "compose",
        service: "redis",
        enabled: true,
        group: "sidecar",
        labels: {},
        port: 6379,
        ports: [6379],
        env: {},
        env_file: [],
        depends_on: [],
        grace_secs: 60,
        health: { type: "tcp", interval_secs: 5, timeout_secs: 3 },
        restart: null,
        extra_args: [],
        cwd: null,
        launch: null,
        module: null,
        jvm_args: [],
        dir: null,
        package_manager: null,
        script: null,
        logging: null,
      },
      "mall-db": {
        kind: "compose",
        service: "mysql",
        enabled: true,
        group: "sidecar",
        labels: {},
        port: 3306,
        ports: [3306],
        env: {},
        env_file: [],
        depends_on: ["redis"],
        grace_secs: 60,
        health: { type: "tcp", interval_secs: 5, timeout_secs: 3 },
        restart: null,
        extra_args: [],
        cwd: null,
        launch: null,
        module: null,
        jvm_args: [],
        dir: null,
        package_manager: null,
        script: null,
        logging: null,
      },
    },
    scripts: {
      build: {
        desc: "全量构建",
        cmds: ["mvn -q -pl knife4j-gateway,knife4j-auth,knife4j-order clean package -DskipTests"],
        cwd: null,
        env: {},
        timeout_secs: 600,
        depends_on: [],
      },
    },
    logging: null,
    // 1.3：compose sidecar + 显式镜像构建条目（feature spec §5.1/§6）
    docker: {
      compose_file: "compose.yaml",
      project_name: "mall",
      builds: [
        { name: "mall-user", context: "user-service", dockerfile: null, tags: ["mall-user:local"] },
      ],
    },
    // 1.6：demo 网关（nginx + 3 条路由，§11 mock 要求）
    gateway: {
      kind: "nginx",
      enabled: true,
      port: 9090,
      bin: null,
      tls: "off",
      routes: [
        { host: null, path: "/api", target: "gateway", upstream: null },
        { host: "api.localhost", path: "/", target: "gateway", upstream: null },
        { host: null, path: "/", target: "web-console", upstream: null },
      ],
    },
  };
}

type ServiceRT = {
  id: string;
  state: RtState;
  pid: number | null;
  port: number | null;
  kind: string;
  health: { ok: boolean; at_ms: number; detail: string } | null;
  started_at_ms: number | null;
  last_exit: { code: number; at_ms: number } | null;
  last_error: string | null;
  log_seq: number;
};

const mockCloud = {
  loggedIn: false,
  email: null as string | null,
  lastSyncedMs: null as number | null,
  conflicts: ["w1"] as string[],
  telemetryEnabled: false,
  endpoint: "https://cloud.supertask.local.example",
};

/** 2.1 AI mock（确定性回文；命名多配置 + 模板/全局指令；key 只存布尔，绝不回显）。 */
let mockAiSeq = 0;
const mockAi = {
  configs: [] as { id: string; name: string; isDefault: boolean; provider: string; model: string; baseUrl: string; timeoutSecs: number; maxTokens: number; authMethod: string; proxyEnabled: boolean; proxyUrl: string | null; contextWindow: number | null; maxRetries: number }[],
  templates: [] as { id: string; name: string; content: string; enabled: boolean }[],
  instructions: "" as string,
  keySet: false,
  usage: { date: "", count: 0 } as { date: string; count: number },
};
function mockAiUsage() {
  const today = new Date().toISOString().slice(0, 10);
  if (mockAi.usage.date !== today) mockAi.usage = { date: today, count: 0 };
  return { date: mockAi.usage.date, count: mockAi.usage.count };
}
function mockAiDefault() {
  return mockAi.configs.find((c) => c.isDefault) ?? mockAi.configs[0] ?? null;
}
function mockAiEmitChunk(requestId: string, delta: string) {
  mockEmit(event.AI, { request_id: requestId, delta });
}

function mockAiEcho(task: AiTask, payload: Record<string, unknown>): string {
  const mirror = JSON.stringify(payload);
  const reversed = [...mirror].reverse().join("");
  if (task === "explain_logs") {
    const lines = Array.isArray(payload.lines) ? payload.lines.length : 0;
    return `【Mock AI · explain_logs】已收到 ${lines} 行日志（确定性回文镜像）：\n\n${reversed.slice(0, 120)}`;
  }
  if (task === "config_suggest") {
    return `【Mock AI · config_suggest】建议保持现状即可；以下为参考稿（确定性回文镜像）：\n\n\`\`\`yaml\n${reversed.slice(0, 80)}\n\`\`\``;
  }
  if (task === "test_connection") {
    return "OK";
  }
  return `【Mock AI · enrich_draft】草稿增强（确定性回文镜像）：\n\n${reversed.slice(0, 120)}`;
}
const state = {
  opened: false,
  spec: demoSpec(),
  services: {} as Record<string, ServiceRT>,
  script: null as ScriptRuntimeView | null,
  logSeq: 0,
  logs: [] as LogLine[],
  /** git.pull 成功后 ahead/behind 归零（确定性状态机） */
  gitPulled: false,
  /** 发现列表：killProcess 后移除对应进程（确定性状态机） */
  discover: null as ForeignService[] | null,
  /** 1.2：工具链探测状态（install/upgrade 成功后原地更新） */
  probe: null as ToolchainProbeOut | null,
  /** 1.6：网关 demo（conf 来自 yaml；state 为托管状态机）。未配置 = null */
  gateway: (demoSpec().gateway?.kind
    ? { conf: demoSpec().gateway as GatewayConf, state: "stopped" as RtState, pid: null as number | null, startedAt: null as number | null }
    : null) as { conf: GatewayConf; state: RtState; pid: number | null; startedAt: number | null } | null,
};

function defaultDiscover(): ForeignService[] {
  return [
    { pid: 41001, name: "java.exe", kind: "java", ports: [8080, 8081], cwd: "C:\\demo\\modules\\api", cmd_line: "java -jar target/api.jar", cpu_percent: 3.2, memory_bytes: 486539264 },
    { pid: 41240, name: "node.exe", kind: "node", ports: [5173], cwd: "C:\\demo\\web", cmd_line: "node C:\\demo\\web\\node_modules\\vite\\bin\\vite.js", cpu_percent: 0.6, memory_bytes: 113246208 },
    { pid: 41355, name: "python.exe", kind: "python", ports: [8000], cwd: null, cmd_line: null, cpu_percent: 1.1, memory_bytes: 41943040 },
    { pid: 41500, name: "esbuild.exe", kind: "other", ports: [9229], cwd: "C:\\demo\\web", cmd_line: "esbuild --serve=9229", cpu_percent: null, memory_bytes: null },
  ];
}

function seedRuntime() {
  if (Object.keys(state.services).length) return;
  for (const [id, s] of Object.entries(state.spec.services)) {
    const compose = s.kind === "compose";
    // compose sidecar 覆盖两种容器态：redis 运行中、mall-db 已退出（外部退出语义）
    const running = id === "gateway" || id === "auth-service" || id === "redis";
    const exited = id === "mall-db";
    state.services[id] = {
      id,
      state: exited ? "exited" : running ? "running" : "stopped",
      // compose 服务无宿主进程：pid 恒为 null，UI 显示「容器托管」
      pid: running && !compose ? 1000 + Object.keys(state.services).length : null,
      port: s.port ?? null,
      kind: s.kind,
      health: running ? { ok: true, at_ms: Date.now(), detail: "200 OK" } : null,
      started_at_ms: running ? Date.now() - 120000 : null,
      last_exit: exited ? { code: 0, at_ms: Date.now() - 60000 } : null,
      last_error: null,
      log_seq: 0,
    };
  }
  // compose 服务日志样例：stdout 容器输出 + system 来源的 docker CLI 行（§5.4）
  if (state.services["redis"]?.state === "running") {
    pushLog({ kind: "service", id: "redis" }, "system", "[docker] docker compose -f compose.yaml logs --follow redis");
    pushLog({ kind: "service", id: "redis" }, "stdout", "1:M 28 Aug 2026 10:00:00.031 * Redis version=7.2.5, bits=64, commit=00000000, modified=0");
    pushLog({ kind: "service", id: "redis" }, "stdout", "1:M 28 Aug 2026 10:00:00.033 * Ready to accept connections tcp");
  }
}

let pidCounter = 5000;

function pushLog(source: LogSource, stream: LogLine["stream"], text: string) {
  state.logSeq += 1;
  state.logs.push({ seq: state.logSeq, source, stream, ts_ms: Date.now(), text });
  if (state.logs.length > 5000) state.logs.splice(0, state.logs.length - 5000);
}

seedRuntime();

function snapshot(): RuntimeSnapshot {
  const services: Record<string, ServiceRuntimeView> = {};
  for (const [id, s] of Object.entries(state.services)) {
    services[id] = { ...s };
  }
  return {
    protocol: PROTOCOL,
    workspace_id: state.spec.root,
    services,
    script: state.script ? { ...state.script } : null,
    gateway: state.gateway
      ? {
          kind: state.gateway.conf.kind ?? "nginx",
          state: state.gateway.state,
          pid: state.gateway.pid,
          port: state.gateway.conf.port,
          health: state.gateway.state === "running" ? { ok: true, at_ms: Date.now(), detail: `tcp ${state.gateway.conf.port}` } : null,
          started_at_ms: state.gateway.startedAt,
          last_exit: null,
          last_error: null,
          exit_reason: null,
        }
      : null,
  };
}

/** 1.6：网关状态变化 → 广播 st.runtime（对齐引擎 emit_runtime 语义）。 */
function emitGatewayRuntime() {
  mockEmit("st-runtime", { reason: "full", services: snapshot().services, script: null, gateway: snapshot().gateway });
}

function toYaml(spec: SuperTaskFile): string {
  const lines: string[] = [];
  lines.push("version: 1");
  lines.push(`kind: ${spec.kind ?? "supertask.workspace"}`);
  lines.push(`name: ${spec.name ?? "workspace"}`);
  if (spec.description) lines.push(`description: ${spec.description}`);
  lines.push(`root: ${spec.root}`);
  const envKeys = Object.keys(spec.env);
  if (envKeys.length) {
    lines.push("env:");
    for (const k of envKeys) lines.push(`  ${k}: ${spec.env[k]}`);
  }
  lines.push("services:");
  for (const [id, s] of Object.entries(spec.services)) {
    lines.push(`  ${id}:`);
    lines.push(`    kind: ${s.kind}`);
    lines.push(`    enabled: ${s.enabled}`);
    if (s.group) lines.push(`    group: ${s.group}`);
    lines.push(`    port: ${s.port ?? "null"}`);
    lines.push("    env:");
    for (const [k, v] of Object.entries(s.env)) lines.push(`      ${k}: ${v}`);
    if (s.depends_on.length) lines.push(`    depends_on: [${s.depends_on.join(", ")}]`);
    if (s.health) lines.push(`    health:\n      type: ${s.health.type}`);
    if (s.kind === "node") {
      lines.push(`    package_manager: ${s.package_manager ?? "npm"}`);
      lines.push(`    script: ${s.script ?? "null"}`);
    } else if (s.module) {
      lines.push(`    module: ${s.module}`);
    }
  }
  lines.push("scripts:");
  for (const [id, sc] of Object.entries(spec.scripts)) {
    lines.push(`  ${id}:`);
    lines.push(`    cmds: [${sc.cmds.map((c) => `"${c}"`).join(", ")}]`);
  }
  return lines.join("\n") + "\n";
}

function hashOf(s: string): string {
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) >>> 0;
  return h.toString(16);
}

const emptyProbe = { found: false, version: null, path: null };

/** mock secrets：浏览器 dev 用内存 map，模拟 .env.local（绝不出现在日志）。 */
const mockSecrets = new Map<string, string>();

// ---------------------------------------------------------------------------
// Mock 事件桥：浏览器模式下 provider 通过 mockListen 订阅、mock 命令经 mockEmit
// 推送，信封形状与 Tauri 事件 payload 一致（{ protocol, event, workspace_id, ts_ms, payload }）。
// ---------------------------------------------------------------------------

type MockEventEnvelope = {
  protocol: number;
  event: string;
  workspace_id: string | null;
  ts_ms: number;
  payload: unknown;
};

const mockEventListeners = new Map<string, Set<(envelope: MockEventEnvelope) => void>>();

export function mockListen(eventName: string, cb: (envelope: MockEventEnvelope) => void): () => void {
  let set = mockEventListeners.get(eventName);
  if (!set) {
    set = new Set();
    mockEventListeners.set(eventName, set);
  }
  set.add(cb);
  return () => {
    set?.delete(cb);
  };
}

function mockEmit(eventName: string, payload: unknown, workspaceId: string | null = null) {
  const envelope: MockEventEnvelope = {
    protocol: PROTOCOL,
    event: eventName,
    workspace_id: workspaceId,
    ts_ms: Date.now(),
    payload,
  };
  for (const cb of mockEventListeners.get(eventName) ?? []) cb(envelope);
}

// ---------------------------------------------------------------------------
// Mock 长操作（st.operation）：queued → running(progress) → succeeded/failed
// ---------------------------------------------------------------------------

let opSeq = 0;

function emitOperation(
  kind: string,
  operationId: string,
  opState: OpState,
  progress: number | null,
  message: string | null,
  errorCode: string | null,
  result: unknown,
) {
  mockEmit(
    "st-operation",
    { operation_id: operationId, kind, state: opState, progress, message, error_code: errorCode, result },
  );
}

// ---------------------------------------------------------------------------
// Mock 偏好（app.load / app.savePrefs）：localStorage 持久化
// ---------------------------------------------------------------------------

const MOCK_PREFS_KEY = "st:mockPrefs";
const DEFAULT_PREFS: Prefs = {
  theme: "light",
  restoreLast: true,
  closeToTray: true,
  startOnLogin: false,
  updateCheck: true,
  locale: "auto",
};

function readMockPrefs(): Prefs {
  try {
    const raw = localStorage.getItem(MOCK_PREFS_KEY);
    if (raw) return { ...DEFAULT_PREFS, ...(JSON.parse(raw) as Partial<Prefs>) };
  } catch {
    /* ignore */
  }
  return { ...DEFAULT_PREFS };
}

function writeMockPrefs(prefs: Prefs) {
  try {
    localStorage.setItem(MOCK_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    /* ignore */
  }
}

// ---------------------------------------------------------------------------
// Mock 模板（与 crates/supertask-core/src/template.rs 的清单概览一致）
// ---------------------------------------------------------------------------

const MOCK_TEMPLATES: TemplateSummary[] = [
  {
    id: "spring-multimodule-node",
    version: "1",
    name: "Spring 多模块 + Node（完整示例）",
    description: "Spring Boot 多模块后端 + 零依赖 Node 前端，含健康检查与依赖关系",
    stacks: ["spring-boot", "node"],
    files: [
      "backend/pom.xml",
      "backend/src/main/java/com/supertask/demo/DemoApplication.java",
      "backend/src/main/resources/application.properties",
      "pom.xml",
      "supertask.yaml",
      "web/package.json",
      "web/server.js",
    ],
    source: "builtin",
    invalid: false,
    invalid_reason: null,
  },
  {
    id: "spring-multimodule-node-minimal",
    version: "1",
    name: "Spring 多模块 + Node（最小起步）",
    description: "一个可运行的 Spring 模块 + 一个 Node 服务，YAML 精简，健康检查由引擎兜底",
    stacks: ["spring-boot", "node"],
    files: [
      "backend/pom.xml",
      "backend/src/main/java/com/supertask/demo/DemoApplication.java",
      "pom.xml",
      "supertask.yaml",
      "web/package.json",
      "web/server.js",
    ],
    source: "builtin",
    invalid: false,
    invalid_reason: null,
  },
  // 本地模板（%APPDATA%/SuperTask/templates/）：一好一坏，覆盖 invalid 展示
  {
    id: "my-node-api",
    version: "2",
    name: "我的 Node API",
    description: "团队自用的 Express API 起步模板（本地示例）",
    stacks: ["node"],
    files: ["supertask.yaml", "package.json", "src/index.js"],
    source: "local",
    invalid: false,
    invalid_reason: null,
  },
  {
    id: "broken-tpl",
    version: "",
    name: "broken-tpl",
    description: "template.yaml 缺少 name 字段",
    stacks: [],
    files: [],
    source: "local",
    invalid: true,
    invalid_reason: "template.yaml: 缺少 name 字段",
  },
  {
    id: "spring-boot-single",
    version: "1",
    name: "Spring Boot 单模块",
    description: "最简单的 Spring Boot Web 应用，module 为 \".\"，直接 mvn spring-boot:run 启动",
    stacks: ["spring-boot"],
    files: [
      "pom.xml",
      "src/main/java/com/supertask/demo/DemoApplication.java",
      "src/main/resources/application.properties",
      "supertask.yaml",
    ],
    source: "builtin",
    invalid: false,
    invalid_reason: null,
    params: [{ key: "project_name", label: "项目名", required: false }],
  },
  {
    id: "node-fullstack",
    version: "1",
    name: "Node 双服务（API + Web）",
    description: "零依赖 Node 前后端：API 服务 + Web 服务，含 depends_on 依赖关系",
    stacks: ["node"],
    files: ["api/package.json", "api/server.js", "supertask.yaml", "web/package.json", "web/server.js"],
    source: "builtin",
    invalid: false,
    invalid_reason: null,
  },
  {
    id: "spring-node-combo",
    version: "1",
    name: "Spring + Node（自由组合）",
    description: "按需组合 Spring Boot 后端与 Node 前端，向导中勾选服务块并分配端口",
    stacks: ["spring-boot", "node"],
    files: [
      "pom.xml",
      "backend/pom.xml",
      "backend/src/main/java/com/supertask/demo/DemoApplication.java",
      "backend/src/main/resources/application.properties",
      "web/package.json",
      "web/server.js",
    ],
    source: "builtin",
    invalid: false,
    invalid_reason: null,
    blocks: [
      { id: "backend", label: "Spring Boot 后端", kind: "spring-boot", requires: [], default_port: 8081, services: ["backend"] },
      { id: "web", label: "Node 前端", kind: "node", requires: ["backend"], default_port: 5173, services: ["web"] },
    ],
  },
];

/** 组合模板 mock 的 services 片段（{{port}} 占位在端口分配时替换）。 */
const MOCK_BLOCK_SERVICES: Record<string, Record<string, unknown>> = {
  backend: {
    kind: "spring-boot",
    module: "backend",
    health: { type: "http", http: "http://127.0.0.1:{{port}}/actuator/health" },
  },
  web: { kind: "node", dir: "web", depends_on: ["backend"], health: { type: "tcp" } },
};

/** mock 端的块组合计划：依赖闭合 + 端口查重，语义对齐 core 的 plan_blocks。 */
function mockPlanBlocks(
  tpl: TemplateSummary,
  blockIds?: string[],
  ports?: Record<string, number>,
): { chosen: string[]; services: Record<string, Record<string, unknown>>; files: string[] } | null {
  const blocks = tpl.blocks ?? [];
  if (blocks.length === 0) return null;
  const chosen = [...(blockIds ?? blocks.map((b) => b.id))];
  for (const id of chosen) {
    if (!blocks.some((b) => b.id === id)) {
      throw { protocol: PROTOCOL, code: "TEMPLATE_BLOCK_DEP", message: `块 ${id} 在模板中不存在`, retryable: false };
    }
  }
  for (let i = 0; i < chosen.length; i++) {
    const b = blocks.find((x) => x.id === chosen[i]);
    for (const r of b?.requires ?? []) {
      if (!chosen.includes(r)) chosen.push(r);
    }
  }
  const services: Record<string, Record<string, unknown>> = {};
  const used = new Map<number, string>();
  const files: string[] = [];
  for (const b of blocks) {
    if (!chosen.includes(b.id)) continue;
    for (const svcId of b.services) {
      const port = ports?.[svcId] ?? b.default_port;
      if (port == null) {
        throw { protocol: PROTOCOL, code: "TEMPLATE_BLOCK_PORT", message: `服务 ${svcId} 未分配端口`, retryable: false };
      }
      if (used.has(port)) {
        throw { protocol: PROTOCOL, code: "TEMPLATE_BLOCK_PORT", message: `端口 ${port} 同时分配给 ${used.get(port)} 与 ${svcId}`, retryable: false };
      }
      used.set(port, svcId);
      const fragment = JSON.stringify(MOCK_BLOCK_SERVICES[svcId] ?? { kind: b.kind });
      services[svcId] = JSON.parse(fragment.split("{{port}}").join(String(port)));
    }
    for (const f of tpl.files) {
      const belongs = (b.id === "backend" && (f.startsWith("backend/") || f === "pom.xml")) || (b.id === "web" && f.startsWith("web/"));
      if (belongs && !files.includes(f)) files.push(f);
    }
  }
  return { chosen, services, files };
}

/** 单层目录名校验，语义对齐 core 的 validate_directory_name。 */
function invalidDirectoryName(name: string): string | null {
  if (!name) return "不能为空";
  if (name === "." || name === "..") return "不允许 . 或 ..";
  if (name.startsWith("\\\\") || name.startsWith("//")) return "不允许 UNC 路径";
  if (name.includes("/") || name.includes("\\")) return "不能包含路径分隔符";
  if (name.includes(":")) return "不能包含盘符分隔符 ':'";
  return null;
}

function mockGitStatus(): GitStatus {
  const root = state.spec.root;
  // 确定性 dirty 态：工作区路径含 "dirty"（不区分大小写）即视为有未提交修改，
  // 方便浏览器里验证 pull 的 GIT_DIRTY 分支；pull 成功后 ahead/behind 归零。
  const dirty = root.toLowerCase().includes("dirty");
  const pulled = state.gitPulled;
  return {
    workspace_id: root,
    is_repository: true,
    branch: "main",
    detached: false,
    dirty,
    ahead: dirty && !pulled ? 1 : 0,
    behind: dirty && !pulled ? 2 : 0,
    staged: dirty ? 2 : 0,
    unstaged: dirty ? 1 : 0,
    untracked: dirty ? 3 : 0,
    remote: "origin",
  };
}

function noWorkspaceError() {
  return { protocol: PROTOCOL, code: "NO_WORKSPACE", message: "没有打开的工作区", retryable: false };
}

function mockScanPreview(): ScanPreviewOut {
  const spec = state.spec;
  const items: ScanMergeItem[] = [];
  // match_same：gateway（发现结果与当前一致）
  items.push({
    service_id: "gateway",
    status: "match_same",
    discovered: spec.services["gateway"] ?? null,
    current: spec.services["gateway"] ?? null,
    field_diffs: [],
    candidate_id: null,
    selected: true,
  });
  // match_diff：auth-service（扫描器发现端口变化，用户字段保留）
  if (spec.services["auth-service"]) {
    const discovered = { ...spec.services["auth-service"], port: 8083, ports: [8083] };
    items.push({
      service_id: "auth-service",
      status: "match_diff",
      discovered,
      current: spec.services["auth-service"],
      field_diffs: ["port", "ports"],
      candidate_id: null,
      selected: true,
    });
  }
  // missing：docs-site（yaml 里有、本次未发现；不删除只警告）
  if (spec.services["docs-site"]) {
    items.push({
      service_id: "docs-site",
      status: "missing",
      discovered: null,
      current: spec.services["docs-site"],
      field_diffs: [],
      candidate_id: null,
      selected: true,
    });
  }
  // added：新发现 report-service
  items.push({
    service_id: "report-service",
    status: "added",
    discovered: {
      kind: "spring-boot",
      enabled: true,
      group: "backend",
      labels: {},
      port: 8090,
      ports: [8090],
      env: {},
      env_file: [],
      depends_on: [],
      grace_secs: 30,
      health: { type: "http", http: "http://localhost:8090/actuator/health", interval_secs: 5, timeout_secs: 3 },
      restart: null,
      extra_args: [],
      cwd: null,
      launch: null,
      module: "knife4j-report",
      jvm_args: [],
      dir: null,
      package_manager: null,
      script: null,
      logging: null,
    },
    current: null,
    field_diffs: [],
    candidate_id: null,
    selected: false,
  });
  return {
    items,
    warnings: ["docs-site 未在本次扫描中发现，保留原配置不删除。"],
  };
}

/**
 * 2.1 README 导入 mock（ipc.md §10.13）：README-only 新增服务（带 provenance/置信度）
 * + 脚本合并项 + 端口提示；未打开工作区时 warnings 给人话提示。
 */
function mockReadmePreview(): ReadmePreviewOut {
  const spec = state.spec;
  const base = mockScanPreview();
  // README-only 新增：python 服务（uvicorn）
  const readmeAdded: ScanMergeItem = {
    service_id: "uvicorn-api",
    status: "added",
    discovered: {
      kind: "python",
      enabled: true,
      labels: {},
      port: null,
      ports: [],
      env: {},
      env_file: [],
      depends_on: [],
      grace_secs: 15,
      health: null,
      extra_args: ["app:app", "--reload"],
      jvm_args: [],
      dir: ".",
      module: "uvicorn",
      entry: null,
      script: null,
    } as ServiceSpec,
    current: null,
    field_diffs: [],
    candidate_id: null,
    selected: false,
    fields_meta: [
      { field: "kind", source: "readme", confidence: "high" },
      { field: "dir", source: "readme", confidence: "high" },
      { field: "module", source: "readme", confidence: "high" },
      { field: "extra_args", source: "readme", confidence: "high" },
    ],
  };
  // README 命中已有 node 服务：字段冲突 → scan 值保留，README 值进建议列
  const hitItems: ScanMergeItem[] = base.items.map((it) =>
    it.status === "match_same" && it.discovered?.script
      ? {
          ...it,
          fields_meta: [
            { field: "dir", source: "scan" },
            {
              field: "script",
              source: "scan",
              readme_value: String(it.discovered.script) === "dev" ? "start" : "dev",
            },
          ],
        }
      : it,
  );
  return {
    items: [...hitItems, readmeAdded],
    script_items: [
      {
        script_id: "install",
        status: spec.scripts["install"] ? "match_diff" : "added",
        discovered: {
          cmds: ["npm install"],
          cwd: null,
          env: {},
          timeout_secs: 1800,
          depends_on: [],
        },
        current: spec.scripts["install"] ?? null,
        selected: !spec.scripts["install"],
        fields_meta: [{ field: "cmds", source: "readme", confidence: "high" }],
      },
    ],
    warnings: [
      "README 提示端口 8000（uvicorn app:app）；请确认后手填到服务 port",
      "1 条命令未识别，已忽略",
    ],
    readme_path: "README.md",
  };
}

/**
 * 1.4 Taskfile 导入 mock（ipc.md §10.8）：含插值 / internal / deps / id 冲突样例，
 * 与 demoSpec.scripts.build 冲突验证 id_conflict 默认 keep 分支。
 */
function mockTaskfilePreview(): TaskfilePreviewOut {
  const items: TaskfileImportItem[] = [
    {
      task: "bootstrap",
      script_id: "bootstrap",
      cmds_count: 2,
      selected: true,
      warnings: [],
      internal: false,
      id_conflict: false,
    },
    {
      task: "build",
      script_id: "build",
      cmds_count: 1,
      selected: false,
      warnings: ["目标已存在同名脚本 id，默认保留现有脚本；勾选将覆盖"],
      internal: false,
      id_conflict: true,
    },
    {
      task: "deploy-web",
      script_id: "deploy-web",
      cmds_count: 1,
      selected: false,
      warnings: ["包含插值变量 TARGET, API_KEY，未解析；勾选后按原文导入"],
      internal: false,
      id_conflict: false,
    },
    {
      task: "helper",
      script_id: "helper",
      cmds_count: 1,
      selected: false,
      warnings: ["internal 任务不导入"],
      internal: true,
      id_conflict: false,
    },
    {
      task: "lint-all",
      script_id: "lint-all",
      cmds_count: 1,
      selected: true,
      warnings: ["deps 忽略（scripts.depends_on 预留）", "platforms 约束忽略，导入后的脚本无平台限制"],
      internal: false,
      id_conflict: false,
    },
  ];
  return {
    tasks: items,
    warnings: ["includes 不支持且未跟随：1 个子 Taskfile 已跳过，需要的任务请手工补录"],
  };
}

/** Browser / `vite` without WebView: same shapes as Tauri, no real spawn. */
// ---------------------------------------------------------------------------
// Mock 终端（ipc.md §10.15）：确定性假 shell（浏览器 dev 演示，不拉真实进程）。
// 事件序列与 Tauri 真链路一致（st.term 信封 + kind output/exited）。
// ---------------------------------------------------------------------------

type MockTermSession = { id: number; cwd: string; line: string };

const mockTerms = new Map<number, MockTermSession>();
let mockTermSeq = 0;

function mockTermEmit(
  sessionId: number,
  kind: "output" | "exited",
  data?: string,
  exitCode?: number,
) {
  mockEmit("st-term", {
    session_id: sessionId,
    kind,
    ...(data !== undefined ? { data } : {}),
    ...(exitCode !== undefined ? { exit_code: exitCode } : {}),
  });
}

function mockTermPrompt(s: MockTermSession): string {
  return `\r\n\x1b[36m[mock] ${s.cwd}\x1b[0m \x1b[32m$\x1b[0m `;
}

function mockTermExec(s: MockTermSession, raw: string): { out: string; exit: boolean } {
  const argv = raw.trim().split(/\s+/).filter(Boolean);
  const name = argv[0] ?? "";
  switch (name) {
    case "":
      return { out: "", exit: false };
    case "help":
      return { out: "可用命令：help · echo · pwd · ls/dir · ver · date · clear · exit", exit: false };
    case "echo":
      return { out: argv.slice(1).join(" "), exit: false };
    case "pwd":
      return { out: s.cwd, exit: false };
    case "ls":
    case "dir":
      return { out: "src/  package.json  supertask.yaml", exit: false };
    case "ver":
      return { out: "SuperTask mock shell（浏览器演示，不拉起真实进程）", exit: false };
    case "date":
      return { out: new Date().toLocaleString(), exit: false };
    case "clear":
      return { out: "\x1b[2J\x1b[3J\x1b[H", exit: false };
    case "exit":
      return { out: "", exit: true };
    default:
      return {
        out: `\x1b[31m${name}: command not found\x1b[0m（mock 假 shell，输入 help 查看可用命令）`,
        exit: false,
      };
  }
}

function mockTermHandleInput(s: MockTermSession, data: string) {
  // 假 shell 只按行驱动：退格/回车/Ctrl+C，ESC 控制序列整体忽略（无历史/光标编辑）
  const chars = Array.from(data);
  let i = 0;
  while (i < chars.length) {
    const ch = chars[i];
    if (ch === "\x1b") {
      i += 3;
      continue;
    }
    i += 1;
    if (ch === "\r") {
      const raw = s.line;
      s.line = "";
      mockTermEmit(s.id, "output", "\r\n");
      const { out, exit } = mockTermExec(s, raw);
      if (exit) {
        mockTerms.delete(s.id);
        mockTermEmit(s.id, "exited", undefined, 0);
        return;
      }
      if (out) mockTermEmit(s.id, "output", `${out}\r\n`);
      mockTermEmit(s.id, "output", mockTermPrompt(s));
      continue;
    }
    if (ch === "\u007f") {
      s.line = s.line.slice(0, -1);
      mockTermEmit(s.id, "output", "\b \b");
      continue;
    }
    if (ch === "\x03") {
      s.line = "";
      mockTermEmit(s.id, "output", `^C${mockTermPrompt(s)}`);
      continue;
    }
    if (ch < " " || ch === "\x7f") continue;
    s.line += ch;
    mockTermEmit(s.id, "output", ch);
  }
}

export async function mockInvoke(command: string, args?: Record<string, unknown>): Promise<unknown> {
  if (command === "session.hello") {
    const protocol = Number(args?.protocol ?? PROTOCOL);
    if (protocol !== PROTOCOL) {
      throw { protocol: PROTOCOL, code: "PROTOCOL", message: "protocol 不匹配", retryable: false };
    }
    const hello: HelloOut = {
      protocol: PROTOCOL,
      engine: "supertask-core",
      engine_version: "0.1.0",
      product_version: "1.0.0-dev",
      os: "web-mock",
      features: [
        { id: "run", path: "/run", status: "live", since: "1.0" },
        { id: "logs", path: "/logs", status: "live", since: "1.0" },
        { id: "config", path: "/config", status: "live", since: "1.0" },
        { id: "templates", path: "/templates", status: "live", since: "1.1" },
        { id: "env", path: "/env", status: "live", since: "1.0" },
        { id: "workspaces", path: "/workspaces", status: "live", since: "1.1" },
        { id: "discover", path: "/discover", status: "live", since: "1.1" },
        { id: "git", path: "/git", status: "live", since: "1.1" },
        { id: "docker", path: "/docker", status: "live", since: "1.3" },
        { id: "gateway", path: "/gateway", status: "live", since: "1.6" },
        { id: "cloud", path: "/cloud", status: "live", since: "2.0" },
        { id: "ai", path: "/ai", status: "live", since: "2.1" },
        { id: "settings", path: "/settings", status: "live", since: "1.0" },
      ],
    };
    return hello;
  }

  if (command === "app.load") {
    const load: AppLoadOut = {
      protocol: PROTOCOL,
      prefs: readMockPrefs(),
      recents: [DEMO_ROOT],
      probe: {
        java: { found: true, version: "17.0.10", path: "/usr/lib/jvm/java-17" },
        maven: { found: true, version: "3.9.6", path: "/opt/maven" },
        gradle: { found: false, version: null, path: null },
        node: { found: true, version: "22.4.0", path: "/usr/local/bin/node" },
        npm: { found: true, version: "10.7.0", path: "/usr/local/bin/npm" },
        pnpm: emptyProbe,
        yarn: emptyProbe,
        bun: emptyProbe,
        gateway: {
          nginx: { found: true, version: "1.26.1", path: "C:/mock/nginx/nginx.exe" },
          caddy: { found: true, version: "2.8.4", path: "C:/mock/caddy/caddy.exe" },
          apache: emptyProbe,
        },
      },
      stale: [],
    };
    return load;
  }

  if (command === "workspace.add" || command === "workspace.scanDraft") {
    const root = (args?.path as string) || DEMO_ROOT;
    state.spec.root = root;
    const out: WorkspaceOpenOut = { workspace_id: root, spec: state.spec, warnings: [] };
    return out;
  }

  if (command === "workspace.open") {
    if (state.opened) {
      throw {
        protocol: PROTOCOL,
        code: "ALREADY_IN_PROGRESS",
        message: "已打开工作区，请先 close",
        retryable: false,
      };
    }
    state.opened = true;
    const root = (args?.path as string) || DEMO_ROOT;
    state.spec.root = root;
    const out: WorkspaceOpenOut = { workspace_id: root, spec: state.spec, warnings: [] };
    return out;
  }

  if (command === "workspace.init") {
    if (state.opened) {
      throw {
        protocol: PROTOCOL,
        code: "ALREADY_IN_PROGRESS",
        message: "已打开工作区，请先 close",
        retryable: false,
      };
    }
    const spec = args?.spec as SuperTaskFile;
    state.spec = spec;
    state.services = {};
    seedRuntime();
    state.opened = true;
    const out: WorkspaceOpenOut = { workspace_id: spec.root, spec: state.spec, warnings: [] };
    return out;
  }

  if (command === "workspace.close" || command === "workspace.forget") {
    state.opened = false;
    state.services = {};
    return { ok: true };
  }

  // mock 语义：detach 不杀服务，保留 runtime 状态；重开同 root 工作区时看起来仍是 running
  if (command === "workspace.detach") {
    return { ok: true };
  }

  if (command === "system.discover") {
    state.discover ??= defaultDiscover();
    return state.discover;
  }

  if (command === "system.killProcess") {
    const pid = args?.pid as number;
    const list = (state.discover ??= defaultDiscover());
    if (!list.some((s) => s.pid === pid)) {
      throw {
        protocol: PROTOCOL,
        code: "NOT_FOUND",
        message: `pid ${pid} 不在发现列表中`,
        retryable: false,
      };
    }
    state.discover = list.filter((s) => s.pid !== pid);
    return { ok: true };
  }

  if (command === "workspace.openExplorer") return { ok: true };

  // -------------------------------------------------------------------------
  // 1.2：工具链（ipc 增量 §13.1）。probe 状态化：install 成功后原地更新，
  // 让浏览器 mock 下的 /env 安装流程与真机行为一致。
  // -------------------------------------------------------------------------

  const MOCK_MANAGERS: ManagerAvailability = { mise: false, winget: true };
  const MOCK_DEFAULT_VERSIONS: Record<string, string> = {
    java: "21",
    maven: "3.9",
    node: "20",
    npm: "20",
    pnpm: "9",
    yarn: "1",
    bun: "1",
    python: "3.12",
    go: "1.23",
  };

  function ensureProbe(): ToolchainProbeOut {
    state.probe ??= {
      java: { found: true, version: "17.0.10", path: "/usr/lib/jvm/java-17/bin/java" },
      maven: { found: true, version: "3.9.6", path: "/opt/maven" },
      gradle: { found: false, version: null, path: null },
      node: { found: false, version: null, path: null },
      npm: { found: true, version: "10.7.0", path: "/usr/local/bin/npm" },
      pnpm: emptyProbe,
      yarn: emptyProbe,
      bun: emptyProbe,
      // 1.7：python / go 默认缺失，与 node 同样用于演示安装流程（安装成功后原地翻为 found）
      python: { found: false, version: null, path: null },
      go: { found: false, version: null, path: null },
      managers: MOCK_MANAGERS,
      gateway: {
        nginx: { found: true, version: "1.26.1", path: "C:/mock/nginx/nginx.exe" },
        caddy: { found: true, version: "2.8.4", path: "C:/mock/caddy/caddy.exe" },
        apache: emptyProbe,
      },
      // P1：本机已装枚举样例（java 走注册表/目录多版本，node 走 nvm 目录）
      installs: [
        { tool: "java", version: "17.0.10", home: "/usr/lib/jvm/java-17", source: "directory", active: true },
        { tool: "java", version: "11.0.20", home: "C:/Program Files/Java/jdk-11", source: "registry", active: false },
        { tool: "java", version: "21.0.3", home: "C:/Program Files/Java/jdk-21", source: "env_var", active: false },
        { tool: "node", version: "20.18.1", home: "C:/Users/demo/AppData/Roaming/nvm/v20.18.1", source: "nvm_dir", active: false },
        { tool: "node", version: "22.17.1", home: "C:/Users/demo/AppData/Roaming/nvm/v22.17.1", source: "nvm_dir", active: false },
      ],
    };
    return state.probe;
  }

  if (command === "toolchain.probe") {
    return { ...ensureProbe() };
  }

  // S1：可选版本列表（真机 = 白名单 ∪ mise ls-remote；mock 给静态近似值）
  if (command === "toolchain.versions") {
    return {
      tools: {
        java: ["21", "17", "11", "lts", "21.0.2", "22.0.0"],
        maven: ["3.9", "3", "lts"],
        node: ["20", "22", "18", "lts", "20.18.0"],
        npm: ["20", "lts"],
        pnpm: ["9", "10", "lts"],
        yarn: ["1", "lts"],
        bun: ["1", "lts"],
        python: ["3.12", "3.13", "3.11", "lts", "3.12.4"],
        go: ["1.23", "1.22", "lts", "1.23.1"],
      },
    };
  }

  if (command === "toolchain.install" || command === "toolchain.upgrade") {
    const tool = ((args?.tool as string) ?? "").trim();
    if (!(tool in MOCK_DEFAULT_VERSIONS)) {
      throw { protocol: PROTOCOL, code: "SPEC_INVALID", message: `tool 仅接受 java|maven|node|npm|pnpm|yarn|bun|python|go，收到 ${tool}`, retryable: false };
    }
    const version = ((args?.version as string) ?? MOCK_DEFAULT_VERSIONS[tool]).trim();
    const isLts = version.toLowerCase() === "lts";
    // 版本字符集与后端一致（lts 别名合法，禁前导 -）
    if (!isLts && (version.startsWith("-") || /[^0-9A-Za-z._\-+@]/.test(version))) {
      throw { protocol: PROTOCOL, code: "TOOLCHAIN_VERSION_INVALID", message: `非法版本 ${version}：只允许数字、点号、连字符与 lts 别名`, retryable: false };
    }
    const persist = args?.persist === true;
    const baseHash = args?.baseHash as string | null | undefined;
    if (persist && !baseHash) {
      throw { protocol: PROTOCOL, code: "SPEC_INVALID", message: "persist=true 必须携带 base_hash", retryable: false };
    }
    const probe = ensureProbe();
    const probeSlot = (probe: ToolchainProbeOut, t: string) =>
      probe[t as keyof Omit<ToolchainProbe, "gateway">] as ToolProbe;
    if (command === "toolchain.upgrade" && !probeSlot(probe, tool).found) {
      throw { protocol: PROTOCOL, code: "MISSING_TOOL", message: `未安装 ${tool}，请先安装再升级`, retryable: false };
    }
    const opId = `op-${++opSeq}`;
    const kind = command;
    emitOperation(kind, opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation(kind, opId, "running", 0.35, `正在通过 winget 下载 ${tool} ${version}…`, null, null), 500);
    setTimeout(() => emitOperation(kind, opId, "running", 0.8, "正在刷新 PATH 并解析工具…", null, null), 1100);
    setTimeout(
      () => {
        const path = `C:\\mock\\tools\\${tool}\\${version}\\bin`;
        const slot = probeSlot(probe, tool);
        slot.found = true;
        slot.version = version;
        slot.path = path;
        emitOperation(kind, opId, "succeeded", 1, "完成", null, {
          tool,
          version,
          manager: "winget",
          path,
          ...(persist ? { hash: hashOf(`${baseHash}+${tool}@${version}`) } : {}),
        });
      },
      1600,
    );
    return { operation_id: opId };
  }

  // -------------------------------------------------------------------------
  // 1.2 phase 3–7：端口 / secrets / 日志 / 指标 / profile / build
  // -------------------------------------------------------------------------

  if (command === "ports.inspect") {
    // 与真机语义一致：服务自身运行占用已排除（运行中 → 该端口算可用）；
    // 外部占用用固定演示集：4100 始终被一个外部进程占用
    const EXTERNAL_OCCUPIED = new Set<number>([4100]);
    const runningPorts: Set<number> =
      Object.values(state.services)
        .filter((s) => s.state === "running" && s.port != null)
        .map((s) => s.port!)
        .reduce((set, p) => set.add(p), new Set<number>());
    // 传了 port → 只查该服务/候选端口（用输入框填的号）；否则逐个已配置端口
    const ids = args?.port != null ? [args.id as string] : Object.keys(state.services);
    const items = ids.map((sid) => {
      const s = state.services[sid];
      const port = args?.port != null ? (args.port as number) : (s?.port ?? 0);
      // 自身运行占用不计：运行中服务的当前端口视为本服务持有
      const selfOwned = port !== 0 && runningPorts.has(port);
      const external = port !== 0 && EXTERNAL_OCCUPIED.has(port) && !selfOwned;
      return {
        id: sid,
        port,
        in_use: external,
        pid: external ? 41355 : null,
        process_name: external ? "python.exe" : null,
        managed: false,
      };
    });
    return { items };
  }

  if (command === "ports.suggest") {
    const id = args?.id as string;
    const cur = state.services[id]?.port ?? 8080;
    const used = new Set<number>(Object.values(state.services).map((s) => s.port ?? 0));
    const out: number[] = [];
    for (let p = cur + 1; out.length < 5 && p < cur + 129; p++) {
      if (p >= 1024 && !used.has(p)) out.push(p);
    }
    return { candidates: out };
  }

  if (command === "ports.assign") {
    const id = args?.id as string;
    const port = args?.port as number;
    const svc = state.services[id];
    if (!svc) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `没有服务 ${id}`, retryable: false };
    }
    const hash = hashOf(JSON.stringify(args?.baseHash ?? "") + `:${id}:${port}`);
    if (svc.port) svc.port = port;
    const notes: string[] = [];
    return { operation_id: null, spec: state.spec, hash, restart_required: false, notes };
  }

  // env.effective：与真机语义一致——引擎自报最近一次启动注入的生效环境
  // （工作区 env < 服务 env < 端口自动注入；未启动过 → 空快照）
  if (command === "env.effective") {
    const id = args?.id as string;
    const svc = state.services[id];
    if (!svc) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `没有服务 ${id}`, retryable: false };
    }
    const specSvc = state.spec.services[id];
    const entries: { key: string; value: string; source: string }[] = [];
    const seen = new Set<string>();
    for (const [k, v] of Object.entries(state.spec.env ?? {})) {
      if (!seen.has(k)) entries.push({ key: k, value: String(v), source: "workspace" });
      seen.add(k);
    }
    for (const [k, v] of Object.entries(specSvc?.env ?? {})) {
      entries.push({ key: k, value: String(v), source: "service" });
      seen.add(k);
    }
    const portKey =
      specSvc?.kind === "spring-boot" ? "SERVER_PORT" : ["node", "python", "go"].includes(specSvc?.kind ?? "") ? "PORT" : null;
    const port = specSvc?.port ?? svc.port;
    if (portKey && port != null && !seen.has(portKey)) {
      entries.push({ key: portKey, value: String(port), source: "port" });
    }
    return { id, captured_at_ms: svc.started_at_ms, entries };
  }

  // spring.inspect：mock 下给一份典型 Spring Boot 配置，演示静态解析视图
  if (command === "spring.inspect") {
    const id = args?.id as string;
    const specSvc = state.spec.services[id];
    if (!specSvc || specSvc.kind !== "spring-boot") {
      return { id, server_port: null, entries: [], warnings: [] };
    }
    const file = "src/main/resources/application.yml";
    const devFile = "src/main/resources/application-dev.yml";
    const port = specSvc.port ?? 8080;
    return {
      id,
      server_port: port,
      entries: [
        { key: "server.port", value: String(port), file, masked: false },
        { key: "spring.application.name", value: id, file, masked: false },
        { key: "spring.profiles.active", value: "dev", file, masked: false },
        { key: "spring.datasource.url", value: "jdbc:mysql://localhost:3306/demo", file, masked: false },
        { key: "spring.datasource.username", value: "root", file, masked: false },
        { key: "spring.datasource.password", value: "••••••", file, masked: true },
        { key: "management.endpoints.web.exposure.include", value: "health,info", file, masked: false },
        // profile 文件：演示 base vs dev 覆盖/新增视角
        { key: "server.port", value: String(port + 1000), file: devFile, masked: false },
        { key: "spring.datasource.url", value: "jdbc:mysql://dev-host:3306/demo", file: devFile, masked: false },
        { key: "logging.level.root", value: "DEBUG", file: devFile, masked: false },
      ],
      warnings: [],
    };
  }

  if (command === "secrets.status") {
    const keys = [...mockSecrets.entries()].map(([key]) => ({
      key,
      source: "file",
      present: true,
      parse_ok: true,
      git_tracked: false,
    }));
    return { backend: "file", file: ".env.local", keys, git_ignored: true };
  }

  if (command === "secrets.set") {
    mockSecrets.set(args?.key as string, args?.value as string);
    return { ok: true, key: args?.key as string };
  }

  if (command === "secrets.delete") {
    mockSecrets.delete(args?.key as string);
    return { ok: true, key: args?.key as string };
  }

  if (command === "secrets.validate") {
    const required = (state.spec.secrets as { required?: string[] } | null)?.required ?? [];
    const missing = required.filter((k) => !mockSecrets.has(k));
    return { ok: missing.length === 0, missing, warnings: [] };
  }

  if (command === "logs.search") {
    const q = ((args?.query as string) ?? "").toLowerCase();
    const src = args?.source as { kind: string; id: string } | null;
    const inScope = state.logs.filter((l) => !src || (l.source.kind === src.kind && l.source.id === src.id));
    const items = inScope
      .filter((l) => l.text.toLowerCase().includes(q))
      .slice(0, (args?.limit as number) ?? 200)
      .map((l, i) => ({
        kind: l.source.kind,
        id: l.source.id,
        file: `${l.source.id}.log`,
        line_no: i + 1,
        text: l.text,
        ts: l.ts_ms,
      }));
    const opId = `op-${++opSeq}`;
    emitOperation("logs.search", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("logs.search", opId, "running", 0.5, "正在读取历史日志…", null, null), 180);
    setTimeout(
      () =>
        emitOperation("logs.search", opId, "succeeded", 1, "搜索完成", null, {
          items,
          truncated: false,
          files_scanned: inScope.length > 0 ? 1 : 0,
        }),
      420,
    );
    return { operation_id: opId };
  }

  if (command === "logs.export") {
    const opId = `op-${++opSeq}`;
    emitOperation("logs.export", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("logs.export", opId, "running", 0.5, "正在写出日志…", null, null), 180);
    setTimeout(
      () => emitOperation("logs.export", opId, "succeeded", 1, "导出完成", null, {
        count: state.logs.length,
        destination: args?.destinationPath as string,
      }),
      420,
    );
    return { operation_id: opId };
  }

  if (command === "logs.retention.run") {
    return { deleted_files: 0, deleted_bytes: 0 };
  }

  if (command === "system.metrics") {
    // Mock：随时间轻微波动的主机指标，便于无 Tauri 环境预览状态栏。
    const tick = Date.now() / 1000;
    const total = 32 * 1024 ** 3;
    return {
      cpuPercent: 18 + 12 * Math.abs(Math.sin(tick / 7)),
      memoryUsedBytes: Math.round(total * (0.44 + 0.06 * Math.sin(tick / 11))),
      memoryTotalBytes: total,
      diskUsedBytes: Math.round(931 * 1024 ** 3 * 0.62),
      diskTotalBytes: 931 * 1024 ** 3,
      cpuTempC: 52 + 6 * Math.abs(Math.sin(tick / 13)),
      sampledAtMs: Date.now(),
    };
  }

  if (command === "metrics.snapshot") {
    // 与真机语义对齐：仅运行中的非 compose 服务有宿主进程指标；值由 id 哈希决定（确定性）
    const services: Record<string, ServiceMetrics | null> = {};
    for (const s of Object.values(state.services)) {
      if (s.state !== "running" || s.kind === "compose") {
        services[s.id] = null;
        continue;
      }
      let h = 0;
      for (let i = 0; i < s.id.length; i++) h = (h * 31 + s.id.charCodeAt(i)) >>> 0;
      services[s.id] = {
        cpu_percent: (h % 40) / 10,
        memory_bytes: (180 + (h % 320)) * 1024 * 1024,
        process_count: 1 + (h % 4),
        sampled_at_ms: Date.now(),
      };
    }
    return { services };
  }

  if (command === "metrics.subscribe" || command === "metrics.unsubscribe") return { ok: true };

  if (command === "profiles.list") {
    const profiles = (state.spec.profiles as { items?: Record<string, unknown> } | null)?.items ?? {};
    const items = Object.entries(profiles).map(([id]) => ({ id, enabled_count: null }));
    return { active: "default", profiles: items };
  }

  if (command === "profiles.activate") {
    const hash = hashOf(`profile:${args?.id}`);
    return { spec: state.spec, hash, active: args?.id as string };
  }

  if (command === "runtime.build") {
    const id = args?.id as string;
    const opId = `op-${++opSeq}`;
    emitOperation("runtime.build", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("runtime.build", opId, "running", 0.4, "正在 package…", null, null), 500);
    setTimeout(
      () => emitOperation("runtime.build", opId, "succeeded", 1, "构建完成", null, {
        id,
        artifact: `C:\mock\target\${id}-1.0.jar`,
      }),
      1200,
    );
    return { operation_id: opId };
  }

  // -------------------------------------------------------------------------
  // 1.3：docker probe / ps / images / build（feature spec §9）
  // 浏览器调试 fixture：
  //   localStorage["st:mockDockerMode"] = "online"（默认）
  //     | "desktop_stopped"（已装未运行）| "not_found"（未安装）| "no_compose"（无 compose 插件）
  //   localStorage["st:mockDockerBuildFail"] = "1" → 下一次 docker.build 失败（IMAGE_BUILD_FAILED）
  // -------------------------------------------------------------------------

  type DockerMockMode = "online" | "desktop_stopped" | "not_found" | "no_compose";

  function mockDockerMode(): DockerMockMode {
    try {
      const v = localStorage.getItem("st:mockDockerMode");
      if (v === "desktop_stopped" || v === "not_found" || v === "no_compose") return v;
    } catch {
      /* ignore */
    }
    return "online";
  }

  function dockerUnavailableError() {
    if (mockDockerMode() === "not_found") {
      return { protocol: PROTOCOL, code: "DOCKER_NOT_FOUND", message: "PATH 中没有 docker 可执行文件", retryable: false };
    }
    return { protocol: PROTOCOL, code: "DOCKER_ENGINE_UNREACHABLE", message: "Docker 引擎未运行，请启动 Docker Desktop", retryable: true };
  }

  const MOCK_CONTAINER_META: Record<string, { container_id: string; image: string; health: string | null }> = {
    redis: { container_id: "a1b2c3d4e5f6", image: "redis:7.2-alpine", health: "healthy" },
    mysql: { container_id: "f6e5d4c3b2a1", image: "mysql:8.4", health: null },
  };

  if (command === "docker.probe") {
    const mode = mockDockerMode();
    if (mode === "not_found") {
      return { found: false, version: null, compose_version: null, running: false };
    }
    if (mode === "desktop_stopped") {
      return { found: true, version: "27.1.1", compose_version: "2.29.1", running: false };
    }
    if (mode === "no_compose") {
      return { found: true, version: "27.1.1", compose_version: null, running: true };
    }
    return { found: true, version: "27.1.1", compose_version: "2.29.1", running: true };
  }

  if (command === "docker.ps") {
    if (mockDockerMode() !== "online") throw dockerUnavailableError();
    const containers = Object.entries(state.spec.services)
      .filter(([, s]) => s.kind === "compose")
      .map(([id, s]) => {
        const svcName = s.service ?? id;
        const meta = MOCK_CONTAINER_META[svcName] ?? {
          container_id: "000000000000",
          image: `${svcName}:latest`,
          health: null,
        };
        const running = state.services[id]?.state === "running";
        return {
          service: svcName,
          container_id: meta.container_id,
          image: meta.image,
          state: running ? "running" : "exited",
          health: running ? meta.health : null,
          ports: [...s.ports],
        };
      });
    return { containers };
  }

  const MOCK_IMAGES = [
    { repository: "mall-user", tag: "local", id: "sha256:9f2c81b0a7de", size_bytes: 238_016_512, created_ms: Date.now() - 26 * 3600_000 },
    { repository: "redis", tag: "7.2-alpine", id: "sha256:5d12c7a3beff", size_bytes: 40_632_320, created_ms: Date.now() - 30 * 24 * 3600_000 },
  ];

  if (command === "docker.images") {
    if (mockDockerMode() !== "online") throw dockerUnavailableError();
    return { images: MOCK_IMAGES.map((i) => ({ ...i })) };
  }

  if (command === "docker.build") {
    const name = args?.name as string;
    const build = state.spec.docker?.builds?.find((b) => b.name === name);
    if (!build) {
      throw { protocol: PROTOCOL, code: "DOCKER_BUILD_UNKNOWN", message: `docker.builds 中没有条目 ${name}`, retryable: false };
    }
    if (mockDockerMode() !== "online") throw dockerUnavailableError();
    let fail = false;
    try {
      fail = localStorage.getItem("st:mockDockerBuildFail") === "1";
      if (fail) localStorage.removeItem("st:mockDockerBuildFail");
    } catch {
      /* ignore */
    }
    const opId = `op-${++opSeq}`;
    emitOperation("docker.build", opId, "queued", null, "排队中…", null, { name });
    setTimeout(() => emitOperation("docker.build", opId, "running", 0.2, "[1/4] FROM node:20-alpine", null, { name }), 400);
    setTimeout(() => emitOperation("docker.build", opId, "running", 0.6, "[3/4] RUN npm ci --omit=dev", null, { name }), 900);
    setTimeout(() => {
      if (fail) {
        emitOperation(
          "docker.build",
          opId,
          "failed",
          null,
          "ERROR: failed to solve: process \"/bin/sh -c npm ci\" did not complete successfully: exit code: 1",
          "IMAGE_BUILD_FAILED",
          { name },
        );
        return;
      }
      emitOperation("docker.build", opId, "succeeded", 1, `naming to ${build.tags[0]} done`, null, { name, image: build.tags[0] });
    }, 1500);
    return { operation_id: opId };
  }

  if (command === "docker.buildCancel") {
    // mock 语义：best effort —— 直接把该 operation 标记为 cancelled（已提交层缓存不回滚）
    const opId = args?.operationId as string;
    emitOperation("docker.build", opId, "cancelled", null, "已取消（已提交的层缓存保留）", null, null);
    return { ok: true };
  }

  if (command === "yaml.get") {
    const text = toYaml(state.spec);
    const view: YamlView = { text, spec: state.spec, hash: hashOf(text) };
    return view;
  }

  if (command === "yaml.saveText") {
    const text = args?.text as string;
    const hash = hashOf(text);
    const view: YamlSaveOut = { spec: state.spec, hash, warnings: [] };
    return view;
  }

  if (command === "yaml.saveForm") {
    const spec = args?.spec as SuperTaskFile;
    state.spec = spec;
    const text = toYaml(spec);
    const view: YamlSaveOut = { spec: state.spec, hash: hashOf(text), warnings: [] };
    return view;
  }

  if (command === "runtime.snapshot") return snapshot();

  // -------------------------------------------------------------------------
  // 1.6：网关（ipc.md §10.10）。demo：nginx + 3 路由 + 启停状态机 +
  // 假渲染文本（spec §11 mock 要求：交互全可走）。
  // -------------------------------------------------------------------------

  function mockGatewayStatus(): GatewayStatusOut {
    const gw = state.gateway;
    const routes = (gw?.conf.routes ?? []).map((r) => {
      const tp = r.target ? (state.spec.services[r.target]?.port ?? null) : null;
      const upPort = tp ?? (r.upstream ? Number(r.upstream.split(":").pop()) : null);
      const alive =
        upPort != null &&
        Object.values(state.services).some((s) => s.port === upPort && s.state === "running");
      return {
        host: r.host ?? null,
        path: r.path,
        target: r.target ?? null,
        upstream: r.upstream ?? null,
        target_port: tp,
        upstream_alive: alive,
      };
    });
    return {
      configured: !!gw,
      enabled: gw?.conf.enabled ?? true,
      kind: gw?.conf.kind ?? null,
      port: gw?.conf.port ?? null,
      state: gw?.state ?? null,
      pid: gw?.pid ?? null,
      last_error: null,
      routes,
      conf_path: gw ? "C:/mock/.supertask/gateway/nginx.conf" : null,
    };
  }

  if (command === "gateway.status") return mockGatewayStatus();

  if (command === "gateway.preview") {
    const conf = (args?.gateway as GatewayConf | null | undefined) ?? state.gateway?.conf ?? null;
    if (!conf?.kind) {
      throw { protocol: PROTOCOL, code: "GATEWAY_NOT_CONFIGURED", message: "gateway 未配置 kind", retryable: false };
    }
    const upstreamOf = (r: { target?: string | null; upstream?: string | null }) => {
      if (r.upstream) return r.upstream;
      const p = state.spec.services[r.target ?? ""]?.port;
      return `127.0.0.1:${p ?? 0}`;
    };
    let body: string;
    if (conf.kind === "nginx") {
      body = [
        "# Generated by SuperTask — source of truth is supertask.yaml (gateway section).",
        "worker_processes 1;",
        "daemon off;",
        `pid "C:/mock/.supertask/gateway/nginx.pid";`,
        `error_log "C:/mock/.supertask/gateway/nginx-error.log";`,
        "",
        "http {",
        `    access_log "C:/mock/.supertask/gateway/nginx-access.log";`,
        "    map $http_upgrade $connection_upgrade { default upgrade; '' close; }",
        "",
        "    server {",
        `        listen 127.0.0.1:${conf.port} default_server;`,
        "        server_name _;",
        ...conf.routes.map(
          (r) => [
            `        location ${r.path} {`,
            `            proxy_pass http://${upstreamOf(r)};`,
            "            proxy_http_version 1.1;",
            "            proxy_set_header Host $host;",
            "            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;",
            "        }",
          ].join("\n"),
        ),
        "    }",
        "}",
      ].join("\n");
    } else if (conf.kind === "caddy") {
      body = [
        "{",
        "\tadmin off",
        "}",
        "",
        `${conf.tls === "internal" ? "https" : "http"}://localhost:${conf.port} {`,
        ...(conf.tls === "internal" ? ["\ttls internal"] : []),
        ...conf.routes.flatMap((r) => [
          `\t@r${r.path.replace(/\//g, "_")} path ${r.path} ${r.path}/*`,
          `\thandle @r${r.path.replace(/\//g, "_")} {`,
          `\t\treverse_proxy ${upstreamOf(r)}`,
          "\t}",
        ]),
        "}",
      ].join("\n");
    } else {
      body = [
        "# Generated by SuperTask — source of truth is supertask.yaml (gateway section).",
        "ServerName localhost",
        `Listen 127.0.0.1:${conf.port}`,
        "",
        "ProxyRequests Off",
        "ProxyPreserveHost On",
        "",
        "<VirtualHost 127.0.0.1:" + conf.port + ">",
        ...conf.routes.flatMap((r) => [
          `    ProxyPass ${r.path} http://${upstreamOf(r)}${r.path}`,
          `    ProxyPassReverse ${r.path} http://${upstreamOf(r)}${r.path}`,
        ]),
        "</VirtualHost>",
      ].join("\n");
    }
    const name = conf.kind === "nginx" ? "nginx.conf" : conf.kind === "caddy" ? "Caddyfile" : "httpd.conf";
    return { files: [{ name, content: body }] };
  }

  if (command === "gateway.validate") {
    const conf = (args?.gateway as GatewayConf | null | undefined) ?? state.gateway?.conf ?? null;
    if (!conf?.kind) {
      throw { protocol: PROTOCOL, code: "GATEWAY_NOT_CONFIGURED", message: "gateway 未配置 kind", retryable: false };
    }
    if (!(1024 <= conf.port && conf.port <= 65535)) {
      return { ok: false, message: "gateway.port 超出 1024–65535", stderr: null };
    }
    const dup = Object.values(state.spec.services).some((s) => s.port === conf.port);
    if (dup) {
      return { ok: false, message: `gateway.port ${conf.port} 与服务端口重复`, stderr: "[emerg] bind() to 127.0.0.1:PORT failed" };
    }
    for (const [i, r] of conf.routes.entries()) {
      if (!r.path.startsWith("/")) {
        return { ok: false, message: `第 ${i + 1} 条路由：path 必须以 / 开头`, stderr: null };
      }
      if (r.target && !state.spec.services[r.target]) {
        return { ok: false, message: `第 ${i + 1} 条路由：target 服务 ${r.target} 不存在`, stderr: null };
      }
    }
    return { ok: true, message: null, stderr: null };
  }

  if (command === "gateway.apply") {
    const conf = args?.gateway as GatewayConf;
    const baseHash = args?.baseHash as string;
    const cur = hashOf(toYaml(state.spec));
    if (cur !== baseHash) {
      throw { protocol: PROTOCOL, code: "YAML_CONFLICT", message: "配置已被别处修改，请刷新后再保存", retryable: false };
    }
    const wasRunning = state.gateway?.state === "running";
    state.spec = { ...state.spec, gateway: conf };
    state.gateway =
      conf.kind && conf.enabled
        ? { conf, state: wasRunning ? "running" : "stopped", pid: wasRunning ? ++pidCounter : null, startedAt: wasRunning ? Date.now() : null }
        : null;
    emitGatewayRuntime();
    return { spec: state.spec, hash: cur, restarted: wasRunning && !!conf.kind && conf.enabled, warnings: [] };
  }

  if (command === "gateway.start") {
    if (!state.gateway) {
      throw { protocol: PROTOCOL, code: "GATEWAY_NOT_CONFIGURED", message: "gateway 未配置或未启用", retryable: false };
    }
    state.gateway.state = "starting";
    state.gateway.pid = ++pidCounter;
    emitGatewayRuntime();
    pushLog({ kind: "gateway", id: "gateway" }, "system", "[mock] 网关进程启动（前台，进程树托管）");
    const port = state.gateway.conf.port;
    setTimeout(() => {
      if (state.gateway) {
        state.gateway.state = "running";
        state.gateway.startedAt = Date.now();
        emitGatewayRuntime();
        pushLog({ kind: "gateway", id: "gateway" }, "stderr", `[mock] nginx ready — listening on 127.0.0.1:${port}`);
      }
    }, 1200);
    return { accepted: true, order: null };
  }

  if (command === "gateway.stop") {
    if (state.gateway) {
      state.gateway.state = "stopped";
      state.gateway.pid = null;
      state.gateway.startedAt = null;
      pushLog({ kind: "gateway", id: "gateway" }, "system", "[mock] 网关已停止（进程树终止，无残留）");
      emitGatewayRuntime();
    }
    return { accepted: true, order: null };
  }

  if (command === "gateway.restart") {
    if (!state.gateway) {
      throw { protocol: PROTOCOL, code: "GATEWAY_NOT_CONFIGURED", message: "gateway 未配置或未启用", retryable: false };
    }
    state.gateway.state = "starting";
    state.gateway.pid = ++pidCounter;
    emitGatewayRuntime();
    setTimeout(() => {
      if (state.gateway) {
        state.gateway.state = "running";
        state.gateway.startedAt = Date.now();
        emitGatewayRuntime();
      }
    }, 1200);
    return { accepted: true, order: null };
  }

  if (command === "gateway.trust") {
    if (state.gateway?.conf.kind !== "caddy") {
      throw { protocol: PROTOCOL, code: "GATEWAY_NOT_CONFIGURED", message: "gateway.trust 仅支持 kind: caddy", retryable: false };
    }
    pushLog({ kind: "gateway", id: "gateway" }, "system", "[mock] caddy trust 完成（本机 CA 已加入系统信任库）");
    return { accepted: true, order: null };
  }

  if (command === "runtime.startOne") {
    const id = args?.id as string;
    const s = state.services[id];
    if (s) {
      const compose = s.kind === "compose";
      s.state = "running";
      s.pid = compose ? null : ++pidCounter;
      s.started_at_ms = Date.now();
      s.health = { ok: true, at_ms: Date.now(), detail: compose ? "tcp 6379" : "200 OK" };
      if (compose) {
        pushLog({ kind: "service", id }, "system", "[docker] docker compose -f compose.yaml up -d --no-deps");
        pushLog({ kind: "service", id }, "system", "[mock] Container started（容器托管，无宿主 pid）");
      } else {
        pushLog({ kind: "service", id }, "stdout", `[mock] ${id} 已启动 (pid ${s.pid})`);
      }
    }
    return { accepted: true, order: null };
  }

  if (command === "runtime.stopOne") {
    const id = args?.id as string;
    const s = state.services[id];
    if (s) {
      const compose = s.kind === "compose";
      s.state = "stopped";
      s.pid = null;
      s.health = null;
      s.started_at_ms = null;
      if (compose) {
        pushLog({ kind: "service", id }, "system", "[docker] docker compose -f compose.yaml stop");
        pushLog({ kind: "service", id }, "system", "[mock] Container stopped");
      } else {
        pushLog({ kind: "service", id }, "stdout", `[mock] ${id} 已停止`);
      }
    }
    return { accepted: true, order: null };
  }

  if (command === "runtime.stopAll") {
    for (const s of Object.values(state.services)) {
      s.state = "stopped";
      s.pid = null;
      s.health = null;
      s.started_at_ms = null;
    }
    return { accepted: true, order: null };
  }

  if (command === "runtime.startAll") {
    const order: string[] = [];
    for (const s of Object.values(state.services)) {
      const specSvc = state.spec.services[s.id];
      if (specSvc?.enabled) {
        const compose = s.kind === "compose";
        s.state = "running";
        s.pid = compose ? null : ++pidCounter;
        s.started_at_ms = Date.now();
        s.health = { ok: true, at_ms: Date.now(), detail: compose ? "tcp" : "200 OK" };
        order.push(s.id);
      }
    }
    return { accepted: true, order };
  }

  if (command === "runtime.restartOne") {
    const id = args?.id as string;
    const s = state.services[id];
    if (s) {
      const compose = s.kind === "compose";
      s.state = "running";
      s.pid = compose ? null : ++pidCounter;
      s.started_at_ms = Date.now();
      s.health = { ok: true, at_ms: Date.now(), detail: compose ? "tcp" : "200 OK" };
      pushLog({ kind: "service", id }, compose ? "system" : "stdout", `[mock] ${id} 已重启`);
    }
    return { accepted: true, order: null };
  }

  if (command === "script.run") {
    const id = args?.id as string;
    if (state.script?.state === "running") {
      throw { protocol: PROTOCOL, code: "SCRIPT_BUSY", message: "已有脚本在运行", retryable: false };
    }
    if (!state.spec.scripts[id]) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `没有脚本 ${id}`, retryable: false };
    }
    const cmds = state.spec.scripts[id].cmds;
    state.script = {
      id,
      state: "running",
      pid: ++pidCounter,
      last_exit: null,
      last_error: null,
    };
    pushLog({ kind: "script", id }, "stdout", `[mock] ${id} 开始执行…`);
    // 引擎语义：cmds 顺序执行，全部成功 → 退出码 0；与真机一致走 Exited 终态
    window.setTimeout(() => {
      if (state.script?.id !== id || state.script.state !== "running") return;
      for (const c of cmds) {
        pushLog({ kind: "script", id }, "stdout", `[mock] $ ${c}`);
      }
      state.script.state = "exited";
      state.script.pid = null;
      state.script.last_exit = { code: 0, at_ms: Date.now() };
      pushLog({ kind: "script", id }, "stdout", `[mock] ${id} 执行完成`);
    }, 2500);
    return { accepted: true, order: null };
  }

  if (command === "script.cancel") {
    if (state.script?.state === "running") {
      const id = state.script.id;
      state.script.state = "exited";
      state.script.pid = null;
      state.script.last_exit = { code: 130, at_ms: Date.now() };
      state.script.last_error = "已取消";
      pushLog({ kind: "script", id }, "system", `[mock] ${id} 已取消`);
    }
    return { accepted: true, order: null };
  }

  if (command === "logs.snapshot") {
    const source = args?.source as LogSource | null;
    let items = state.logs;
    if (source) {
      items = state.logs.filter((l) => l.source.kind === source.kind && l.source.id === source.id);
    }
    const limit = (args?.limit as number) ?? 2000;
    if (items.length > limit) items = items.slice(items.length - limit);
    // simulate a trickle of new lines for running services
    for (const s of Object.values(state.services)) {
      if (s.state === "running") {
        pushLog({ kind: "service", id: s.id }, "stdout", `[${new Date().toISOString().slice(11, 19)}] ${s.id} heartbeat ok`);
      }
    }
    return { items: state.logs.slice(-limit), next_seq: state.logSeq + 1 };
  }

  if (command === "logs.clearView") {
    const source = args?.source as LogSource;
    state.logs = state.logs.filter((l) => !(l.source.kind === source.kind && l.source.id === source.id));
    return { ok: true };
  }

  // -------------------------------------------------------------------------
  // 1.1：偏好 / 应用数据
  // -------------------------------------------------------------------------

  if (command === "app.savePrefs") {
    const prefs = readMockPrefs();
    if (typeof args?.theme === "string") prefs.theme = args.theme;
    if (typeof args?.restoreLast === "boolean") prefs.restoreLast = args.restoreLast;
    if (typeof args?.closeToTray === "boolean") prefs.closeToTray = args.closeToTray;
    if (typeof args?.startOnLogin === "boolean") prefs.startOnLogin = args.startOnLogin;
    if (typeof args?.updateCheck === "boolean") prefs.updateCheck = args.updateCheck;
    if (typeof args?.locale === "string") prefs.locale = args.locale;
    writeMockPrefs(prefs);
    return { ok: true };
  }

  if (command === "app.importRecents") return { ok: true };

  // -------------------------------------------------------------------------
  // 1.1：模板（ipc.md §10.1）
  // -------------------------------------------------------------------------

  if (command === "templates.list") {
    return { templates: MOCK_TEMPLATES.map((t) => ({ ...t })) };
  }

  if (command === "templates.preview") {
    const templateId = args?.templateId as string;
    const tpl = MOCK_TEMPLATES.find((t) => t.id === templateId);
    if (!tpl) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `模板不存在: ${templateId}`, retryable: false };
    }
    if (tpl.invalid) {
      throw { protocol: PROTOCOL, code: "TEMPLATE_INVALID", message: tpl.invalid_reason ?? "模板清单损坏", retryable: false };
    }
    const plan = mockPlanBlocks(tpl, args?.blocks as string[] | undefined, args?.ports as Record<string, number> | undefined);
    return {
      services: plan?.services ?? {},
      files: plan?.files ?? tpl.files,
      warnings: [] as string[],
    };
  }

  if (command === "templates.create") {
    const templateId = args?.templateId as string;
    const source = (args?.source as string) ?? "builtin";
    const parentPath = ((args?.parentPath as string) ?? "").trim();
    const dirName = ((args?.directoryName as string) ?? "").trim();
    const tpl = MOCK_TEMPLATES.find((t) => t.id === templateId);
    if (!tpl || (source === "local" && tpl.source === "builtin")) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `模板不存在: ${templateId}`, retryable: false };
    }
    if (source === "local" && templateId === "spring-multimodule-node") {
      // 与后端一致的冲突防御（mock 里 builtin id 不会被 local 覆盖，仅为语义对齐）
      throw { protocol: PROTOCOL, code: "TEMPLATE_ID_CONFLICT", message: `本地模板 id ${templateId} 与内置模板冲突`, retryable: false };
    }
    if (tpl.invalid) {
      throw {
        protocol: PROTOCOL,
        code: "TEMPLATE_INVALID",
        message: tpl.invalid_reason ?? "模板清单损坏",
        retryable: false,
      };
    }
    if (!parentPath) {
      throw { protocol: PROTOCOL, code: "CWD_MISSING", message: "请选择父目录", retryable: false };
    }
    const invalid = invalidDirectoryName(dirName);
    if (invalid) {
      throw { protocol: PROTOCOL, code: "PATH_ESCAPE", message: `非法目录名 ${JSON.stringify(dirName)}: ${invalid}`, retryable: false };
    }
    // 参数校验语义对齐 core：required 缺失 → TEMPLATE_PARAM_MISSING；未声明键 → TEMPLATE_PARAM_UNKNOWN
    const declared = tpl.params ?? [];
    const provided = (args?.params as Record<string, string> | undefined) ?? {};
    for (const [k, v] of Object.entries(provided)) {
      if (!declared.some((p) => p.key === k)) {
        throw { protocol: PROTOCOL, code: "TEMPLATE_PARAM_UNKNOWN", message: `模板未声明参数 ${k}`, retryable: false };
      }
      if (!v?.trim()) {
        throw { protocol: PROTOCOL, code: "TEMPLATE_PARAM_MISSING", message: `参数 ${k} 的值不能为空`, retryable: false };
      }
    }
    for (const p of declared) {
      if (p.required && !provided[p.key]?.trim()) {
        throw { protocol: PROTOCOL, code: "TEMPLATE_PARAM_MISSING", message: `缺少必填参数 ${p.key}`, retryable: false };
      }
    }
    // 组合模板：与 preview 同一套块校验（依赖闭合 + 端口查重）
    mockPlanBlocks(tpl, args?.blocks as string[] | undefined, args?.ports as Record<string, number> | undefined);
    const opId = `op-${++opSeq}`;
    const wsId = `${parentPath.replace(/[\\/]+$/, "")}\\${dirName}`;
    emitOperation("templates.create", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("templates.create", opId, "running", 0.3, "正在复制模板文件…", null, null), 400);
    setTimeout(() => emitOperation("templates.create", opId, "running", 0.7, "正在写入 supertask.yaml…", null, null), 900);
    setTimeout(() => emitOperation("templates.create", opId, "succeeded", 1, "创建完成", null, { workspace_id: wsId }), 1400);
    return { operation_id: opId };
  }

  // -------------------------------------------------------------------------
  // 1.5：导出包（ipc.md §10.9）——浏览器 mock 语义对齐，不做真 zip
  // -------------------------------------------------------------------------

  if (command === "workspace.exportPackage") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    const destPath = (args?.destPath as string) ?? "";
    const withSecrets = args?.withSecrets === true;
    if (!workspaceId) {
      throw { protocol: PROTOCOL, code: "NO_WORKSPACE", message: "未打开工作区", retryable: false };
    }
    if (!destPath.trim()) {
      throw { protocol: PROTOCOL, code: "CWD_MISSING", message: "请选择导出路径", retryable: false };
    }
    return {
      path: destPath,
      entries: [
        { path: "supertask.yaml", bytes: 1024 },
        ...(withSecrets ? [{ path: ".env.local", bytes: 64 }] : []),
      ],
      warnings: [],
    };
  }

  if (command === "workspace.importPackage") {
    const pkgPath = (args?.pkgPath as string) ?? "";
    const destDir = (args?.destDir as string) ?? "";
    if (!pkgPath.trim() || !pkgPath.endsWith(".zip")) {
      throw { protocol: PROTOCOL, code: "PKG_NOT_FOUND", message: "导出包不存在或不可读", retryable: false };
    }
    if (!destDir.trim()) {
      throw { protocol: PROTOCOL, code: "CWD_MISSING", message: "请选择目标目录", retryable: false };
    }
    if (pkgPath.includes("conflict")) {
      throw { protocol: PROTOCOL, code: "PKG_TARGET_EXISTS", message: "目标目录已有 supertask.yaml，不覆盖", retryable: false };
    }
    return { root: destDir, warnings: [] };
  }

  // -------------------------------------------------------------------------
  // 1.1：Git（ipc.md §10.2）
  // -------------------------------------------------------------------------

  if (command === "git.clone") {
    const url = ((args?.url as string) ?? "").trim();
    const targetPath = ((args?.targetPath as string) ?? "").trim();
    if (!url) {
      throw { protocol: PROTOCOL, code: "GIT_REMOTE", message: "仓库 URL 不能为空", retryable: false };
    }
    if (url.includes("@") && /:\/\/[^/]+@/.test(url)) {
      throw { protocol: PROTOCOL, code: "GIT_AUTH", message: "URL 不允许内嵌凭据，请使用 Git Credential Manager", retryable: false };
    }
    if (!targetPath) {
      throw { protocol: PROTOCOL, code: "CWD_MISSING", message: "请选择目标目录", retryable: false };
    }
    const opId = `op-${++opSeq}`;
    emitOperation("git.clone", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("git.clone", opId, "running", 0.3, "正在克隆仓库…", null, null), 500);
    setTimeout(() => emitOperation("git.clone", opId, "running", 0.7, "正在检出文件…", null, null), 1000);
    setTimeout(() => emitOperation("git.clone", opId, "succeeded", 1, "克隆完成", null, { workspace_id: targetPath }), 1500);
    return { operation_id: opId };
  }

  if (command === "git.status") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    return mockGitStatus();
  }

  if (command === "git.pull") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    const st = mockGitStatus();
    if (st.dirty && args?.allowDirty !== true) {
      throw {
        protocol: PROTOCOL,
        code: "GIT_DIRTY",
        message: "工作区有未提交修改，已阻止拉取；确认后可带 allow_dirty 重试",
        retryable: false,
      };
    }
    const opId = `op-${++opSeq}`;
    emitOperation("git.pull", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("git.pull", opId, "running", 0.4, "正在拉取 origin/main", null, null), 500);
    setTimeout(() => emitOperation("git.pull", opId, "running", 0.8, "正在快进合并…", null, null), 1000);
    setTimeout(
      () => {
        state.gitPulled = true;
        emitOperation("git.pull", opId, "succeeded", 1, "拉取完成", null, { workspace_id: workspaceId });
      },
      1400,
    );
    return { operation_id: opId };
  }

  // -------------------------------------------------------------------------
  // 1.1：IDE / 扫描合并（ipc.md §10.3–10.4）
  // -------------------------------------------------------------------------

  // 确定性两种态：explorer / code 成功，cursor / idea 返回 IDE_NOT_FOUND，
  // 方便 UI 验证两条分支（不引入随机）。
  if (command === "workspace.openIde") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    const ide = (args?.ide as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    if (ide === "explorer" || ide === "code") {
      const exe = ide === "explorer" ? "C:\\Windows\\explorer.exe" : "C:\\Program Files\\Microsoft VS Code\\bin\\code.cmd";
      return { accepted: true, ide, path: exe };
    }
    throw {
      protocol: PROTOCOL,
      code: "IDE_NOT_FOUND",
      message: `固定候选中没有检测到 ${ide}，请确认已安装并加入 PATH`,
      retryable: false,
    };
  }

  if (command === "workspace.scanPreview") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    return mockScanPreview();
  }

  if (command === "workspace.scanApply") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    const text = toYaml(state.spec);
    const out: YamlSaveOut = { spec: state.spec, hash: hashOf(text), warnings: [] };
    return out;
  }

  // -------------------------------------------------------------------------
  // 2.1：README 导入（ipc.md §10.13）
  // -------------------------------------------------------------------------

  if (command === "import.readme") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    return mockReadmePreview();
  }

  if (command === "import.readmeApply") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    const choices = (args?.choices as { id: string; action: string; target?: string }[]) ?? [];
    const preview = mockReadmePreview();
    for (const c of choices) {
      if (c.action === "keep") continue;
      if (c.target === "script") {
        const item = preview.script_items.find((s) => s.script_id === c.id);
        if (item?.discovered && !state.spec.scripts[c.id]) {
          state.spec.scripts[c.id] = item.discovered;
        }
        continue;
      }
      const item = preview.items.find((s) => s.service_id === c.id);
      if (item?.discovered && c.action === "add" && !state.spec.services[c.id]) {
        state.spec.services[c.id] = item.discovered;
      }
    }
    const text = toYaml(state.spec);
    return { spec: state.spec, hash: hashOf(text), warnings: ["已应用 README 草稿（mock）"] };
  }

  // -------------------------------------------------------------------------
  // 1.4：Taskfile 导入（ipc.md §10.8）
  // -------------------------------------------------------------------------

  if (command === "import.taskfilePreview") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    return mockTaskfilePreview();
  }

  if (command === "import.taskfileApply") {
    const workspaceId = (args?.workspaceId as string) ?? "";
    if (!workspaceId || !state.opened) throw noWorkspaceError();
    const selected = (args?.selected as string[]) ?? [];
    // YAML_CONFLICT 分支：localStorage st:mockTaskfileConflict=1 模拟外部修改；
    // 或 base_hash 与当前 spec 不一致（真实语义对齐 core）。
    let forcedConflict = false;
    try {
      forcedConflict = localStorage.getItem("st:mockTaskfileConflict") === "1";
      if (forcedConflict) localStorage.removeItem("st:mockTaskfileConflict");
    } catch {
      /* ignore */
    }
    const currentHash = hashOf(toYaml(state.spec));
    if (forcedConflict || (args?.baseHash as string) !== currentHash) {
      throw { protocol: PROTOCOL, code: "YAML_CONFLICT", message: "supertask.yaml 已被外部修改，请重新加载后重试", retryable: false };
    }
    const preview = mockTaskfilePreview();
    for (const item of preview.tasks) {
      if (!selected.includes(item.script_id)) continue;
      if (item.internal) continue; // internal 不可导入
      state.spec.scripts[item.script_id] = {
        desc: item.task === "bootstrap" ? "安装依赖并初始化" : `导入自 Taskfile：${item.task}`,
        cmds: [`${item.script_id} mock cmd 1`, `${item.script_id} mock cmd 2`].slice(0, item.cmds_count),
        cwd: null,
        env: {},
        timeout_secs: 1800,
        depends_on: [],
      };
    }
    const text = toYaml(state.spec);
    return { spec: state.spec, hash: hashOf(text), warnings: ["已导入一次性迁移脚本（mock）"] };
  }

  // -------------------------------------------------------------------------
  // 1.1：更新（ipc.md §10.6）
  // -------------------------------------------------------------------------

  if (command === "app.update.check") {
    const opId = `op-${++opSeq}`;
    emitOperation("app.update", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("app.update", opId, "running", null, "正在检查更新…", null, null), 400);
    setTimeout(
      () => emitOperation("app.update", opId, "succeeded", null, "检查完成", null, { status: "up_to_date" }),
      900,
    );
    return { operation_id: opId };
  }

  if (command === "app.update.install") {
    throw { protocol: PROTOCOL, code: "UPDATE_FAILED", message: "没有可安装的更新（mock）", retryable: false };
  }

  // ---- 2.0 云（mock provider：确定性演示；含登录/同步/冲突旋钮） ----
  if (command === cmd.CLOUD_STATUS) {
    const endpoint = typeof window !== "undefined"
      ? window.localStorage.getItem("st:cloudEndpoint") ?? mockCloud.endpoint
      : mockCloud.endpoint;
    const out: CloudStatusOut = {
      logged_in: mockCloud.loggedIn,
      email: mockCloud.loggedIn ? mockCloud.email : null,
      device: "mockdevice0000000",
      endpoint,
      last_synced_ms: mockCloud.lastSyncedMs,
      conflicts: mockCloud.conflicts.length,
      conflict_ids: [...mockCloud.conflicts],
      quota: mockCloud.loggedIn
        ? { entities: 3, entities_max: 100, bytes: 4096, bytes_max: 10000000 }
        : null,
      telemetry_enabled: mockCloud.telemetryEnabled,
    };
    return out;
  }
  if (command === cmd.CLOUD_LOGIN) {
    const email = String(args?.email ?? "").trim();
    if (!email || !String(args?.password ?? "")) {
      throw { protocol: PROTOCOL, code: "CLOUD_AUTH_FAILED", message: "邮箱或密码不能为空", retryable: false };
    }
    mockCloud.loggedIn = true;
    mockCloud.email = email;
    return { account_id: "acc-demo", email, expires_in_secs: 900 };
  }
  if (command === cmd.CLOUD_LOGOUT) {
    mockCloud.loggedIn = false;
    mockCloud.email = null;
    return { ok: true };
  }
  if (command === cmd.CLOUD_SYNC) {
    if (!mockCloud.loggedIn) throw { protocol: PROTOCOL, code: "CLOUD_NOT_LOGGED_IN", message: "未登录", retryable: false };
    mockCloud.lastSyncedMs = Date.now();
    return { pushed: 1, pulled: 0, pending: [], skipped: [], conflicts: [...mockCloud.conflicts] } satisfies CloudSyncOut;
  }
  if (command === cmd.CLOUD_RESOLVE) {
    if (!mockCloud.loggedIn) throw { protocol: PROTOCOL, code: "CLOUD_NOT_LOGGED_IN", message: "未登录", retryable: false };
    const entityId = String(args?.entity_id ?? args?.entityId ?? "");
    mockCloud.conflicts = mockCloud.conflicts.filter((id) => id !== entityId);
    return { pushed: 0, pulled: 1, pending: [], skipped: [], conflicts: [] } satisfies CloudSyncOut;
  }
  if (command === cmd.CLOUD_MIGRATE_PLAN) {
    if (!mockCloud.loggedIn) throw { protocol: PROTOCOL, code: "CLOUD_NOT_LOGGED_IN", message: "未登录", retryable: false };
    const out: CloudMigratePlanOut = {
      entities: [{ id: "workspace-demo", type: "workspace", name: "Demo workspace" }, { id: "app-settings", type: "settings", name: "Settings" }],
      toolchain_gaps: [{ status: "missing", tool: "python", version: "3.12" }],
    };
    return out;
  }
  if (command === cmd.CLOUD_MIGRATE_APPLY) {
    if (!mockCloud.loggedIn) throw { protocol: PROTOCOL, code: "CLOUD_NOT_LOGGED_IN", message: "未登录", retryable: false };
    mockCloud.lastSyncedMs = Date.now();
    const workspaces = Array.isArray(args?.workspaces) ? args?.workspaces as { entity_id?: string; entityId?: string; dir: string }[] : [];
    const out: CloudMigrateApplyOut = {
      pushed: 0, pulled: workspaces.length, pending: [], skipped: [], conflicts: [],
      applied: workspaces.map((item) => item.entity_id ?? item.entityId ?? "").filter(Boolean), warnings: [],
    };
    return out;
  }
  if (command === cmd.CLOUD_ENDPOINT_SET) {
    const endpoint = String(args?.endpoint ?? "").trim().replace(/\/$/, "");
    if (!/^https?:\/\/[^\s/]+(?:\/[^\s]*)?$/.test(endpoint)) {
      throw { protocol: PROTOCOL, code: "CLOUD_PROTOCOL_ERROR", message: "云端点无效", retryable: false };
    }
    if (typeof window !== "undefined") window.localStorage.setItem("st:cloudEndpoint", endpoint);
    return { endpoint, supported: false, local_only: true };
  }
  if (command === cmd.CLOUD_TELEMETRY_SET) {
    mockCloud.telemetryEnabled = !!args?.enabled;
    return { enabled: mockCloud.telemetryEnabled };
  }

  // ---- 2.1 AI（mock：命名多配置 + 确定性回文；未配置 → AI_NOT_CONFIGURED） ----
  if (command === cmd.AI_CONFIG_SAVE) {
    const input = (args?.input ?? {}) as Record<string, unknown>;
    const name = String(input.name ?? "").trim();
    const baseUrl = String(input.baseUrl ?? "").trim().replace(/\/$/, "");
    const model = String(input.model ?? "").trim();
    if (!name || name.length > 50) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "配置名不能为空且不超过 50 字符", retryable: false };
    }
    const dupe = mockAi.configs.some(
      (c) => c.name.toLowerCase() === name.toLowerCase() && c.id !== input.id,
    );
    if (dupe) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: `配置名 ${name} 已存在`, retryable: false };
    }
    if (input.baseUrl != null && !/^https?:\/\/[^\s/]+(?:\/[^\s]*)?$/.test(baseUrl)) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "base_url 无效", retryable: false };
    }
    if (!model) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "model 不能为空", retryable: false };
    }
    let id = typeof input.id === "string" ? input.id : "";
    if (id && mockAi.configs.some((c) => c.id === id)) {
      mockAi.configs = mockAi.configs.map((c) =>
        c.id === id
          ? { ...c, name, baseUrl, model, provider: String(input.provider ?? c.provider) }
          : c,
      );
    } else {
      id = `mock-cfg-${++mockAiSeq}`;
      mockAi.configs.push({
        id,
        name,
        isDefault: mockAi.configs.length === 0,
        provider: String(input.provider ?? "openai-compatible"),
        model,
        baseUrl,
        timeoutSecs: Number(input.timeoutSecs ?? 30) || 30,
        maxTokens: Number(input.maxTokens ?? 8192) || 8192,
        authMethod: String(input.authMethod ?? "api_key"),
        proxyEnabled: !!input.proxyEnabled,
        proxyUrl: input.proxyUrl ? String(input.proxyUrl) : null,
        contextWindow: input.contextWindow != null ? Number(input.contextWindow) : null,
        maxRetries: Number(input.maxRetries ?? 2),
      });
    }
    if (typeof input.apiKey === "string") mockAi.keySet = input.apiKey.length > 0;
    const saved = mockAi.configs.find((c) => c.id === id)!;
    const out: AiConfigOut = {
      id: saved.id,
      name: saved.name,
      base_url: saved.baseUrl,
      model: saved.model,
      timeout_secs: saved.timeoutSecs,
      max_tokens: saved.maxTokens,
      provider: saved.provider,
      api_style: saved.provider === "claude" ? "anthropic_messages" : "openai_completions",
      auth_method: saved.authMethod === "bearer" ? "bearer" : "api_key",
      proxy_enabled: saved.proxyEnabled,
      proxy_url: saved.proxyUrl,
      context_window: saved.contextWindow,
      max_retries: saved.maxRetries,
    };
    return out;
  }
  if (command === cmd.AI_CONFIG_DELETE) {
    const id = String(args?.id ?? "");
    mockAi.configs = mockAi.configs.filter((c) => c.id !== id);
    if (mockAi.configs.length && !mockAi.configs.some((c) => c.isDefault)) mockAi.configs[0].isDefault = true;
    return { ok: true };
  }
  if (command === cmd.AI_CONFIG_DEFAULT) {
    const id = String(args?.id ?? "");
    if (!mockAi.configs.some((c) => c.id === id)) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `AI 配置不存在: ${id}`, retryable: false };
    }
    mockAi.configs = mockAi.configs.map((c) => ({ ...c, isDefault: c.id === id }));
    return { ok: true };
  }
  if (command === cmd.AI_INSTRUCTIONS_SAVE) {
    const text = String(args?.text ?? "").trim();
    if (text.length > 8000) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "全局指令超过 8000 字符", retryable: false };
    }
    mockAi.instructions = text;
    return { text };
  }
  if (command === cmd.AI_TEMPLATE_SAVE) {
    const input = (args?.input ?? {}) as Record<string, unknown>;
    const name = String(input.name ?? "").trim();
    const content = String(input.content ?? "").trim();
    if (!name || name.length > 50) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "模板名不能为空且不超过 50 字符", retryable: false };
    }
    if (!content || content.length > 8000) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "模板内容不能为空且不超过 8000 字符", retryable: false };
    }
    let id = typeof input.id === "string" ? input.id : "";
    if (id && mockAi.templates.some((t) => t.id === id)) {
      mockAi.templates = mockAi.templates.map((t) =>
        t.id === id ? { ...t, name, content, enabled: !!input.enabled } : t,
      );
    } else {
      id = `mock-tpl-${++mockAiSeq}`;
      mockAi.templates.push({ id, name, content, enabled: !!input.enabled });
    }
    const saved = mockAi.templates.find((t) => t.id === id)!;
    const out: AiTemplate = { id: saved.id, name: saved.name, content: saved.content, enabled: saved.enabled };
    return out;
  }
  if (command === cmd.AI_TEMPLATE_DELETE) {
    const id = String(args?.id ?? "");
    if (!mockAi.templates.some((t) => t.id === id)) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `模板不存在: ${id}`, retryable: false };
    }
    mockAi.templates = mockAi.templates.filter((t) => t.id !== id);
    return { ok: true };
  }
  if (command === cmd.AI_STATUS) {
    const def = mockAiDefault();
    const out: AiStatusOut = {
      configs: mockAi.configs.map((c) => ({
        id: c.id,
        name: c.name,
        is_default: c.isDefault,
        provider: c.provider,
        model: c.model,
        base_url: c.baseUrl,
      })),
      default_id: def?.id ?? null,
      templates: [...mockAi.templates],
      global_instructions: mockAi.instructions || null,
      key_set: mockAi.keySet,
      usage_today: mockAiUsage(),
    };
    return out;
  }
  if (command === cmd.AI_MODELS) {
    if (!mockAiDefault()) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "AI 未配置，请先在 /ai 页新增配置", retryable: false };
    }
    return ["demo-model", "demo-model-mini", "qwen2.5:7b"];
  }
  if (command === cmd.AI_COMPLETE) {
    const def = mockAiDefault();
    if (!def) {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "AI 未配置，请先在 /ai 页新增配置", retryable: false };
    }
    if (!mockAi.keySet && def.provider !== "ollama") {
      throw { protocol: PROTOCOL, code: "AI_NOT_CONFIGURED", message: "AI key 未设置，请先在 /ai 页保存密钥", retryable: false };
    }
    const task = String(args?.task ?? "") as AiTask;
    if (!["explain_logs", "config_suggest", "enrich_draft", "test_connection"].includes(task)) {
      throw { protocol: PROTOCOL, code: "PROTOCOL", message: `未知 ai.complete task: ${task}`, retryable: false };
    }
    mockAi.usage.count += 1;
    const payload = (args?.payload ?? {}) as Record<string, unknown>;
    const fullText = mockAiEcho(task, payload);
    const requestId = args?.requestId ? String(args.requestId) : "";
    if (requestId) {
      const chunkSize = 20;
      for (let i = 0; i < fullText.length; i += chunkSize) {
        mockAiEmitChunk(requestId, fullText.slice(i, i + chunkSize));
        await new Promise((r) => setTimeout(r, 35));
      }
    }
    return {
      text: fullText,
      usage: mockAiUsage(),
      model: def.model,
      tokens: { prompt_tokens: 10, completion_tokens: 6 },
    };
  }

  // ---- 运行页终端（mock：确定性假 shell；序列与真链路一致） ----
  if (command === cmd.TERM_OPEN) {
    const serviceId = args?.serviceId ? String(args.serviceId) : null;
    const svc = serviceId ? state.spec.services[serviceId] : null;
    if (serviceId && !svc) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `没有服务 ${serviceId}`, retryable: false };
    }
    const cwd = svc ? `${state.spec.root}/${svc.module ?? svc.dir ?? "."}` : state.spec.root;
    mockTermSeq += 1;
    const s: MockTermSession = { id: mockTermSeq, cwd, line: "" };
    mockTerms.set(s.id, s);
    const banner =
      "\x1b[1;36mSuperTask mock 终端\x1b[0m（浏览器演示：确定性假 shell，不拉起真实进程）\r\n" +
      "输入 \x1b[1mhelp\x1b[0m 查看可用命令。\r\n";
    mockTermEmit(s.id, "output", banner + mockTermPrompt(s));
    return { session_id: s.id, shell: "mock-shell" };
  }
  if (command === cmd.TERM_WRITE) {
    const sessionId = Number(args?.sessionId ?? 0);
    const s = mockTerms.get(sessionId);
    if (!s) {
      throw {
        protocol: PROTOCOL,
        code: "TERM_SESSION_NOT_FOUND",
        message: `终端会话不存在或已退出: ${sessionId}`,
        retryable: false,
      };
    }
    // 异步处理输入，对齐真链路「write 立即返回、输出经 st.term 流回」的语义
    const data = String(args?.data ?? "");
    setTimeout(() => {
      const cur = mockTerms.get(sessionId);
      if (cur) mockTermHandleInput(cur, data);
    }, 10);
    return { accepted: true };
  }
  if (command === cmd.TERM_RESIZE) {
    const sessionId = Number(args?.sessionId ?? 0);
    if (!mockTerms.has(sessionId)) {
      throw {
        protocol: PROTOCOL,
        code: "TERM_SESSION_NOT_FOUND",
        message: `终端会话不存在或已退出: ${sessionId}`,
        retryable: false,
      };
    }
    return { accepted: true };
  }
  if (command === cmd.TERM_CLOSE) {
    mockTerms.delete(Number(args?.sessionId ?? 0));
    return { accepted: true };
  }

  throw {
    protocol: PROTOCOL,
    code: "FEATURE_SOON",
    message: `${command} 尚未接到引擎（脚手架阶段）`,
    retryable: false,
  };
}
