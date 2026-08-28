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
};

// ---------------------------------------------------------------------------
// Runtime DTOs — mirror `crates/supertask-core/src/engine.rs`
// ---------------------------------------------------------------------------

export type RtState = "stopped" | "starting" | "running" | "unhealthy" | "stopping" | "exited";
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
  payload: { reason: string; services: Record<string, ServiceRuntimeView>; script: ScriptRuntimeView | null };
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

