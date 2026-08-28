/** IPC names and error codes. Mirror `crates/supertask-core/src/ipc`. */

export const PROTOCOL = 1 as const;

export const cmd = {
  SESSION_HELLO: "session.hello",
  APP_LOAD: "app.load",
  APP_SAVE_PREFS: "app.savePrefs",
  WORKSPACE_ADD: "workspace.add",
  WORKSPACE_OPEN: "workspace.open",
  WORKSPACE_INIT: "workspace.init",
  WORKSPACE_CLOSE: "workspace.close",
  WORKSPACE_DETACH: "workspace.detach",
  WORKSPACE_FORGET: "workspace.forget",
  SYSTEM_DISCOVER: "system.discover",
  SYSTEM_KILL_PROCESS: "system.killProcess",
  WORKSPACE_SCAN_DRAFT: "workspace.scanDraft",
  WORKSPACE_OPEN_EXPLORER: "workspace.openExplorer",
  YAML_GET: "yaml.get",
  YAML_SAVE_TEXT: "yaml.saveText",
  YAML_SAVE_FORM: "yaml.saveForm",
  RUNTIME_SNAPSHOT: "runtime.snapshot",
  RUNTIME_START_ONE: "runtime.startOne",
  RUNTIME_START_ALL: "runtime.startAll",
  RUNTIME_STOP_ONE: "runtime.stopOne",
  RUNTIME_STOP_ALL: "runtime.stopAll",
  RUNTIME_RESTART_ONE: "runtime.restartOne",
  SCRIPT_RUN: "script.run",
  SCRIPT_CANCEL: "script.cancel",
  TOOLCHAIN_PROBE: "toolchain.probe",
  TOOLCHAIN_INSTALL: "toolchain.install",
  TOOLCHAIN_UPGRADE: "toolchain.upgrade",
  PORTS_INSPECT: "ports.inspect",
  PORTS_SUGGEST: "ports.suggest",
  PORTS_ASSIGN: "ports.assign",
  SECRETS_STATUS: "secrets.status",
  SECRETS_SET: "secrets.set",
  SECRETS_DELETE: "secrets.delete",
  SECRETS_VALIDATE: "secrets.validate",
  LOGS_SEARCH: "logs.search",
  LOGS_EXPORT: "logs.export",
  LOGS_RETENTION_RUN: "logs.retention.run",
  METRICS_SNAPSHOT: "metrics.snapshot",
  METRICS_SUBSCRIBE: "metrics.subscribe",
  METRICS_UNSUBSCRIBE: "metrics.unsubscribe",
  PROFILES_LIST: "profiles.list",
  PROFILES_ACTIVATE: "profiles.activate",
  RUNTIME_BUILD: "runtime.build",
  LOGS_SUBSCRIBE: "logs.subscribe",
  LOGS_UNSUBSCRIBE: "logs.unsubscribe",
  LOGS_SNAPSHOT: "logs.snapshot",
  LOGS_CLEAR_VIEW: "logs.clearView",
  TEMPLATES_LIST: "templates.list",
  TEMPLATES_CREATE: "templates.create",
  GIT_CLONE: "git.clone",
  GIT_STATUS: "git.status",
  GIT_PULL: "git.pull",
  WORKSPACE_OPEN_IDE: "workspace.openIde",
  WORKSPACE_SCAN_PREVIEW: "workspace.scanPreview",
  WORKSPACE_SCAN_APPLY: "workspace.scanApply",
  APP_IMPORT_RECENTS: "app.importRecents",
  APP_UPDATE_CHECK: "app.update.check",
  APP_UPDATE_INSTALL: "app.update.install",
} as const;

export const event = {
  RUNTIME: "st.runtime",
  LOGS: "st.logs",
  OPERATION: "st.operation",
} as const;

export type FeatureStatus = "live" | "preview" | "soon";

export type Feature = {
  id: string;
  path: string;
  status: FeatureStatus;
  since: string;
};

export type HelloOut = {
  protocol: number;
  engine: string;
  engine_version: string;
  product_version: string;
  os: string;
  features: Feature[];
};

export type ToolProbe = {
  found: boolean;
  version: string | null;
  path: string | null;
};

export type ToolchainProbe = {
  java: ToolProbe;
  maven: ToolProbe;
  node: ToolProbe;
  npm: ToolProbe;
  pnpm: ToolProbe;
  yarn: ToolProbe;
};

