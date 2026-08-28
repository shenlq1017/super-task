import { invoke } from "./invoke";
import {
  cmd,
  type AppLoadOut,
  type GitStatus,
  type HelloOut,
  type IdeTarget,
  type LogLine,
  type LogSnapshotOut,
  type MergeChoice,
  type OpenIdeOut,
  type OperationIdOut,
  type Prefs,
  type RuntimeSnapshot,
  type ScanPreviewOut,
  type ScriptRuntimeView,
  type ServiceRuntimeView,
  type SuperTaskFile,
  type TemplatesListOut,
  type ToolchainProbe,
  type WorkspaceOpenOut,
  type YamlSaveOut,
  type YamlView,
  type Accepted,
  type ForeignService,
  type LogSource,
} from "./protocol";

// ---------------------------------------------------------------------------
// Session / app bootstrap
// ---------------------------------------------------------------------------

export const apiHello = (protocol: number) =>
  invoke<HelloOut>(cmd.SESSION_HELLO, { client: "ui", protocol });

export const apiAppLoad = () => invoke<AppLoadOut>(cmd.APP_LOAD);

export const apiSavePrefs = (prefs: Partial<Prefs>) =>
  invoke<{ ok: boolean }>(cmd.APP_SAVE_PREFS, prefs);

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

export const apiWorkspaceAdd = (path: string) =>
  invoke<WorkspaceOpenOut>(cmd.WORKSPACE_ADD, { path });

export const apiWorkspaceOpen = (path: string) =>
  invoke<WorkspaceOpenOut>(cmd.WORKSPACE_OPEN, { path });

export const apiWorkspaceScanDraft = (path: string) =>
  invoke<WorkspaceOpenOut>(cmd.WORKSPACE_SCAN_DRAFT, { path });

export const apiWorkspaceInit = (path: string, spec: SuperTaskFile) =>
  invoke<WorkspaceOpenOut>(cmd.WORKSPACE_INIT, { path, spec });

export const apiWorkspaceForget = (id: string) =>
  invoke<{ ok: boolean }>(cmd.WORKSPACE_FORGET, { id });

// 注意：Rust 端 workspace.close 要求 workspace_id 参数（仅占位也必须传），
// 否则 Tauri 抛 invalid args 且会被上层 catch 吞掉，导致引擎实际未关闭。
export const apiWorkspaceClose = (workspaceId?: string) =>
  invoke<{ ok: boolean }>(cmd.WORKSPACE_CLOSE, { workspaceId: workspaceId ?? "" });

/// 切换工作区专用：不停进程，活服务移交后台（重开同根工作区时接管）。
export const apiWorkspaceDetach = () => invoke<{ ok: boolean }>(cmd.WORKSPACE_DETACH);

export const apiSystemDiscover = () => invoke<ForeignService[]>(cmd.SYSTEM_DISCOVER);

/// 终止发现列表中的监听进程（core 侧护栏：pid≤4 / 自身 / 非监听 pid 拒绝）。
export const apiSystemKillProcess = (pid: number) =>
  invoke<{ ok: boolean }>(cmd.SYSTEM_KILL_PROCESS, { pid });

export const apiOpenExplorer = (workspaceId: string, rel?: string) =>
  invoke<{ ok: boolean }>(cmd.WORKSPACE_OPEN_EXPLORER, {
    workspaceId,
    rel: rel ?? null,
  });

// ---------------------------------------------------------------------------
// YAML
// ---------------------------------------------------------------------------

export const apiYamlGet = () => invoke<YamlView>(cmd.YAML_GET, {});

export const apiYamlSaveText = (text: string, baseHash: string) =>
  invoke<YamlSaveOut>(cmd.YAML_SAVE_TEXT, {
    workspaceId: "",
    text,
    baseHash,
  });

export const apiYamlSaveForm = (spec: SuperTaskFile, baseHash: string) =>
  invoke<YamlSaveOut>(cmd.YAML_SAVE_FORM, {
    workspaceId: "",
    spec,
    baseHash,
  });

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

export const apiRuntimeSnapshot = () =>
  invoke<RuntimeSnapshot>(cmd.RUNTIME_SNAPSHOT, {});

export const apiStartOne = (id: string) =>
  invoke<Accepted>(cmd.RUNTIME_START_ONE, { workspaceId: "", id });

