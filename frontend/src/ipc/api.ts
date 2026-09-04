import { invoke, isTauri } from "./invoke";
import {
  cmd,
  event,
  type AppLoadOut,
  type GitStatus,
  type HelloOut,
  type IdeTarget,
  type LogLine,
  type LogSnapshotOut,
  type MergeChoice,
  type OpenIdeOut,
  type OperationIdOut,
  type ReadmePreviewOut,
  type Prefs,
  type RuntimeSnapshot,
  type ScanPreviewOut,
  type ScriptRuntimeView,
  type ServiceRuntimeView,
  type SuperTaskFile,
  type TemplatesListOut,
  type WorkspaceExportPackageOut,
  type WorkspaceImportPackageOut,
  type TemplatesPreviewOut,
  type TemplateSource,
  type ToolchainInstallOpts,
  type ToolchainProbeOut,
  type ToolchainVersionsOut,
  type TaskfilePreviewOut,
  type WorkspaceOpenOut,
  type YamlSaveOut,
  type YamlView,
  type Accepted,
  type ForeignService,
  type LogSource,
  type DockerProbe,
  type DockerPsOut,
  type DockerImagesOut,
  type PortsInspectOut,
  type EnvEffectiveOut,
  type SpringConfigOut,
  type PortsSuggestOut,
  type PortsAssignOut,
  type SecretsStatusOut,
  type SecretsValidateOut,
  type HostMetrics,
  type TempMode,
  type MetricsSnapshotOut,
  type ProfilesListOut,
  type ProfilesActivateOut,
  type GatewayConf,
  type GatewayStatusOut,
  type GatewayPreviewOut,
  type GatewayValidateOut,
  type GatewayApplyOut,
  type CloudStatusOut,
  type CloudLoginOut,
  type CloudSyncOut,
  type CloudResolveChoice,
  type CloudResolveOut,
  type CloudMigratePlanOut,
  type CloudMigrateApplyOut,
  type CloudTelemetryOut,
  type CloudEndpointSetOut,
  type AiTask,
  type AiCliProbeOut,
  type AiConfigOut,
  type AiConfigSaveIn,
  type AiStatusOut,
  type AiCompleteOut,
  type AiStreamEnvelope,
  type AiTemplate,
  type AiTemplateSaveIn,
  type TermOpenOut,
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

/** 工具链探测（后端会话内 TTL 缓存；refresh=true 强制重探，「重新探测」按钮用）。 */
export const apiToolchainProbe = (refresh = false) =>
  invoke<ToolchainProbeOut>(cmd.TOOLCHAIN_PROBE, { refresh });

/** 每工具可选版本列表（后端白名单 ∪ mise ls-remote；/env 版本下拉数据源）。 */
export const apiToolchainVersions = () =>
  invoke<ToolchainVersionsOut>(cmd.TOOLCHAIN_VERSIONS, {});

/** 安装缺失工具链（长操作，返回 operation_id；§13.1）。persist 需带 baseHash。 */
export const apiToolchainInstall = (tool: string, opts: ToolchainInstallOpts = {}) =>
  invoke<OperationIdOut>(cmd.TOOLCHAIN_INSTALL, {
    tool,
    version: opts.version ?? null,
    manager: opts.manager ?? null,
    persist: opts.persist ?? false,
    baseHash: opts.baseHash ?? null,
  });

/** 升级工具链（长操作，返回 operation_id；§13.1）。 */
export const apiToolchainUpgrade = (tool: string, opts: ToolchainInstallOpts = {}) =>
  invoke<OperationIdOut>(cmd.TOOLCHAIN_UPGRADE, {
    tool,
    version: opts.version ?? null,
    manager: opts.manager ?? null,
    persist: opts.persist ?? false,
    baseHash: opts.baseHash ?? null,
  });

/** 1.2 §5–§10：端口 / secrets / 日志历史 / 指标 / profile / build。 */
export const apiPortsInspect = (workspaceId: string, id: string, port?: number) =>
  invoke<PortsInspectOut>(cmd.PORTS_INSPECT, { workspaceId, id, ...(port != null ? { port } : {}) });

/** env.effective：服务最近一次启动实际注入的生效环境快照。 */
export const apiEnvEffective = (workspaceId: string, id: string) =>
  invoke<EnvEffectiveOut>(cmd.ENV_EFFECTIVE, { workspaceId, id });

/** spring.inspect：spring-boot 服务的项目自身配置静态解析（只读）。 */
export const apiSpringInspect = (workspaceId: string, id: string) =>
  invoke<SpringConfigOut>(cmd.SPRING_INSPECT, { workspaceId, id });

export const apiPortsSuggest = (workspaceId: string, id: string) =>
  invoke<PortsSuggestOut>(cmd.PORTS_SUGGEST, { workspaceId, id });

export const apiPortsAssign = (
  workspaceId: string,
  id: string,
  port: number,
  baseHash: string,
  restart?: boolean,
) =>
  invoke<PortsAssignOut>(cmd.PORTS_ASSIGN, {
    workspaceId,
    id,
    port,
    baseHash,
    restart: restart ?? false,
  });

export const apiSecretsStatus = (workspaceId: string) =>
  invoke<SecretsStatusOut>(cmd.SECRETS_STATUS, { workspaceId });

export const apiSecretsSet = (workspaceId: string, key: string, value: string) =>
  invoke<{ ok: boolean; key: string }>(cmd.SECRETS_SET, { workspaceId, key, value });

export const apiSecretsDelete = (workspaceId: string, key: string) =>
  invoke<{ ok: boolean; key: string }>(cmd.SECRETS_DELETE, { workspaceId, key });

export const apiSecretsValidate = (workspaceId: string, id?: string) =>
  invoke<SecretsValidateOut>(cmd.SECRETS_VALIDATE, { workspaceId, id: id ?? null });

export const apiLogsSearch = (
  workspaceId: string,
  query: string,
  opts?: { source?: LogSource | null; caseSensitive?: boolean; limit?: number },
) =>
  invoke<OperationIdOut>(cmd.LOGS_SEARCH, {
    workspaceId,
    source: opts?.source ?? null,
    query,
    caseSensitive: opts?.caseSensitive ?? false,
    limit: opts?.limit ?? null,
  });

export const apiLogsExport = (
  workspaceId: string,
  format: "text" | "jsonl",
  destinationPath: string,
  opts?: { source?: LogSource | null; query?: string | null; caseSensitive?: boolean },
) =>
  invoke<OperationIdOut>(cmd.LOGS_EXPORT, {
    workspaceId,
    source: opts?.source ?? null,
    query: opts?.query ?? null,
    caseSensitive: opts?.caseSensitive ?? false,
    format,
    destinationPath,
  });

export const apiLogsRetentionRun = (workspaceId: string) =>
  invoke<OperationIdOut>(cmd.LOGS_RETENTION_RUN, { workspaceId });

export const apiMetricsSnapshot = (workspaceId: string) =>
  invoke<MetricsSnapshotOut>(cmd.METRICS_SNAPSHOT, { workspaceId });

export const apiMetricsSubscribe = (workspaceId: string) =>
  invoke<{ ok: boolean }>(cmd.METRICS_SUBSCRIBE, { workspaceId });

export const apiMetricsUnsubscribe = (workspaceId: string) =>
  invoke<{ ok: boolean }>(cmd.METRICS_UNSUBSCRIBE, { workspaceId });

export const apiSystemMetrics = (temp: TempMode = "auto") =>
  invoke<HostMetrics>(cmd.SYSTEM_METRICS, { temp });

export const apiProfilesList = (workspaceId: string) =>
  invoke<ProfilesListOut>(cmd.PROFILES_LIST, { workspaceId });

export const apiProfilesActivate = (workspaceId: string, id: string, baseHash: string) =>
  invoke<ProfilesActivateOut>(cmd.PROFILES_ACTIVATE, { workspaceId, id, baseHash });

export const apiRuntimeBuild = (workspaceId: string, id: string) =>
  invoke<OperationIdOut>(cmd.RUNTIME_BUILD, { workspaceId, id });

// ---------------------------------------------------------------------------
// Docker（1.3，feature spec §9；compose 服务起停复用 runtime.startOne/stopOne）
// ---------------------------------------------------------------------------

/** 探测 docker CLI 与 daemon（结果会话内缓存；refresh=true 强制重探）。 */
export const apiDockerProbe = (refresh = false) =>
  invoke<DockerProbe>(cmd.DOCKER_PROBE, { refresh });

/** 当前 compose project 的容器列表（只读；无 compose 文件则空）。 */
export const apiDockerPs = (workspaceId: string) =>
  invoke<DockerPsOut>(cmd.DOCKER_PS, { workspaceId });

/** 本机镜像列表（只读，1.3 不提供删除）。 */
export const apiDockerImages = () => invoke<DockerImagesOut>(cmd.DOCKER_IMAGES, {});

/** 触发 docker.builds 中已定义条目的镜像构建（长操作，返回 operation_id）。 */
export const apiDockerBuild = (workspaceId: string, name: string) =>
  invoke<OperationIdOut>(cmd.DOCKER_BUILD, { workspaceId, name });

/** 取消进行中的镜像构建（best effort：已提交的层缓存不回滚）。 */
export const apiDockerBuildCancel = (workspaceId: string, operationId: string) =>
  invoke<{ ok: boolean }>(cmd.DOCKER_BUILD_CANCEL, { workspaceId, operationId });

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

export type TemplatesCreateArgs = {
  templateId: string;
  parentPath: string;
  directoryName: string;
  source?: TemplateSource;
  params?: Record<string, string>;
  /** 组合模板：选中的块 id（缺省 = 全块，依赖自动闭合在后端） */
  blocks?: string[];
  /** 组合模板：服务 id → 端口 */
  ports?: Record<string, number>;
};

export const apiTemplatesCreate = (args: TemplatesCreateArgs) => {
  const { templateId, parentPath, directoryName, source, params, blocks, ports } = args;
  return invoke<OperationIdOut>(cmd.TEMPLATES_CREATE, {
    templateId,
    parentPath,
    directoryName,
    ...(source ? { source } : {}),
    ...(params && Object.keys(params).length > 0 ? { params } : {}),
    ...(blocks ? { blocks } : {}),
    ...(ports && Object.keys(ports).length > 0 ? { ports } : {}),
  });
};

/** 组合模板预览（纯计算，无副作用）。 */
export const apiTemplatesPreview = (args: {
  templateId: string;
  source?: TemplateSource;
  blocks?: string[];
  ports?: Record<string, number>;
  params?: Record<string, string>;
}) =>
  invoke<TemplatesPreviewOut>(cmd.TEMPLATES_PREVIEW, {
    templateId: args.templateId,
    ...(args.source ? { source: args.source } : {}),
    ...(args.blocks ? { blocks: args.blocks } : {}),
    ...(args.ports && Object.keys(args.ports).length > 0 ? { ports: args.ports } : {}),
    ...(args.params && Object.keys(args.params).length > 0 ? { params: args.params } : {}),
  });

// ---------------------------------------------------------------------------
// 导出包（1.5，ipc.md §10.9）
// ---------------------------------------------------------------------------

/** 导出当前工作区为离线迁移包（zip；只读，不取锁）。 */
export const apiWorkspaceExportPackage = (args: {
  workspaceId: string;
  destPath: string;
  withSecrets: boolean;
}) =>
  invoke<WorkspaceExportPackageOut>(cmd.WORKSPACE_EXPORT_PACKAGE, {
    workspaceId: args.workspaceId,
    destPath: args.destPath,
    withSecrets: args.withSecrets,
  });

/** 导入导出包（只落盘；成功后用返回的 root 打开工作区）。 */
export const apiWorkspaceImportPackage = (args: { pkgPath: string; destDir: string }) =>
  invoke<WorkspaceImportPackageOut>(cmd.WORKSPACE_IMPORT_PACKAGE, {
    pkgPath: args.pkgPath,
    destDir: args.destDir,
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

/** 2.1 README 导入预览（ipc.md §10.13；缺失/未发现时 warnings 给人话提示）。 */
export const apiImportReadme = (workspaceId: string, path?: string) =>
  invoke<ReadmePreviewOut>(cmd.IMPORT_README, { workspaceId, path: path ?? null });

/** 2.1 README 导入应用：走 saveForm 机制（base_hash 冲突 → YAML_CONFLICT）。 */
export const apiImportReadmeApply = (
  workspaceId: string,
  path: string | null,
  choices: MergeChoice[],
  baseHash: string,
) => invoke<YamlSaveOut>(cmd.IMPORT_README_APPLY, { workspaceId, path, choices, baseHash });

// ---------------------------------------------------------------------------
// Taskfile 导入（1.4，ipc.md §10.8）
// ---------------------------------------------------------------------------

/** Taskfile v3 导入预览（纯内存计算；缺失 TASKFILE_NOT_FOUND / 版本不支持 TASKFILE_INVALID）。 */
export const apiTaskfilePreview = (workspaceId: string) =>
  invoke<TaskfilePreviewOut>(cmd.IMPORT_TASKFILE_PREVIEW, { workspaceId });

/** 应用所选任务；只增改所选 scripts.*，base_hash 冲突 → YAML_CONFLICT。 */
export const apiTaskfileApply = (workspaceId: string, selected: string[], baseHash: string) =>
  invoke<YamlSaveOut>(cmd.IMPORT_TASKFILE_APPLY, { workspaceId, selected, baseHash });

// ---------------------------------------------------------------------------
// 网关（1.6，ipc.md §10.10）
// ---------------------------------------------------------------------------

export const apiGatewayStatus = (workspaceId: string) =>
  invoke<GatewayStatusOut>(cmd.GATEWAY_STATUS, { workspaceId });

/** 纯内存渲染草稿（gateway 缺省用当前 yaml）；不落盘不校验。 */
export const apiGatewayPreview = (workspaceId: string, gateway?: GatewayConf | null) =>
  invoke<GatewayPreviewOut>(cmd.GATEWAY_PREVIEW, { workspaceId, gateway: gateway ?? null });

/** 静态校验 + 二进制探测 + spawn 本机校验；失败以 ok=false 返回（非 IPC 错误）。 */
export const apiGatewayValidate = (workspaceId: string, gateway?: GatewayConf | null) =>
  invoke<GatewayValidateOut>(cmd.GATEWAY_VALIDATE, { workspaceId, gateway: gateway ?? null });

/** 写 yaml（base_hash 冲突 → YAML_CONFLICT）+ 重新生成 + 运行中则重启。 */
export const apiGatewayApply = (workspaceId: string, gateway: GatewayConf, baseHash: string) =>
  invoke<GatewayApplyOut>(cmd.GATEWAY_APPLY, { workspaceId, gateway, baseHash });

export const apiGatewayStart = (workspaceId: string) =>
  invoke<Accepted>(cmd.GATEWAY_START, { workspaceId });

export const apiGatewayStop = (workspaceId: string) =>
  invoke<Accepted>(cmd.GATEWAY_STOP, { workspaceId });

export const apiGatewayRestart = (workspaceId: string) =>
  invoke<Accepted>(cmd.GATEWAY_RESTART, { workspaceId });

/** 仅 kind: caddy；UI 必须先弹风险确认（修改系统信任库）。 */
export const apiGatewayTrust = (workspaceId: string) =>
  invoke<Accepted>(cmd.GATEWAY_TRUST, { workspaceId });

// ---------------------------------------------------------------------------
// 应用数据 / 更新（1.1，ipc.md §10.5–10.6）
// ---------------------------------------------------------------------------

export const apiImportRecents = (recents: string[], last?: string | null) =>
  invoke<{ ok: boolean }>(cmd.APP_IMPORT_RECENTS, { recents, last: last ?? null });

export const apiUpdateCheck = () => invoke<OperationIdOut>(cmd.APP_UPDATE_CHECK, {});

export const apiUpdateInstall = (version: string) =>
  invoke<OperationIdOut>(cmd.APP_UPDATE_INSTALL, { version });

// ---------------------------------------------------------------------------
// Cloud（2.0，typed wrappers；页面不得裸 invoke）
// ---------------------------------------------------------------------------

export const apiCloudStatus = () => invoke<CloudStatusOut>(cmd.CLOUD_STATUS, {});

/** Password is sent only to the IPC boundary and never returned to callers. */
export const apiCloudLogin = (email: string, password: string) =>
  invoke<CloudLoginOut>(cmd.CLOUD_LOGIN, { email, password });

export const apiCloudLogout = () => invoke<{ ok: boolean }>(cmd.CLOUD_LOGOUT, {});
export const apiCloudSync = () => invoke<CloudSyncOut>(cmd.CLOUD_SYNC, {});
export const apiCloudResolve = (entityId: string, choice: CloudResolveChoice) =>
  invoke<CloudResolveOut>(cmd.CLOUD_RESOLVE, { entity_id: entityId, choice });
export const apiCloudMigratePlan = () => invoke<CloudMigratePlanOut>(cmd.CLOUD_MIGRATE_PLAN, {});
export const apiCloudMigrateApply = (args: {
  workspaces: { entityId: string; dir: string }[];
  includeTemplates: boolean;
  includeSettings: boolean;
}) =>
  invoke<CloudMigrateApplyOut>(cmd.CLOUD_MIGRATE_APPLY, {
    workspaces: args.workspaces.map(({ entityId, dir }) => ({ entity_id: entityId, dir })),
    include_templates: args.includeTemplates,
    include_settings: args.includeSettings,
  });
export const apiCloudTelemetrySet = (enabled: boolean) =>
  invoke<CloudTelemetryOut>(cmd.CLOUD_TELEMETRY_SET, { enabled });

/** Persist and switch the endpoint through the backend when available. */
export const apiCloudSetEndpoint = (endpoint: string) => {
  const normalized = endpoint.trim().replace(/\/$/, "");
  if (!normalized) return Promise.reject(new Error("Endpoint is required"));
  return invoke<CloudEndpointSetOut>(cmd.CLOUD_ENDPOINT_SET, { endpoint: normalized });
};

// ---------------------------------------------------------------------------
// AI（2.1，ipc.md §10.13；key 经 IPC 写入 secrets 后端，绝不回显）
// ---------------------------------------------------------------------------

export const apiAiStatus = () => invoke<AiStatusOut>(cmd.AI_STATUS, {});

/** 新建/更新命名配置；input.apiKey：undefined 不动 / "" 清除 / 非空覆盖。 */
export const apiAiConfigSave = (input: AiConfigSaveIn) =>
  invoke<AiConfigOut>(cmd.AI_CONFIG_SAVE, { input });

export const apiAiConfigDelete = (id: string) =>
  invoke<{ ok: boolean }>(cmd.AI_CONFIG_DELETE, { id });

export const apiAiConfigDefault = (id: string) =>
  invoke<{ ok: boolean }>(cmd.AI_CONFIG_DEFAULT, { id });

/** 探测本机编码 CLI（--version），不保存配置。 */
export const apiAiCliProbe = (
  provider: string,
  cliPath?: string | null,
  cliEnv?: Record<string, string>,
) => invoke<AiCliProbeOut>(cmd.AI_CLI_PROBE, { provider, cliPath, cliEnv });

/** 全局自定义指令（trim；空串清除；≤8000 字符）。 */
export const apiAiInstructionsSave = (text: string) =>
  invoke<{ text: string }>(cmd.AI_INSTRUCTIONS_SAVE, { text });

export const apiAiTemplateSave = (input: AiTemplateSaveIn) =>
  invoke<AiTemplate>(cmd.AI_TEMPLATE_SAVE, { input });

export const apiAiTemplateDelete = (id: string) =>
  invoke<{ ok: boolean }>(cmd.AI_TEMPLATE_DELETE, { id });

/** OpenAI 兼容端点模型发现（GET /models）；configId 缺省用默认配置。 */
export const apiAiModels = (configId?: string) =>
  invoke<string[]>(cmd.AI_MODELS, { configId: configId ?? null });

/** 仅用户显式触发（零后台调用约定）；task ∈ explain_logs | config_suggest | enrich_draft | test_connection。 */
export const apiAiComplete = (
  task: AiTask,
  payload: unknown,
  configId?: string,
  requestId?: string,
) =>
  invoke<AiCompleteOut>(cmd.AI_COMPLETE, {
    task,
    payload,
    configId: configId ?? null,
    requestId: requestId ?? null,
  });

/** 订阅 `st-ai` 流式增量；返回取消函数。 */
export async function subscribeAiStream(
  requestId: string,
  onDelta: (delta: string) => void,
): Promise<() => void> {
  const handler = (envelope: AiStreamEnvelope) => {
    const p = envelope?.payload;
    if (!p || p.request_id !== requestId || !p.delta) return;
    onDelta(p.delta);
  };
  if (isTauri()) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen(event.AI, (e) => handler(e.payload as AiStreamEnvelope));
  }
  const { mockListen } = await import("./mock");
  return mockListen(event.AI, (envelope) => handler(envelope as AiStreamEnvelope));
}

// ---------------------------------------------------------------------------
// 终端（运行页 Tab；ipc.md §10.15；PTY 会话后端托管，前端只传语义参数）
// ---------------------------------------------------------------------------

/** 打开终端会话。serviceId 缺省 = 工作区根 + 工作区环境链。 */
export const apiTermOpen = (args: {
  workspaceId: string;
  serviceId?: string | null;
  cols?: number;
  rows?: number;
}) =>
  invoke<TermOpenOut>(cmd.TERM_OPEN, {
    workspaceId: args.workspaceId,
    serviceId: args.serviceId ?? null,
    cols: args.cols ?? 80,
    rows: args.rows ?? 24,
  });

/** 写入用户输入（xterm onData 原样透传，回车为 \r）。 */
export const apiTermWrite = (sessionId: number, data: string) =>
  invoke<{ accepted: boolean }>(cmd.TERM_WRITE, { sessionId, data });

export const apiTermResize = (sessionId: number, cols: number, rows: number) =>
  invoke<{ accepted: boolean }>(cmd.TERM_RESIZE, { sessionId, cols, rows });

/** 关闭会话（幂等；ConPTY 关闭即整树终止）。 */
export const apiTermClose = (sessionId: number) =>
  invoke<{ accepted: boolean }>(cmd.TERM_CLOSE, { sessionId });

// ---------------------------------------------------------------------------
// Re-exports so callers can construct arg shapes without re-importing protocol
// ---------------------------------------------------------------------------

export type { LogLine, ServiceRuntimeView, ScriptRuntimeView, RuntimeSnapshot };