/** 1.2：provider 可用性（toolchain.probe 输出扩展字段）。 */
export type ManagerAvailability = {
  mise: boolean;
  winget: boolean;
};

/** `toolchain.probe` 输出：原有六工具探测 + managers（§13.1）。 */
export type ToolchainProbeOut = ToolchainProbe & { managers: ManagerAvailability };

/** `toolchain.install` / `toolchain.upgrade` 选项（§13.1：version/manager 缺省走后端默认）。 */
export type ToolchainInstallOpts = {
  version?: string | null;
  manager?: "auto" | "mise" | "winget" | null;
  /** true 时必须携带 baseHash，把版本写回工作区 toolchain。 */
  persist?: boolean;
  baseHash?: string | null;
};

/** 1.2 §5：端口检查/改端口（对应 core ipc::PortInspection 等）。 */
export type PortInspection = {
  id: string;
  port: number;
  in_use: boolean;
  pid: number | null;
  process_name: string | null;
  managed: boolean;
};

export type PortsInspectOut = { items: PortInspection[] };
export type PortsSuggestOut = { candidates: number[] };
export type PortsAssignOut = {
  operation_id: string | null;
  spec: unknown;
  hash: string;
  restart_required: boolean;
  notes: string[];
};

/** 1.2 §6：secrets——状态只含 key 名，绝不含值。 */
export type SecretKeyStatus = {
  key: string;
  source: string;
  present: boolean;
  parse_ok: boolean | null;
  git_tracked: boolean | null;
};
export type SecretsStatusOut = {
  backend: string;
  file: string | null;
  keys: SecretKeyStatus[];
  git_ignored: boolean;
};
export type SecretsValidateOut = {
  ok: boolean;
  missing: string[];
  warnings: string[];
};

/** 1.2 §8：日志历史搜索（literal）。 */
export type LogSearchHit = {
  kind: string;
  id: string;
  file: string;
  line_no: number;
  text: string;
  ts: number | null;
};
export type LogsSearchResult = { items: LogSearchHit[]; truncated: boolean };

/** 1.2 §9：指标样本。 */
export type ServiceMetrics = {
  cpu_percent: number | null;
  memory_bytes: number | null;
  process_count: number | null;
  sampled_at_ms: number;
};
export type MetricsSnapshotOut = {
  services: Record<string, ServiceMetrics | null>;
};

/** 1.2 §10：profile。 */
export type ProfileSummary = { id: string; enabled_count: number | null };
export type ProfilesListOut = { active: string; profiles: ProfileSummary[] };
export type ProfilesActivateOut = { spec: unknown; hash: string; active: string };

/** st.metrics 事件信封负载。 */
export type MetricsEventPayload = {
  protocol: number;
  event: "st.metrics";
  workspace_id: string;
  ts_ms: number;
  payload: { services: Record<string, ServiceMetrics | null> };
};

/** 安装/升级 operation 成功终态的 result 负载。 */
export type ToolchainOpResult = {
  tool: string;
  version: string;
  manager: "mise" | "winget";
  path: string;
  /** persist=true 时返回写回后的新 hash。 */
  hash?: string;
};

/** app.load 的 prefs（除本类型外其余 DTO 均为 snake_case）。 */
export type Prefs = {
  theme: string;
  restoreLast: boolean;
  closeToTray: boolean;
  startOnLogin: boolean;
  updateCheck: boolean;
};

export type AppLoadOut = {
  protocol: number;
  prefs: Prefs;
  recents: string[];
  probe: ToolchainProbe;
  /** 本机曾打开但磁盘上已不存在的工作区路径 */
  stale: string[];
};

export type IpcError = {
  protocol: number;
  code: string;
  message: string;
  retryable: boolean;
};

export class IpcFailure extends Error {
  readonly code: string;
  readonly retryable: boolean;

  constructor(err: IpcError) {
    super(err.message);
    this.name = "IpcFailure";
    this.code = err.code;
    this.retryable = err.retryable;
  }
}

export function isIpcError(v: unknown): v is IpcError {
  return (
    typeof v === "object" &&
    v !== null &&
    "code" in v &&
    "message" in v &&
    typeof (v as IpcError).code === "string"
  );
}