export const apiStartAll = () =>
  invoke<Accepted>(cmd.RUNTIME_START_ALL, { workspaceId: "" });

export const apiStopOne = (id: string) =>
  invoke<Accepted>(cmd.RUNTIME_STOP_ONE, { workspaceId: "", id });

export const apiStopAll = () =>
  invoke<Accepted>(cmd.RUNTIME_STOP_ALL, { workspaceId: "" });

export const apiRestartOne = (id: string) =>
  invoke<Accepted>(cmd.RUNTIME_RESTART_ONE, { workspaceId: "", id });

// ---------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------

export const apiScriptRun = (id: string) =>
  invoke<Accepted>(cmd.SCRIPT_RUN, { workspaceId: "", id });

export const apiScriptCancel = (id: string) =>
  invoke<Accepted>(cmd.SCRIPT_CANCEL, { workspaceId: "", id });

// ---------------------------------------------------------------------------
// Toolchain
// ---------------------------------------------------------------------------

export const apiToolchainProbe = () => invoke<ToolchainProbe>(cmd.TOOLCHAIN_PROBE, {});

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

export const apiLogsSnapshot = (source: LogSource | null, limit = 2000) =>
  invoke<LogSnapshotOut>(cmd.LOGS_SNAPSHOT, {
    source,
    limit,
  });

export const apiLogsClearView = (source: LogSource) =>
  invoke<{ ok: boolean }>(cmd.LOGS_CLEAR_VIEW, { source });

// ---------------------------------------------------------------------------
// Templates（1.1，ipc.md §10.1）
// ---------------------------------------------------------------------------

export const apiTemplatesList = () => invoke<TemplatesListOut>(cmd.TEMPLATES_LIST, {});

export const apiTemplatesCreate = (templateId: string, parentPath: string, directoryName: string) =>
  invoke<OperationIdOut>(cmd.TEMPLATES_CREATE, {
    templateId,
    parentPath,
    directoryName,
  });

// ---------------------------------------------------------------------------
// Git（1.1，ipc.md §10.2）
// ---------------------------------------------------------------------------

export const apiGitClone = (url: string, targetPath: string, branch?: string | null) =>
  invoke<OperationIdOut>(cmd.GIT_CLONE, {
    url,
    targetPath,
    branch: branch || null,
  });

export const apiGitStatus = (workspaceId: string) =>
  invoke<GitStatus>(cmd.GIT_STATUS, { workspaceId });

export const apiGitPull = (
  workspaceId: string,
  opts?: { remote?: string | null; branch?: string | null; allowDirty?: boolean },
) =>
  invoke<OperationIdOut>(cmd.GIT_PULL, {
    workspaceId,
    remote: opts?.remote ?? null,
    branch: opts?.branch ?? null,
    allowDirty: opts?.allowDirty ?? null,
  });

// ---------------------------------------------------------------------------
// IDE / 扫描合并（1.1，ipc.md §10.3–10.4）
// ---------------------------------------------------------------------------

export const apiOpenIde = (workspaceId: string, ide: IdeTarget) =>
  invoke<OpenIdeOut>(cmd.WORKSPACE_OPEN_IDE, { workspaceId, ide });

export const apiScanPreview = (workspaceId: string) =>
  invoke<ScanPreviewOut>(cmd.WORKSPACE_SCAN_PREVIEW, { workspaceId });

export const apiScanApply = (workspaceId: string, choices: MergeChoice[], baseHash: string) =>
  invoke<YamlSaveOut>(cmd.WORKSPACE_SCAN_APPLY, { workspaceId, choices, baseHash });

// ---------------------------------------------------------------------------
// 应用数据 / 更新（1.1，ipc.md §10.5–10.6）
// ---------------------------------------------------------------------------

export const apiImportRecents = (recents: string[], last?: string | null) =>
  invoke<{ ok: boolean }>(cmd.APP_IMPORT_RECENTS, { recents, last: last ?? null });

export const apiUpdateCheck = () => invoke<OperationIdOut>(cmd.APP_UPDATE_CHECK, {});

export const apiUpdateInstall = (version: string) =>
  invoke<OperationIdOut>(cmd.APP_UPDATE_INSTALL, { version });

// ---------------------------------------------------------------------------
// Re-exports so callers can construct arg shapes without re-importing protocol
// ---------------------------------------------------------------------------

export type { LogLine, ServiceRuntimeView, ScriptRuntimeView, RuntimeSnapshot };
