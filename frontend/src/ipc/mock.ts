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
  ScriptRuntimeView,
  ServiceRuntimeView,
  ForeignService,
  SuperTaskFile,
  TemplateSummary,
  ToolchainProbe,
  WorkspaceOpenOut,
  YamlView,
  YamlSaveOut,
  RtState,
  ScriptState,
} from "./protocol";
import { PROTOCOL } from "./protocol";

// ---------------------------------------------------------------------------
// In-memory demo workspace so the UI is fully interactive in a plain browser
// (vite) without Tauri. Mirrors a Spring Boot + Node stack like the
// knife4j-demo-openapi3 project the integration tests target.
// ---------------------------------------------------------------------------

const DEMO_ROOT = "<knife4j-root>/knife4j/knife4j-demo-openapi3";

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

const state = {
  opened: false,
  spec: demoSpec(),
  services: {} as Record<string, ServiceRT>,
  script: { id: "build", state: "idle" as ScriptState, pid: null, last_exit: null, last_error: null } as ScriptRuntimeView,
  logSeq: 0,
  logs: [] as LogLine[],
  /** git.pull 成功后 ahead/behind 归零（确定性状态机） */
  gitPulled: false,
  /** 发现列表：killProcess 后移除对应进程（确定性状态机） */
  discover: null as ForeignService[] | null,
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
    const running = id === "gateway" || id === "auth-service";
    state.services[id] = {
      id,
      state: running ? "running" : "stopped",
      pid: running ? 1000 + Object.keys(state.services).length : null,
      port: s.port ?? null,
      kind: s.kind,
      health: running ? { ok: true, at_ms: Date.now(), detail: "200 OK" } : null,
      started_at_ms: running ? Date.now() - 120000 : null,
      last_exit: null,
      last_error: null,
      log_seq: 0,
    };
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
    script: { ...state.script },
  };
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
    "st.operation",
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
// Mock 内置模板（与 crates/supertask-core/src/template.rs 的 manifest 概览一致）
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
  },
];

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

/** Browser / `vite` without WebView: same shapes as Tauri, no real spawn. */
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
        { id: "docker", path: "/docker", status: "soon", since: "1.3" },
        { id: "gateway", path: "/gateway", status: "soon", since: "1.6" },
        { id: "cloud", path: "/cloud", status: "soon", since: "2.0" },
        { id: "ai", path: "/ai", status: "soon", since: "2.1" },
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
        node: { found: true, version: "22.4.0", path: "/usr/local/bin/node" },
        npm: { found: true, version: "10.7.0", path: "/usr/local/bin/npm" },
        pnpm: emptyProbe,
        yarn: emptyProbe,
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

  if (command === "toolchain.probe") {
    const probe: ToolchainProbe = {
      java: { found: true, version: "17.0.10", path: "/usr/lib/jvm/java-17" },
      maven: { found: true, version: "3.9.6", path: "/opt/maven" },
      node: { found: true, version: "22.4.0", path: "/usr/local/bin/node" },
      npm: { found: true, version: "10.7.0", path: "/usr/local/bin/npm" },
      pnpm: emptyProbe,
      yarn: emptyProbe,
    };
    return probe;
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

  if (command === "runtime.startOne") {
    const id = args?.id as string;
    const s = state.services[id];
    if (s) {
      s.state = "running";
      s.pid = ++pidCounter;
      s.started_at_ms = Date.now();
      s.health = { ok: true, at_ms: Date.now(), detail: "200 OK" };
      pushLog({ kind: "service", id }, "stdout", `[mock] ${id} 已启动 (pid ${s.pid})`);
    }
    return { accepted: true, order: null };
  }

  if (command === "runtime.stopOne") {
    const id = args?.id as string;
    const s = state.services[id];
    if (s) {
      s.state = "stopped";
      s.pid = null;
      s.health = null;
      s.started_at_ms = null;
      pushLog({ kind: "service", id }, "stdout", `[mock] ${id} 已停止`);
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
      if (state.spec.services[s.id]?.enabled) {
        s.state = "running";
        s.pid = ++pidCounter;
        s.started_at_ms = Date.now();
        s.health = { ok: true, at_ms: Date.now(), detail: "200 OK" };
        order.push(s.id);
      }
    }
    return { accepted: true, order };
  }

  if (command === "runtime.restartOne") {
    const id = args?.id as string;
    const s = state.services[id];
    if (s) {
      s.state = "running";
      s.pid = ++pidCounter;
      s.started_at_ms = Date.now();
      s.health = { ok: true, at_ms: Date.now(), detail: "200 OK" };
      pushLog({ kind: "service", id }, "stdout", `[mock] ${id} 已重启`);
    }
    return { accepted: true, order: null };
  }

  if (command === "script.run") {
    state.script.state = "running";
    state.script.pid = ++pidCounter;
    pushLog({ kind: "script", id: "build" }, "stdout", "[mock] build 开始执行…");
    return { accepted: true, order: null };
  }

  if (command === "script.cancel") {
    state.script.state = "exited";
    state.script.last_exit = { code: 130, at_ms: Date.now() };
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

  if (command === "templates.create") {
    const templateId = args?.templateId as string;
    const parentPath = ((args?.parentPath as string) ?? "").trim();
    const dirName = ((args?.directoryName as string) ?? "").trim();
    if (!MOCK_TEMPLATES.some((t) => t.id === templateId)) {
      throw { protocol: PROTOCOL, code: "NOT_FOUND", message: `模板不存在: ${templateId}`, retryable: false };
    }
    if (!parentPath) {
      throw { protocol: PROTOCOL, code: "CWD_MISSING", message: "请选择父目录", retryable: false };
    }
    const invalid = invalidDirectoryName(dirName);
    if (invalid) {
      throw { protocol: PROTOCOL, code: "PATH_ESCAPE", message: `非法目录名 ${JSON.stringify(dirName)}: ${invalid}`, retryable: false };
    }
    const opId = `op-${++opSeq}`;
    const wsId = `${parentPath.replace(/[\\/]+$/, "")}\\${dirName}`;
    emitOperation("templates.create", opId, "queued", null, "排队中…", null, null);
    setTimeout(() => emitOperation("templates.create", opId, "running", 0.3, "正在复制模板文件…", null, null), 400);
    setTimeout(() => emitOperation("templates.create", opId, "running", 0.7, "正在写入 supertask.yaml…", null, null), 900);
    setTimeout(() => emitOperation("templates.create", opId, "succeeded", 1, "创建完成", null, { workspace_id: wsId }), 1400);
    return { operation_id: opId };
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

  throw {
    protocol: PROTOCOL,
    code: "FEATURE_SOON",
    message: `${command} 尚未接到引擎（脚手架阶段）`,
    retryable: false,
  };
}