// ---------------------------------------------------------------------------
// Spec DTOs — mirror `crates/supertask-core/src/spec/file.rs` (serde output)
// ---------------------------------------------------------------------------

export type HealthType = "none" | "tcp" | "http";
export type PackageManager = "npm" | "pnpm" | "yarn";

export type HealthSpec = {
  type: HealthType;
  http?: string | null;
  interval_secs: number;
  timeout_secs: number;
};

export type LoggingSpec = {
  max_bytes?: number | null;
  ring_lines?: number | null;
  retain_tail_bytes?: number | null;
};

export type ServiceSpec = {
  kind: string;
  /** 1.3 kind: compose：compose 文件内的服务名（非 SuperTask id）。 */
  service?: string | null;
  enabled: boolean;
  group?: string | null;
  labels: Record<string, string>;
  port?: number | null;
  ports: number[];
  env: Record<string, string>;
  env_file: string[];
  depends_on: string[];
  grace_secs?: number | null;
  health?: HealthSpec | null;
  restart?: string | null;
  extra_args: string[];
  cwd?: string | null;
  launch?: string | null;
  module?: string | null;
  jvm_args: string[];
  dir?: string | null;
  package_manager?: PackageManager | null;
  script?: string | null;
  logging?: LoggingSpec | null;
};

export type ScriptSpec = {
  desc?: string | null;
  cmds: string[];
  cwd?: string | null;
  env: Record<string, string>;
  timeout_secs?: number | null;
  depends_on: string[];
};

/** 1.2 顶层 toolchain 段（typed，工作区版本要求）。 */
export type ToolchainSpec = {
  manager?: "auto" | "mise" | "winget" | null;
  java?: string | null;
  maven?: string | null;
  node?: string | null;
  package_manager?: PackageManager | null;
};

/** 1.3 `docker.builds` 条目。context/dockerfile 相对 root。 */
export type DockerBuild = {
  name: string;
  context: string;
  dockerfile?: string | null;
  tags: string[];
};

/** 1.3 顶层 `docker` 段（typed）。compose 文件是容器行为唯一真源。 */
export type DockerSpec = {
  compose_file?: string | null;
  project_name?: string | null;
  builds: DockerBuild[];
};

export type SuperTaskFile = {
  version: number;
  kind?: string | null;
  name?: string | null;
  description?: string | null;
  root: string;
  env: Record<string, string>;
  services: Record<string, ServiceSpec>;
  scripts: Record<string, ScriptSpec>;
  logging?: LoggingSpec | null;
  toolchain?: ToolchainSpec | null;
  docker?: DockerSpec | null;
  secrets?: { backend?: string | null; file?: string | null; required?: string[] } | null;
  profiles?: {
    active?: string | null;
    items?: Record<string, unknown>;
  } | null;
};

// ---------------------------------------------------------------------------
// Runtime DTOs — mirror `crates/supertask-core/src/engine.rs`
// ---------------------------------------------------------------------------

export type RtState =
  | "stopped"
  | "building"
  | "starting"
  | "running"
  | "unhealthy"
  | "stopping"
  | "exited";
export type ScriptState = "idle" | "running" | "exited";

export type HealthView = { ok: boolean; at_ms: number; detail: string };
export type ExitView = { code: number; at_ms: number };

export type ServiceRuntimeView = {
  id: string;
  state: RtState;
  pid?: number | null;
  port?: number | null;
  kind: string;
  health?: HealthView | null;
  started_at_ms?: number | null;
  last_exit?: ExitView | null;
  exit_reason?: string | null;
  last_error?: string | null;
  log_seq: number;
  /** false = 外部进程（端口识别，仅监控；停止走 taskkill） */
  managed?: boolean;
};

export type ForeignService = {
  pid: number;
  name: string;
  /** 运行时归类：java / node / python / deno / bun / other */
  kind: string;
  ports: number[];
  /** 进程工作目录；读取失败为 null */
  cwd: string | null;
  /** 完整命令行；读取失败为 null */
  cmd_line: string | null;
  /** CPU 占用%（整机口径）。首次采样无差值 / 读取失败为 null */
  cpu_percent: number | null;
  /** 物理内存占用（工作集，字节）；读取失败为 null */
  memory_bytes: number | null;
};

export type ScriptRuntimeView = {
  id: string;
  state: ScriptState;
  pid?: number | null;
  last_exit?: ExitView | null;
  last_error?: string | null;
};

export type RuntimeSnapshot = {
  protocol: number;
  workspace_id: string;
  services: Record<string, ServiceRuntimeView>;
  script?: ScriptRuntimeView | null;
  /** 1.2：最近一次 Job 指标快照；未订阅或无 Job 时为空。 */
  metrics?: Record<string, ServiceMetrics | null>;
};

// ---------------------------------------------------------------------------
// Logs DTOs — mirror `crates/supertask-core/src/log/ring.rs`
// ---------------------------------------------------------------------------

export type LogSourceKind = "service" | "script" | "system";
export type LogStream = "stdout" | "stderr" | "system";
export type LogSource = { kind: LogSourceKind; id: string };
export type LogLine = {
  seq: number;
  source: LogSource;
  stream: LogStream;
  ts_ms: number;
  text: string;
};

// ---------------------------------------------------------------------------
// Command I/O shapes
// ---------------------------------------------------------------------------

export type WorkspaceOpenOut = {
  workspace_id: string;
  spec: SuperTaskFile;
  warnings: string[];
};

export type YamlView = { text: string; spec: SuperTaskFile; hash: string };
export type YamlSaveOut = { spec: SuperTaskFile; hash: string; warnings: string[] };
export type Accepted = { accepted: boolean; order?: string[] };
export type LogSubOut = { ok: boolean; cursor: { next_seq: number } };
export type LogSnapshotOut = { items: LogLine[]; next_seq: number };

// Event payloads emitted by the engine bridge.
export type RuntimeEventPayload = {
  protocol: number;
  event: "st.runtime";
  workspace_id: string;
  ts_ms: number;
  payload: {
    reason: string;
    services: Record<string, ServiceRuntimeView>;
    script: ScriptRuntimeView | null;
    metrics?: Record<string, ServiceMetrics | null>;
  };
};

export type LogsEventPayload = {
  protocol: number;
  event: "st.logs";
  workspace_id: string;
  ts_ms: number;
  payload: { items: LogLine[] };
};

// ---------------------------------------------------------------------------
// 1.1 DTOs — mirror `src-tauri/src/commands.rs` + `crates/supertask-core`
// (templates / git / scan merge / operation / update)
// ---------------------------------------------------------------------------

export type TemplateSummary = {
  id: string;
  version: string;
  name: string;
  description: string;
  stacks: string[];
  /** 模板内相对路径概览（`/` 分隔），仅展示 */
  files: string[];
};

export type TemplatesListOut = { templates: TemplateSummary[] };

/** 全 snake_case（serde 默认），mirror `crates/supertask-core/src/git.rs`。 */
export type GitStatus = {
  workspace_id: string;
  is_repository: boolean;
  branch: string | null;
  detached: boolean;
  dirty: boolean;
  ahead: number;
  behind: number;
  staged: number;
  unstaged: number;
  untracked: number;
  remote: string | null;
};

export type OpState = "queued" | "running" | "succeeded" | "failed";

export type OperationEventPayload = {
  protocol: number;
  event: "st.operation";
  workspace_id: string | null;
  ts_ms: number;
  payload: {
    operation_id: string;
    kind: string;
    state: OpState;
    progress: number | null;
    message: string | null;
    error_code: string | null;
    result: unknown;
  };
};

export type ScanMergeStatus = "added" | "match_same" | "match_diff" | "missing" | "id_conflict";

export type ScanMergeItem = {
  service_id: string;
  status: ScanMergeStatus;
  discovered: ServiceSpec | null;
  current: ServiceSpec | null;
  field_diffs: string[];
  candidate_id: string | null;
  selected: boolean;
};

export type ScanPreviewOut = { items: ScanMergeItem[]; warnings: string[] };

export type MergeAction = "add" | "keep" | "update";
export type MergeChoice = { id: string; action: MergeAction; fields?: string[] };

export type IdeTarget = "explorer" | "cursor" | "idea" | "code";
export type OpenIdeOut = { accepted: boolean; ide: string; path: string };

export type OperationIdOut = { operation_id: string };

export type UpdateCheckResult = {
  status: "up_to_date" | "available";
  version?: string;
  notes?: string | null;
  date?: string | null;
};

