use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};
use crate::graph::start_order;
use crate::health;
use crate::ipc::{LogSource, LogSourceKind, LogStream, DEFAULT_RING_LINES};
use crate::launcher::{log_file_rel, CommandSpec};
use crate::log::{LogBatcher, LogFile, LogHub, LogLine};
use crate::probe;
use crate::runtime::{apply, RtEvent, RtState};
use crate::sandbox;
use crate::spec::{parse_yaml, spec_hash, to_yaml, HealthType, ParseWarning, SuperTaskFile};

use crate::docker::{DockerRunner, DockerSpawn};

/// 1.3 §5.1 compose 服务默认宽限：60s（覆盖首次拉镜像的慢启动）。
const COMPOSE_DEFAULT_GRACE_SECS: u64 = 60;
/// compose up 超时：首次拉镜像可能数分钟，up 属于 runtime 状态机而非 operation。
const COMPOSE_UP_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// compose stop 超时。
const COMPOSE_STOP_TIMEOUT: Duration = Duration::from_secs(60);
/// ps / images 类只读命令超时（§4.2）。
const COMPOSE_QUERY_TIMEOUT: Duration = Duration::from_secs(10);
/// /env 深化 D1：工具链探测缓存窗口。窗口外自动重探，覆盖"用户在应用外装了新工具"。
const TOOLCHAIN_PROBE_TTL: Duration = Duration::from_secs(60);

/// compose 服务的运行期上下文：由哪个 compose 文件 / 项目启动。
/// `started_by_engine` 只在本引擎执行过 up 后为 true——退出清场只处理这些
/// 服务（§5.6：用户手工起的容器不动）。
#[derive(Debug, Clone)]
pub struct ComposeInfo {
    pub file: PathBuf,
    pub project: Option<String>,
    pub service: String,
    pub started_by_engine: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthView {
    pub ok: bool,
    pub at_ms: u64,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExitView {
    pub code: i32,
    pub at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceRuntimeView {
    pub id: String,
    pub state: RtState,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub kind: String,
    pub health: Option<HealthView>,
    pub started_at_ms: Option<u64>,
    pub last_exit: Option<ExitView>,
    pub last_error: Option<String>,
    /// 1.2 §8.5：进程退出原因（"crash"/"stop"），崩溃通知与 toast 用
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_reason: Option<String>,
    /// 2.2 restart：当前/最近一次自动重启的尝试序号（1 起）。手动启动清零；
    /// 仅策略 != never 且发生过自动重启的服务出现。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart_attempt: Option<u32>,
    pub log_seq: u64,
    /// false = 外部进程（端口探测识别，无 Job 无法优雅树管理）
    #[serde(default = "default_true")]
    pub managed: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScriptState {
    Idle,
    Running,
    Exited,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptRuntimeView {
    pub id: String,
    pub state: ScriptState,
    pub pid: Option<u32>,
    pub last_exit: Option<ExitView>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSnapshot {
    pub protocol: u32,
    pub workspace_id: String,
    pub services: IndexMap<String, ServiceRuntimeView>,
    pub script: Option<ScriptRuntimeView>,
    /// 1.2 §9：最近一次指标快照（无采样时为空表）
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub metrics: IndexMap<String, Option<crate::ipc::ServiceMetrics>>,
    /// 1.6：网关托管状态（未配置/未启用时为 None——独立字段，避免前端
    /// 把网关误当 service 渲染）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewayRuntimeView>,
}

/// 1.6 网关运行时视图（snapshot.gateway）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayRuntimeView {
    pub kind: String,
    pub state: RtState,
    pub pid: Option<u32>,
    pub port: u16,
    pub health: Option<HealthView>,
    pub started_at_ms: Option<u64>,
    pub last_exit: Option<ExitView>,
    pub last_error: Option<String>,
    pub exit_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YamlView {
    pub text: String,
    pub spec: SuperTaskFile,
    pub hash: String,
}

/// ports.assign 返回视图：预览（restart_required=true）或已保存的新配置。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PortsAssignView {
    pub spec: SuperTaskFile,
    pub hash: String,
    /// §5.3：显式环境变量/自定义健康 URL 未跟随的提示
    pub notes: Vec<String>,
    pub restart_required: bool,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Runtime(RuntimeSnapshot),
    Logs {
        workspace_id: String,
        items: Vec<LogLine>,
    },
    /// 1.2 §9.2：sampler 批量指标（仅订阅时产生）
    Metrics(crate::ipc::MetricsPayload),
}

#[derive(Clone, Copy)]
enum SpawnerKind {
    Real,
    #[cfg(test)]
    Ping,
    /// Tests: `cmd /C exit 1` so dependents see DEP_DEAD.
    #[cfg(test)]
    Fail,
}

/// 2.2：自动重启用的最小启动计划。spawn_core 成功后捕获当次参数，
/// 崩溃后由监管线程原样重放——不重规划命令、不复检工具（会话内环境未变），
/// 手动 start 仍走 spawn_service 完整链路。
#[derive(Clone)]
struct RestartPlan {
    planned: CommandSpec,
    cwd: PathBuf,
    health_spec: Option<crate::spec::HealthSpec>,
    health_none: bool,
    port: Option<u16>,
    kind: String,
    pkg: Option<String>,
    svc_grace: u64,
    build_tool: Option<String>,
    spawner: SpawnerKind,
    env_snapshot: Vec<crate::ipc::EnvEffectiveEntry>,
    restart: crate::spec::RestartSpec,
}

struct Slot {
    state: RtState,
    pid: Option<u32>,
    port: Option<u16>,
    kind: String,
    /// Arc：健康线程需要跨锁读取 Job 的进程树做端点发现
    job: Option<Arc<dyn crate::proc::ProcessTree>>,
    stop_requested: bool,
    started: Option<Instant>,
    started_at_ms: Option<u64>,
    grace: Duration,
    health: Option<HealthView>,
    last_error: Option<String>,
    last_exit: Option<ExitView>,
    cancel: Arc<AtomicBool>,
    /// 外部进程（端口被占时识别为已运行的非 SuperTask 托管实例）
    managed: bool,
    /// 1.2 launch: jar：已构建 artifact 的绝对路径
    artifact: Option<PathBuf>,
    /// 1.2 §8.5：进程退出原因（crash / stop）
    exit_reason: Option<&'static str>,
    // ---- 2.2 restart 策略（方向一·服务监管）----
    /// spawn 时随 spec 解析的策略；手动启动重置，spec 改动需手动重启生效
    restart: crate::spec::RestartSpec,
    /// 剩余自动重启次数（手动启动 = max_retries；每次自动重启尝试 -1）
    restart_left: u32,
    /// 当前/最近一次自动重启尝试序号（1 起；手动 spawn 清 None）
    restart_attempt: Option<u32>,
    /// 待执行自动重启的撤销标志：手动 stop 置位；手动 start 换新
    restart_cancel: Arc<AtomicBool>,
    /// 自动重启用的最小启动计划（spawn_core 成功后捕获；不做全量重规划/工具复检）
    restart_plan: Option<RestartPlan>,
    /// 1.3：compose 服务的容器运行时上下文（kind != compose 时为 None）
    compose: Option<ComposeInfo>,
    /// 最近一次启动实际注入的生效环境快照（`env.effective`；未启动/ compose 为 None）。
    /// 带采集时间并持久化到 `.supertask/env-snapshots.json`，应用重启/重开工作区后仍可回看。
    env_snapshot: Option<EnvSnapshot>,
}

/// `env.effective` 快照的可持久化形态：采集时间 + 注入键值。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EnvSnapshot {
    pub captured_at_ms: u64,
    pub entries: Vec<crate::ipc::EnvEffectiveEntry>,
}

struct ScriptSlot {
    id: String,
    state: ScriptState,
    pid: Option<u32>,
    job: Option<Arc<dyn crate::proc::ProcessTree>>,
    cancel: Arc<AtomicBool>,
    last_exit: Option<ExitView>,
    last_error: Option<String>,
}

/// 1.6 网关托管 slot：与 service slot 同语义的进程树托管
/// （状态机 / 日志泵 / TCP 健康 / 指标 / 端口排除 / stop_all 清场）。
struct GatewaySlot {
    state: RtState,
    pid: Option<u32>,
    port: u16,
    kind: crate::spec::GatewayKind,
    job: Option<Arc<dyn crate::proc::ProcessTree>>,
    stop_requested: bool,
    cancel: Arc<AtomicBool>,
    started: Option<Instant>,
    started_at_ms: Option<u64>,
    health: Option<HealthView>,
    last_exit: Option<ExitView>,
    last_error: Option<String>,
    exit_reason: Option<&'static str>,
}

struct Inner {
    root: PathBuf,
    workspace_id: String,
    spec: SuperTaskFile,
    spec_hash: String,
    yaml_text: String,
    yaml_path: PathBuf,
    slots: HashMap<String, Slot>,
    script: Option<ScriptSlot>,
    /// 1.6：网关 slot（未配置/未启用 = None，零开销路径）
    gateway: Option<GatewaySlot>,
    logs: LogHub,
    files: HashMap<String, LogFile>,
    script_file: Option<LogFile>,
    subscribers: u32,
    events: SyncSender<EngineEvent>,
    log_tx: Sender<LogLine>,
    /// 1.2 §9 metrics：订阅计数、最近样本、上一窗口 CPU 时间（差分用）
    metrics_sub: u32,
    metrics: IndexMap<String, Option<crate::ipc::ServiceMetrics>>,
    metrics_prev: HashMap<String, (u64, Instant)>,
}

/// 切换工作区时移交给全局注册表的进程存活信息。
/// Job 句柄被 hold 住 → kill-on-close 不触发，进程继续跑；
/// 同一应用会话内重新打开同根工作区时按 service_id 精确接管。
struct DetachedSlot {
    job: Arc<dyn crate::proc::ProcessTree>,
    pid: Option<u32>,
    started_at_ms: Option<u64>,
}

/// key = 规范化小写盘符的 root 字符串（与 same_workspace 前端语义一致）。
static DETACHED: std::sync::Mutex<Option<HashMap<String, HashMap<String, DetachedSlot>>>> =
    std::sync::Mutex::new(None);

fn norm_root(p: &Path) -> String {
    p.to_string_lossy().replace('/', "\\").to_lowercase()
}

fn detached_take(root_norm: &str) -> Option<HashMap<String, DetachedSlot>> {
    DETACHED
        .lock()
        .expect("detached lock")
        .as_mut()
        .and_then(|m| m.remove(root_norm))
}

fn detached_put(root_norm: String, slots: HashMap<String, DetachedSlot>) {
    let mut guard = DETACHED.lock().expect("detached lock");
    guard
        .get_or_insert_with(HashMap::new)
        .insert(root_norm, slots);
}

pub struct Engine {
    inner: Arc<Mutex<Inner>>,
    events_rx: Mutex<Receiver<EngineEvent>>,
    spawner: SpawnerKind,
    /// 1.3：docker CLI runner（fake 可注入，测试不真调 docker）
    docker: Arc<dyn DockerRunner>,
    /// 1.3：compose config 解析（mtime+hash 缓存；spec 打开/端口检查/启动共用）
    compose_loader: crate::docker::ComposeConfigLoader,
    /// 1.3 §4.1：探测结果会话内缓存，`refresh=true` 强制刷新
    probe_cache: Mutex<Option<crate::docker::DockerProbe>>,
    /// 1.3 §3.2：镜像构建长操作（queued → running → succeeded/failed/cancelled）
    operations: crate::operation::OperationHub,
    /// 1.6：网关校验命令执行器（测试注入 fake；生产 spawn nginx -t 等）
    validator: Arc<dyn crate::gateway::validate::ValidateRunner>,
    /// 1.5 §3.1：工作区锁持有者标签（前端身份：desktop/cli/mcp）
    holder: crate::lock::LockHolder,
    /// 1.7 §7：app 级网络默认（代理/镜像），壳层与 CLI 在 open 前注入；缺省 None=全走 workspace 段
    app_network: Mutex<Option<crate::appdata::AppNetwork>>,
    /// /env 深化 D1：工具链探测会话缓存（TTL；refresh 强制；install/upgrade 成功后失效）
    toolchain_probe_cache: Mutex<Option<(Instant, crate::probe::ToolchainProbeBundle)>>,
    /// 探测函数接缝：生产 `probe::probe_bundle`；测试注入假实现验证缓存语义。
    toolchain_probe_fn: Mutex<Arc<dyn Fn() -> crate::probe::ToolchainProbeBundle + Send + Sync>>,
}

impl Engine {
    pub fn new() -> Self {
        Self::create(
            SpawnerKind::Real,
            Arc::new(crate::docker::ProcessDockerRunner),
        )
    }

    /// 注入自定义 DockerRunner（测试用 fake；生产走 [`Engine::new`]）。
    pub fn with_docker_runner(runner: Arc<dyn DockerRunner>) -> Self {
        Self::create(SpawnerKind::Real, runner)
    }

    /// 1.5：以指定 holder 身份持有工作区锁（CLI/MCP 壳用；桌面缺省 Desktop）。
    pub fn with_holder(holder: crate::lock::LockHolder) -> Self {
        Self {
            holder,
            ..Self::new()
        }
    }

    /// 1.6：注入自定义网关校验执行器（测试用 fake；生产走 [`Engine::new`]）。
    pub fn with_validator(runner: Arc<dyn crate::gateway::validate::ValidateRunner>) -> Self {
        Self {
            validator: runner,
            ..Self::new()
        }
    }

    #[cfg(test)]
    pub fn ping_for_test() -> Self {
        Self::create(
            SpawnerKind::Ping,
            Arc::new(crate::docker::ProcessDockerRunner),
        )
    }

    #[cfg(test)]
    pub fn fail_for_test() -> Self {
        Self::create(
            SpawnerKind::Fail,
            Arc::new(crate::docker::ProcessDockerRunner),
        )
    }

    fn create(spawner: SpawnerKind, docker: Arc<dyn DockerRunner>) -> Self {
        let (events_tx, events_rx) = mpsc::sync_channel(512);
        let (log_tx, log_rx) = mpsc::channel::<LogLine>();
        let inner = Arc::new(Mutex::new(Inner {
            root: PathBuf::new(),
            workspace_id: String::new(),
            spec: empty_spec(),
            spec_hash: String::new(),
            yaml_text: String::new(),
            yaml_path: PathBuf::new(),
            slots: HashMap::new(),
            script: None,
            gateway: None,
            logs: LogHub::new(DEFAULT_RING_LINES),
            files: HashMap::new(),
            script_file: None,
            subscribers: 0,
            events: events_tx.clone(),
            log_tx: log_tx.clone(),
            metrics_sub: 0,
            metrics: IndexMap::new(),
            metrics_prev: HashMap::new(),
        }));
        let inner_b = Arc::clone(&inner);
        thread::Builder::new()
            .name("st-log-batch".into())
            .spawn(move || batch_loop(inner_b, log_rx))
            .expect("log batch thread");
        Self {
            inner,
            events_rx: Mutex::new(events_rx),
            spawner,
            compose_loader: crate::docker::ComposeConfigLoader::new(Arc::clone(&docker)),
            docker,
            probe_cache: Mutex::new(None),
            operations: crate::operation::OperationHub::new(),
            validator: Arc::new(crate::gateway::validate::ProcessValidateRunner),
            holder: crate::lock::LockHolder::Desktop,
            app_network: Mutex::new(None),
            toolchain_probe_cache: Mutex::new(None),
            toolchain_probe_fn: Mutex::new(Arc::new(crate::probe::probe_bundle)),
        }
    }

    /// 1.7 §7：壳层 / CLI 在 open 前注入 app 级网络默认（代理 + 镜像）。
    pub fn set_app_network(&self, net: crate::appdata::AppNetwork) {
        *self.app_network.lock().expect("app_network lock") = Some(net);
    }

    /// /env 深化 D1：工具链探测会话缓存。窗口内复用结果；`refresh=true`
    /// 强制重探（「重新探测」按钮）；install/upgrade 成功后由
    /// [`Engine::invalidate_toolchain_probe`] 立即失效。不要求已打开工作区。
    pub fn toolchain_probe(&self, refresh: bool) -> crate::probe::ToolchainProbeBundle {
        crate::toolchain::resolver::refresh_process_path();
        let mut cache = self
            .toolchain_probe_cache
            .lock()
            .expect("toolchain probe cache");
        let fresh = cache
            .as_ref()
            .is_some_and(|(at, _)| !refresh && at.elapsed() < TOOLCHAIN_PROBE_TTL);
        if !fresh {
            let f = Arc::clone(&self.toolchain_probe_fn.lock().expect("toolchain probe fn"));
            *cache = Some((Instant::now(), f()));
        }
        cache.as_ref().expect("cache filled above").1.clone()
    }

    /// 工具链状态可能已变（安装/升级成功）→ 丢弃缓存，下次访问重探。
    pub fn invalidate_toolchain_probe(&self) {
        *self
            .toolchain_probe_cache
            .lock()
            .expect("toolchain probe cache") = None;
    }

    /// 测试接缝：替换探测函数（同时清缓存），用于验证缓存/失效语义而不真 spawn。
    #[cfg(test)]
    pub fn set_toolchain_probe_fn_for_test(
        &self,
        f: impl Fn() -> crate::probe::ToolchainProbeBundle + Send + Sync + 'static,
    ) {
        *self.toolchain_probe_fn.lock().expect("toolchain probe fn") = Arc::new(f);
        self.invalidate_toolchain_probe();
    }

    /// 方向三：声明式 `needs` 解析（resolve-only dry-run，ipc.md §10.17）。
    /// 纯只读：不安装、不下载、不写盘；结果由 (needs 声明, 探测缓存, 内置归档
    /// 目录, 当前平台) 完全决定。`refresh=true` 强制重探工具链（同 probe 按钮）。
    pub fn needs_resolve(&self, refresh: bool) -> Result<crate::needs::NeedsResolveOut> {
        let spec = self.spec()?;
        let bundle = self.toolchain_probe(refresh);
        Ok(crate::needs::resolve(
            spec.needs.as_deref().unwrap_or(&[]),
            &bundle,
        ))
    }

    // ---- 方向六：数据快照（ipc.md §10.18）。快照是离线文件快照：
    // 绑定服务未停止则 create/restore 拒绝（SNAPSHOT_BUSY）；预览不受限。----

    /// 快照存储目录：`<root>/.supertask/snapshots/<volume_id>`。
    fn snapshots_dir(root: &Path, volume_id: &str) -> PathBuf {
        root.join(".supertask").join("snapshots").join(volume_id)
    }

    fn data_volume_of(g: &Inner, volume_id: &str) -> Result<crate::spec::DataVolumeSpec> {
        g.spec
            .data
            .as_ref()
            .and_then(|d| d.volumes.get(volume_id))
            .cloned()
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::NotFound,
                    format!(
                        "data 卷不存在: {volume_id}（在 supertask.yaml 的 data.volumes 中声明）"
                    ),
                )
            })
    }

    /// 快照 id 仅允许数字（created_at 毫秒 stem），杜绝路径拼接面。
    fn snapshot_zip_path(root: &Path, volume_id: &str, snapshot_id: &str) -> Result<PathBuf> {
        if snapshot_id.is_empty() || !snapshot_id.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::new(
                ErrorCode::SnapshotInvalid,
                format!("快照 id 非法: {snapshot_id:?}"),
            ));
        }
        Ok(Self::snapshots_dir(root, volume_id).join(format!("{snapshot_id}.zip")))
    }

    /// 离线快照守护：绑定服务处于非 Stopped/Exited 态即拒绝。
    fn ensure_bound_service_stopped(g: &Inner, service: Option<&str>) -> Result<()> {
        let Some(svc) = service else {
            return Ok(());
        };
        let busy = g.slots.get(svc).is_some_and(|s| {
            matches!(
                s.state,
                RtState::Starting
                    | RtState::Running
                    | RtState::Unhealthy
                    | RtState::Stopping
                    | RtState::Building
            )
        });
        if busy {
            return Err(Error::new(
                ErrorCode::SnapshotBusy,
                format!("绑定的服务 {svc} 正在运行，停止后再快照/恢复"),
            ));
        }
        Ok(())
    }

    fn meta_view(m: crate::snapshot::SnapshotMeta) -> crate::ipc::DataSnapshotView {
        crate::ipc::DataSnapshotView {
            id: m.id,
            created_at: m.created_at,
            bytes: m.bytes,
            file_count: m.file_count,
            total_bytes: m.total_bytes,
            note: m.note,
        }
    }

    /// `workspace.dataList`：数据卷与各自快照（只读）。
    pub fn data_list(&self) -> Result<crate::ipc::DataListOut> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        let mut warnings: Vec<String> = Vec::new();
        let mut volumes = Vec::new();
        if let Some(data) = &g.spec.data {
            for (id, vol) in &data.volumes {
                let out_dir = Self::snapshots_dir(&g.root, id);
                let (metas, mut w) = crate::snapshot::list_snapshots(&out_dir);
                warnings.append(&mut w);
                // 上次恢复中断可能遗留 stash（正常路径已自动回滚清理）。
                if let Ok(read) = fs::read_dir(&out_dir) {
                    if read.flatten().any(|e| {
                        e.file_name().to_string_lossy().starts_with(".stash-") && e.path().is_dir()
                    }) {
                        warnings.push(format!(
                            "数据卷 {id} 存在上次恢复的遗留备份（.stash-*），确认后可手动删除"
                        ));
                    }
                }
                volumes.push(crate::ipc::DataVolumeView {
                    id: id.clone(),
                    service: vol.service.clone(),
                    dir: vol.dir.clone(),
                    snapshots: metas.into_iter().map(Self::meta_view).collect(),
                });
            }
        }
        Ok(crate::ipc::DataListOut { volumes, warnings })
    }

    /// `workspace.dataSnapshotCreate`：为数据卷创建离线快照。
    pub fn data_snapshot_create(
        &self,
        volume_id: &str,
        note: &str,
    ) -> Result<crate::ipc::DataSnapshotCreatedOut> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        let vol = Self::data_volume_of(&g, volume_id)?;
        Self::ensure_bound_service_stopped(&g, vol.service.as_deref())?;
        let dir = sandbox::confine(&g.root, &vol.dir)?;
        let out_dir = Self::snapshots_dir(&g.root, volume_id);
        let meta = crate::snapshot::create_snapshot(
            &dir,
            &out_dir,
            volume_id,
            vol.service.as_deref(),
            note,
        )?;
        let mut warnings = Vec::new();
        if vol.service.is_some() {
            warnings.push(format!(
                "快照为离线文件快照，绑定服务 {} 需保持停止直到恢复完成",
                vol.service.unwrap()
            ));
        }
        Ok(crate::ipc::DataSnapshotCreatedOut {
            volume_id: volume_id.to_string(),
            snapshot: Self::meta_view(meta),
            warnings,
        })
    }

    /// `workspace.dataRestorePreview`：恢复预览（纯只读，不要求服务已停止）。
    pub fn data_restore_preview(
        &self,
        volume_id: &str,
        snapshot_id: &str,
    ) -> Result<crate::ipc::DataRestorePreviewOut> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        let vol = Self::data_volume_of(&g, volume_id)?;
        let zip = Self::snapshot_zip_path(&g.root, volume_id, snapshot_id)?;
        let dir = sandbox::confine(&g.root, &vol.dir)?;
        let preview = crate::snapshot::restore_preview(&zip, &dir)?;
        let mut blockers = Vec::new();
        if let Some(svc) = &vol.service {
            let busy = g.slots.get(svc).is_some_and(|s| {
                matches!(
                    s.state,
                    RtState::Starting
                        | RtState::Running
                        | RtState::Unhealthy
                        | RtState::Stopping
                        | RtState::Building
                )
            });
            if busy {
                blockers.push(format!("绑定的服务 {svc} 正在运行，停止后才能恢复"));
            }
        }
        Ok(crate::ipc::DataRestorePreviewOut {
            volume_id: volume_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
            ready: blockers.is_empty(),
            blockers,
            target_exists: preview.target_exists,
            current_files: preview.current_files,
            snapshot_files: preview.snapshot_files,
            total_bytes: preview.total_bytes,
            remove_count: preview.remove_count,
            remove_sample: preview.remove_sample,
            warnings: Vec::new(),
        })
    }

    /// `workspace.dataRestore`：恢复（校验 → stash → 解压 → 失败回滚）。
    pub fn data_restore(
        &self,
        volume_id: &str,
        snapshot_id: &str,
    ) -> Result<crate::ipc::DataRestoreOut> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        let vol = Self::data_volume_of(&g, volume_id)?;
        Self::ensure_bound_service_stopped(&g, vol.service.as_deref())?;
        let zip = Self::snapshot_zip_path(&g.root, volume_id, snapshot_id)?;
        let dir = sandbox::confine(&g.root, &vol.dir)?;
        let stash = Self::snapshots_dir(&g.root, volume_id).join(format!(".stash-{}", now_ms()));
        let out = crate::snapshot::restore_snapshot(&zip, &dir, &stash)?;
        Ok(crate::ipc::DataRestoreOut {
            volume_id: volume_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
            restored_files: out.restored_files,
            removed_files: out.removed_files,
            warnings: Vec::new(),
        })
    }

    /// `workspace.dataSnapshotDelete`：删除单个快照文件。
    pub fn data_snapshot_delete(
        &self,
        volume_id: &str,
        snapshot_id: &str,
    ) -> Result<crate::ipc::DataSnapshotDeletedOut> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Self::data_volume_of(&g, volume_id)?;
        let zip = Self::snapshot_zip_path(&g.root, volume_id, snapshot_id)?;
        crate::snapshot::delete_snapshot(&zip)?;
        Ok(crate::ipc::DataSnapshotDeletedOut {
            volume_id: volume_id.to_string(),
            snapshot_id: snapshot_id.to_string(),
        })
    }

    pub fn open(&self, path: &Path) -> Result<(Vec<ParseWarning>, RuntimeSnapshot)> {
        // 1.5 §3.1：fail-fast，避免对已打开工作区误释放重入拿到的锁
        {
            let g = self.inner.lock().expect("engine lock");
            if !g.workspace_id.is_empty() {
                return Err(Error::new(
                    ErrorCode::AlreadyInProgress,
                    "已打开工作区，请先 close",
                ));
            }
        }
        let root = sandbox::strip_verbatim(
            fs::canonicalize(path)
                .map_err(|e| Error::new(ErrorCode::CwdMissing, format!("无法打开目录: {e}")))?,
        );
        // 工作区所有权锁：打开即占；失败路径必须释放，避免本进程自己挡自己
        crate::lock::acquire(&root, self.holder)?;
        match self.open_locked(&root) {
            Ok(ok) => Ok(ok),
            Err(e) => {
                let _ = crate::lock::release(&root);
                Err(e)
            }
        }
    }

    fn open_locked(&self, root: &Path) -> Result<(Vec<ParseWarning>, RuntimeSnapshot)> {
        let root = root.to_path_buf();
        let (yaml_path, text, file, mut warnings) = load_yaml_at(&root)?;
        let workspace_id = root.to_string_lossy().into_owned();
        let mut slots = HashMap::new();
        let _ = crate::log::run_retention(&root, file.log_retention.as_ref());
        let mut files = HashMap::new();
        // 端口归属一次枚举多服务复用：仅端口命中不可信，必须结合工作目录 + 程序类型，
        // 否则外部进程占同端口会被误判为本服务运行中（还会误导停止去杀外部进程）。
        let discovered = crate::discover::discover_services().ok();
        for (id, svc) in &file.services {
            let rel = log_file_rel("service", id);
            let abs = root.join(&rel);
            let lf = LogFile::open_with_files(
                abs,
                file.logging.as_ref().and_then(|l| l.max_bytes),
                file.logging.as_ref().and_then(|l| l.retain_tail_bytes),
                file.log_retention.as_ref().and_then(|r| r.max_files),
            )
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法创建日志文件: {e}")))?;
            files.insert(id.clone(), lf);
            // 端口 + 工作目录 + 程序类型三维判定：
            // Owned → 外部已在运行（用户手动起的），非托管仅监控；
            // Conflict → 外部进程占位，本服务 Stopped + 提示换端口，且禁止启动；
            // Unknown（发现表不可读）→ 退回旧口径按外部运行展示。
            let (state, managed, last_error) = match svc.port {
                Some(p) => {
                    let ownership = match &discovered {
                        Some(all) => crate::discover::classify_with_list(p, &svc.kind, &root, all),
                        None => {
                            if port_is_serving(p) {
                                crate::discover::PortOwnership::Unknown
                            } else {
                                crate::discover::PortOwnership::Free
                            }
                        }
                    };
                    match ownership {
                        crate::discover::PortOwnership::Owned(_) => {
                            warnings.push(ParseWarning {
                                code: ErrorCode::PortDup,
                                message: format!(
                                    "{id}: 端口 {p} 已被本工作区进程监听，按外部已运行服务显示（仅监控）"
                                ),
                            });
                            (RtState::Running, false, None)
                        }
                        crate::discover::PortOwnership::Conflict(occs) => {
                            let msg = conflict_message(id, p, &occs);
                            warnings.push(ParseWarning {
                                code: ErrorCode::PortInUse,
                                message: msg.clone(),
                            });
                            (RtState::Stopped, true, Some(msg))
                        }
                        crate::discover::PortOwnership::Unknown => {
                            warnings.push(ParseWarning {
                                code: ErrorCode::PortDup,
                                message: format!(
                                    "{id}: 端口 {p} 已被占用（归属未验证），按外部已运行服务显示（仅监控）"
                                ),
                            });
                            (RtState::Running, false, None)
                        }
                        crate::discover::PortOwnership::Free => (RtState::Stopped, true, None),
                    }
                }
                None => (RtState::Stopped, true, None),
            };
            slots.insert(
                id.clone(),
                Slot {
                    state,
                    pid: None,
                    port: svc.port,
                    kind: svc.kind.clone(),
                    job: None,
                    stop_requested: false,
                    started: None,
                    started_at_ms: None,
                    grace: Duration::from_secs(svc.grace_secs.unwrap_or(0) as u64),
                    health: None,
                    last_error,
                    last_exit: None,
                    cancel: Arc::new(AtomicBool::new(false)),
                    managed,
                    artifact: None,
                    exit_reason: None,
                    restart: crate::spec::RestartSpec::default(),
                    restart_left: 0,
                    restart_attempt: None,
                    restart_cancel: Arc::new(AtomicBool::new(false)),
                    restart_plan: None,
                    compose: None,
                    env_snapshot: None,
                },
            );
        }
        // 恢复上次会话留存的生效环境快照（排障连续性：重启后"生效环境"仍可见）
        load_env_snapshots(&root, &mut slots);
        // 1.3 §2.4/§4.3：compose 引用打开时校验（service 存在 / 端口一致）。
        // Docker 不可用或解析失败 → 静默跳过，启动时再给出真实错误。
        warnings.extend(self.compose_open_warnings(&file, &root));
        // 1.4 §5.1：build_tool 缺省时按构建文件探测——并存 BUILD_TOOL_AMBIGUOUS、
        // 都没有 MISSING_TOOL；只警告不阻塞打开，启动时才是硬错误。
        warnings.extend(self.build_tool_open_warnings(&file, &root));
        // 1.6 §4.1：网关段打开时静态校验（warning，不阻塞打开）；
        // 已配置且启用 → 建 Stopped slot + 日志文件（source=gateway）
        let mut gateway_slot = None;
        if let Some(conf) = &file.gateway {
            if conf.kind.is_some() && conf.enabled {
                for issue in crate::gateway::validate_static(&file, conf) {
                    warnings.push(ParseWarning {
                        code: ErrorCode::GatewayRouteInvalid,
                        message: match issue.route {
                            Some(i) => format!("gateway 第 {} 条路由：{}", i + 1, issue.message),
                            None => format!("gateway: {}", issue.message),
                        },
                    });
                }
                let rel = log_file_rel("gateway", "gateway");
                let lf = LogFile::open_with_files(
                    root.join(&rel),
                    file.logging.as_ref().and_then(|l| l.max_bytes),
                    file.logging.as_ref().and_then(|l| l.retain_tail_bytes),
                    file.log_retention.as_ref().and_then(|r| r.max_files),
                )
                .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法创建网关日志文件: {e}")))?;
                files.insert("gateway".into(), lf);
                gateway_slot = Some(GatewaySlot {
                    state: RtState::Stopped,
                    pid: None,
                    port: conf.port,
                    kind: conf.kind.expect("kind checked"),
                    job: None,
                    stop_requested: false,
                    cancel: Arc::new(AtomicBool::new(false)),
                    started: None,
                    started_at_ms: None,
                    health: None,
                    last_exit: None,
                    last_error: None,
                    exit_reason: None,
                });
            }
        }
        {
            let mut g = self.inner.lock().expect("engine lock");
            if !g.workspace_id.is_empty() {
                return Err(Error::new(
                    ErrorCode::AlreadyInProgress,
                    "已打开工作区，请先 close",
                ));
            }
            g.root = root.clone();
            g.workspace_id = workspace_id.clone();
            g.spec = file;
            g.spec_hash = spec_hash(&text);
            g.yaml_text = text;
            g.yaml_path = yaml_path;
            g.slots = slots;
            g.files = files;
            g.script = None;
            g.script_file = None;
            g.gateway = gateway_slot;
            g.logs = LogHub::new(
                g.spec
                    .logging
                    .as_ref()
                    .and_then(|l| l.ring_lines)
                    .unwrap_or(DEFAULT_RING_LINES as u32) as usize,
            );
            g.subscribers = 0;
        }
        self.adopt_detached(&root, &workspace_id);
        Ok((warnings, self.snapshot()?))
    }

    /// open 后回填同根工作区被 detach 的进程：job 仍活着 → 服务直接 Running。
    fn adopt_detached(&self, root: &Path, workspace_id: &str) {
        let Some(detached) = detached_take(&norm_root(root)) else {
            return;
        };
        let mut g = self.inner.lock().expect("engine lock");
        if g.workspace_id != workspace_id {
            return; // 理论不可达；防错
        }
        for (id, d) in detached {
            let Some(slot) = g.slots.get_mut(&id) else {
                continue; // yaml 已删掉该服务：丢弃 Job（kill-on-close 结束进程）
            };
            if !d.job.has_live_process() {
                continue; // 进程已退出：drop job 句柄即可
            }
            slot.state = RtState::Running;
            slot.pid = d.pid;
            slot.started_at_ms = d.started_at_ms;
            slot.started = None;
            slot.job = Some(d.job);
            slot.cancel = Arc::new(AtomicBool::new(false));
        }
    }

    /// 1.3 §2.4/§4.3：打开时对 `kind: compose` 服务做静态引用校验。
    /// 只做「能便宜验证」的部分：compose 文件解析结果里有没有这个 service、
    /// YAML port 与 compose 映射端口是否一致。失败不阻塞打开。
    fn compose_open_warnings(&self, file: &SuperTaskFile, root: &Path) -> Vec<ParseWarning> {
        let mut out = Vec::new();
        if !file.services.values().any(|s| s.kind == "compose") {
            return out;
        }
        let Some(docker_cfg) = file.docker.as_ref() else {
            return out;
        };
        let rel = match docker_cfg.compose_file.clone() {
            Some(r) => r,
            None => match crate::scan::discover_compose_file(root) {
                Some(r) => r,
                None => {
                    out.push(ParseWarning {
                        code: ErrorCode::ComposeFileMissing,
                        message: "未找到 compose 文件（compose.yaml / compose.yml / docker-compose.yml / docker-compose.yaml）"
                            .into(),
                    });
                    return out;
                }
            },
        };
        // 加载失败（docker 不可用 / config 非零 / 不可解析）→ 打开不阻塞
        let Ok(model) = self
            .compose_loader
            .load(root, &rel, docker_cfg.project_name.as_deref())
        else {
            return out;
        };
        for (id, svc) in &file.services {
            if svc.kind != "compose" {
                continue;
            }
            let name = svc.service.as_deref().unwrap_or("");
            match model.find(name) {
                None => out.push(ParseWarning {
                    code: ErrorCode::ComposeServiceMissing,
                    message: format!("{id}: compose 文件中没有服务 {name:?}"),
                }),
                Some(m) => {
                    if let Some(p) = svc.port {
                        if m.port.is_some() && m.port != Some(p) {
                            out.push(ParseWarning {
                                code: ErrorCode::ComposePortMismatch,
                                message: format!(
                                    "{id}: YAML port {p} 与 compose 映射端口 {} 不一致（健康与冲突检查以 YAML 为准，需同步修改 compose 文件）",
                                    m.port.unwrap()
                                ),
                            });
                        }
                    }
                }
            }
        }
        out
    }

    /// 1.4 §5.1：打开时对 `kind: spring-boot` 服务做构建工具探测（显式
    /// build_tool 跳过）。探测失败的 code 直接作为 warning code 透出。
    /// 测试 spawner 无真实工程文件，跳过。
    fn build_tool_open_warnings(&self, file: &SuperTaskFile, root: &Path) -> Vec<ParseWarning> {
        let mut out = Vec::new();
        if !matches!(self.spawner, SpawnerKind::Real) {
            return out;
        }
        for (id, svc) in &file.services {
            if svc.kind != "spring-boot" || svc.build_tool.is_some() {
                continue;
            }
            let module = svc.module.as_deref().unwrap_or(".");
            let Ok(dir) = sandbox::confine(root, module) else {
                continue;
            };
            if let Err(e) = crate::launcher::detect_build_tool(&dir) {
                out.push(ParseWarning {
                    code: e.code(),
                    message: format!("{id}: {}", e.message()),
                });
            }
        }
        out
    }

    /// Write a fresh `supertask.yaml` from a spec, then open the workspace.
    /// Used by the scan wizard: confirm a draft → persist → open in one step.
    pub fn init(
        &self,
        path: &Path,
        mut file: SuperTaskFile,
    ) -> Result<(Vec<ParseWarning>, RuntimeSnapshot)> {
        let root = sandbox::strip_verbatim(
            fs::canonicalize(path)
                .map_err(|e| Error::new(ErrorCode::CwdMissing, format!("无法打开目录: {e}")))?,
        );
        {
            let g = self.inner.lock().expect("engine lock");
            if !g.workspace_id.is_empty() {
                return Err(Error::new(
                    ErrorCode::AlreadyInProgress,
                    "已打开工作区，请先 close",
                ));
            }
        }
        let yaml_path = root.join("supertask.yaml");
        if yaml_path.exists() {
            return Err(Error::new(
                ErrorCode::YamlConflict,
                "已存在 supertask.yaml，请直接用 workspace.open",
            ));
        }
        if file.root.is_empty() {
            file.root = ".".into();
        }
        file.apply_defaults();
        let text = to_yaml(&file)?;
        fs::write(&yaml_path, text)
            .map_err(|e| Error::new(ErrorCode::YamlParse, format!("写 YAML 失败: {e}")))?;
        self.open(&root)
    }

    pub fn close(&self) -> Result<()> {
        let ids: Vec<String> = {
            let g = self.inner.lock().expect("engine lock");
            if g.workspace_id.is_empty() {
                return Ok(());
            }
            g.slots.keys().cloned().collect()
        };
        for id in ids.into_iter().rev() {
            let _ = self.stop_one(&id);
        }
        // 1.6 §7：引擎退出清场一律包含网关
        let _ = self.gateway_stop();
        let _ = self.cancel_script();
        self.wait_script_idle(Duration::from_secs(8));
        let mut g = self.inner.lock().expect("engine lock");
        g.workspace_id.clear();
        g.slots.clear();
        g.files.clear();
        g.script = None;
        g.script_file = None;
        g.gateway = None;
        g.yaml_path = PathBuf::new();
        let root = std::mem::take(&mut g.root);
        drop(g);
        // 1.5 §3.1：close 释放工作区锁
        let _ = crate::lock::release(&root);
        Ok(())
    }

    /// 切换工作区专用：不清进程，把「有 Job 的活服务」移交全局 DETACHED 注册表，
    /// 然后清空活动工作区状态（与 close 的区别就在是否 stop）。
    pub fn detach(&self) -> Result<()> {
        let mut detached: HashMap<String, DetachedSlot> = HashMap::new();
        let root_norm_path;
        {
            let mut g = self.inner.lock().expect("engine lock");
            if g.workspace_id.is_empty() {
                return Ok(());
            }
            root_norm_path = g.root.clone();
            // 1.6：网关不进 DETACHED 移交，随工作区切换终止（§2.4）
            if let Some(slot) = g.gateway.as_mut() {
                slot.stop_requested = true;
                slot.cancel.store(true, Ordering::SeqCst);
                if let Some(job) = slot.job.take() {
                    let _ = job.terminate();
                }
                g.gateway = None;
            }
            for (id, slot) in g.slots.iter_mut() {
                // 只接管持有 Job 且非自然退出态的服务
                if !matches!(slot.state, RtState::Stopped | RtState::Exited) {
                    if let Some(job) = slot.job.take() {
                        detached.insert(
                            id.clone(),
                            DetachedSlot {
                                job,
                                pid: slot.pid,
                                started_at_ms: slot.started_at_ms,
                            },
                        );
                    }
                }
                // 停日志泵（管道读端关闭，子进程写 stdout 会得到错误但不受影响）
                slot.cancel.store(true, Ordering::SeqCst);
            }
            g.slots.clear();
            g.files.clear();
            g.workspace_id.clear();
            g.script = None;
            g.script_file = None;
            g.yaml_path = PathBuf::new();
            g.root = PathBuf::new();
        }
        // 1.6：网关已在上方随 detach 终止（不进 DETACHED 移交）
        if !detached.is_empty() {
            let root_norm = norm_root(&root_norm_path);
            detached_put(root_norm, detached);
        }
        // 1.5 §3.1：detach（切工作区）释放工作区锁
        let _ = crate::lock::release(&root_norm_path);
        Ok(())
    }

    pub fn snapshot(&self) -> Result<RuntimeSnapshot> {
        let g = self.inner.lock().expect("engine lock");
        if g.workspace_id.is_empty() {
            return Err(Error::new(ErrorCode::NoWorkspace, "未打开工作区"));
        }
        Ok(build_snapshot(&g))
    }

    pub fn workspace_id(&self) -> Result<String> {
        let g = self.inner.lock().expect("engine lock");
        if g.workspace_id.is_empty() {
            return Err(Error::new(ErrorCode::NoWorkspace, "未打开工作区"));
        }
        Ok(g.workspace_id.clone())
    }

    pub fn spec(&self) -> Result<SuperTaskFile> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Ok(g.spec.clone())
    }

    pub fn yaml_get(&self) -> Result<YamlView> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Ok(YamlView {
            text: g.yaml_text.clone(),
            spec: g.spec.clone(),
            hash: g.spec_hash.clone(),
        })
    }

    pub fn save_text(
        &self,
        text: &str,
        base_hash: &str,
    ) -> Result<(SuperTaskFile, String, Vec<ParseWarning>)> {
        let (file, warnings) = parse_yaml(text)?;
        let mut g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        if g.spec_hash != base_hash {
            return Err(Error::new(
                ErrorCode::YamlConflict,
                "配置已被别处修改，请刷新后再保存",
            ));
        }
        check_can_replace_spec(&g, &file)?;
        // ponytail: hold lock for ≤1MiB write so hash stays atomic; split if yaml saves contend
        fs::write(&g.yaml_path, text)
            .map_err(|e| Error::new(ErrorCode::YamlParse, format!("写入 YAML 失败: {e}")))?;
        apply_spec_slots(&mut g, &file)?;
        g.spec = file.clone();
        g.yaml_text = text.to_string();
        g.spec_hash = spec_hash(text);
        emit_runtime(&g);
        Ok((file, g.spec_hash.clone(), warnings))
    }

    pub fn save_form(
        &self,
        spec: &SuperTaskFile,
        base_hash: &str,
    ) -> Result<(SuperTaskFile, String, Vec<ParseWarning>)> {
        let text = to_yaml(spec)?;
        self.save_text(&text, base_hash)
    }

    pub fn subscribe_logs(&self) -> Result<u64> {
        let mut g = self.inner.lock().expect("engine lock");
        if g.workspace_id.is_empty() {
            return Err(Error::new(ErrorCode::NoWorkspace, "未打开工作区"));
        }
        g.subscribers = g.subscribers.saturating_add(1);
        Ok(g.logs.next_seq())
    }

    pub fn unsubscribe_logs(&self) -> Result<()> {
        let mut g = self.inner.lock().expect("engine lock");
        g.subscribers = g.subscribers.saturating_sub(1);
        Ok(())
    }

    pub fn logs_snapshot(
        &self,
        source: Option<&LogSource>,
        limit: usize,
    ) -> Result<(Vec<LogLine>, u64)> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Ok(g.logs.snapshot(source, limit.max(1)))
    }

    pub fn clear_logs(&self, source: &LogSource) -> Result<()> {
        let mut g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        g.logs.clear_source(source);
        Ok(())
    }

    pub fn try_recv_event(&self) -> Option<EngineEvent> {
        self.events_rx.lock().expect("rx").try_recv().ok()
    }

    pub fn recv_event_timeout(&self, d: Duration) -> Option<EngineEvent> {
        self.events_rx.lock().expect("rx").recv_timeout(d).ok()
    }

    pub fn start_one(&self, id: &str) -> Result<()> {
        {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let slot = g
                .slots
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            if matches!(
                slot.state,
                RtState::Starting
                    | RtState::Running
                    | RtState::Unhealthy
                    | RtState::Stopping
                    | RtState::Building
            ) {
                return Err(Error::new(
                    ErrorCode::AlreadyInProgress,
                    format!("{id} 已在运行、正在切换或构建"),
                ));
            }
        }
        self.ensure_started(id)?;
        Ok(())
    }

    pub fn start_all(&self) -> Result<Vec<String>> {
        let order = {
            let g = self.inner.lock().expect("engine lock");
            if g.workspace_id.is_empty() {
                return Err(Error::new(ErrorCode::NoWorkspace, "未打开工作区"));
            }
            start_order(&g.spec)?
        };
        for id in &order {
            let disabled = {
                let g = self.inner.lock().expect("engine lock");
                crate::profiles::effective_service(&g.spec, id)
                    .map(|e| !e.enabled)
                    .unwrap_or(false)
            };
            if disabled {
                continue; // §10：profile 关闭的服务在 startAll 中跳过
            }
            match self.ensure_started(id) {
                Ok(()) => {
                    // launch: jar 先经历 Building（package 可达数分钟）
                    self.wait_building_done(id, Duration::from_secs(20 * 60))?;
                    self.wait_ready(id, Duration::from_secs(90))?;
                    if self.state_of(id) == Some(RtState::Exited) {
                        // dependents will see DEP_DEAD
                    }
                }
                Err(e) if e.code() == ErrorCode::DepDead => continue,
                Err(e) => return Err(e),
            }
        }
        // 1.6 §12：全部启动 = 先服务后网关；网关失败不阻塞主目标
        // （错误落 slot.last_error 与网关日志，gateway.status 可见）
        if let Err(e) = self.gateway_start() {
            push_line(
                &self.inner,
                LogSource {
                    kind: LogSourceKind::Gateway,
                    id: "gateway".into(),
                },
                LogStream::System,
                format!("GATEWAY_START_FAILED: {}", e.message()),
            );
        }
        Ok(order)
    }

    pub fn stop_one(&self, id: &str) -> Result<()> {
        {
            let mut g = self.inner.lock().expect("engine lock");
            let root = g.root.clone();
            let slot = g
                .slots
                .get_mut(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            if matches!(slot.state, RtState::Stopped) {
                return Ok(());
            }
            // 1.3 §5.2/§5.6：compose 服务走容器运行时分支——容器进程不进
            // Job/taskkill，只 `docker compose stop`；未由本引擎启动过的容器不动。
            if slot.kind == "compose" {
                match slot.compose.clone().filter(|c| c.started_by_engine) {
                    Some(info) => {
                        slot.stop_requested = true;
                        slot.cancel.store(true, Ordering::SeqCst);
                        if let Ok(next) = apply(slot.state, RtEvent::StopRequested) {
                            slot.state = next;
                        }
                        emit_runtime(&g);
                        drop(g);
                        return self.compose_stop(id, info);
                    }
                    None => {
                        if slot.compose.is_some() && slot.state == RtState::Starting {
                            // up 仍在执行：置位请求，up 完成后自动补 stop（compose_up_flow）
                            slot.stop_requested = true;
                            slot.cancel.store(true, Ordering::SeqCst);
                            emit_runtime(&g);
                        }
                        // 其余（外部手工起的容器 / 从未启动）：不动（§5.6）
                        return Ok(());
                    }
                }
            }
            // 外部进程：无 Job 可 terminate；停止前复核归属，只结束本工作区的外部进程，
            // 归属外部的占位进程（端口冲突）绝不动（只把显示置 Stopped）。
            if !slot.managed {
                let port = slot.port;
                let kind = slot.kind.clone();
                slot.stop_requested = true;
                slot.cancel.store(true, Ordering::SeqCst);
                slot.state = RtState::Stopped;
                emit_runtime(&g);
                drop(g);
                match port {
                    Some(p) => match crate::discover::classify_port_owner(p, &kind, &root) {
                        crate::discover::PortOwnership::Owned(occ) => kill_foreign_by_pid(occ.pid)?,
                        _ => {} // 占位已消失 / 归属外部 / 不可见：不杀任何进程
                    },
                    None => {} // 无端口：按已停止处理
                }
                return Ok(());
            }
            slot.stop_requested = true;
            slot.cancel.store(true, Ordering::SeqCst);
            // 2.2：手动停止同时撤销待执行的自动重启
            slot.restart_cancel.store(true, Ordering::SeqCst);
            if let Ok(next) = apply(slot.state, RtEvent::StopRequested) {
                slot.state = next;
            }
            if let Some(job) = &slot.job {
                job.terminate()?;
            }
            emit_runtime(&g);
        }
        self.wait_state(
            id,
            &[RtState::Stopped, RtState::Exited],
            Duration::from_secs(8),
        )
        .map_err(|_| Error::new(ErrorCode::JobKill, format!("{id} 停止超时")))?;
        Ok(())
    }

    pub fn stop_all(&self) -> Result<()> {
        let mut ids: Vec<String> = {
            let g = self.inner.lock().expect("engine lock");
            start_order(&g.spec).unwrap_or_default()
        };
        ids.reverse();
        for id in ids {
            let _ = self.stop_one(&id);
        }
        // 1.6 §7：stop_all 清场包含网关
        let _ = self.gateway_stop();
        Ok(())
    }

    pub fn restart_one(&self, id: &str) -> Result<()> {
        self.stop_one(id)?;
        self.start_one(id)
    }

    pub fn run_script(&self, id: &str) -> Result<()> {
        crate::toolchain::resolver::refresh_process_path();
        let (cmds, cwd, cwd_rel, mut env, timeout, root, toolchain) = {
            let mut g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            if g.script
                .as_ref()
                .is_some_and(|s| s.state == ScriptState::Running)
            {
                return Err(Error::new(ErrorCode::ScriptBusy, "已有脚本在运行"));
            }
            let spec = g
                .spec
                .scripts
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有脚本 {id}")))?;
            let cmds = spec.cmds.clone();
            let timeout = Duration::from_secs(spec.timeout_secs.unwrap_or(1800) as u64);
            let cwd = match spec.cwd.as_deref() {
                None | Some(".") | Some("") => g.root.clone(),
                Some(rel) => sandbox::confine(&g.root, rel)?,
            };
            if !cwd.is_dir() {
                return Err(Error::new(
                    ErrorCode::CwdMissing,
                    format!("脚本工作目录不存在: {}", cwd.display()),
                ));
            }
            let mut env = g.spec.env.clone();
            for (k, v) in &spec.env {
                env.insert(k.clone(), v.clone());
            }
            let cwd_rel = spec.cwd.clone().unwrap_or_else(|| ".".into());
            let root = g.root.clone();
            let toolchain = g.spec.toolchain.clone();
            let rel = log_file_rel("script", id);
            let abs = g.root.join(&rel);
            let lf = LogFile::open_with_files(
                abs,
                g.spec.logging.as_ref().and_then(|l| l.max_bytes),
                g.spec.logging.as_ref().and_then(|l| l.retain_tail_bytes),
                g.spec.log_retention.as_ref().and_then(|r| r.max_files),
            )
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法创建脚本日志: {e}")))?;
            g.script_file = Some(lf);
            let job = crate::proc::create_tree()?;
            g.script = Some(ScriptSlot {
                id: id.to_string(),
                state: ScriptState::Running,
                pid: None,
                job: Some(job),
                cancel: Arc::new(AtomicBool::new(false)),
                last_exit: None,
                last_error: None,
            });
            emit_runtime(&g);
            (cmds, cwd, cwd_rel, env, timeout, root, toolchain)
        };
        // Scripts are build entry points too. Apply the same project JDK
        // selection as services so `mvn install` does not inherit the desktop
        // process JDK (for example JDK 25 instead of a project's JDK 17).
        if crate::launcher::project_java_version(&root, &cwd_rel).is_some()
            || toolchain
                .as_ref()
                .and_then(|tc| tc.java.as_deref())
                .is_some()
        {
            let installs = self.toolchain_probe(false).tools.installs;
            crate::launcher::apply_java_version_env(
                toolchain.as_ref(),
                &IndexMap::new(),
                &root,
                &cwd_rel,
                &installs,
                &mut env,
            );
        }
        let inner = Arc::clone(&self.inner);
        let sid = id.to_string();
        thread::Builder::new()
            .name(format!("st-script-{sid}"))
            .spawn(move || run_script_cmds(inner, sid, cmds, cwd, env, timeout))
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法启动脚本线程: {e}")))?;
        Ok(())
    }

    pub fn cancel_script(&self) -> Result<()> {
        let g = self.inner.lock().expect("engine lock");
        if g.workspace_id.is_empty() {
            return Ok(());
        }
        let Some(slot) = g.script.as_ref() else {
            return Ok(());
        };
        slot.cancel.store(true, Ordering::SeqCst);
        if let Some(job) = &slot.job {
            job.terminate()?;
        }
        Ok(())
    }

    fn wait_script_idle(&self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let running = self
                .inner
                .lock()
                .ok()
                .and_then(|g| g.script.as_ref().map(|s| s.state == ScriptState::Running))
                .unwrap_or(false);
            if !running {
                return;
            }
            thread::sleep(Duration::from_millis(40));
        }
    }

    fn ensure_started(&self, id: &str) -> Result<()> {
        let deps = {
            let g = self.inner.lock().expect("engine lock");
            let svc = g
                .spec
                .services
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            svc.depends_on.clone()
        };
        for dep in &deps {
            let dep_disabled = {
                let g = self.inner.lock().expect("engine lock");
                crate::profiles::effective_service(&g.spec, dep)
                    .map(|e| !e.enabled)
                    .unwrap_or(false)
            };
            if dep_disabled {
                continue; // 禁用依赖不拉起，视为满足
            }
            self.ensure_started(dep)?;
            self.wait_ready(dep, Duration::from_secs(90))?;
            match self.state_of(dep) {
                Some(RtState::Exited) | Some(RtState::Stopped) => {
                    return Err(Error::new(
                        ErrorCode::DepDead,
                        format!("依赖 {dep} 失败，未启动 {id}"),
                    ));
                }
                _ => {}
            }
        }
        self.spawn_service(id)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_service(&self, id: &str) -> Result<()> {
        let (
            run_spec,
            build_spec,
            reactor_prep,
            root,
            health_spec,
            health_none,
            port,
            kind,
            pkg,
            svc_grace,
            is_jar,
            bt,
            module,
            cwd,
            env_snapshot,
            restart,
        ) = {
            let mut g = self.inner.lock().expect("engine lock");
            let slot = g
                .slots
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            match slot.state {
                RtState::Starting | RtState::Running | RtState::Unhealthy => return Ok(()),
                RtState::Stopping | RtState::Building => {
                    return Err(Error::new(
                        ErrorCode::AlreadyInProgress,
                        format!("{id} 正在停止或构建"),
                    ));
                }
                RtState::Stopped | RtState::Exited => {}
            }
            // 1.2 §10：base + active profile overlay（不写回 base 字段）
            let eff_spec = crate::profiles::overlay_spec(&g.spec, id)?;
            let eff_svc = eff_spec.services.get(id).unwrap().clone();
            // 2.2 restart：手动启动重置预算/序号/撤销标志（spec 改动随重启生效）
            let restart = crate::spec::resolve_restart(&eff_svc);
            if let Some(slot) = g.slots.get_mut(id) {
                slot.restart = restart;
                slot.restart_left = match restart.policy {
                    crate::spec::RestartPolicy::Never => 0,
                    _ => restart.max_retries,
                };
                slot.restart_attempt = None;
                slot.restart_cancel = Arc::new(AtomicBool::new(false));
            }
            if eff_svc.kind == "compose" {
                // 1.3 §5.2：compose 服务走容器运行时分支（up/stop），不经本地命令规划
                drop(g);
                return self.spawn_compose(id, &eff_svc);
            }
            let is_jar = eff_svc.kind == "spring-boot" && eff_svc.launch.as_deref() == Some("jar");
            // §6.3 环境链：ws+profile < secrets/env_file < 服务+profile env < 端口注入 < 网络注入(最低)
            let app_net = self.app_network.lock().expect("app_network lock").clone();
            let (mut env, env_sources) =
                build_service_env(&eff_spec, id, &g.root, app_net.as_ref())?;
            // P2：服务 env 的版本选择优先于工作区 toolchain，命中本机已装安装后
            // 前插子进程 PATH +（java）JAVA_HOME。仅真机 spawner 且确有钉扎时探测。
            if matches!(self.spawner, SpawnerKind::Real) {
                let tc = eff_spec.toolchain.as_ref();
                let has_pin = tc
                    .map(|t| match eff_svc.kind.as_str() {
                        "spring-boot" => {
                            t.java.as_deref().is_some_and(|v| !v.trim().is_empty())
                                || t.maven.as_deref().is_some_and(|v| !v.trim().is_empty())
                        }
                        "node" => t.node.as_deref().is_some_and(|v| !v.trim().is_empty()),
                        "python" => t.python.as_deref().is_some_and(|v| !v.trim().is_empty()),
                        "go" => t.go.as_deref().is_some_and(|v| !v.trim().is_empty()),
                        _ => false,
                    })
                    .unwrap_or(false)
                    || (eff_svc.kind == "spring-boot"
                        && crate::launcher::project_java_version(
                            &g.root,
                            eff_svc.cwd.as_deref().unwrap_or("."),
                        )
                        .is_some())
                    || eff_svc.env.iter().any(|(key, value)| {
                        !value.trim().is_empty()
                            && matches!(
                                (eff_svc.kind.as_str(), key.as_str()),
                                ("spring-boot", crate::launcher::SERVICE_JAVA_VERSION_ENV)
                                    | ("spring-boot", crate::launcher::SERVICE_MAVEN_VERSION_ENV)
                                    | ("node", crate::launcher::SERVICE_NODE_VERSION_ENV)
                                    | ("python", crate::launcher::SERVICE_PYTHON_VERSION_ENV)
                                    | ("go", crate::launcher::SERVICE_GO_VERSION_ENV)
                            )
                    });
                if has_pin {
                    let installs = self.toolchain_probe(false).tools.installs;
                    if eff_svc.kind == "spring-boot" {
                        crate::launcher::apply_java_version_env(
                            tc,
                            &eff_svc.env,
                            &g.root,
                            eff_svc.cwd.as_deref().unwrap_or("."),
                            &installs,
                            &mut env,
                        );
                    } else {
                        crate::launcher::apply_pinned_version_env(
                            tc,
                            &eff_svc.env,
                            &eff_svc.kind,
                            &installs,
                            &mut env,
                        );
                    }
                    if eff_svc.kind == "spring-boot"
                        && (tc
                            .and_then(|t| t.maven.as_deref())
                            .is_some_and(|v| !v.trim().is_empty())
                            || eff_svc
                                .env
                                .get(crate::launcher::SERVICE_MAVEN_VERSION_ENV)
                                .is_some_and(|v| !v.trim().is_empty()))
                    {
                        crate::launcher::apply_pinned_version_env(
                            tc,
                            &eff_svc.env,
                            "maven",
                            &installs,
                            &mut env,
                        );
                    }
                }
            }
            // 1.4 §5.1：build_tool 解析（显式优先，缺省按构建文件探测）。
            // 测试 spawner 无真实 fs 上下文：只认显式字段，缺省按 maven。
            let real = matches!(self.spawner, SpawnerKind::Real);
            let plan_root = real.then_some(g.root.as_path());
            let bt = if eff_svc.kind == "spring-boot" {
                if real {
                    crate::launcher::resolve_build_tool(&g.root, &eff_svc)?
                } else {
                    crate::launcher::explicit_build_tool(&eff_svc)
                        .unwrap_or(crate::launcher::BuildTool::Maven)
                }
            } else {
                crate::launcher::BuildTool::Maven
            };
            let (planned, build_spec, reactor_prep) = if is_jar {
                let build = crate::launcher::plan_jar_build_in(&eff_svc, env.clone(), plan_root)?;
                let run = crate::launcher::plan_jar_run(&eff_svc, env);
                (run, Some(build), None)
            } else {
                let mut planned = crate::launcher::plan_service_in(&eff_spec, id, plan_root)?;
                planned.env = env.clone();
                // 1.7 §5：显式 `toolchain.manager: mise` 时合并 mise 工具的 PATH env_delta
                if matches!(self.spawner, SpawnerKind::Real) {
                    crate::launcher::apply_pinned_mise_env(
                        eff_spec.toolchain.as_ref(),
                        &eff_svc.kind,
                        &g.root,
                        &mut planned.env,
                    );
                }
                let reactor_prep = plan_root.and_then(|r| {
                    if eff_svc.kind == "spring-boot"
                        && eff_svc.launch.as_deref().unwrap_or("run") == "run"
                        && bt == crate::launcher::BuildTool::Maven
                    {
                        crate::launcher::plan_maven_reactor_prep_install_in(
                            &eff_svc,
                            env.clone(),
                            r,
                        )
                    } else {
                        None
                    }
                });
                (planned, None, reactor_prep)
            };
            let health_spec = eff_svc.health.clone();
            let health_none = health_spec
                .as_ref()
                .map(|h| h.r#type == HealthType::None)
                .unwrap_or(true);
            let pkg = eff_svc.package_manager.map(|p| match p {
                crate::spec::PackageManager::Npm => "npm",
                crate::spec::PackageManager::Pnpm => "pnpm",
                crate::spec::PackageManager::Yarn => "yarn",
                crate::spec::PackageManager::Bun => "bun",
            });
            let module = eff_svc.module.clone().unwrap_or_else(|| ".".into());
            let cwd = eff_svc.cwd.clone().unwrap_or_else(|| ".".into());
            // 生效环境快照：本次启动真正注入 planned.env 的键值 + 来源层
            // （mise PATH 增量不在 env_sources 里 → 归 toolchain）。
            let env_snapshot: Vec<crate::ipc::EnvEffectiveEntry> = planned
                .env
                .iter()
                .map(|(k, v)| crate::ipc::EnvEffectiveEntry {
                    key: k.clone(),
                    value: v.clone(),
                    source: env_sources
                        .get(k)
                        .copied()
                        .unwrap_or("toolchain")
                        .to_string(),
                })
                .collect();
            (
                planned,
                build_spec,
                reactor_prep,
                g.root.clone(),
                health_spec,
                health_none,
                eff_svc.port,
                eff_svc.kind.clone(),
                pkg,
                eff_svc.grace_secs.unwrap_or(0) as u64,
                is_jar,
                bt,
                module,
                cwd,
                env_snapshot,
                restart,
            )
        };

        // 启动前端口归属复核：被外部进程占位直接拒绝（PORT_IN_USE + 换端口指引）。
        if let Some(p) = port {
            ensure_port_not_conflicted(&root, id, p, &kind)?;
        }

        if is_jar {
            // 1.2 §11：package（Building）→ artifact → java -jar。构建在后台线程，
            // startOne 立即 accepted；输出进服务日志。
            let inner = Arc::clone(&self.inner);
            let id2 = id.to_string();
            let spawner = self.spawner;
            let _ = thread::Builder::new()
                .name(format!("st-jar-{id}"))
                .spawn(move || {
                    let r = jar_flow(
                        inner.clone(),
                        &id2,
                        build_spec.expect("jar build spec"),
                        run_spec,
                        root,
                        health_spec,
                        health_none,
                        port,
                        kind,
                        pkg,
                        svc_grace,
                        bt,
                        spawner,
                        env_snapshot,
                        restart,
                    );
                    if let Err(e) = r {
                        jar_flow_fail(&inner, &id2, e);
                    }
                });
            return Ok(());
        }

        if let Some(prep_spec) = reactor_prep {
            let inner = Arc::clone(&self.inner);
            let id2 = id.to_string();
            let spawner = self.spawner;
            let _ = thread::Builder::new()
                .name(format!("st-reactor-{id}"))
                .spawn(move || {
                    let r = maven_reactor_run_flow(
                        inner.clone(),
                        &id2,
                        prep_spec,
                        run_spec,
                        root,
                        health_spec,
                        health_none,
                        port,
                        kind,
                        pkg,
                        svc_grace,
                        bt,
                        spawner,
                        env_snapshot,
                        restart,
                    );
                    if let Err(e) = r {
                        jar_flow_fail(&inner, &id2, e);
                    }
                });
            return Ok(());
        }

        if matches!(self.spawner, SpawnerKind::Real) {
            probe::require_tools_for_kind_with_path(
                &kind,
                pkg,
                Some(bt.as_str()),
                &run_spec.program,
                run_spec.env.get("PATH").map(String::as_str),
            )?;
        }
        // 1.4 §5.1：gradle 服务 wrapper 优先（root/module gradlew[.bat] → PATH gradle），
        // 都无 → GRADLE_WRAPPER_MISSING；测试 spawner 跳过 fs 解析。
        let mut run_spec = run_spec;
        if bt == crate::launcher::BuildTool::Gradle && matches!(self.spawner, SpawnerKind::Real) {
            let (program, args, warns) = crate::launcher::resolve_gradle_launcher(
                &root,
                &cwd,
                &module,
                &run_spec.program,
                &run_spec.args,
            )?;
            for w in warns {
                push_line(
                    &self.inner,
                    LogSource {
                        kind: LogSourceKind::Service,
                        id: id.to_string(),
                    },
                    LogStream::System,
                    w,
                );
            }
            run_spec.program = program;
            run_spec.args = args;
        }
        let cwd = resolve_cwd(&root, &run_spec.cwd_rel)?;
        spawn_core(
            Arc::clone(&self.inner),
            id.to_string(),
            run_spec,
            cwd,
            health_spec,
            health_none,
            port,
            kind,
            pkg,
            svc_grace,
            Some(bt.as_str()),
            self.spawner,
            env_snapshot,
            restart,
        )
    }

    // -------------------------------------------------------------------
    // 1.2 phase 3–6：端口 / secrets / 日志 / 指标 / profile / build
    // -------------------------------------------------------------------

    /// §5.1 端口检查：本机 TCP 监听表 + 引擎托管 PID 对照。
    /// 自身进程树（含 mvn 派生的 java 等）不算占用；引擎其它 Job 树占用的端口
    /// 标记 managed。`target = Some(p)` 时只检查该候选端口（配合环境页输入框），
    /// 否则检查全部已配置端口。
    pub fn ports_inspect(
        &self,
        id: &str,
        target: Option<u16>,
    ) -> Result<Vec<crate::ipc::PortInspection>> {
        let (spec, managed, own) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let mut managed: std::collections::HashSet<u32> = std::collections::HashSet::new();
            let mut own: std::collections::HashMap<String, crate::ports::OwnRuntime> =
                std::collections::HashMap::new();
            for (slot_id, s) in &g.slots {
                // 本服务整个 Job 树（含派生子进程）——Spring 由 mvn 派生 java 监听
                let mut pids = s.job.as_ref().map(|j| j.pids()).unwrap_or_default();
                if let Some(p) = s.pid {
                    if !pids.contains(&p) {
                        pids.push(p);
                    }
                }
                if s.managed {
                    managed.extend(pids.iter().copied());
                }
                own.insert(
                    slot_id.clone(),
                    crate::ports::OwnRuntime {
                        pids,
                        running: matches!(
                            s.state,
                            RtState::Running | RtState::Starting | RtState::Unhealthy
                        ),
                    },
                );
            }
            // 1.6 §2.4：网关进程树计入托管集合（网关自身端口按「当前由网关
            // 占用」的托管语义呈现）
            if let Some(gw) = &g.gateway {
                let mut pids = gw.job.as_ref().map(|j| j.pids()).unwrap_or_default();
                if let Some(p) = gw.pid {
                    if !pids.contains(&p) {
                        pids.push(p);
                    }
                }
                managed.extend(pids.iter().copied());
                own.insert(
                    "gateway".to_string(),
                    crate::ports::OwnRuntime {
                        pids,
                        running: matches!(
                            gw.state,
                            RtState::Running | RtState::Starting | RtState::Unhealthy
                        ),
                    },
                );
            }
            (g.spec.clone(), managed, own)
        };
        let listeners = crate::ports::tcp_listeners()?;
        match target {
            Some(p) => Ok(vec![crate::ports::inspect_single(
                &spec, id, p, &listeners, &managed, &own,
            )?]),
            None => Ok(crate::ports::inspect(&spec, &listeners, &managed, &own)),
        }
    }

    /// §5.2 建议端口（跳过兄弟服务/系统保留/已监听）。
    pub fn ports_suggest(&self, id: &str) -> Result<Vec<u16>> {
        let spec = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            g.spec.clone()
        };
        let listeners = crate::ports::tcp_listeners()?;
        crate::ports::suggest(&spec, id, &listeners)
    }

    /// §5.3/§5.4 一键改端口。运行中且未确认 restart → 只预览（restart_required）。
    /// restart=true：stop → 结构化保存（base_hash 冲突不部分写入）→ start。
    pub fn ports_assign(
        &self,
        id: &str,
        port: u16,
        base_hash: &str,
        restart: bool,
    ) -> Result<PortsAssignView> {
        let running = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let slot = g
                .slots
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            matches!(
                slot.state,
                RtState::Starting
                    | RtState::Running
                    | RtState::Unhealthy
                    | RtState::Stopping
                    | RtState::Building
            )
        };
        let mut preview = self.spec()?;
        let notes = crate::ports::apply_port_assign(&mut preview, id, port)?;
        if running && !restart {
            let hash = self.yaml_get()?.hash;
            return Ok(PortsAssignView {
                spec: preview,
                hash,
                notes,
                restart_required: true,
            });
        }
        if running {
            self.stop_one(id)?;
        }
        let saved = self.save_form(&preview, base_hash);
        if running {
            // stop 已成功但保存失败：服务保持 stopped，错误原样上抛（§5.4）
            let (spec, hash, _) = saved?;
            self.start_one(id)?;
            return Ok(PortsAssignView {
                spec,
                hash,
                notes,
                restart_required: false,
            });
        }
        let (spec, hash, _) = saved?;
        Ok(PortsAssignView {
            spec,
            hash,
            notes,
            restart_required: false,
        })
    }

    // ---- secrets（值绝不返回）----

    pub fn secrets_status(&self) -> Result<crate::ipc::SecretsStatusOutput> {
        let (spec, root) = self.ws_ref()?;
        crate::secrets::status(&spec, &root)
    }

    pub fn secrets_set(&self, key: &str, value: &str) -> Result<()> {
        let (spec, root) = self.ws_ref()?;
        crate::secrets::set_key(&spec, &root, key, value)
    }

    pub fn secrets_delete(&self, key: &str) -> Result<()> {
        let (spec, root) = self.ws_ref()?;
        crate::secrets::delete_key(&spec, &root, key)
    }

    pub fn secrets_validate(
        &self,
        service: Option<&str>,
    ) -> Result<crate::ipc::SecretsValidateOutput> {
        let (spec, root) = self.ws_ref()?;
        crate::secrets::validate(&spec, &root, service)
    }

    // ---- 日志历史 / 保留 ----

    pub fn search_logs(
        &self,
        source: Option<&LogSource>,
        query: &str,
        case_sensitive: bool,
        limit: Option<usize>,
    ) -> Result<crate::log::SearchResult> {
        let (_, root) = self.ws_ref()?;
        crate::log::search_logs(&root, source, query, case_sensitive, limit)
    }

    pub fn export_logs(
        &self,
        source: Option<&LogSource>,
        query: Option<&str>,
        case_sensitive: bool,
        format: &str,
        dest: &Path,
    ) -> Result<usize> {
        let (_, root) = self.ws_ref()?;
        crate::log::export_logs(&root, source, query, case_sensitive, format, dest)
    }

    pub fn run_log_retention(&self) -> Result<crate::log::RetentionSummary> {
        let (_, root) = self.ws_ref()?;
        let g = self.inner.lock().expect("engine lock");
        crate::log::run_retention(&root, g.spec.log_retention.as_ref())
    }

    // ---- 指标 ----

    pub fn metrics_subscribe(&self) -> Result<()> {
        let start = {
            let mut g = self.inner.lock().expect("engine lock");
            g.metrics_sub += 1;
            g.metrics_sub == 1
        };
        if start {
            let inner = Arc::clone(&self.inner);
            thread::Builder::new()
                .name("st-metrics".into())
                .spawn(move || metrics_loop(inner))
                .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法启动 sampler: {e}")))?;
        }
        Ok(())
    }

    pub fn metrics_unsubscribe(&self) -> Result<()> {
        let mut g = self.inner.lock().expect("engine lock");
        g.metrics_sub = g.metrics_sub.saturating_sub(1);
        Ok(())
    }

    pub fn metrics_snapshot(&self) -> Result<crate::ipc::MetricsSnapshotOutput> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Ok(crate::ipc::MetricsSnapshotOutput {
            services: g.metrics.clone(),
        })
    }

    // ---- 运行页终端（ipc.md §10.15）：cwd + 环境链（PTY 会话由壳层托管）----

    /// `env.effective`：最近一次启动实际注入的环境快照（引擎自报，非读进程内存）。
    /// 快照持久化在 `.supertask/env-snapshots.json`，重启/重开工作区后可回看。
    /// 从未本地启动过（或 compose 服务）→ entries 空、captured_at_ms None。
    pub fn env_effective(&self, id: &str) -> Result<crate::ipc::EnvEffectiveOutput> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        let slot = g
            .slots
            .get(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
        Ok(crate::ipc::EnvEffectiveOutput {
            id: id.to_string(),
            captured_at_ms: slot
                .env_snapshot
                .as_ref()
                .map(|s| s.captured_at_ms)
                .filter(|t| *t > 0),
            entries: slot
                .env_snapshot
                .as_ref()
                .map(|s| s.entries.clone())
                .unwrap_or_default(),
        })
    }

    /// `spring.inspect`：静态解析 spring-boot 服务的项目自身配置
    /// （`src/main/resources/application*.{yml,yaml,properties}`）。
    /// 搜索顺序与 launch 一致取目录链：cwd → module → 根；非 spring-boot 返回空结果。
    pub fn spring_inspect(&self, id: &str) -> Result<crate::spring::SpringConfigOutput> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        let svc = g
            .spec
            .services
            .get(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
        if svc.kind != "spring-boot" {
            return Ok(crate::spring::SpringConfigOutput {
                id: id.to_string(),
                server_port: None,
                entries: Vec::new(),
                warnings: Vec::new(),
            });
        }
        let mut dirs: Vec<String> = Vec::new();
        for d in [svc.cwd.as_deref(), svc.module.as_deref(), Some(".")] {
            if let Some(d) = d {
                if !dirs.iter().any(|seen| seen == d) {
                    dirs.push(d.to_string());
                }
            }
        }
        Ok(crate::spring::inspect(id, &g.root, &dirs))
    }

    /// 终端目标目录与环境。service_id 缺省 = 工作区根 + 工作区环境链；
    /// 指定服务 = 服务 cwd（与启动一致，复用 plan cwd_rel 解析）+ 服务环境链
    /// （§6.3 环境链 + 1.7 §7 镜像/代理注入，注入最低优先级）。
    pub fn term_target(&self, service_id: Option<&str>) -> Result<crate::term::TermTarget> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        crate::toolchain::resolver::refresh_process_path();
        let app_net = self.app_network.lock().expect("app_network lock").clone();
        let root = g.root.clone();
        let mut env = match service_id {
            Some(id) => {
                let eff_spec = crate::profiles::overlay_spec(&g.spec, id)?;
                build_service_env(&eff_spec, id, &root, app_net.as_ref())?.0
            }
            None => {
                let (file_env, _warnings) = crate::secrets::load_file_layers(&g.spec, &root, None)?;
                let mut env = g.spec.env.clone();
                for (k, v) in file_env {
                    env.insert(k, v);
                }
                let eff_net = crate::network::resolve(g.spec.network.as_ref(), app_net.as_ref())?;
                let (_, _inject_warns) = crate::network::inject_env(&eff_net, &root, &mut env);
                env
            }
        };
        if let Some(id) = service_id {
            let eff_spec = crate::profiles::overlay_spec(&g.spec, id)?;
            let svc = eff_spec
                .services
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            let installs = self.toolchain_probe(false).tools.installs;
            if svc.kind == "spring-boot" {
                crate::launcher::apply_java_version_env(
                    eff_spec.toolchain.as_ref(),
                    &svc.env,
                    &root,
                    svc.cwd.as_deref().unwrap_or("."),
                    &installs,
                    &mut env,
                );
                if eff_spec
                    .toolchain
                    .as_ref()
                    .and_then(|tc| tc.maven.as_deref())
                    .is_some_and(|v| !v.trim().is_empty())
                    || svc
                        .env
                        .get(crate::launcher::SERVICE_MAVEN_VERSION_ENV)
                        .is_some_and(|v| !v.trim().is_empty())
                {
                    crate::launcher::apply_pinned_version_env(
                        eff_spec.toolchain.as_ref(),
                        &svc.env,
                        "maven",
                        &installs,
                        &mut env,
                    );
                }
            } else {
                crate::launcher::apply_pinned_version_env(
                    eff_spec.toolchain.as_ref(),
                    &svc.env,
                    &svc.kind,
                    &installs,
                    &mut env,
                );
            }
            crate::launcher::apply_pinned_mise_env(
                eff_spec.toolchain.as_ref(),
                &svc.kind,
                &root,
                &mut env,
            );
        }
        let cwd = match service_id {
            Some(id) => {
                let eff_spec = crate::profiles::overlay_spec(&g.spec, id)?;
                let run_spec = crate::launcher::plan_service_in(&eff_spec, id, Some(&root))?;
                resolve_cwd(&root, &run_spec.cwd_rel)?
            }
            None => root.clone(),
        };
        Ok(crate::term::TermTarget { cwd, env })
    }

    // ---- profile ----

    pub fn profiles_list(&self) -> Result<crate::ipc::ProfilesListOutput> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Ok(crate::profiles::list(&g.spec))
    }

    /// §10.2 切换 active profile：忙则 PROFILE_SWITCH_BUSY；base_hash 保存。
    pub fn profiles_activate(&self, id: &str, base_hash: &str) -> Result<(SuperTaskFile, String)> {
        crate::profiles::require_profile(&self.spec()?, id)?;
        {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let busy = g.slots.values().any(|s| {
                matches!(
                    s.state,
                    RtState::Starting
                        | RtState::Running
                        | RtState::Unhealthy
                        | RtState::Stopping
                        | RtState::Building
                )
            }) || g
                .script
                .as_ref()
                .is_some_and(|s| s.state == ScriptState::Running);
            if busy {
                return Err(Error::new(
                    ErrorCode::ProfileSwitchBusy,
                    "有服务或脚本正在运行，停止后再切换 profile",
                ));
            }
        }
        let mut spec = self.spec()?;
        match &mut spec.profiles {
            Some(p) => p.active = Some(id.to_string()),
            None => {
                spec.profiles = Some(crate::spec::ProfilesSpec {
                    active: Some(id.to_string()),
                    items: Default::default(),
                    extra: Default::default(),
                })
            }
        }
        let (spec, hash, _) = self.save_form(&spec, base_hash)?;
        Ok((spec, hash))
    }

    /// 1.2 §11 runtime.build：预构建 launch: jar 的 artifact（不启动）。
    pub fn build_jar(&self, id: &str) -> Result<PathBuf> {
        let (build_spec, root, bt) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let slot = g
                .slots
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            if slot.state == RtState::Building {
                return Err(Error::new(ErrorCode::BuildBusy, format!("{id} 正在构建")));
            }
            if matches!(
                slot.state,
                RtState::Starting | RtState::Running | RtState::Unhealthy | RtState::Stopping
            ) {
                return Err(Error::new(
                    ErrorCode::BuildBusy,
                    format!("{id} 运行中，停止后再构建"),
                ));
            }
            let eff_spec = crate::profiles::overlay_spec(&g.spec, id)?;
            let eff_svc = eff_spec.services.get(id).unwrap().clone();
            if !(eff_svc.kind == "spring-boot" && eff_svc.launch.as_deref() == Some("jar")) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id} 不是 launch: jar 的 spring-boot 服务"),
                ));
            }
            let bt = crate::launcher::resolve_build_tool(&g.root, &eff_svc)?;
            let app_net = self.app_network.lock().expect("app_network lock").clone();
            let (mut env, _) = build_service_env(&eff_spec, id, &g.root, app_net.as_ref())?;
            let installs = self.toolchain_probe(false).tools.installs;
            crate::launcher::apply_java_version_env(
                eff_spec.toolchain.as_ref(),
                &eff_svc.env,
                &g.root,
                eff_svc.cwd.as_deref().unwrap_or("."),
                &installs,
                &mut env,
            );
            if eff_spec
                .toolchain
                .as_ref()
                .and_then(|tc| tc.maven.as_deref())
                .is_some_and(|v| !v.trim().is_empty())
                || eff_svc
                    .env
                    .get(crate::launcher::SERVICE_MAVEN_VERSION_ENV)
                    .is_some_and(|v| !v.trim().is_empty())
            {
                crate::launcher::apply_pinned_version_env(
                    eff_spec.toolchain.as_ref(),
                    &eff_svc.env,
                    "maven",
                    &installs,
                    &mut env,
                );
            }
            (
                crate::launcher::plan_jar_build_in(&eff_svc, env, Some(&g.root))?,
                g.root.clone(),
                bt,
            )
        };
        jar_build_phase(
            Arc::clone(&self.inner),
            id,
            build_spec,
            &root,
            bt,
            BuildPhaseKind::JarArtifact,
        )
    }

    // -------------------------------------------------------------------
    // 1.3 phase 3/4：compose 运行时（§5）与镜像构建（§6）
    // -------------------------------------------------------------------

    /// §4.1 docker 探测：会话内缓存；`refresh=true` 强制刷新（「重试探测」按钮）。
    /// 不要求已打开工作区。结果不改写 DOCKER_HOST 等用户环境。
    pub fn docker_probe(&self, refresh: bool) -> crate::docker::DockerProbe {
        let mut cache = self.probe_cache.lock().expect("probe cache");
        if refresh || cache.is_none() {
            *cache = Some(crate::docker::probe_docker(self.docker.as_ref()));
        }
        cache.clone().expect("probe cache")
    }

    fn probe_ready(&self) -> Result<crate::docker::DockerProbe> {
        let probe = self.docker_probe(false);
        crate::docker::ensure_compose_ready(&probe)?;
        Ok(probe)
    }

    /// §9 docker.ps：当前 compose project 的容器只读列表（无 compose 文件则空）。
    pub fn docker_ps(&self) -> Result<Vec<crate::ipc::ContainerSummary>> {
        let (root, file, project) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            match self.compose_file_of(&g)? {
                None => return Ok(Vec::new()),
                Some((f, p)) => (g.root.clone(), f, p),
            }
        };
        let mut args = crate::docker::compose_base_args(&file, project.as_deref());
        args.extend(["ps".to_string(), "--format".to_string(), "json".to_string()]);
        let out = self
            .docker
            .run(&DockerSpawn {
                args,
                cwd: Some(root),
                timeout: COMPOSE_QUERY_TIMEOUT,
            })
            .map_err(map_docker_spawn_err)?;
        if out.code != 0 {
            return Err(Error::new(
                ErrorCode::ComposeConfigFailed,
                format!(
                    "docker compose ps 退出码 {}: {}",
                    out.code,
                    out.stderr.trim()
                ),
            ));
        }
        Ok(crate::docker::parse_ps(&out.stdout)
            .iter()
            .map(|c| c.summary())
            .collect())
    }

    /// §9 docker.images：本机镜像只读列表（不要求工作区）。
    pub fn docker_images(&self) -> Result<Vec<crate::ipc::ImageSummary>> {
        let out = self
            .docker
            .run(&DockerSpawn {
                args: vec!["images".into(), "--format".into(), "json".into()],
                cwd: None,
                timeout: COMPOSE_QUERY_TIMEOUT,
            })
            .map_err(map_docker_spawn_err)?;
        if out.code != 0 {
            return Err(Error::new(
                ErrorCode::DockerEngineUnreachable,
                format!("docker images 退出码 {}: {}", out.code, out.stderr.trim()),
            ));
        }
        Ok(crate::docker::parse_images(&out.stdout))
    }

    /// 解析当前 compose 文件：显式 `docker.compose_file` 或按 §7 候选顺序探测。
    /// 返回 (绝对路径, project name)；无 compose 文件 → None。
    fn compose_file_of(&self, g: &Inner) -> Result<Option<(PathBuf, Option<String>)>> {
        let Some(cfg) = g.spec.docker.as_ref() else {
            return Ok(None);
        };
        let rel = match cfg.compose_file.clone() {
            Some(r) => r,
            None => match crate::scan::discover_compose_file(&g.root) {
                Some(r) => r,
                None => return Ok(None),
            },
        };
        Ok(Some((
            sandbox::confine(&g.root, &rel)?,
            cfg.project_name.clone(),
        )))
    }

    /// §5.2 compose 启动：同步前置检查（失败不 accepted）→ 异步 up。
    /// 同步部分：docker 三态、compose 文件存在、service 在解析结果中、
    /// 端口 PORT_DUP（1.2 口径，compose 主机端口参与）。
    fn spawn_compose(&self, id: &str, svc: &crate::spec::ServiceSpec) -> Result<()> {
        // 1) docker 三态前置（probe 缓存；锁外执行，最多 5s）
        self.probe_ready()?;
        let service = svc.service.clone().ok_or_else(|| {
            Error::new(
                ErrorCode::SpecInvalid,
                format!("{id}: kind: compose 缺少 service 字段"),
            )
        })?;
        let (file, project) = {
            let g = self.inner.lock().expect("engine lock");
            match self.compose_file_of(&g)? {
                Some(v) => v,
                None => {
                    return Err(Error::new(
                        ErrorCode::ComposeFileMissing,
                        format!(
                            "{id}: 未找到 compose 文件（docker.compose_file 未配置且工作区根没有 compose.yaml / compose.yml / docker-compose.yml / docker-compose.yaml）"
                        ),
                    ))
                }
            }
        };
        // 2) service 必须在 compose 解析结果中（loader 带 mtime+hash 缓存）
        let model = {
            let g = self.inner.lock().expect("engine lock");
            let rel = g
                .spec
                .docker
                .as_ref()
                .and_then(|d| d.compose_file.clone())
                .or_else(|| crate::scan::discover_compose_file(&g.root));
            match rel {
                Some(r) => {
                    let project = g.spec.docker.as_ref().and_then(|d| d.project_name.clone());
                    self.compose_loader.load(&g.root, &r, project.as_deref())?
                }
                None => {
                    return Err(Error::new(
                        ErrorCode::ComposeFileMissing,
                        "未找到 compose 文件",
                    ))
                }
            }
        };
        if model.find(&service).is_none() {
            return Err(Error::new(
                ErrorCode::ComposeServiceMissing,
                format!("{id}: compose 文件中没有服务 {service:?}"),
            ));
        }
        // 3) 端口 PORT_DUP：与其他启用服务撞端口（1.2 口径）
        if let Some(p) = svc.port {
            let dup = {
                let g = self.inner.lock().expect("engine lock");
                g.spec
                    .services
                    .iter()
                    .any(|(oid, osvc)| oid != id && osvc.enabled && osvc.port == Some(p))
            };
            if dup {
                return Err(Error::new(
                    ErrorCode::PortDup,
                    format!("{id}: 端口 {p} 与其他服务重复"),
                ));
            }
            // 3b) 端口被外部进程占位 → PORT_IN_USE（精确归属判定，不只看端口通断）
            let root = self.inner.lock().expect("engine lock").root.clone();
            ensure_port_not_conflicted(&root, id, p, &svc.kind)?;
        }
        // 设 Starting + compose 上下文（up 异步执行，startOne 立即 accepted）
        let health = compose_health(svc);
        let info = ComposeInfo {
            file,
            project,
            service,
            started_by_engine: false,
        };
        let port = svc.port;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut g = self.inner.lock().expect("engine lock");
            let slot = g.slots.get_mut(id).expect("slot exists");
            slot.state = RtState::Starting;
            slot.started = Some(Instant::now());
            slot.started_at_ms = Some(now_ms());
            slot.last_error = None;
            slot.last_exit = None;
            slot.exit_reason = None;
            slot.stop_requested = false;
            slot.cancel = Arc::clone(&cancel);
            slot.port = port;
            slot.kind = "compose".into();
            slot.grace = Duration::from_secs(
                svc.grace_secs
                    .map(|s| s as u64)
                    .unwrap_or(COMPOSE_DEFAULT_GRACE_SECS),
            );
            slot.managed = true;
            slot.compose = Some(info.clone());
            emit_runtime(&g);
        }
        let inner = Arc::clone(&self.inner);
        let runner = Arc::clone(&self.docker);
        let id2 = id.to_string();
        thread::Builder::new()
            .name(format!("st-compose-{id}"))
            .spawn(move || {
                compose_up_flow(inner, id2, info, port, health, cancel, runner);
            })
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法启动 compose 线程: {e}")))?;
        Ok(())
    }

    /// §5.2 停止：`docker compose stop <service>`；非零 → 重查容器实际状态
    /// 对齐 UI；daemon 不可达 → 记录错误，不假报成功。
    fn compose_stop(&self, id: &str, info: ComposeInfo) -> Result<()> {
        let root = self.inner.lock().expect("engine lock").root.clone();
        let src = LogSource {
            kind: LogSourceKind::Service,
            id: id.to_string(),
        };
        let outcome = self.docker.run(&DockerSpawn {
            args: compose_stop_args(&info),
            cwd: Some(root.clone()),
            timeout: COMPOSE_STOP_TIMEOUT,
        });
        let mut g = self.inner.lock().expect("engine lock");
        let Some(slot) = g.slots.get_mut(id) else {
            return Ok(()); // 工作区已切换：容器交给 daemon
        };
        match outcome {
            Ok(out) if out.code == 0 => {
                slot.state = RtState::Stopped;
                slot.exit_reason = None;
                slot.started = None;
                emit_runtime(&g);
                Ok(())
            }
            Ok(out) => {
                push_line(
                    &self.inner,
                    src.clone(),
                    LogStream::System,
                    format!(
                        "COMPOSE_STOP_FAILED: docker compose stop 退出码 {}",
                        out.code
                    ),
                );
                slot.last_error = Some(format!(
                    "COMPOSE_STOP_FAILED: docker compose stop 退出码 {}",
                    out.code
                ));
                // 重查实际状态对齐 UI（§5.2）
                match compose_container_running(&self.docker, &root, &info) {
                    Some(false) => {
                        slot.state = RtState::Stopped;
                        slot.exit_reason = None;
                        emit_runtime(&g);
                        Ok(())
                    }
                    Some(true) => {
                        slot.state = RtState::Running;
                        emit_runtime(&g);
                        Err(Error::new(
                            ErrorCode::ComposeStopFailed,
                            format!(
                                "{}: docker compose stop 退出码 {}，容器仍在运行",
                                id, out.code
                            ),
                        ))
                    }
                    None => {
                        // daemon 不可达：保持现状，不假报成功
                        emit_runtime(&g);
                        Err(Error::new(
                            ErrorCode::ComposeStopFailed,
                            format!("{id}: 停止后无法确认容器状态（Docker 不可达？）"),
                        ))
                    }
                }
            }
            Err(e) => {
                let msg = if e.kind() == std::io::ErrorKind::NotFound {
                    "COMPOSE_STOP_FAILED: 未找到 docker。请安装 Docker Desktop 并确保在 PATH 中。"
                        .to_string()
                } else {
                    format!("COMPOSE_STOP_FAILED: docker compose stop 执行失败: {e}")
                };
                push_line(&self.inner, src, LogStream::System, msg.clone());
                slot.last_error = Some(msg);
                emit_runtime(&g);
                Err(Error::new(
                    ErrorCode::ComposeStopFailed,
                    "docker compose stop 执行失败",
                ))
            }
        }
    }

    /// §6.2 `docker.build`：builds 条目镜像构建，走 operation（可取消，无超时）。
    /// name 必须在 YAML `docker.builds` 中；context/dockerfile 沙箱校验。
    pub fn docker_build(&self, name: &str) -> Result<String> {
        let (root, entry) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let cfg = g.spec.docker.as_ref().ok_or_else(|| {
                Error::new(
                    ErrorCode::DockerBuildUnknown,
                    format!("docker.builds 中没有 {name:?}"),
                )
            })?;
            let entry = cfg
                .builds
                .iter()
                .find(|b| b.name == name)
                .cloned()
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::DockerBuildUnknown,
                        format!("docker.builds 中没有 {name:?}"),
                    )
                })?;
            (g.root.clone(), entry)
        };
        // PATH_ESCAPE 同步返回（§6.2 沙箱校验）
        let args = crate::docker::plan_build_entry(&root, &entry)?;
        let runner = Arc::clone(&self.docker);
        let inner = Arc::clone(&self.inner);
        let src = LogSource {
            kind: LogSourceKind::System,
            id: name.to_string(),
        };
        let label = format!("docker build {name}");
        let spawn_spec = DockerSpawn {
            args,
            cwd: Some(root),
            timeout: Duration::from_secs(3600),
        };
        Ok(self.operations.spawn("docker.build", move |ctx| {
            run_build_streaming(&inner, ctx, &runner, &spawn_spec, &src, &label)
        }))
    }

    /// §6.1 compose 服务「构建镜像」：`docker compose build <service>`，走 operation。
    /// 非 compose 服务调用 → SPEC_INVALID（IPC 层 runtime.build 对 compose 走这里）。
    pub fn build_compose(&self, id: &str) -> Result<String> {
        let (root, rel_file, project, service) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let slot = g
                .slots
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            if slot.kind != "compose" {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id} 不是 kind: compose 服务，无法构建镜像"),
                ));
            }
            let svc = g
                .spec
                .services
                .get(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            let service = svc.service.clone().ok_or_else(|| {
                Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id}: kind: compose 缺少 service 字段"),
                )
            })?;
            let (rel_file, project) = match g.spec.docker.as_ref() {
                Some(d) => (
                    d.compose_file
                        .clone()
                        .or_else(|| crate::scan::discover_compose_file(&g.root)),
                    d.project_name.clone(),
                ),
                None => (crate::scan::discover_compose_file(&g.root), None),
            };
            let rel_file = rel_file
                .ok_or_else(|| Error::new(ErrorCode::ComposeFileMissing, "未找到 compose 文件"))?;
            (g.root.clone(), rel_file, project, service)
        };
        // service 存在性（缓存解析；沙箱校验）
        let file = sandbox::confine(&root, &rel_file)?;
        let model = self
            .compose_loader
            .load(&root, &rel_file, project.as_deref())?;
        if model.find(&service).is_none() {
            return Err(Error::new(
                ErrorCode::ComposeServiceMissing,
                format!("{id}: compose 文件中没有服务 {service:?}"),
            ));
        }
        let args = crate::docker::plan_compose_build(&file, project.as_deref(), &service);
        let runner = Arc::clone(&self.docker);
        let inner = Arc::clone(&self.inner);
        let src = LogSource {
            kind: LogSourceKind::Service,
            id: id.to_string(),
        };
        let label = format!("compose build {service}");
        let spawn_spec = DockerSpawn {
            args,
            cwd: Some(root),
            timeout: Duration::from_secs(3600),
        };
        Ok(self.operations.spawn("compose.build", move |ctx| {
            run_build_streaming(&inner, ctx, &runner, &spawn_spec, &src, &label)
        }))
    }

    /// 取消引擎内的长操作（镜像构建 best effort：杀进程，不删层）。
    pub fn cancel_operation(&self, id: &str) -> bool {
        self.operations.cancel(id)
    }

    /// 引擎内长操作 hub：`st.operation` 事件桥需要另外轮询它
    /// （与 src-tauri 侧 git/模板 hub 相互独立）。
    pub fn operations(&self) -> &crate::operation::OperationHub {
        &self.operations
    }

    /// 工作区引用（spec 快照 + root），供 secrets/日志等文件型操作使用。
    fn ws_ref(&self) -> Result<(SuperTaskFile, PathBuf)> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Ok((g.spec.clone(), g.root.clone()))
    }

    fn state_of(&self, id: &str) -> Option<RtState> {
        self.inner.lock().ok()?.slots.get(id).map(|s| s.state)
    }

    /// 等 launch: jar 的 Building 阶段结束（成功/失败/被停都离开 Building）。
    fn wait_building_done(&self, id: &str, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if self.state_of(id) != Some(RtState::Building) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::new(ErrorCode::BuildFailed, format!("{id} 构建超时")));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_ready(&self, id: &str, timeout: Duration) -> Result<()> {
        self.wait_state(
            id,
            &[
                RtState::Running,
                RtState::Unhealthy,
                RtState::Exited,
                RtState::Stopped,
            ],
            timeout,
        )
    }

    fn wait_state(&self, id: &str, want: &[RtState], timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(st) = self.state_of(id) {
                if want.contains(&st) {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::new(ErrorCode::JobKill, format!("{id} 等待状态超时")));
            }
            thread::sleep(Duration::from_millis(40));
        }
    }

    // -------------------------------------------------------------------
    // 1.6 phase 4：网关托管（GatewaySlot；规格 §7 / §8）
    // -------------------------------------------------------------------

    /// 当前生效的网关配置：无段 / 无 kind / enabled=false 都是未配置。
    fn gateway_conf_active(&self, g: &Inner) -> Result<crate::spec::GatewayConf> {
        g.spec
            .gateway
            .as_ref()
            .filter(|c| c.kind.is_some() && c.enabled)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::GatewayNotConfigured, "gateway 未配置或未启用"))
    }

    /// §8 gateway.status：只读快照（路由 + 上游端口 + 存活）。
    pub fn gateway_status(&self) -> Result<crate::ipc::GatewayStatusOutput> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        let (configured, enabled, kind, port, routes_conf) = match &g.spec.gateway {
            Some(c) => (
                c.kind.is_some(),
                c.enabled,
                c.kind,
                c.port,
                c.routes.clone(),
            ),
            None => (false, true, None, 8080, Vec::new()),
        };
        let mut routes = Vec::new();
        for r in &routes_conf {
            let target_port = r.target.as_deref().and_then(|t| {
                g.spec
                    .services
                    .get(t)
                    .and_then(|s| s.port.or_else(|| s.ports.first().copied()))
            });
            let upstream_port = target_port.or_else(|| {
                r.upstream
                    .as_deref()
                    .and_then(|u| crate::gateway::parse_upstream(u).ok())
                    .map(|a| a.port)
            });
            let alive = upstream_port.map(crate::ports::is_serving);
            routes.push(crate::ipc::GatewayRouteView {
                host: r.host.clone(),
                path: r.path.clone(),
                target: r.target.clone(),
                upstream: r.upstream.clone(),
                strip_prefix: r.strip_prefix,
                cors: r.cors.clone(),
                redirect: r.redirect.clone(),
                redirect_status: r.redirect_status,
                static_dir: r.static_dir.clone(),
                target_port,
                upstream_alive: alive,
            });
        }
        let (state, pid, last_error) = match &g.gateway {
            Some(slot) => (
                Some(rt_state_str(slot.state)),
                slot.pid,
                slot.last_error.clone(),
            ),
            None => (None, None, None),
        };
        let conf_path = kind.and_then(|k| {
            let p = crate::gateway::validate::gateway_dir(&g.root)
                .join(crate::gateway::validate::conf_file_name(k));
            p.is_file().then(|| p.to_string_lossy().into_owned())
        });
        Ok(crate::ipc::GatewayStatusOutput {
            configured,
            enabled,
            kind: kind.map(|k| k.as_str().to_string()),
            port: configured.then_some(port),
            state,
            pid,
            last_error,
            routes,
            conf_path,
        })
    }

    /// §8 gateway.preview：纯内存渲染草稿（不落盘、不校验、不启动）。
    pub fn gateway_preview(
        &self,
        conf: Option<crate::spec::GatewayConf>,
    ) -> Result<crate::ipc::GatewayPreviewOutput> {
        let (root, spec) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            (g.root.clone(), g.spec.clone())
        };
        let conf = match conf {
            Some(c) => c,
            None => spec
                .gateway
                .clone()
                .filter(|c| c.kind.is_some())
                .ok_or_else(|| {
                    Error::new(ErrorCode::GatewayNotConfigured, "gateway 未配置 kind")
                })?,
        };
        crate::gateway::ensure_static(&spec, &conf)?;
        let ir = crate::gateway::model::resolve(
            &spec,
            &conf,
            &|_| "127.0.0.1".into(),
            &root.to_string_lossy(),
        )?;
        let dir = crate::gateway::validate::gateway_dir(&root);
        let (name, content) =
            crate::gateway::render::render_conf(&ir, &dir.to_string_lossy(), "modules")?;
        Ok(crate::ipc::GatewayPreviewOutput {
            files: vec![crate::ipc::GatewayFileView {
                name: name.into(),
                content,
            }],
        })
    }

    /// §8 gateway.validate：静态校验 + 二进制探测 + spawn 本机校验（不启动）。
    /// 失败以 ok=false + message/stderr 返回（不作为 IPC 错误抛出）。
    pub fn gateway_validate(
        &self,
        conf: Option<crate::spec::GatewayConf>,
    ) -> Result<crate::ipc::GatewayValidateOutput> {
        let (root, spec) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            (g.root.clone(), g.spec.clone())
        };
        let conf = match conf {
            Some(c) => c,
            None => spec
                .gateway
                .clone()
                .filter(|c| c.kind.is_some())
                .ok_or_else(|| {
                    Error::new(ErrorCode::GatewayNotConfigured, "gateway 未配置 kind")
                })?,
        };
        let kind = conf.kind.expect("kind checked");
        if let Err(e) = crate::gateway::ensure_static(&spec, &conf) {
            return Ok(crate::ipc::GatewayValidateOutput {
                ok: false,
                message: Some(e.message().to_string()),
                stderr: None,
            });
        }
        let bin = match crate::gateway::probe::resolve_gateway_bin(kind, conf.bin.as_deref()) {
            Ok(b) => b,
            Err(e) => {
                return Ok(crate::ipc::GatewayValidateOutput {
                    ok: false,
                    message: Some(e.message().to_string()),
                    stderr: None,
                })
            }
        };
        let mut ir = crate::gateway::model::resolve(
            &spec,
            &conf,
            &loopback_host_for,
            &root.to_string_lossy(),
        )?;
        ir.apache_modules_dir = apache_modules_dir_of(&bin);
        match crate::gateway::validate::validate_gateway(&root, &ir, &bin, self.validator.as_ref())
        {
            Ok(_) => Ok(crate::ipc::GatewayValidateOutput {
                ok: true,
                message: None,
                stderr: None,
            }),
            Err(e) => Ok(crate::ipc::GatewayValidateOutput {
                ok: false,
                message: Some(e.message().to_string()),
                stderr: stderr_from_details(&e),
            }),
        }
    }

    /// 校验链（§6.1 第 1–3 步）：静态 → 探测 → 渲染落盘 → spawn 校验。
    /// 返回 spawn 所需的 (root, conf, bin, conf_path)。
    fn gateway_prepare(&self) -> Result<(PathBuf, crate::spec::GatewayConf, PathBuf, PathBuf)> {
        let (root, spec) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            (g.root.clone(), g.spec.clone())
        };
        let conf = spec
            .gateway
            .as_ref()
            .filter(|c| c.kind.is_some() && c.enabled)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::GatewayNotConfigured, "gateway 未配置或未启用"))?;
        crate::gateway::ensure_static(&spec, &conf)?;
        let kind = conf.kind.expect("kind checked");
        let bin = crate::gateway::probe::resolve_gateway_bin(kind, conf.bin.as_deref())?;
        let mut ir = crate::gateway::model::resolve(
            &spec,
            &conf,
            &loopback_host_for,
            &root.to_string_lossy(),
        )?;
        ir.apache_modules_dir = apache_modules_dir_of(&bin);
        let conf_path =
            crate::gateway::validate::validate_gateway(&root, &ir, &bin, self.validator.as_ref())?;
        Ok((root, conf, bin, conf_path))
    }

    /// §8 gateway.start：校验链 → spawn → Starting（健康探测异步达标转 Running）。
    pub fn gateway_start(&self) -> Result<()> {
        let (root, conf, bin, conf_path) = self.gateway_prepare()?;
        let kind = conf.kind.expect("kind checked");
        let argv = crate::gateway::validate::start_argv(
            kind,
            &conf_path,
            &crate::gateway::validate::gateway_dir(&root),
        );
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut g = self.inner.lock().expect("engine lock");
            if let Some(slot) = &g.gateway {
                if matches!(
                    slot.state,
                    RtState::Starting | RtState::Running | RtState::Unhealthy | RtState::Stopping
                ) {
                    return Err(Error::new(
                        ErrorCode::AlreadyInProgress,
                        "网关已在运行或正在切换",
                    ));
                }
            }
            // save_text 改配置后 slot 可能未重建：按当前 spec 重建（幂等）
            rebuild_gateway_slot(&mut g)?;
            let spawn = spawn_gateway_process(&bin, &argv, &root);
            let (mut child, job) = match spawn {
                Ok(v) => v,
                Err(e) => {
                    let slot = g.gateway.as_mut().expect("rebuild ensured slot");
                    slot.pid = None;
                    slot.job = None;
                    slot.last_error = Some(format!("GATEWAY_START_FAILED: {}", e.message()));
                    if let Ok(s) = apply(slot.state, RtEvent::SpawnFailed) {
                        slot.state = s;
                    }
                    emit_runtime(&g);
                    return Err(Error::new(
                        ErrorCode::GatewayStartFailed,
                        format!("网关进程启动失败: {}", e.message()),
                    ));
                }
            };
            let pid = child.id();
            let slot = g.gateway.as_mut().expect("rebuild ensured slot");
            slot.pid = Some(pid);
            slot.job = Some(job);
            slot.cancel = Arc::clone(&cancel);
            slot.stop_requested = false;
            slot.started = Some(Instant::now());
            slot.started_at_ms = Some(now_ms());
            slot.last_error = None;
            slot.last_exit = None;
            slot.exit_reason = None;
            match apply(slot.state, RtEvent::Spawned { health_none: false }) {
                Ok(s) => slot.state = s,
                Err(e) => slot.last_error = Some(e.to_string()),
            }
            emit_runtime(&g);
            let stdout = child.stdout.take();
            let stderr = child.stderr.take();
            drop(g);
            let src = LogSource {
                kind: LogSourceKind::Gateway,
                id: "gateway".into(),
            };
            if let Some(out) = stdout {
                spawn_pump(
                    Arc::clone(&self.inner),
                    src.clone(),
                    LogStream::Stdout,
                    out,
                    Arc::clone(&cancel),
                );
            }
            if let Some(err) = stderr {
                spawn_pump(
                    Arc::clone(&self.inner),
                    src,
                    LogStream::Stderr,
                    err,
                    Arc::clone(&cancel),
                );
            }
            spawn_gateway_waiter(Arc::clone(&self.inner), child);
            spawn_gateway_health(Arc::clone(&self.inner), conf.port, Arc::clone(&cancel));
        }
        Ok(())
    }

    /// §8 gateway.stop：进程树终止（与 service 同语义；幂等）。
    pub fn gateway_stop(&self) -> Result<()> {
        {
            let mut g = self.inner.lock().expect("engine lock");
            let Some(slot) = g.gateway.as_mut() else {
                return Ok(());
            };
            if matches!(slot.state, RtState::Stopped | RtState::Exited) {
                return Ok(());
            }
            slot.stop_requested = true;
            slot.cancel.store(true, Ordering::SeqCst);
            if let Ok(next) = apply(slot.state, RtEvent::StopRequested) {
                slot.state = next;
            }
            if let Some(job) = &slot.job {
                job.terminate()?;
            }
            emit_runtime(&g);
        }
        // 等待收尾（waiter 线程或本函数兜底置 Stopped）
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            {
                let g = self.inner.lock().expect("engine lock");
                match &g.gateway {
                    None => return Ok(()),
                    Some(slot) => {
                        if matches!(slot.state, RtState::Stopped | RtState::Exited) {
                            return Ok(());
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                let mut g = self.inner.lock().expect("engine lock");
                if let Some(slot) = g.gateway.as_mut() {
                    slot.state = RtState::Stopped;
                    slot.pid = None;
                    slot.job = None;
                    slot.exit_reason = None;
                    emit_runtime(&g);
                }
                return Err(Error::new(ErrorCode::JobKill, "网关停止超时"));
            }
            thread::sleep(Duration::from_millis(40));
        }
    }

    /// §8 gateway.restart：stop → start（非热重载；apply 复用同一语义）。
    pub fn gateway_restart(&self) -> Result<()> {
        self.gateway_stop()?;
        self.gateway_start()
    }

    /// §8 gateway.apply：静态校验 → save_form 写 yaml（YAML_CONFLICT 冲突）
    /// → 重新生成 → 运行中则重启（stop→start）。
    pub fn gateway_apply(
        &self,
        conf: crate::spec::GatewayConf,
        base_hash: &str,
    ) -> Result<crate::ipc::GatewayApplyOutput> {
        {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
        }
        let mut spec = self.spec()?;
        // 用新 conf 对当前服务表做静态校验（fail-fast，不写 yaml）
        crate::gateway::ensure_static(&spec, &conf)?;
        // 先落盘再重启：YAML_CONFLICT 时网关保持运行不受影响
        spec.gateway = Some(conf.clone());
        let (spec, hash, warnings) = self.save_form(&spec, base_hash)?;
        let was_running = {
            let g = self.inner.lock().expect("engine lock");
            g.gateway
                .as_ref()
                .map(|s| {
                    matches!(
                        s.state,
                        RtState::Starting
                            | RtState::Running
                            | RtState::Unhealthy
                            | RtState::Stopping
                    )
                })
                .unwrap_or(false)
        };
        if was_running {
            self.gateway_stop()?;
        }
        // 重建 slot（enabled/kind/port 可能变化；stop 后旧 slot 已归零）
        {
            let mut g = self.inner.lock().expect("engine lock");
            rebuild_gateway_slot(&mut g)?;
            emit_runtime(&g);
        }
        let restarted = if was_running && conf.enabled && conf.kind.is_some() {
            self.gateway_start()?;
            true
        } else {
            false
        };
        Ok(crate::ipc::GatewayApplyOutput {
            spec: serde_yaml::to_value(&spec)
                .map_err(|e| Error::new(ErrorCode::SpecInvalid, e.to_string()))?,
            hash,
            restarted,
            warnings: warnings.iter().map(|w| w.message.clone()).collect(),
        })
    }

    /// §8 gateway.trust：`caddy trust`（UI 显式确认在前；修改系统信任库）。
    pub fn gateway_trust(&self) -> Result<()> {
        let conf = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            self.gateway_conf_active(&g)?
        };
        if conf.kind != Some(crate::spec::GatewayKind::Caddy) {
            return Err(Error::new(
                ErrorCode::GatewayNotConfigured,
                "gateway.trust 仅支持 kind: caddy",
            ));
        }
        let bin = crate::gateway::probe::resolve_gateway_bin(
            crate::spec::GatewayKind::Caddy,
            conf.bin.as_deref(),
        )?;
        let out = self.validator.run(
            &bin,
            &crate::gateway::validate::trust_argv(),
            Duration::from_secs(60),
        )?;
        if out.code != 0 {
            return Err(Error::new(
                ErrorCode::GatewayConfigInvalid,
                format!("caddy trust 退出码 {}: {}", out.code, out.stderr.trim()),
            ));
        }
        Ok(())
    }
}

fn empty_spec() -> SuperTaskFile {
    parse_yaml(
        r#"
version: 1
services:
  placeholder:
    kind: node
    dir: x
    port: 1
    enabled: false
"#,
    )
    .map(|(f, _)| f)
    .expect("placeholder spec")
}

pub fn load_yaml_at(root: &Path) -> Result<(PathBuf, String, SuperTaskFile, Vec<ParseWarning>)> {
    let yaml = root.join("supertask.yaml");
    let yml = root.join("supertask.yml");
    let path = match (yaml.is_file(), yml.is_file()) {
        (true, true) => {
            return Err(Error::new(
                ErrorCode::YamlDupFile,
                "同时存在 supertask.yaml 与 supertask.yml",
            ));
        }
        (false, false) => {
            return Err(Error::new(ErrorCode::NoYaml, "尚未配置，请先生成草稿"));
        }
        (true, false) => yaml,
        (false, true) => yml,
    };
    let text = fs::read_to_string(&path)
        .map_err(|e| Error::new(ErrorCode::YamlParse, format!("读 YAML 失败: {e}")))?;
    let (file, warnings) = parse_yaml(&text)?;
    Ok((path, text, file, warnings))
}

fn require_ws(g: &Inner) -> Result<()> {
    if g.workspace_id.is_empty() {
        Err(Error::new(ErrorCode::NoWorkspace, "未打开工作区"))
    } else {
        Ok(())
    }
}

fn check_can_replace_spec(g: &Inner, file: &SuperTaskFile) -> Result<()> {
    for (id, slot) in &g.slots {
        if file.services.contains_key(id) {
            continue;
        }
        if !matches!(slot.state, RtState::Stopped | RtState::Exited) {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("请先停止 {id} 再从配置中删除"),
            ));
        }
    }
    Ok(())
}

fn apply_spec_slots(g: &mut Inner, file: &SuperTaskFile) -> Result<()> {
    let gone: Vec<String> = g
        .slots
        .keys()
        .filter(|id| !file.services.contains_key(*id))
        .cloned()
        .collect();
    for id in gone {
        g.slots.remove(&id);
        g.files.remove(&id);
    }
    for (id, svc) in &file.services {
        if let Some(slot) = g.slots.get_mut(id) {
            if matches!(slot.state, RtState::Stopped | RtState::Exited) {
                slot.port = svc.port;
                slot.kind = svc.kind.clone();
                slot.grace = Duration::from_secs(svc.grace_secs.unwrap_or(0) as u64);
            }
            continue;
        }
        let rel = log_file_rel("service", id);
        let abs = g.root.join(&rel);
        let lf = LogFile::open_with_files(
            abs,
            file.logging.as_ref().and_then(|l| l.max_bytes),
            file.logging.as_ref().and_then(|l| l.retain_tail_bytes),
            file.log_retention.as_ref().and_then(|r| r.max_files),
        )
        .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法创建日志文件: {e}")))?;
        g.files.insert(id.clone(), lf);
        g.slots.insert(
            id.clone(),
            Slot {
                state: RtState::Stopped,
                pid: None,
                port: svc.port,
                kind: svc.kind.clone(),
                job: None,
                stop_requested: false,
                started: None,
                started_at_ms: None,
                grace: Duration::from_secs(svc.grace_secs.unwrap_or(0) as u64),
                health: None,
                last_error: None,
                last_exit: None,
                cancel: Arc::new(AtomicBool::new(false)),
                managed: true,
                artifact: None,
                exit_reason: None,
                restart: crate::spec::RestartSpec::default(),
                restart_left: 0,
                restart_attempt: None,
                restart_cancel: Arc::new(AtomicBool::new(false)),
                restart_plan: None,
                compose: None,
                env_snapshot: None,
            },
        );
    }
    Ok(())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `env.effective` 快照持久化文件（工作区级，非机密目录之外的敏感数据与 .env 同级）。
fn env_snapshot_file(root: &Path) -> PathBuf {
    root.join(".supertask").join("env-snapshots.json")
}

fn persist_env_snapshots(root: &Path, slots: &HashMap<String, Slot>) {
    let mut map: std::collections::BTreeMap<String, EnvSnapshot> = Default::default();
    for (id, s) in slots {
        if let Some(snap) = &s.env_snapshot {
            map.insert(id.clone(), snap.clone());
        }
    }
    let Ok(text) = serde_json::to_string(&map) else {
        return;
    };
    let f = env_snapshot_file(root);
    let Some(dir) = f.parent() else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let _ = fs::write(&f, text);
}

fn load_env_snapshots(root: &Path, slots: &mut HashMap<String, Slot>) {
    let Ok(text) = fs::read_to_string(env_snapshot_file(root)) else {
        return;
    };
    let Ok(map) = serde_json::from_str::<std::collections::BTreeMap<String, EnvSnapshot>>(&text)
    else {
        return;
    };
    for (id, snap) in map {
        if let Some(slot) = slots.get_mut(&id) {
            slot.env_snapshot = Some(snap);
        }
    }
}

fn build_snapshot(g: &Inner) -> RuntimeSnapshot {
    let mut services = IndexMap::new();
    for (id, slot) in &g.slots {
        services.insert(
            id.clone(),
            ServiceRuntimeView {
                id: id.clone(),
                state: slot.state,
                pid: slot.pid,
                port: slot.port,
                kind: slot.kind.clone(),
                health: slot.health.clone(),
                started_at_ms: slot.started_at_ms,
                last_exit: slot.last_exit.clone(),
                last_error: slot.last_error.clone(),
                exit_reason: slot.exit_reason.map(str::to_string),
                restart_attempt: slot.restart_attempt,
                log_seq: g.logs.next_seq().saturating_sub(1),
                // 有 Job 即本引擎托管（防止历史 slot.managed=false 误标「外部」）
                managed: slot.managed || slot.job.is_some(),
            },
        );
    }
    RuntimeSnapshot {
        protocol: crate::ipc::PROTOCOL,
        workspace_id: g.workspace_id.clone(),
        services,
        metrics: g.metrics.clone(),
        script: g.script.as_ref().map(|s| ScriptRuntimeView {
            id: s.id.clone(),
            state: s.state,
            pid: s.pid,
            last_exit: s.last_exit.clone(),
            last_error: s.last_error.clone(),
        }),
        gateway: g.gateway.as_ref().map(|gw| GatewayRuntimeView {
            kind: gw.kind.as_str().to_string(),
            state: gw.state,
            pid: gw.pid,
            port: gw.port,
            health: gw.health.clone(),
            started_at_ms: gw.started_at_ms,
            last_exit: gw.last_exit.clone(),
            last_error: gw.last_error.clone(),
            exit_reason: gw.exit_reason.map(str::to_string),
        }),
    }
}

fn emit_runtime(g: &Inner) {
    let snap = build_snapshot(g);
    let _ = g.events.try_send(EngineEvent::Runtime(snap));
}

/// loopback:port 是否已有服务在监听（实现移交 ports::is_serving，CLI status 只读复用）。
fn port_is_serving(port: u16) -> bool {
    crate::ports::is_serving(port)
}

/// 端口被外部进程占位时的统一提示：带占位进程名/pid，指引更换端口。
/// 前端靠 `端口…被…占用` 前缀识别冲突态（禁用启动 + 指引改端口），文案调整时保持前缀稳定。
fn conflict_message(id: &str, port: u16, occs: &[crate::discover::ForeignService]) -> String {
    match occs.first() {
        Some(o) => format!(
            "{id}: 端口 {port} 被 {name}(pid {pid}) 占用，请更换端口后启动",
            name = o.name,
            pid = o.pid
        ),
        None => format!("{id}: 端口 {port} 已被占用（占位进程不可见），请更换端口后启动"),
    }
}

/// 启动前端口归属复核：被外部进程占位时直接拒绝（PORT_IN_USE），避免起不来还刷屏。
/// Owned（本工作区外部已运行）与 Free 放行；Unknown（发现表不可读）放行，由启动后的
/// 端口绑定/健康检查给出真实错误。
fn ensure_port_not_conflicted(root: &Path, id: &str, port: u16, kind: &str) -> Result<()> {
    match crate::discover::classify_port_owner(port, kind, root) {
        crate::discover::PortOwnership::Conflict(occs) => Err(Error::new(
            ErrorCode::PortInUse,
            conflict_message(id, port, &occs),
        )),
        _ => Ok(()),
    }
}

/// 按 LISTEN 端口找到外部进程 pid 并 `taskkill /T /F`（等效杀整棵树）。
fn kill_foreign_by_pid(pid: u32) -> Result<()> {
    crate::discover::taskkill_tree(pid)
}

fn spawn_real(
    planned: &CommandSpec,
    cwd: &Path,
) -> Result<(Child, Arc<dyn crate::proc::ProcessTree>)> {
    let program = probe::resolve_program_with_path(
        &planned.program,
        planned.env.get("PATH").map(std::ffi::OsStr::new),
    )?;
    let mut cmd = Command::new(&program);
    cmd.args(&planned.args)
        .current_dir(cwd)
        .envs(planned.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::proc::create_tree()?;
    let child = job.spawn(&mut cmd)?;
    Ok((child, job))
}

#[cfg(test)]
fn spawn_ping() -> Result<(Child, Arc<dyn crate::proc::ProcessTree>)> {
    let mut cmd = Command::new("ping");
    cmd.args(["-t", "127.0.0.1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::proc::create_tree()?;
    let child = job.spawn(&mut cmd)?;
    Ok((child, job))
}

#[cfg(all(test, windows))]
fn spawn_fail() -> Result<(Child, Arc<dyn crate::proc::ProcessTree>)> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "exit 1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::proc::create_tree()?;
    let child = job.spawn(&mut cmd)?;
    Ok((child, job))
}

#[cfg(all(test, not(windows)))]
fn spawn_fail() -> Result<(Child, Arc<dyn crate::proc::ProcessTree>)> {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "exit 1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::proc::create_tree()?;
    let child = job.spawn(&mut cmd)?;
    Ok((child, job))
}

fn spawn_pump(
    inner: Arc<Mutex<Inner>>,
    source: LogSource,
    stream: LogStream,
    pipe: impl std::io::Read + Send + 'static,
    cancel: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name(format!("st-log-{}", source.id))
        .spawn(move || {
            // 按字节读行而非 lines()：UTF-8 严格校验既会把 GBK 行变乱码，
            // 又会在 InvalidData 时中止泵丢掉后续日志
            let mut reader = BufReader::new(pipe);
            let mut buf: Vec<u8> = Vec::with_capacity(512);
            loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                buf.clear();
                match reader.read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                if buf.last() == Some(&b'\r') {
                    buf.pop();
                }
                let text = decode_line(&buf);
                push_line(&inner, source.clone(), stream, text);
            }
        })
        .ok();
}

/// 行字节 → 文本：UTF-8 有效即用；否则按 GBK 解（Windows 中文系统的 cmd /
/// 批处理包装层 echo 中文走 936 代码页）；再失败退回 UTF-8 有损解，
/// 保证极端二进制残留不会显示成整行失控文本。
fn decode_line(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (cow, _, had_errors) = encoding_rs::GBK.decode(bytes);
    if had_errors {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    cow.into_owned()
}

/// 剥离 ANSI 转义序列。Vite/npm/Maven 在 TTY 下会输出颜色码（`\x1b[32m`、
/// `\x1b[1m` 等），管道捕获后原样进入日志，必须清除：
/// - CSI 序列 `ESC [ <params> <final>`（颜色 m、移动、清屏等）
/// - OSC 超链接 `ESC ] ... ESC \`
/// 用 OnceLock 缓存编译好的正则，只在首行支付一次编译成本。
fn strip_ansi(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x1b\\]*(\x1b\\|\x07)|\x1b[B-FH]").unwrap()
    });
    re.replace_all(text, "").into_owned()
}

fn push_line(inner: &Mutex<Inner>, source: LogSource, stream: LogStream, text: String) {
    let text = strip_ansi(&text);
    let mut g = inner.lock().expect("engine lock");
    let line = LogLine {
        seq: 0,
        source: source.clone(),
        stream,
        ts_ms: now_ms(),
        text,
    };
    let stored = g.logs.push(line);
    let file = match source.kind {
        LogSourceKind::Script => g.script_file.as_mut(),
        _ => g.files.get_mut(&source.id),
    };
    if let Some(f) = file {
        let _ = f.append_line(&format!(
            "{} | {:?} | {}",
            stored.ts_ms, stored.stream, stored.text
        ));
    }
    if g.subscribers > 0 {
        let _ = g.log_tx.send(stored);
    }
}

/// 脚本 cmds 执行 shell（1.4 §4.2）。返回 (程序, 前置参数, 可选警告文案)。
#[cfg(windows)]
fn script_shell() -> (String, Vec<String>, Option<String>) {
    ("cmd".into(), vec!["/C".into()], None)
}

#[cfg(not(windows))]
fn script_shell() -> (String, Vec<String>, Option<String>) {
    if crate::probe::find_on_path("bash").is_some() {
        ("bash".into(), vec!["-c".into()], None)
    } else {
        (
            "sh".into(),
            vec!["-c".into()],
            Some("PATH 中没有 bash，脚本回落 sh -c 执行：bash 特有语法可能不兼容".into()),
        )
    }
}

fn run_script_cmds(
    inner: Arc<Mutex<Inner>>,
    id: String,
    cmds: Vec<String>,
    cwd: PathBuf,
    env: IndexMap<String, String>,
    timeout: Duration,
) {
    let deadline = Instant::now() + timeout;
    let mut last_code = 0i32;
    let mut err: Option<String> = None;
    let src = LogSource {
        kind: LogSourceKind::Script,
        id: id.clone(),
    };
    // 脚本 shell（1.4 §4.2）：Windows cmd.exe /C（不变）；Unix bash -c，
    // PATH 无 bash 回落 sh -c 并在日志头警告一次语法风险。
    let (shell_program, shell_args, shell_warning) = script_shell();
    if let Some(warn) = shell_warning {
        push_line(&inner, src.clone(), LogStream::System, warn);
    }
    'cmds: for (i, line) in cmds.iter().enumerate() {
        {
            let g = inner.lock().expect("engine lock");
            if g.script
                .as_ref()
                .map(|s| s.cancel.load(Ordering::Relaxed))
                .unwrap_or(true)
            {
                last_code = 1;
                err = Some("已取消".into());
                break;
            }
        }
        if Instant::now() >= deadline {
            last_code = 1;
            err = Some("脚本超时".into());
            break;
        }
        push_line(
            &inner,
            src.clone(),
            LogStream::System,
            format!("[{}/{}] {line}", i + 1, cmds.len()),
        );
        let mut cmd = Command::new(&shell_program);
        cmd.args(shell_args.iter().map(String::as_str))
            .arg(line)
            .current_dir(&cwd)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let spawned = {
            let g = inner.lock().expect("engine lock");
            let Some(slot) = g.script.as_ref() else { break };
            let Some(job) = slot.job.as_ref() else { break };
            job.spawn(&mut cmd)
        };
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                last_code = 1;
                err = Some(e.to_string());
                break;
            }
        };
        let pid = child.id();
        {
            let mut g = inner.lock().expect("engine lock");
            if let Some(slot) = g.script.as_mut() {
                slot.pid = Some(pid);
            }
            emit_runtime(&g);
        }
        let cancel = inner
            .lock()
            .expect("engine lock")
            .script
            .as_ref()
            .map(|s| Arc::clone(&s.cancel))
            .unwrap_or_else(|| Arc::new(AtomicBool::new(true)));
        if let Some(out) = child.stdout.take() {
            spawn_pump(
                Arc::clone(&inner),
                src.clone(),
                LogStream::Stdout,
                out,
                Arc::clone(&cancel),
            );
        }
        if let Some(errp) = child.stderr.take() {
            spawn_pump(
                Arc::clone(&inner),
                src.clone(),
                LogStream::Stderr,
                errp,
                cancel,
            );
        }
        loop {
            if Instant::now() >= deadline {
                let g = inner.lock().expect("engine lock");
                if let Some(job) = g.script.as_ref().and_then(|s| s.job.as_ref()) {
                    let _ = job.terminate();
                }
                last_code = 1;
                err = Some("脚本超时".into());
                break 'cmds;
            }
            let cancelled = inner
                .lock()
                .expect("engine lock")
                .script
                .as_ref()
                .map(|s| s.cancel.load(Ordering::Relaxed))
                .unwrap_or(true);
            if cancelled {
                last_code = 1;
                err = Some("已取消".into());
                break 'cmds;
            }
            match child.try_wait() {
                Ok(Some(st)) => {
                    last_code = st.code().unwrap_or(1);
                    if last_code != 0 {
                        err = Some(format!("第 {} 条命令退出码 {last_code}", i + 1));
                        break 'cmds;
                    }
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(40)),
                Err(e) => {
                    last_code = 1;
                    err = Some(e.to_string());
                    break 'cmds;
                }
            }
        }
        let mut g = inner.lock().expect("engine lock");
        if let Some(slot) = g.script.as_mut() {
            slot.pid = None;
        }
    }
    let mut g = inner.lock().expect("engine lock");
    if let Some(slot) = g.script.as_mut() {
        slot.pid = None;
        slot.job = None;
        slot.state = ScriptState::Exited;
        slot.last_exit = Some(ExitView {
            code: last_code,
            at_ms: now_ms(),
        });
        slot.last_error = err;
        slot.cancel.store(true, Ordering::SeqCst);
    }
    emit_runtime(&g);
}

// ============================================================================
// 1.2 phase 3–7 引擎辅助：环境链、jar 构建编排、指标 sampler
// ============================================================================

/// §6.3 环境链：ws+profile < secrets/env_file < 服务+profile env < 端口注入。
/// `eff_spec` 已是 overlay 后的文件（profiles::overlay_spec）。
/// 同时返回每个键的最终来源层（后者覆盖前者，同键取最后写入方）：
/// workspace | env_file | service | port | network（键不在返回 env 里时无条目）。
fn build_service_env(
    eff_spec: &SuperTaskFile,
    id: &str,
    root: &Path,
    app_network: Option<&crate::appdata::AppNetwork>,
) -> Result<(IndexMap<String, String>, IndexMap<String, &'static str>)> {
    // Keep long-lived desktop processes in sync with PATH changes made by
    // winget or another installer after SuperTask started.
    crate::toolchain::resolver::refresh_process_path();
    let svc = eff_spec
        .services
        .get(id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
    let (file_env, _warnings) = crate::secrets::load_file_layers(eff_spec, root, Some(id))?;
    let mut env = eff_spec.env.clone();
    let mut sources: IndexMap<String, &'static str> =
        env.keys().map(|k| (k.clone(), "workspace")).collect();
    for (k, v) in &file_env {
        env.insert(k.clone(), v.clone());
        sources.insert(k.clone(), "env_file");
    }
    for (k, v) in &svc.env {
        env.insert(k.clone(), v.clone());
        sources.insert(k.clone(), "service");
    }
    if let Some(p) = svc.port {
        if let Some(key) = crate::ports::port_env_key(&svc.kind) {
            env.entry(key.to_string()).or_insert_with(|| p.to_string());
            sources.insert(key.to_string(), "port");
        }
    }
    // 1.7 §7：镜像/代理注入，最低优先级（已存在的键不覆盖，显式 env 永远赢）。
    // resolve 失败（如 custom 代理缺 URL）随启动硬失败；settings.xml 写失败静默跳过注入。
    let eff_net = crate::network::resolve(eff_spec.network.as_ref(), app_network)?;
    let (_, _inject_warns) = crate::network::inject_env(&eff_net, root, &mut env);
    // inject_env 只补缺失键 → 差集即它实际注入的键
    for k in env.keys() {
        sources.entry(k.clone()).or_insert("network");
    }
    Ok((env, sources))
}

fn resolve_cwd(root: &Path, cwd_rel: &str) -> Result<PathBuf> {
    let cwd = if cwd_rel == "." {
        root.to_path_buf()
    } else {
        sandbox::confine(root, cwd_rel)?
    };
    if !cwd.is_dir() {
        return Err(Error::new(
            ErrorCode::CwdMissing,
            format!("工作目录不存在: {}", cwd.display()),
        ));
    }
    Ok(cwd)
}

/// 单次进程拉起 + 日志管道 + 健康探测（原 spawn_service 尾段，jar 复用）。
#[allow(clippy::too_many_arguments)]
fn spawn_core(
    inner: Arc<Mutex<Inner>>,
    id: String,
    planned: CommandSpec,
    cwd: PathBuf,
    health_spec: Option<crate::spec::HealthSpec>,
    health_none: bool,
    port: Option<u16>,
    kind: String,
    pkg: Option<&str>,
    svc_grace: u64,
    build_tool: Option<&str>,
    spawner: SpawnerKind,
    env_snapshot: Vec<crate::ipc::EnvEffectiveEntry>,
    restart: crate::spec::RestartSpec,
) -> Result<()> {
    if matches!(spawner, SpawnerKind::Real) {
        probe::require_tools_for_kind_with_path(
            &kind,
            pkg,
            build_tool,
            &planned.program,
            planned.env.get("PATH").map(String::as_str),
        )?;
    }
    if !cwd.is_dir() {
        return Err(Error::new(
            ErrorCode::CwdMissing,
            format!("工作目录不存在: {}", cwd.display()),
        ));
    }
    let (mut child, job) = match spawner {
        SpawnerKind::Real => spawn_real(&planned, &cwd)?,
        #[cfg(test)]
        SpawnerKind::Ping => spawn_ping()?,
        #[cfg(test)]
        SpawnerKind::Fail => spawn_fail()?,
    };
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut g = inner.lock().expect("engine lock");
        let slot = g.slots.get_mut(&id).unwrap();
        // 同步当前 spec 的 port/kind/grace：改端口/profile 切换后重启仍显示新配置
        slot.port = port;
        slot.kind = kind.clone();
        slot.grace = Duration::from_secs(svc_grace);
        slot.cancel = Arc::new(AtomicBool::new(false));
        slot.stop_requested = false;
        // 2.2：捕获 restart 启动计划——崩溃后的自动重启按原样重放，
        // 不重规划命令、不复检工具；手动 start 仍走 spawn_service 完整链路。
        slot.restart_plan = Some(RestartPlan {
            planned: planned.clone(),
            cwd: cwd.clone(),
            health_spec: health_spec.clone(),
            health_none,
            port,
            kind: kind.clone(),
            pkg: pkg.map(str::to_string),
            svc_grace,
            build_tool: build_tool.map(str::to_string),
            spawner,
            env_snapshot: env_snapshot.clone(),
            restart,
        });
        slot.job = Some(job);
        // SuperTask 已挂上 Job → 本会话托管；修复「load 时端口占用标成外部，stop 后再 start 仍 managed=false」
        slot.managed = true;
        slot.pid = Some(pid);
        slot.started = Some(Instant::now());
        slot.started_at_ms = Some(now_ms());
        slot.last_error = None;
        slot.exit_reason = None;
        slot.env_snapshot = Some(EnvSnapshot {
            captured_at_ms: slot.started_at_ms.unwrap_or(0),
            entries: env_snapshot,
        });
        match apply(slot.state, RtEvent::Spawned { health_none }) {
            Ok(s) => slot.state = s,
            Err(e) => {
                slot.last_error = Some(e.to_string());
            }
        }
        // 诊断辅助数据：落盘失败不阻断启动，静默降级为仅内存态
        persist_env_snapshots(&g.root, &g.slots);
        emit_runtime(&g);
    }

    let cancel = {
        let g = inner.lock().expect("engine lock");
        g.slots.get(&id).unwrap().cancel.clone()
    };
    if let Some(out) = stdout {
        spawn_pump(
            Arc::clone(&inner),
            LogSource {
                kind: LogSourceKind::Service,
                id: id.clone(),
            },
            LogStream::Stdout,
            out,
            Arc::clone(&cancel),
        );
    }
    if let Some(err) = stderr {
        spawn_pump(
            Arc::clone(&inner),
            LogSource {
                kind: LogSourceKind::Service,
                id: id.clone(),
            },
            LogStream::Stderr,
            err,
            Arc::clone(&cancel),
        );
    }
    spawn_waiter(Arc::clone(&inner), id.clone(), child);
    if !health_none {
        if let Some(hs) = health_spec {
            spawn_health(inner, id, hs, port, cancel);
        }
    }
    Ok(())
}

/// 多模块 Maven reactor：`install -pl -am`（Building）→ `spring-boot:run`（无 `-am`）。
#[allow(clippy::too_many_arguments)]
fn maven_reactor_run_flow(
    inner: Arc<Mutex<Inner>>,
    id: &str,
    prep_spec: CommandSpec,
    run_spec: CommandSpec,
    root: PathBuf,
    health_spec: Option<crate::spec::HealthSpec>,
    health_none: bool,
    port: Option<u16>,
    kind: String,
    pkg: Option<&str>,
    grace: u64,
    bt: crate::launcher::BuildTool,
    spawner: SpawnerKind,
    env_snapshot: Vec<crate::ipc::EnvEffectiveEntry>,
    restart: crate::spec::RestartSpec,
) -> Result<()> {
    reactor_prep_phase(inner.clone(), id, prep_spec, &root, bt)?;
    let cwd = resolve_cwd(&root, &run_spec.cwd_rel)?;
    spawn_core(
        inner,
        id.to_string(),
        run_spec,
        cwd,
        health_spec,
        health_none,
        port,
        kind,
        pkg,
        grace,
        Some(bt.as_str()),
        spawner,
        env_snapshot,
        restart,
    )
}

/// reactor 上游模块 install（Building）；成功不落 artifact，转 Stopped 后由 run 接续。
fn reactor_prep_phase(
    inner: Arc<Mutex<Inner>>,
    id: &str,
    build_spec: CommandSpec,
    root: &Path,
    bt: crate::launcher::BuildTool,
) -> Result<()> {
    let _ = jar_build_phase(inner, id, build_spec, root, bt, BuildPhaseKind::ReactorPrep)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BuildPhaseKind {
    JarArtifact,
    ReactorPrep,
}

/// 1.2 §11 launch: jar 编排：构建（若无 artifact）→ java -jar。1.4 §5.3：gradle 走
/// bootJar，artifact 识别在 module/build/libs。
#[allow(clippy::too_many_arguments)]
fn jar_flow(
    inner: Arc<Mutex<Inner>>,
    id: &str,
    build_spec: CommandSpec,
    mut run_spec: CommandSpec,
    root: PathBuf,
    health_spec: Option<crate::spec::HealthSpec>,
    health_none: bool,
    port: Option<u16>,
    kind: String,
    pkg: Option<&str>,
    grace: u64,
    bt: crate::launcher::BuildTool,
    spawner: SpawnerKind,
    env_snapshot: Vec<crate::ipc::EnvEffectiveEntry>,
    restart: crate::spec::RestartSpec,
) -> Result<()> {
    let artifact = {
        let have = inner
            .lock()
            .expect("engine lock")
            .slots
            .get(id)
            .and_then(|s| s.artifact.clone());
        match have {
            Some(a) => a,
            None => jar_build_phase(
                inner.clone(),
                id,
                build_spec,
                &root,
                bt,
                BuildPhaseKind::JarArtifact,
            )?,
        }
    };
    // args 形如 ["-jar", ...extra_args]；artifact 插在 "-jar" 之后
    run_spec
        .args
        .insert(1, artifact.to_string_lossy().into_owned());
    let cwd = resolve_cwd(&root, &run_spec.cwd_rel)?;
    spawn_core(
        inner,
        id.to_string(),
        run_spec,
        cwd,
        health_spec,
        health_none,
        port,
        kind,
        pkg,
        grace,
        Some(bt.as_str()),
        spawner,
        env_snapshot,
        restart,
    )
}

fn jar_flow_fail(inner: &Arc<Mutex<Inner>>, id: &str, e: crate::error::Error) {
    let mut g = inner.lock().expect("engine lock");
    if let Some(slot) = g.slots.get_mut(id) {
        if slot.state == RtState::Building {
            if let Ok(s) = apply(slot.state, RtEvent::BuildFinished { ok: false }) {
                slot.state = s;
            }
            slot.last_error = Some(e.to_string());
            slot.pid = None;
            slot.job = None;
            emit_runtime(&g);
        }
    }
}

/// package / reactor-prep 阶段：Building 状态 + 输出进服务日志。
/// `ReactorPrep` 成功不落 artifact；`JarArtifact` 解析 jar 路径。
fn jar_build_phase(
    inner: Arc<Mutex<Inner>>,
    id: &str,
    mut build_spec: CommandSpec,
    root: &Path,
    bt: crate::launcher::BuildTool,
    phase: BuildPhaseKind,
) -> Result<PathBuf> {
    let (module, cwd, cancel) = {
        let g = inner.lock().expect("engine lock");
        let slot = g
            .slots
            .get(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
        let module = g
            .spec
            .services
            .get(id)
            .and_then(|s| s.module.clone())
            .unwrap_or_else(|| ".".to_string());
        let cwd = g
            .spec
            .services
            .get(id)
            .and_then(|s| s.cwd.clone())
            .unwrap_or_else(|| ".".to_string());
        (module, cwd, slot.cancel.clone())
    };
    {
        let mut g = inner.lock().expect("engine lock");
        let slot = g.slots.get_mut(id).unwrap();
        slot.state = apply(slot.state, RtEvent::BuildStarted)?;
        slot.last_error = None;
        slot.exit_reason = None;
        emit_runtime(&g);
    }
    let src = LogSource {
        kind: LogSourceKind::Service,
        id: id.to_string(),
    };
    let is_gradle = bt == crate::launcher::BuildTool::Gradle;
    let stage_label = match (phase, is_gradle) {
        (BuildPhaseKind::ReactorPrep, _) => "mvn reactor install",
        (BuildPhaseKind::JarArtifact, true) => "gradle bootJar",
        (BuildPhaseKind::JarArtifact, false) => "mvn package",
    };
    if is_gradle && phase == BuildPhaseKind::JarArtifact {
        // §5.1 wrapper 优先；都无 → GRADLE_WRAPPER_MISSING（building 失败收场）
        let (program, args, warns) = crate::launcher::resolve_gradle_launcher(
            root,
            &cwd,
            &module,
            &build_spec.program,
            &build_spec.args,
        )?;
        for w in warns {
            push_line(&inner, src.clone(), LogStream::System, w);
        }
        build_spec.program = program;
        build_spec.args = args;
    }
    push_line(
        &inner,
        src.clone(),
        LogStream::System,
        format!("开始构建（{stage_label}）"),
    );
    let cwd = resolve_cwd(root, &build_spec.cwd_rel)?;
    let (mut child, job) = spawn_real(&build_spec, &cwd)?;
    {
        let mut g = inner.lock().expect("engine lock");
        if let Some(slot) = g.slots.get_mut(id) {
            slot.job = Some(Arc::clone(&job));
            slot.pid = Some(child.id());
        }
        emit_runtime(&g);
    }
    if let Some(out) = child.stdout.take() {
        spawn_pump(
            Arc::clone(&inner),
            src.clone(),
            LogStream::Stdout,
            out,
            Arc::clone(&cancel),
        );
    }
    if let Some(err) = child.stderr.take() {
        spawn_pump(
            Arc::clone(&inner),
            src,
            LogStream::Stderr,
            err,
            Arc::clone(&cancel),
        );
    }
    let started = Instant::now();
    let status = loop {
        let st = child
            .try_wait()
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("等待构建进程失败: {e}")))?;
        match st {
            Some(st) => break st,
            None => {
                if cancel.load(Ordering::SeqCst) {
                    let _ = job.terminate();
                    let _ = child.wait();
                    {
                        let mut g = inner.lock().expect("engine lock");
                        if let Some(slot) = g.slots.get_mut(id) {
                            slot.pid = None;
                            slot.job = None;
                            if let Ok(s) = apply(
                                slot.state,
                                RtEvent::ProcessExited {
                                    stop_requested: true,
                                },
                            ) {
                                slot.state = s;
                            }
                            emit_runtime(&g);
                        }
                    }
                    return Err(Error::new(ErrorCode::Spawn, "构建已被停止"));
                }
                if started.elapsed() > Duration::from_secs(20 * 60) {
                    let _ = job.terminate();
                    let _ = child.wait();
                    jar_build_fail(&inner, id, Some(format!("{stage_label} 超时（20 分钟）")));
                    return Err(Error::new(
                        ErrorCode::BuildFailed,
                        format!("{stage_label} 超时（20 分钟）"),
                    ));
                }
                thread::sleep(Duration::from_millis(60));
            }
        }
    };
    let code = status.code().unwrap_or(-1);
    if code != 0 {
        jar_build_fail(&inner, id, Some(format!("{stage_label} 退出码 {code}")));
        return Err(Error::new(
            ErrorCode::BuildFailed,
            format!("{stage_label} 退出码 {code}：已保留构建日志，服务未启动"),
        ));
    }
    let artifact = match phase {
        BuildPhaseKind::ReactorPrep => PathBuf::new(),
        BuildPhaseKind::JarArtifact if is_gradle => select_gradle_artifact(root, &module)?,
        BuildPhaseKind::JarArtifact => select_jar_artifact(root, &module)?,
    };
    {
        let mut g = inner.lock().expect("engine lock");
        if let Some(slot) = g.slots.get_mut(id) {
            slot.state = apply(slot.state, RtEvent::BuildFinished { ok: true })?;
            if phase == BuildPhaseKind::JarArtifact {
                slot.artifact = Some(artifact.clone());
            }
            slot.pid = None;
            slot.job = None;
            emit_runtime(&g);
        }
    }
    Ok(artifact)
}

fn jar_build_fail(inner: &Arc<Mutex<Inner>>, id: &str, detail: Option<String>) {
    let mut g = inner.lock().expect("engine lock");
    if let Some(slot) = g.slots.get_mut(id) {
        if slot.state == RtState::Building {
            if let Ok(s) = apply(slot.state, RtEvent::BuildFinished { ok: false }) {
                slot.state = s;
            }
            slot.last_error = Some(match detail {
                // 1.4：非零退出/超时都带 BUILD_FAILED 标记（§9 错误码语义）
                Some(d) if d.starts_with("BUILD_FAILED") => d,
                Some(d) => format!("BUILD_FAILED: {d}"),
                None => "BUILD_FAILED: 构建失败，服务未启动".to_string(),
            });
        }
        slot.pid = None;
        slot.job = None;
        emit_runtime(&g);
    }
}

/// §11.3 artifact 选择：排除 original-/-sources/-javadoc，唯一候选直接用；
/// 多候选按 pom artifactId 收敛；仍不确定 → JAR_AMBIGUOUS（不按时间猜）。
fn select_jar_artifact(root: &Path, module: &str) -> Result<PathBuf> {
    let module_dir = sandbox::confine(root, module)?;
    let target = module_dir.join("target");
    if !target.is_dir() {
        return Err(Error::new(
            ErrorCode::ArtifactMissing,
            format!("target 目录不存在: {}", target.display()),
        ));
    }
    let mut jars: Vec<PathBuf> = Vec::new();
    let entries = fs::read_dir(&target)
        .map_err(|e| Error::new(ErrorCode::ArtifactMissing, format!("无法读取 target: {e}")))?;
    for e in entries {
        let p = e
            .map_err(|e| Error::new(ErrorCode::ArtifactMissing, format!("读取 target 失败: {e}")))?
            .path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".jar") {
            continue;
        }
        if name.starts_with("original-")
            || name.ends_with("-sources.jar")
            || name.ends_with("-javadoc.jar")
        {
            continue;
        }
        jars.push(p);
    }
    if jars.len() == 1 {
        return Ok(jars.pop().expect("len==1"));
    }
    let artifact_id = pom_artifact_id(&module_dir.join("pom.xml"));
    let names_of = |list: &[PathBuf]| -> Vec<String> {
        list.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    if let Some(aid) = &artifact_id {
        let matched: Vec<PathBuf> = jars
            .iter()
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.contains(aid.as_str()))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if matched.len() == 1 {
            return Ok(matched.into_iter().next().expect("len==1"));
        }
        if jars.is_empty() || matched.is_empty() {
            return Err(Error::new(
                ErrorCode::ArtifactMissing,
                format!("target 中没有可执行 jar（artifactId={aid:?}）"),
            ));
        }
        let names = names_of(&matched);
        return Err(Error::new(
            ErrorCode::JarAmbiguous,
            format!("多个候选 jar，无法确定: {}", names.join(", ")),
        )
        .details(serde_yaml::to_value(&names).unwrap_or(serde_yaml::Value::Null)));
    }
    if jars.is_empty() {
        return Err(Error::new(
            ErrorCode::ArtifactMissing,
            "target 中没有可执行 jar",
        ));
    }
    let names = names_of(&jars);
    Err(Error::new(
        ErrorCode::JarAmbiguous,
        format!(
            "多个候选 jar 且 pom 未提供 artifactId: {}",
            names.join(", ")
        ),
    ))
}

/// 1.4 §5.3 gradle artifact 选择：`module/build/libs` 内排除 *-plain / -sources /
/// -javadoc 与非 jar；唯一候选直接用；零候选 ARTIFACT_MISSING、多候选
/// JAR_AMBIGUOUS（不按修改时间猜）；路径逃逸复用 1.2 沙箱（PATH_ESCAPE）。
fn select_gradle_artifact(root: &Path, module: &str) -> Result<PathBuf> {
    let module_dir = sandbox::confine(root, module)?;
    let libs = module_dir.join("build").join("libs");
    if !libs.is_dir() {
        return Err(Error::new(
            ErrorCode::ArtifactMissing,
            format!("build/libs 目录不存在: {}", libs.display()),
        ));
    }
    let mut jars: Vec<PathBuf> = Vec::new();
    let entries = fs::read_dir(&libs).map_err(|e| {
        Error::new(
            ErrorCode::ArtifactMissing,
            format!("无法读取 build/libs: {e}"),
        )
    })?;
    for e in entries {
        let p = e
            .map_err(|e| {
                Error::new(
                    ErrorCode::ArtifactMissing,
                    format!("读取 build/libs 失败: {e}"),
                )
            })?
            .path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".jar") {
            continue;
        }
        if name.ends_with("-plain.jar")
            || name.ends_with("-sources.jar")
            || name.ends_with("-javadoc.jar")
        {
            continue;
        }
        jars.push(p);
    }
    if jars.len() == 1 {
        return Ok(jars.pop().expect("len==1"));
    }
    let names_of = |list: &[PathBuf]| -> Vec<String> {
        list.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    };
    if jars.is_empty() {
        return Err(Error::new(
            ErrorCode::ArtifactMissing,
            "build/libs 中没有可执行 jar（已排除 *-plain.jar / *-sources.jar / *-javadoc.jar）",
        ));
    }
    let names = names_of(&jars);
    Err(Error::new(
        ErrorCode::JarAmbiguous,
        format!("多个候选 jar，无法确定: {}", names.join(", ")),
    )
    .details(serde_yaml::to_value(&names).unwrap_or(serde_yaml::Value::Null)))
}

/// pom.xml 的项目 artifactId（跳过 <parent> 块里的同名标签）。
fn pom_artifact_id(pom: &Path) -> Option<String> {
    let mut text = fs::read_to_string(pom).ok()?;
    if let Some(i) = text.find("<parent>") {
        if let Some(j) = text[i..].find("</parent>") {
            text = format!("{}{}", &text[..i], &text[i + j + "</parent>".len()..]);
        }
    }
    let re = Regex::new(r"<artifactId>\s*([^<\s]+)\s*</artifactId>").ok()?;
    re.captures(&text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// 1.2 §9.2 sampler：仅订阅时运行，每秒一批 `st.metrics`。
fn metrics_loop(inner: Arc<Mutex<Inner>>) {
    loop {
        thread::sleep(Duration::from_secs(1));
        {
            let mut g = inner.lock().expect("engine lock");
            if g.metrics_sub == 0 {
                break;
            }
            let now = Instant::now();
            let mut map: IndexMap<String, Option<crate::ipc::ServiceMetrics>> = IndexMap::new();
            let ids: Vec<String> = g.slots.keys().cloned().collect();
            for id in ids {
                let state = g.slots.get(&id).map(|s| s.state);
                let active = matches!(
                    state,
                    Some(RtState::Starting)
                        | Some(RtState::Running)
                        | Some(RtState::Unhealthy)
                        | Some(RtState::Building)
                );
                if !active {
                    if g.metrics.shift_remove(&id).is_some() {
                        map.insert(id.clone(), None);
                    }
                    g.metrics_prev.remove(&id);
                    continue;
                }
                let Some(job) = g.slots.get(&id).and_then(|s| s.job.clone()) else {
                    continue;
                };
                let cpu_ms = job.total_cpu_ms();
                let cpu_pct = match cpu_ms {
                    Some(c) => {
                        let prev = g.metrics_prev.get(&id).map(|(p, _)| *p);
                        let wall = g
                            .metrics_prev
                            .get(&id)
                            .map(|(_, t)| now.duration_since(*t).as_millis() as u64)
                            .unwrap_or(0)
                            .max(1);
                        crate::metrics::cpu_percent(c, prev, wall)
                    }
                    None => None,
                };
                if let Some(c) = cpu_ms {
                    g.metrics_prev.insert(id.clone(), (c, now));
                }
                let sm = crate::ipc::ServiceMetrics {
                    cpu_percent: cpu_pct,
                    memory_bytes: job.working_set_bytes(),
                    process_count: Some(job.pids().len() as u32),
                    sampled_at_ms: now_ms(),
                };
                g.metrics.insert(id.clone(), Some(sm.clone()));
                map.insert(id.clone(), Some(sm));
            }
            if !map.is_empty() {
                let _ = g
                    .events
                    .try_send(EngineEvent::Metrics(crate::ipc::MetricsPayload {
                        services: map,
                    }));
            }
        }
    }
}

fn spawn_waiter(inner: Arc<Mutex<Inner>>, id: String, mut child: Child) {
    thread::Builder::new()
        .name(format!("st-wait-{id}"))
        .spawn(move || {
            let status = child.wait();
            let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
            // ponytail: pumps may still be writing the last Maven ERROR line
            thread::sleep(Duration::from_millis(80));
            let mut g = inner.lock().expect("engine lock");
            let src = LogSource {
                kind: LogSourceKind::Service,
                id: id.clone(),
            };
            let err_msg =
                if !g.slots.get(&id).map(|s| s.stop_requested).unwrap_or(true) && code != 0 {
                    Some(exit_error_from_logs(&g, &src, code))
                } else {
                    None
                };
            let Some(slot) = g.slots.get_mut(&id) else {
                return;
            };
            slot.pid = None;
            slot.job = None;
            slot.cancel.store(true, Ordering::SeqCst);
            slot.last_exit = Some(ExitView {
                code,
                at_ms: now_ms(),
            });
            if let Some(msg) = err_msg {
                slot.last_error = Some(msg);
            }
            slot.exit_reason = if slot.stop_requested {
                None
            } else {
                Some("crash")
            };
            let ev = RtEvent::ProcessExited {
                stop_requested: slot.stop_requested,
            };
            if let Ok(next) = apply(slot.state, ev) {
                slot.state = next;
            }
            slot.restart_attempt = None;
            // 2.2 restart 监管：仅服务进程本身的意外退出（构建期退出由 build 流程
            // 收场，不走这里；compose 由 compose 文件自管，策略恒 never）。
            let crash_condition = !slot.stop_requested
                && slot.state == RtState::Exited
                && slot.restart.policy != crate::spec::RestartPolicy::Never
                && (slot.restart.policy == crate::spec::RestartPolicy::Always || code != 0);
            let supervise = if crash_condition && slot.restart_left > 0 {
                // 即将进行第 n 次自动重启（预算在监管线程占用额度时扣减）
                slot.restart_attempt = Some(slot.restart.max_retries - slot.restart_left + 1);
                true
            } else {
                if crash_condition {
                    // 预算耗尽后的最后一次崩溃：就地给出放弃原因
                    restart_give_up(slot);
                }
                false
            };
            emit_runtime(&g);
            drop(g);
            if supervise {
                spawn_supervisor(inner, id);
            }
        })
        .ok();
}

/// 2.2：预算耗尽后写放弃原因并清尝试序号。
fn restart_give_up(slot: &mut Slot) {
    slot.restart_attempt = None;
    slot.last_error = Some(format!(
        "自动重启 {} 次后放弃（restart 策略）",
        slot.restart.max_retries
    ));
}

/// 2.2：自动重启退避——1s 起指数递增，16s 封顶（默认 5 次最坏约 31s）。
fn restart_backoff(attempt: u32) -> Duration {
    Duration::from_secs(1u64 << (attempt.saturating_sub(1)).min(4))
}

/// 2.2：按 restart 策略的自动重启执行器。每轮占用一次额度：复核
/// （服务仍处 Exited、未被手动停止/关闭）→ 退避等待 → 复核 → 按启动计划重放。
/// spawn 失败（工具缺失、端口被占等可重试错误）消耗额度继续；
/// NotFound / AlreadyInProgress（工作区已关、与手动操作竞态）直接放弃。
fn spawn_supervisor(inner: Arc<Mutex<Inner>>, id: String) {
    thread::Builder::new()
        .name(format!("st-restart-{id}"))
        .spawn(move || loop {
            let (plan, attempt) = {
                let mut g = inner.lock().expect("engine lock");
                let Some(slot) = g.slots.get_mut(&id) else {
                    return; // 工作区已关闭/移交
                };
                if slot.state != RtState::Exited
                    || slot.stop_requested
                    || slot.restart_cancel.load(Ordering::SeqCst)
                {
                    return; // 手动停止或重新启动接管
                }
                if slot.restart_left == 0 {
                    restart_give_up(slot);
                    emit_runtime(&g);
                    return;
                }
                slot.restart_left -= 1;
                let attempt = slot.restart.max_retries - slot.restart_left;
                slot.restart_attempt = Some(attempt);
                (slot.restart_plan.clone(), attempt)
            };
            let Some(plan) = plan else {
                return; // 无启动计划（理论上策略 != never 必有），防御性退出
            };
            thread::sleep(restart_backoff(attempt));
            // 睡醒复核：退避期间用户可能已手动停止/关闭
            {
                let g = inner.lock().expect("engine lock");
                let keep = g.slots.get(&id).is_some_and(|slot| {
                    slot.state == RtState::Exited
                        && !slot.stop_requested
                        && !slot.restart_cancel.load(Ordering::SeqCst)
                });
                if !keep {
                    return;
                }
            }
            match respawn_from_plan(Arc::clone(&inner), &id, plan) {
                Ok(()) => return,
                Err(e) => {
                    if matches!(e.code(), ErrorCode::NotFound | ErrorCode::AlreadyInProgress) {
                        return;
                    }
                    // 可重试失败：写明原因，下一轮再试（额度在循环顶扣减）
                    let mut g = inner.lock().expect("engine lock");
                    if let Some(slot) = g.slots.get_mut(&id) {
                        slot.last_error = Some(format!("自动重启第 {attempt} 次失败: {e}"));
                        emit_runtime(&g);
                    }
                }
            }
        })
        .ok();
}

/// 2.2：按捕获的启动计划重新拉起服务进程。
fn respawn_from_plan(inner: Arc<Mutex<Inner>>, id: &str, plan: RestartPlan) -> Result<()> {
    spawn_core(
        inner,
        id.to_string(),
        plan.planned,
        plan.cwd,
        plan.health_spec,
        plan.health_none,
        plan.port,
        plan.kind,
        plan.pkg.as_deref(),
        plan.svc_grace,
        plan.build_tool.as_deref(),
        plan.spawner,
        plan.env_snapshot,
        plan.restart,
    )
}

fn exit_error_from_logs(g: &Inner, src: &LogSource, code: i32) -> String {
    let (items, _) = g.logs.snapshot(Some(src), 16);
    let hit = items.iter().rev().find(|l| {
        let t = l.text.to_ascii_uppercase();
        t.contains("ERROR") || t.contains("FAILURE") || t.contains("EXCEPTION")
    });
    match hit {
        Some(l) => l.text.clone(),
        None => format!("进程退出码 {code}"),
    }
}

fn spawn_health(
    inner: Arc<Mutex<Inner>>,
    id: String,
    spec: crate::spec::HealthSpec,
    port: Option<u16>,
    cancel: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name(format!("st-health-{id}"))
        .spawn(move || {
            let interval = Duration::from_secs(spec.interval_secs.max(1) as u64);
            loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                thread::sleep(interval);
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                // 锁外做慢调用：Job 进程树 pid → 系统 LISTEN 端点表（netstat2，<50ms）。
                // 探测目标与真实监听对齐：IPv6-only 监听（Node/Vite 默认）和
                // 猜错的应用端口都能命中；表读取失败则退回双栈 127/[::1]。
                let (eps, in_terminal) = {
                    let g = inner.lock().expect("engine lock");
                    match g.slots.get(&id) {
                        None => break,
                        Some(slot) => {
                            let terminal = matches!(
                                slot.state,
                                RtState::Stopped | RtState::Exited | RtState::Stopping
                            );
                            let pids = slot.job.as_ref().map(|j| j.pids()).unwrap_or_default();
                            let mut eps = Vec::new();
                            if !pids.is_empty() {
                                // 表读取失败 → 空，探测自然退回双栈 127/[::1]
                                if let Ok(m) = crate::discover::listen_endpoints_by_pid() {
                                    for p in &pids {
                                        if let Some(v) = m.get(p) {
                                            eps.extend(v.iter().cloned());
                                        }
                                    }
                                }
                                // IPv4 优先、同族按端口升序；去重防 netstat2 同口多行
                                eps.sort_by_key(|e| (u8::from(e.ip.is_ipv6()), e.port));
                                eps.dedup_by_key(|e| (e.ip, e.port));
                            }
                            (eps, terminal)
                        }
                    }
                };
                if in_terminal {
                    break;
                }
                let r = health::check_with_endpoints(&spec, port, &eps);
                let mut g = inner.lock().expect("engine lock");
                let Some(slot) = g.slots.get_mut(&id) else {
                    break;
                };
                let past = slot
                    .started
                    .map(|t| t.elapsed() >= slot.grace)
                    .unwrap_or(true);
                let ev = if r.ok {
                    RtEvent::HealthOk
                } else {
                    RtEvent::HealthFail { past_grace: past }
                };
                let prev = slot.state;
                if let Ok(next) = apply(slot.state, ev) {
                    slot.state = next;
                }
                slot.health = Some(HealthView {
                    ok: r.ok,
                    at_ms: now_ms(),
                    detail: r.detail,
                });
                if slot.state != prev {
                    emit_runtime(&g);
                }
            }
        })
        .ok();
}

fn batch_loop(inner: Arc<Mutex<Inner>>, rx: Receiver<LogLine>) {
    let mut batcher = LogBatcher::default();
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                let now = now_ms();
                if let Some(items) = batcher.push(now, line) {
                    flush_logs(&inner, items);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(items) = batcher.poll_flush(now_ms()) {
                    flush_logs(&inner, items);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn flush_logs(inner: &Mutex<Inner>, items: Vec<LogLine>) {
    if items.is_empty() {
        return;
    }
    let g = inner.lock().expect("engine lock");
    if g.subscribers == 0 || g.workspace_id.is_empty() {
        return;
    }
    let _ = g.events.try_send(EngineEvent::Logs {
        workspace_id: g.workspace_id.clone(),
        items,
    });
}

// ============================================================================
// 1.3 phase 3/4：compose 运行时与镜像构建（自由函数，测试可断言 argv）
// ============================================================================

fn compose_up_args(info: &ComposeInfo) -> Vec<String> {
    let mut a = crate::docker::compose_base_args(&info.file, info.project.as_deref());
    // --no-deps 必带：SuperTask 依赖图是顺序唯一真源（§5.2）；只允许单服务名
    a.extend([
        "up".to_string(),
        "-d".to_string(),
        "--no-deps".to_string(),
        info.service.clone(),
    ]);
    a
}

fn compose_stop_args(info: &ComposeInfo) -> Vec<String> {
    let mut a = crate::docker::compose_base_args(&info.file, info.project.as_deref());
    a.extend(["stop".to_string(), info.service.clone()]);
    a
}

fn compose_ps_args(info: &ComposeInfo) -> Vec<String> {
    let mut a = crate::docker::compose_base_args(&info.file, info.project.as_deref());
    a.extend([
        "ps".to_string(),
        "--format".to_string(),
        "json".to_string(),
        info.service.clone(),
    ]);
    a
}

fn compose_logs_args(info: &ComposeInfo) -> Vec<String> {
    let mut a = crate::docker::compose_base_args(&info.file, info.project.as_deref());
    a.extend([
        "logs".to_string(),
        "--follow".to_string(),
        "--no-color".to_string(),
        "--timestamps".to_string(),
        info.service.clone(),
    ]);
    a
}

/// compose 服务健康口径（§5.1）：显式 health 优先；默认 tcp（有 port 时），
/// 无 port 则 none。
fn compose_health(svc: &crate::spec::ServiceSpec) -> Option<crate::spec::HealthSpec> {
    if let Some(h) = &svc.health {
        return Some(h.clone());
    }
    svc.port.map(|_| crate::spec::HealthSpec {
        r#type: HealthType::Tcp,
        http: None,
        interval_secs: 2,
        timeout_secs: 2,
    })
}

fn map_docker_spawn_err(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        Error::new(
            ErrorCode::DockerNotFound,
            "未找到 docker。请安装 Docker Desktop 并确保在 PATH 中。",
        )
    } else {
        Error::new(
            ErrorCode::DockerEngineUnreachable,
            format!("docker 命令执行失败: {e}"),
        )
    }
}

/// 重查容器是否仍存在且未退出：Some(true/false)；查询失败（daemon 不可达）None。
fn compose_container_running(
    runner: &Arc<dyn DockerRunner>,
    root: &Path,
    info: &ComposeInfo,
) -> Option<bool> {
    let out = runner
        .run(&DockerSpawn {
            args: compose_ps_args(info),
            cwd: Some(root.to_path_buf()),
            timeout: COMPOSE_QUERY_TIMEOUT,
        })
        .ok()?;
    if out.code != 0 {
        return None;
    }
    let items = crate::docker::parse_ps(&out.stdout);
    Some(items.iter().any(|c| !c.exited()))
}

/// compose up 后台流程（§5.2）：up -d --no-deps →（成功）日志跟随 + 健康 +
/// 状态轮询；（失败）回 stopped + COMPOSE_UP_FAILED。
fn compose_up_flow(
    inner: Arc<Mutex<Inner>>,
    id: String,
    info: ComposeInfo,
    port: Option<u16>,
    health: Option<crate::spec::HealthSpec>,
    cancel: Arc<AtomicBool>,
    runner: Arc<dyn DockerRunner>,
) {
    let src = LogSource {
        kind: LogSourceKind::Service,
        id: id.clone(),
    };
    let root = inner.lock().expect("engine lock").root.clone();
    // docker CLI 自身输出（含拉取进度）→ system stream（§5.4）
    let code = match runner.run(&DockerSpawn {
        args: compose_up_args(&info),
        cwd: Some(root.clone()),
        timeout: COMPOSE_UP_TIMEOUT,
    }) {
        Ok(out) => {
            for line in out.stdout.lines().chain(out.stderr.lines()) {
                if !line.trim().is_empty() {
                    push_line(&inner, src.clone(), LogStream::System, line.to_string());
                }
            }
            out.code
        }
        Err(e) => {
            push_line(
                &inner,
                src.clone(),
                LogStream::System,
                format!("docker compose up 执行失败: {e}"),
            );
            -1
        }
    };
    if code != 0 {
        // up 非零 → 状态回 stopped，last_error 带输出摘要；不进 running（§5.2）
        let mut g = inner.lock().expect("engine lock");
        if let Some(slot) = g.slots.get_mut(&id) {
            if let Ok(s) = apply(
                slot.state,
                RtEvent::ProcessExited {
                    stop_requested: true,
                },
            ) {
                slot.state = s;
            }
            slot.started = None;
            slot.pid = None;
            slot.last_error = Some(format!(
                "COMPOSE_UP_FAILED: docker compose up 退出码 {code}"
            ));
            emit_runtime(&g);
        }
        return;
    }
    let stop_after_up = {
        let mut g = inner.lock().expect("engine lock");
        let Some(slot) = g.slots.get_mut(&id) else {
            return;
        };
        if let Some(c) = slot.compose.as_mut() {
            c.started_by_engine = true;
        }
        // up 期间用户已请求停止：立即补一次 stop，不进入 running
        let stop_requested = slot.stop_requested;
        if !stop_requested {
            let health_none = health
                .as_ref()
                .map(|h| h.r#type == HealthType::None)
                .unwrap_or(true);
            if health_none {
                slot.state = RtState::Running;
            }
            emit_runtime(&g);
        }
        stop_requested
    };
    if stop_after_up {
        let _ = runner.run(&DockerSpawn {
            args: compose_stop_args(&info),
            cwd: Some(root),
            timeout: COMPOSE_STOP_TIMEOUT,
        });
        let mut g = inner.lock().expect("engine lock");
        if let Some(slot) = g.slots.get_mut(&id) {
            slot.state = RtState::Stopped;
            slot.started = None;
            emit_runtime(&g);
        }
        return;
    }
    compose_follow_logs(&inner, &id, &info, &cancel, &runner);
    if let Some(hs) = &health {
        if hs.r#type != HealthType::None {
            spawn_health(inner.clone(), id.clone(), hs.clone(), port, cancel.clone());
        }
    }
    compose_monitor(inner, id, info, health, cancel, runner, root);
}

/// §5.4 日志跟随：`logs --follow --no-color --timestamps`，从当前开始不回放。
/// 容器停止后 --follow 自然结束；cancel 时 kill docker 进程收尾。
fn compose_follow_logs(
    inner: &Arc<Mutex<Inner>>,
    id: &str,
    info: &ComposeInfo,
    cancel: &Arc<AtomicBool>,
    runner: &Arc<dyn DockerRunner>,
) {
    let src = LogSource {
        kind: LogSourceKind::Service,
        id: id.to_string(),
    };
    let root = inner.lock().expect("engine lock").root.clone();
    let stream = match runner.run_stream(&DockerSpawn {
        args: compose_logs_args(info),
        cwd: Some(root),
        timeout: COMPOSE_UP_TIMEOUT, // run_stream 不使用超时
    }) {
        Ok(s) => s,
        Err(e) => {
            push_line(
                inner,
                src,
                LogStream::System,
                format!("日志跟随启动失败（不影响服务状态）: {e}"),
            );
            return;
        }
    };
    let crate::docker::DockerStream {
        stdout,
        stderr,
        kill,
        wait: _, // --follow 无需退出码
    } = stream;
    // stderr：docker CLI 自身错误 → system stream
    spawn_pump(
        inner.clone(),
        src.clone(),
        LogStream::System,
        stderr,
        cancel.clone(),
    );
    let done = Arc::new(AtomicBool::new(false));
    // stdout：容器日志 → stdout stream；EOF → done
    {
        let inner2 = Arc::clone(inner);
        let src2 = src.clone();
        let cancel2 = Arc::clone(cancel);
        let done2 = Arc::clone(&done);
        let _ = thread::Builder::new()
            .name(format!("st-clog-{id}"))
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut buf: Vec<u8> = Vec::with_capacity(512);
                loop {
                    if cancel2.load(Ordering::Relaxed) {
                        break;
                    }
                    buf.clear();
                    match reader.read_until(b'\n', &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    if buf.last() == Some(&b'\n') {
                        buf.pop();
                    }
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                    push_line(&inner2, src2.clone(), LogStream::Stdout, decode_line(&buf));
                }
                done2.store(true, Ordering::SeqCst);
            });
    }
    // watcher：cancel → kill docker logs 进程；正常结束（done）→ 直接退出
    let cancel_w = Arc::clone(cancel);
    let done_w = Arc::clone(&done);
    thread::Builder::new()
        .name(format!("st-clog-kill-{id}"))
        .spawn(move || loop {
            if done_w.load(Ordering::Relaxed) {
                break;
            }
            if cancel_w.load(Ordering::Relaxed) {
                kill();
                break;
            }
            thread::sleep(Duration::from_millis(200));
        })
        .ok();
}

/// §5.3 状态轮询：按健康探测节奏 `compose ps --format json <service>`。
/// 容器 exited 且非本引擎 stop 请求 → exited/crash（复用 1.2 崩溃通知路径）。
#[allow(clippy::too_many_arguments)]
fn compose_monitor(
    inner: Arc<Mutex<Inner>>,
    id: String,
    info: ComposeInfo,
    health: Option<crate::spec::HealthSpec>,
    cancel: Arc<AtomicBool>,
    runner: Arc<dyn DockerRunner>,
    root: PathBuf,
) {
    let interval = Duration::from_secs(
        health
            .as_ref()
            .map(|h| h.interval_secs.max(1) as u64)
            .unwrap_or(2),
    );
    let _ = thread::Builder::new()
        .name(format!("st-cps-{id}"))
        .spawn(move || loop {
            thread::sleep(interval);
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            {
                let g = inner.lock().expect("engine lock");
                match g.slots.get(&id).map(|s| s.state) {
                    None
                    | Some(RtState::Stopped)
                    | Some(RtState::Exited)
                    | Some(RtState::Stopping) => break,
                    _ => {}
                }
            }
            // daemon 暂不可达：跳过本轮，恢复后自动纠正（§12 可靠性）
            let Ok(out) = runner.run(&DockerSpawn {
                args: compose_ps_args(&info),
                cwd: Some(root.clone()),
                timeout: COMPOSE_QUERY_TIMEOUT,
            }) else {
                continue;
            };
            if out.code != 0 {
                continue;
            }
            let Some(c) = crate::docker::parse_ps(&out.stdout)
                .into_iter()
                .find(|c| c.exited())
            else {
                continue;
            };
            let mut g = inner.lock().expect("engine lock");
            let Some(slot) = g.slots.get_mut(&id) else {
                break;
            };
            if slot.stop_requested {
                continue; // 引擎 stop 流程负责收尾
            }
            // 外部退出（docker stop / OOM）→ exited + crash
            slot.last_exit = Some(ExitView {
                code: c.exit_code.unwrap_or(-1),
                at_ms: now_ms(),
            });
            slot.exit_reason = Some("crash");
            slot.pid = None;
            slot.started = None;
            slot.cancel.store(true, Ordering::SeqCst); // 停健康探测与日志泵
            if let Ok(s) = apply(
                slot.state,
                RtEvent::ProcessExited {
                    stop_requested: false,
                },
            ) {
                slot.state = s;
            }
            emit_runtime(&g);
            break;
        });
}

/// §6.2 镜像构建公共执行：流式输出进日志 + 尾部 20 行作 operation message +
/// 逐行检查取消（取消杀进程，不删层，状态如实 cancelled）。
fn run_build_streaming(
    inner: &Arc<Mutex<Inner>>,
    ctx: &crate::operation::OperationCtx,
    runner: &Arc<dyn DockerRunner>,
    spawn: &DockerSpawn,
    src: &LogSource,
    label: &str,
) -> Result<serde_yaml::Value> {
    let crate::docker::DockerStream {
        stdout,
        stderr,
        kill,
        wait,
    } = match runner.run_stream(spawn) {
        Ok(s) => s,
        Err(e) => {
            return Err(map_docker_spawn_err(e));
        }
    };
    ctx.on_cancel(kill); // 取消 → 杀构建进程（best effort，不删已提交层）
                         // stderr 独立线程排水（BuildKit 进度走 stderr；防管道写满互卡）
    let inner2 = Arc::clone(inner);
    let src2 = src.clone();
    let ctx2 = ctx.clone();
    let err_thread = thread::spawn(move || {
        let mut lines: Vec<String> = Vec::new();
        let mut reader = BufReader::new(stderr);
        let mut buf: Vec<u8> = Vec::with_capacity(512);
        loop {
            if ctx2.cancelled() {
                break;
            }
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if buf.last() == Some(&b'\n') {
                buf.pop();
            }
            if buf.last() == Some(&b'\r') {
                buf.pop();
            }
            let text = decode_line(&buf);
            push_line(&inner2, src2.clone(), LogStream::System, text.clone());
            lines.push(text);
        }
        lines
    });
    let mut out_lines: Vec<String> = Vec::new();
    let mut reader = BufReader::new(stdout);
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    loop {
        if ctx.cancelled() {
            break;
        }
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.last() == Some(&b'\r') {
            buf.pop();
        }
        let text = decode_line(&buf);
        push_line(inner, src.clone(), LogStream::System, text.clone());
        out_lines.push(text);
    }
    let err_lines = err_thread.join().unwrap_or_default();
    if ctx.cancelled() {
        return Err(Error::new(ErrorCode::Spawn, "构建已取消"));
    }
    let code = {
        let mut wait = wait;
        wait()
    };
    let mut all = out_lines;
    all.extend(err_lines);
    // 单行截断由日志管道负责；operation message 只带尾部摘要（默认最后 20 行）
    let tail = tail_lines(&all, 20);
    if code != 0 {
        return Err(Error::new(
            ErrorCode::ImageBuildFailed,
            format!("{label} 退出码 {code}：{tail}"),
        ));
    }
    ctx.report(None, tail.clone());
    Ok(serde_yaml::Value::String(tail))
}

/// 取输出尾部（最后 n 行，空行跳过），作构建失败/成功的摘要。
fn tail_lines(lines: &[String], n: usize) -> String {
    let mut tail: Vec<String> = lines
        .iter()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(n)
        .map(|l| crate::log::truncate_line(l.clone()))
        .collect();
    tail.reverse();
    tail.join("\n")
}

// ============================================================================
// 1.6 phase 4：网关托管自由函数（GatewaySlot 生命周期）
// ============================================================================

fn rt_state_str(s: RtState) -> String {
    serde_yaml::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{s:?}"))
}

/// 1.6 §4.2：上游回环地址选择（复用 1.2 监听口径）——IPv4 可达 → 127.0.0.1；
/// 仅 IPv6 监听 → [::1]；未运行/双栈 → 127.0.0.1。
fn loopback_host_for(port: u16) -> String {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    let probe = |ip: IpAddr| {
        TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(150)).is_ok()
    };
    if probe(IpAddr::V4(Ipv4Addr::LOCALHOST)) {
        return "127.0.0.1".into();
    }
    if probe(IpAddr::V6(Ipv6Addr::LOCALHOST)) {
        return "[::1]".into();
    }
    "127.0.0.1".into()
}

/// apache LoadModule 目录：bin 同级 modules/（XAMPP 与官方 zip 布局一致）。
fn apache_modules_dir_of(bin: &Path) -> Option<String> {
    bin.parent()
        .map(|d| d.join("modules").to_string_lossy().into_owned())
}

fn stderr_from_details(e: &Error) -> Option<String> {
    let Error::App {
        details: Some(d), ..
    } = e
    else {
        return None;
    };
    d.get("stderr").and_then(|v| v.as_str()).map(str::to_string)
}

/// 按当前 spec 重建网关 slot（open / gateway_start / apply 共用；幂等）。
fn rebuild_gateway_slot(g: &mut Inner) -> Result<()> {
    g.gateway = None;
    let Some((port, kind)) = g
        .spec
        .gateway
        .as_ref()
        .filter(|c| c.kind.is_some() && c.enabled)
        .map(|c| (c.port, c.kind.expect("kind checked")))
    else {
        return Ok(());
    };
    if !g.files.contains_key("gateway") {
        let rel = log_file_rel("gateway", "gateway");
        let lf = LogFile::open_with_files(
            g.root.join(&rel),
            g.spec.logging.as_ref().and_then(|l| l.max_bytes),
            g.spec.logging.as_ref().and_then(|l| l.retain_tail_bytes),
            g.spec.log_retention.as_ref().and_then(|r| r.max_files),
        )
        .map_err(|e| Error::new(ErrorCode::Spawn, format!("无法创建网关日志文件: {e}")))?;
        g.files.insert("gateway".into(), lf);
    }
    g.gateway = Some(GatewaySlot {
        state: RtState::Stopped,
        pid: None,
        port,
        kind,
        job: None,
        stop_requested: false,
        cancel: Arc::new(AtomicBool::new(false)),
        started: None,
        started_at_ms: None,
        health: None,
        last_exit: None,
        last_error: None,
        exit_reason: None,
    });
    Ok(())
}

fn spawn_gateway_process(
    bin: &Path,
    args: &[String],
    cwd: &Path,
) -> Result<(Child, Arc<dyn crate::proc::ProcessTree>)> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::proc::create_tree()?;
    let child = job.spawn(&mut cmd)?;
    Ok((child, job))
}

/// 网关进程退出收尾（与 spawn_waiter 同语义，source=gateway）。
fn spawn_gateway_waiter(inner: Arc<Mutex<Inner>>, mut child: Child) {
    thread::Builder::new()
        .name("st-gw-wait".into())
        .spawn(move || {
            let status = child.wait();
            let code = status.ok().and_then(|s| s.code()).unwrap_or(-1);
            // 日志泵可能还有最后几行在写
            thread::sleep(Duration::from_millis(80));
            let mut g = inner.lock().expect("engine lock");
            let stop_requested = g.gateway.as_ref().map(|s| s.stop_requested).unwrap_or(true);
            let err_msg = if !stop_requested && code != 0 {
                let src = LogSource {
                    kind: LogSourceKind::Gateway,
                    id: "gateway".into(),
                };
                Some(format!(
                    "GATEWAY_START_FAILED: {}",
                    exit_error_from_logs(&g, &src, code)
                ))
            } else {
                None
            };
            let Some(slot) = g.gateway.as_mut() else {
                return;
            };
            slot.pid = None;
            slot.job = None;
            slot.cancel.store(true, Ordering::SeqCst);
            slot.last_exit = Some(ExitView {
                code,
                at_ms: now_ms(),
            });
            if let Some(msg) = err_msg {
                slot.last_error = Some(msg);
            }
            slot.exit_reason = if stop_requested { None } else { Some("crash") };
            let ev = RtEvent::ProcessExited { stop_requested };
            if let Ok(next) = apply(slot.state, ev) {
                slot.state = next;
            }
            emit_runtime(&g);
        })
        .ok();
}

/// 网关 TCP 健康（§7）：loopback 双栈探测自身监听端口；grace 3s。
fn spawn_gateway_health(inner: Arc<Mutex<Inner>>, port: u16, cancel: Arc<AtomicBool>) {
    thread::Builder::new()
        .name("st-gw-health".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_secs(1));
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let ok = crate::ports::is_serving(port);
            let mut g = inner.lock().expect("engine lock");
            let Some(slot) = g.gateway.as_mut() else {
                break;
            };
            let past = slot
                .started
                .map(|t| t.elapsed() >= Duration::from_secs(3))
                .unwrap_or(true);
            let ev = if ok {
                RtEvent::HealthOk
            } else {
                RtEvent::HealthFail { past_grace: past }
            };
            let prev = slot.state;
            if let Ok(next) = apply(slot.state, ev) {
                slot.state = next;
            }
            slot.health = Some(HealthView {
                ok,
                at_ms: now_ms(),
                detail: if ok {
                    format!("tcp 127.0.0.1:{port} 可达")
                } else {
                    format!("tcp 127.0.0.1:{port} 无监听")
                },
            });
            if slot.state != prev {
                emit_runtime(&g);
            }
        })
        .ok();
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    use crate::docker::FakeDockerRunner;

    /// 回归：node 服务日志出现乱码
    /// 1) cmd/npm 包装层 echo 的中文是 GBK（936）字节，UTF-8 严格解码必乱
    /// 2) 旧实现 lines() 遇 InvalidData 直接断流，后续日志全部丢失
    #[test]
    fn decode_line_utf8_gbk_and_junk() {
        // UTF-8 原样通过
        assert_eq!(decode_line("服务启动 ok".as_bytes()), "服务启动 ok");
        // GBK 字节：服=0xB7FE 务=0xCEF1（npm.cmd / 批处理 echo 的形态）
        assert_eq!(decode_line(&[0xB7, 0xFE, 0xCE, 0xF1]), "服务");
        // 混输：GBK 中文夹在 ASCII 里
        assert_eq!(
            decode_line(&[b'i', b'n', b's', b't', b'a', b'l', b'l', b' ', 0xB7, 0xFE]),
            "install 服"
        );
        // 无法解释的字节 → 有损替换，不 panic、不丢行
        assert!(decode_line(&[0xFF, 0xFE, 0xFD]).contains('\u{FFFD}'));
        // 空行 / CRLF 已在上游裁剪
        assert_eq!(decode_line(&[]), "");
    }

    /// 回归：Vite 启动横幅带 ANSI 颜色码，剥离后应还原为可读纯文本
    #[test]
    fn strip_ansi_vite_banner() {
        // 用户实拍：`[32m[1mVITE[22m v5.4.21[39m ... ready`
        let raw = "\u{1b}[32m\u{1b}[1mVITE\u{1b}[22m v5.4.21\u{1b}[39m  \u{1b}[2mready in \u{1b}[0m\u{1b}[1m222\u{1b}[22m\u{1b}[2m\u{1b}[0m ms\u{1b}[22m";
        assert_eq!(strip_ansi(raw), "VITE v5.4.21  ready in 222 ms");
        // Local 行：URL 颜色分段
        let loc = "  \u{1b}[1mLocal\u{1b}[22m:   \u{1b}[36mhttp://localhost:\u{1b}[1m5173\u{1b}[39m\u{1b}[22m/";
        assert_eq!(strip_ansi(loc), "  Local:   http://localhost:5173/");
        // 纯文本原样
        assert_eq!(strip_ansi("hello 世界"), "hello 世界");
        // 裸 ESC 单字符（游标移动）也清掉
        assert_eq!(strip_ansi("\u{1b}[Aok"), "ok");
    }

    fn write_ws_yaml(yaml: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root =
            crate::sandbox::test_temp_dir().join(format!("st-eng-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("supertask.yaml"), yaml).unwrap();
        root
    }

    fn ping_yaml() -> &'static str {
        r#"
version: 1
services:
  ping:
    kind: spring-boot
    module: x
    port: 1
    health:
      type: none
    grace_secs: 1
"#
    }

    fn wait_eq(eng: &Engine, id: &str, want: RtState) -> bool {
        for _ in 0..80 {
            if eng.state_of(id) == Some(want) {
                return true;
            }
            thread::sleep(Duration::from_millis(40));
        }
        false
    }

    fn wait_eq_for(eng: &Engine, id: &str, want: RtState, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if eng.state_of(id) == Some(want) {
                return true;
            }
            thread::sleep(Duration::from_millis(40));
        }
        false
    }

    // ---- 2.2 restart 策略：自动重启监管（Fail spawner = 立即 exit 1 的进程） ----

    fn ping_yaml_with_restart(policy: &str, max_retries: u32) -> String {
        format!(
            r#"
version: 1
services:
  ping:
    kind: spring-boot
    module: x
    port: 1
    health:
      type: none
    grace_secs: 1
    restart: {policy}
    max_retries: {max_retries}
"#
        )
    }

    /// on-failure：崩溃 → 自动重启（1s/2s 退避）→ 预算耗尽 → 放弃并写原因。
    #[test]
    fn restart_on_failure_exhausts_budget_and_gives_up() {
        let root = write_ws_yaml(&ping_yaml_with_restart("on-failure", 2));
        let eng = Engine::fail_for_test();
        eng.open(&root).unwrap();
        eng.start_one("ping").unwrap();
        assert!(
            wait_eq(&eng, "ping", RtState::Exited),
            "首次崩溃应置 Exited"
        );
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut seen_attempts = std::collections::HashSet::new();
        let mut gave_up = false;
        while Instant::now() < deadline {
            let view = eng
                .snapshot()
                .unwrap()
                .services
                .get("ping")
                .unwrap()
                .clone();
            if let Some(n) = view.restart_attempt {
                seen_attempts.insert(n);
            }
            if view
                .last_error
                .as_deref()
                .is_some_and(|e| e.contains("自动重启 2 次后放弃"))
            {
                gave_up = true;
                assert_eq!(view.restart_attempt, None, "放弃后序号应清空");
                break;
            }
            thread::sleep(Duration::from_millis(40));
        }
        assert!(gave_up, "应在预算耗尽后写放弃原因");
        assert!(
            seen_attempts.contains(&1),
            "应观察到第 1 次自动重启序号: {seen_attempts:?}"
        );
        assert_eq!(eng.state_of("ping"), Some(RtState::Exited));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    /// 崩溃后的退避窗口内手动停止：监管线程必须让位，不再拉起。
    #[test]
    fn restart_manual_stop_cancels_pending_restart() {
        let root = write_ws_yaml(&ping_yaml_with_restart("on-failure", 3));
        let eng = Engine::fail_for_test();
        eng.open(&root).unwrap();
        eng.start_one("ping").unwrap();
        assert!(wait_eq(&eng, "ping", RtState::Exited));
        eng.stop_one("ping").unwrap();
        assert_eq!(eng.state_of("ping"), Some(RtState::Stopped));
        // 覆盖 1s 退避窗口：若监管线程未被取消，这里会看到重新 Starting
        thread::sleep(Duration::from_millis(2500));
        assert_eq!(eng.state_of("ping"), Some(RtState::Stopped));
        let view = eng
            .snapshot()
            .unwrap()
            .services
            .get("ping")
            .unwrap()
            .clone();
        assert!(
            !view
                .last_error
                .as_deref()
                .is_some_and(|e| e.contains("放弃")),
            "手动停止不应触发放弃原因: {:?}",
            view.last_error
        );
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    // ---- 1.3 phase 3/4：compose 运行时与镜像构建（全 fake，不真调 docker） ----

    fn script_probe_ok(fake: &FakeDockerRunner) {
        fake.push_ok(r#"{"Client":{"Version":"27.1.1"},"Server":{"Version":"27.1.1"}}"#);
        fake.push_ok(r#"{"ComposeVersion":"v2.29.1"}"#);
    }

    fn compose_config_json() -> String {
        r#"{"services":{
            "redis":{"image":"redis:7","ports":[{"target":6379,"published":6379}],"build":{"context":"."}},
            "mysql":{"image":"mysql:8","ports":[{"target":3306,"published":3306}]}
        }}"#
        .into()
    }

    /// compose 工作区：supertask.yaml + compose.yaml。
    fn compose_ws(services_yaml: &str, docker_yaml: &str) -> PathBuf {
        static N: AtomicU64 = AtomicU64::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let root = crate::sandbox::test_temp_dir().join(format!(
            "st-eng-comp{}-{}",
            std::process::id(),
            n
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("compose.yaml"),
            "services:\n  redis:\n    image: redis:7\n",
        )
        .unwrap();
        fs::write(
            root.join("supertask.yaml"),
            format!(
                "version: 1\nservices:\n{services_yaml}\ndocker:\n  compose_file: compose.yaml\n{docker_yaml}"
            ),
        )
        .unwrap();
        root
    }

    fn redis_only_yaml() -> &'static str {
        "  redis:\n    kind: compose\n    service: redis\n"
    }

    #[test]
    fn compose_start_running_stop_and_argv() {
        let root = compose_ws(redis_only_yaml(), "");
        let fake = Arc::new(FakeDockerRunner::new());
        // open 时的 compose 校验（config）
        fake.push_ok(compose_config_json());
        // start：probe ×2 + up（config 走缓存不重复 spawn）
        script_probe_ok(&fake);
        fake.push_ok("");
        fake.push_stream_ok("redis log line\n");
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();
        eng.subscribe_logs().unwrap();
        eng.start_one("redis").unwrap();

        assert!(
            wait_eq(&eng, "redis", RtState::Running),
            "{:?}",
            eng.state_of("redis")
        );

        let calls = fake.calls();
        // up argv：compose --ansi never -f <file> up -d --no-deps <单服务名>
        let up = calls
            .iter()
            .find(|c| c.args.contains(&"up".to_string()))
            .expect("up call");
        assert_eq!(
            &up.args[0..4],
            &[
                "compose".to_string(),
                "--ansi".to_string(),
                "never".to_string(),
                "-f".to_string()
            ]
        );
        assert!(up.args[4].ends_with("compose.yaml"));
        assert_eq!(
            &up.args[5..],
            &[
                "up".to_string(),
                "-d".to_string(),
                "--no-deps".to_string(),
                "redis".to_string()
            ],
            "up 必带 --no-deps 且只允许单服务名"
        );
        assert_eq!(up.cwd.as_deref(), Some(root.as_path()));

        // config 缓存：open 1 次 + start 0 次
        let config_calls = calls
            .iter()
            .filter(|c| {
                c.args
                    .windows(2)
                    .any(|w| w == ["config".to_string(), "--format".to_string()])
            })
            .count();
        assert_eq!(config_calls, 1, "config 应命中 mtime+hash 缓存");

        // logs --follow argv + 输出进 stdout stream
        let logs = calls
            .iter()
            .find(|c| c.args.contains(&"logs".to_string()))
            .expect("logs call");
        assert_eq!(
            &logs.args[logs.args.len() - 5..],
            &[
                "logs".to_string(),
                "--follow".to_string(),
                "--no-color".to_string(),
                "--timestamps".to_string(),
                "redis".to_string(),
            ]
        );
        thread::sleep(Duration::from_millis(300));
        let (lines, _) = eng
            .logs_snapshot(
                Some(&LogSource {
                    kind: LogSourceKind::Service,
                    id: "redis".into(),
                }),
                50,
            )
            .unwrap();
        assert!(
            lines.iter().any(|l| l.text.contains("redis log line")),
            "{lines:?}"
        );

        // snapshot：compose 服务 pid=None、metrics 不出现、managed=true、kind=compose
        let snap = eng.snapshot().unwrap();
        let view = snap.services.get("redis").unwrap();
        assert_eq!(view.kind, "compose");
        assert_eq!(view.pid, None);
        assert!(view.managed);
        assert!(snap.metrics.get("redis").is_none());

        // stop：不带 --rm / down / rm
        eng.stop_one("redis").unwrap();
        assert_eq!(eng.state_of("redis"), Some(RtState::Stopped));
        let calls = fake.calls();
        let stop = calls
            .iter()
            .find(|c| c.args.contains(&"stop".to_string()))
            .expect("stop call");
        assert_eq!(
            &stop.args[stop.args.len() - 2..],
            &["stop".to_string(), "redis".to_string()],
        );
        assert!(!stop
            .args
            .iter()
            .any(|a| a == "--rm" || a == "down" || a == "rm"));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_up_failure_back_to_stopped() {
        let root = compose_ws(redis_only_yaml(), "");
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        script_probe_ok(&fake);
        fake.push_fail(1, "pull access denied for redis");
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();
        eng.start_one("redis").unwrap(); // accepted，up 异步执行
        assert!(
            wait_eq_for(&eng, "redis", RtState::Stopped, 5),
            "{:?}",
            eng.state_of("redis")
        );
        let snap = eng.snapshot().unwrap();
        let err = snap
            .services
            .get("redis")
            .unwrap()
            .last_error
            .clone()
            .unwrap();
        assert!(err.contains("COMPOSE_UP_FAILED"), "{err}");
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_external_exit_becomes_exited_crash() {
        let root = compose_ws(redis_only_yaml(), "");
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        script_probe_ok(&fake);
        fake.push_ok(""); // up
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();
        eng.start_one("redis").unwrap();
        assert!(wait_eq(&eng, "redis", RtState::Running));
        // 外部 docker stop / OOM：compose ps 报 exited
        fake.push_ok(
            r#"[{"ID":"a1","Name":"ws-redis-1","Service":"redis","Image":"redis:7","State":"exited","ExitCode":137}]"#,
        );
        assert!(
            wait_eq_for(&eng, "redis", RtState::Exited, 6),
            "状态轮询应发现容器退出"
        );
        let snap = eng.snapshot().unwrap();
        let view = snap.services.get("redis").unwrap();
        assert_eq!(view.exit_reason.as_deref(), Some("crash"));
        assert_eq!(view.last_exit.as_ref().unwrap().code, 137);
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_cleanup_stops_only_engine_started() {
        let root = compose_ws(
            "  redis:\n    kind: compose\n    service: redis\n  mysql:\n    kind: compose\n    service: mysql\n",
            "",
        );
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        script_probe_ok(&fake);
        fake.push_ok(""); // up redis
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();
        eng.start_one("redis").unwrap();
        assert!(wait_eq(&eng, "redis", RtState::Running));
        eng.close().unwrap();
        let calls = fake.calls();
        assert!(
            calls
                .iter()
                .any(|c| c.args.ends_with(&["stop".to_string(), "redis".to_string()])),
            "引擎启动过的 redis 应被清场"
        );
        assert!(
            !calls
                .iter()
                .any(|c| c.args.ends_with(&["stop".to_string(), "mysql".to_string()])),
            "未由引擎启动的 mysql 不应被清场（§5.6）"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_precheck_error_mapping() {
        // a) PATH 无 docker → DOCKER_NOT_FOUND
        let root = compose_ws(redis_only_yaml(), "");
        let fake = Arc::new(FakeDockerRunner::new());
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap(); // open 校验：config 默认 ok → 解析失败静默跳过
        fake.push_err(std::io::ErrorKind::NotFound); // probe 时 spawn 失败
        let e = eng.start_one("redis").unwrap_err();
        assert_eq!(e.code(), ErrorCode::DockerNotFound);
        eng.close().unwrap();

        // b) Docker Desktop 已装未运行 → DOCKER_ENGINE_UNREACHABLE
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(r#"{"Client":{"Version":"27.1.1"},"Server":null}"#);
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();
        let e = eng.start_one("redis").unwrap_err();
        assert_eq!(e.code(), ErrorCode::DockerEngineUnreachable);
        eng.close().unwrap();

        // c) compose 文件缺失 → COMPOSE_FILE_MISSING（不 spawn config）
        let root2 =
            crate::sandbox::test_temp_dir().join(format!("st-eng-compmiss-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root2);
        fs::create_dir_all(&root2).unwrap();
        fs::write(
            root2.join("supertask.yaml"),
            "version: 1\nservices:\n  redis:\n    kind: compose\n    service: redis\ndocker:\n  compose_file: missing.yaml\n",
        )
        .unwrap();
        let fake = Arc::new(FakeDockerRunner::new());
        script_probe_ok(&fake);
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root2).unwrap();
        let e = eng.start_one("redis").unwrap_err();
        assert_eq!(e.code(), ErrorCode::ComposeFileMissing);
        assert!(
            !fake.calls().iter().any(|c| c
                .args
                .windows(2)
                .any(|w| w == ["config".to_string(), "--format".to_string()])),
            "文件缺失时不应 spawn config"
        );
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root2);

        // d) service 不在 compose 文件中 → COMPOSE_SERVICE_MISSING
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(r#"{"services":{"mysql":{"image":"mysql:8"}}}"#); // open 校验
        script_probe_ok(&fake);
        fake.push_ok(r#"{"services":{"mysql":{"image":"mysql:8"}}}"#); // start 校验
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();
        let e = eng.start_one("redis").unwrap_err();
        assert_eq!(e.code(), ErrorCode::ComposeServiceMissing);
        eng.close().unwrap();

        // e) 端口与其他服务重复 → PORT_DUP（用高位端口，避免真机 6379 被占用）
        let root3 = compose_ws(
            "  redis:\n    kind: compose\n    service: redis\n    port: 36379\n  mysql:\n    kind: compose\n    service: mysql\n    port: 36379\n",
            "",
        );
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        script_probe_ok(&fake);
        fake.push_ok(compose_config_json());
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root3).unwrap();
        let e = eng.start_one("redis").unwrap_err();
        assert_eq!(e.code(), ErrorCode::PortDup);
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&root3);
    }

    #[test]
    fn compose_open_warnings_service_missing_and_port_mismatch() {
        let root = compose_ws(
            "  redis:\n    kind: compose\n    service: redis\n    port: 7000\n  ghost:\n    kind: compose\n    service: nosuch\n",
            "",
        );
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        let eng = Engine::with_docker_runner(fake.clone());
        let (warnings, _) = eng.open(&root).unwrap();
        assert!(warnings
            .iter()
            .any(|w| w.code == ErrorCode::ComposeServiceMissing));
        assert!(warnings
            .iter()
            .any(|w| w.code == ErrorCode::ComposePortMismatch));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn docker_build_entry_and_unknown() {
        let root = compose_ws(
            redis_only_yaml(),
            "  builds:\n    - name: mall-user\n      context: .\n      tags:\n        - mall-user:local\n",
        );
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        fake.push_stream_ok("Step 1/2 : FROM scratch\nSuccessfully built abc\n");
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();

        // DOCKER_BUILD_UNKNOWN：name 不在 builds 列表
        let e = eng.docker_build("nope").unwrap_err();
        assert_eq!(e.code(), ErrorCode::DockerBuildUnknown);

        let op_id = eng.docker_build("mall-user").unwrap();
        let mut terminal = None;
        for _ in 0..200 {
            if let Some(ev) = eng.operations().get(&op_id) {
                if matches!(
                    ev.state,
                    crate::operation::OpState::Succeeded | crate::operation::OpState::Failed
                ) {
                    terminal = Some(ev);
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        let ev = terminal.expect("operation terminal");
        assert_eq!(ev.state, crate::operation::OpState::Succeeded);
        assert_eq!(ev.kind, "docker.build");
        assert!(ev
            .message
            .as_deref()
            .unwrap()
            .contains("Successfully built"));

        // build argv 顺序：build -t <tag> <context>
        let all_calls = fake.calls();
        let build = all_calls
            .iter()
            .find(|c| c.args.first().map(String::as_str) == Some("build"))
            .expect("build call");
        assert_eq!(build.args[0], "build");
        assert_eq!(
            &build.args[1..3],
            &["-t".to_string(), "mall-user:local".to_string()]
        );
        assert_eq!(build.args.len(), 4);
        assert_eq!(build.cwd.as_deref(), Some(root.as_path()));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn compose_build_via_operation_and_non_compose_rejected() {
        let root = compose_ws(
            "  redis:\n    kind: compose\n    service: redis\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n    health:\n      type: none\n",
            "",
        );
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        fake.push_stream_full(1, "Step 1\n", "ERROR: failed to solve\n");
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();

        // 非 compose 服务 → SPEC_INVALID
        let e = eng.build_compose("api").unwrap_err();
        assert_eq!(e.code(), ErrorCode::SpecInvalid);

        // compose 构建：compose --ansi never -f <file> build <service>
        let op_id = eng.build_compose("redis").unwrap();
        let mut terminal = None;
        for _ in 0..200 {
            if let Some(ev) = eng.operations().get(&op_id) {
                if matches!(
                    ev.state,
                    crate::operation::OpState::Succeeded | crate::operation::OpState::Failed
                ) {
                    terminal = Some(ev);
                    break;
                }
            }
            thread::sleep(Duration::from_millis(20));
        }
        let ev = terminal.expect("operation terminal");
        assert_eq!(ev.state, crate::operation::OpState::Failed);
        assert_eq!(ev.kind, "compose.build");
        assert_eq!(ev.error_code.as_deref(), Some("IMAGE_BUILD_FAILED"));
        assert!(ev.message.as_deref().unwrap().contains("failed to solve"));

        let all_calls = fake.calls();
        let build = all_calls
            .iter()
            .find(|c| c.args.contains(&"build".to_string()))
            .expect("compose build call");
        assert_eq!(
            &build.args[0..3],
            &[
                "compose".to_string(),
                "--ansi".to_string(),
                "never".to_string()
            ]
        );
        assert_eq!(
            &build.args[build.args.len() - 2..],
            &["build".to_string(), "redis".to_string()]
        );
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn docker_probe_cache_and_refresh() {
        let fake = Arc::new(FakeDockerRunner::new());
        script_probe_ok(&fake);
        script_probe_ok(&fake); // refresh 用
        let eng = Engine::with_docker_runner(fake.clone());
        let p1 = eng.docker_probe(false);
        assert!(p1.found && p1.running);
        assert_eq!(p1.compose_version.as_deref(), Some("2.29.1"));
        assert_eq!(fake.calls().len(), 2);
        let p2 = eng.docker_probe(false);
        assert_eq!(fake.calls().len(), 2, "会话内缓存不重复 spawn");
        assert_eq!(p1, p2);
        let _ = eng.docker_probe(true);
        assert_eq!(fake.calls().len(), 4, "refresh=true 强制重新探测");
    }

    #[test]
    fn toolchain_probe_cache_refresh_and_invalidate() {
        use std::sync::atomic::AtomicUsize;
        let eng = Engine::new();
        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        eng.set_toolchain_probe_fn_for_test(move || {
            c.fetch_add(1, Ordering::SeqCst);
            crate::probe::ToolchainProbeBundle {
                tools: crate::probe::ToolchainProbe::default(),
                managers: crate::toolchain::ManagerAvailability {
                    mise: false,
                    winget: true,
                },
            }
        });
        let p1 = eng.toolchain_probe(false);
        let _ = eng.toolchain_probe(false);
        assert_eq!(count.load(Ordering::SeqCst), 1, "TTL 窗口内命中缓存不重探");
        assert!(p1.managers.winget);
        let _ = eng.toolchain_probe(true);
        assert_eq!(count.load(Ordering::SeqCst), 2, "refresh=true 强制重探");
        eng.invalidate_toolchain_probe();
        let _ = eng.toolchain_probe(false);
        assert_eq!(
            count.load(Ordering::SeqCst),
            3,
            "install/upgrade 后失效重探"
        );
    }

    #[test]
    fn docker_ps_and_images() {
        let root = compose_ws(redis_only_yaml(), "");
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(compose_config_json());
        fake.push_ok(
            r#"[{"ID":"abc","Name":"ws-redis-1","Service":"redis","Image":"redis:7","State":"running","Publishers":[{"PublishedPort":6379}]}]"#,
        );
        // images NDJSON
        fake.push_ok(
            r#"{"ID":"sha256:aaa","RepoTags":["mall:local"],"Size":42,"CreatedAt":"2026-01-02T03:04:05Z"}"#,
        );
        let eng = Engine::with_docker_runner(fake.clone());
        eng.open(&root).unwrap();

        let containers = eng.docker_ps().unwrap();
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].service, "redis");
        assert_eq!(containers[0].state, "running");
        assert_eq!(containers[0].ports, vec![6379]);

        let images = eng.docker_images().unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].repository, "mall");
        assert_eq!(images[0].size_bytes, Some(42));
        assert!(images[0].created_ms.is_some());

        // daemon 不可达 → DOCKER_NOT_FOUND
        fake.push_err(std::io::ErrorKind::NotFound);
        let e = eng.docker_images().unwrap_err();
        assert_eq!(e.code(), ErrorCode::DockerNotFound);
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);

        // 无 compose 文件的工作区 → docker_ps 为空（不 spawn）
        let root2 = write_ws_yaml(ping_yaml());
        let eng2 = Engine::with_docker_runner(Arc::new(FakeDockerRunner::new()));
        eng2.open(&root2).unwrap();
        assert!(eng2.docker_ps().unwrap().is_empty());
        eng2.close().unwrap();
        let _ = fs::remove_dir_all(&root2);
    }

    #[test]
    fn ping_start_stop_and_logs() {
        let root = write_ws_yaml(ping_yaml());
        let eng = Engine::ping_for_test();
        eng.open(&root).unwrap();
        eng.subscribe_logs().unwrap();
        eng.start_one("ping").unwrap();
        assert!(
            wait_eq(&eng, "ping", RtState::Running),
            "{:?}",
            eng.state_of("ping")
        );
        let snap = eng.snapshot().unwrap();
        let view = snap.services.get("ping").expect("ping view");
        assert!(
            view.managed,
            "spawned service must be managed (not external)"
        );
        thread::sleep(Duration::from_millis(400));
        let (lines, _) = eng.logs_snapshot(None, 50).unwrap();
        assert!(!lines.is_empty(), "ping should emit stdout");
        let src = LogSource {
            kind: LogSourceKind::Service,
            id: "ping".into(),
        };
        eng.clear_logs(&src).unwrap();
        let (cleared, _) = eng.logs_snapshot(Some(&src), 50).unwrap();
        assert!(cleared.is_empty());
        let e = eng.start_one("ping").unwrap_err();
        assert_eq!(e.code(), ErrorCode::AlreadyInProgress);
        eng.stop_one("ping").unwrap();
        assert_eq!(eng.state_of("ping"), Some(RtState::Stopped));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn env_effective_snapshot_sources_and_lifecycle() {
        let root = write_ws_yaml(
            r#"
version: 1
env:
  WS_VAR: ws
services:
  ping:
    kind: spring-boot
    module: x
    port: 18081
    health:
      type: none
    grace_secs: 1
    env:
      SVC_VAR: svc
      WS_VAR: svc-overrides
"#,
        );
        let eng = Engine::ping_for_test();
        eng.open(&root).unwrap();

        // 未启动过 → 空快照（不报错，前端空态）
        let empty = eng.env_effective("ping").unwrap();
        assert!(empty.entries.is_empty() && empty.captured_at_ms.is_none());
        assert_eq!(
            eng.env_effective("nope").unwrap_err().code(),
            ErrorCode::NotFound
        );

        eng.start_one("ping").unwrap();
        assert!(
            wait_eq(&eng, "ping", RtState::Running),
            "{:?}",
            eng.state_of("ping")
        );
        let snap = eng.env_effective("ping").unwrap();
        assert!(snap.captured_at_ms.is_some(), "启动后应有采集时间");
        let src = |k: &str| {
            snap.entries
                .iter()
                .find(|e| e.key == k)
                .map(|e| (e.value.as_str(), e.source.as_str()))
        };
        // 端口自动注入（service env 未写 SERVER_PORT）
        assert_eq!(src("SERVER_PORT"), Some(("18081", "port")));
        // 服务 env 覆盖工作区 env：来源取最后写入层
        assert_eq!(src("SVC_VAR"), Some(("svc", "service")));
        assert_eq!(src("WS_VAR"), Some(("svc-overrides", "service")));

        eng.stop_one("ping").unwrap();
        // 停止后保留最后一次快照（排障用）
        let after = eng.env_effective("ping").unwrap();
        assert!(!after.entries.is_empty());
        // B：快照落盘 + 重开工作区可回看
        assert!(
            root.join(".supertask").join("env-snapshots.json").is_file(),
            "启动后应持久化快照"
        );
        eng.close().unwrap();
        let eng2 = Engine::ping_for_test();
        eng2.open(&root).unwrap();
        let restored = eng2.env_effective("ping").unwrap();
        assert!(!restored.entries.is_empty(), "重开工作区后快照应恢复");
        assert_eq!(restored.captured_at_ms, after.captured_at_ms);
        eng2.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn yaml_save_conflict_and_form() {
        let root = write_ws_yaml(ping_yaml());
        let eng = Engine::new();
        eng.open(&root).unwrap();
        let view = eng.yaml_get().unwrap();
        let e = eng.save_text(&view.text, "deadbeef").unwrap_err();
        assert_eq!(e.code(), ErrorCode::YamlConflict);
        let mut spec = view.spec.clone();
        spec.name = Some("from-form".into());
        let (_, hash, _) = eng.save_form(&spec, &view.hash).unwrap();
        assert_ne!(hash, view.hash);
        let again = eng.yaml_get().unwrap();
        assert_eq!(again.spec.name.as_deref(), Some("from-form"));
        let added = r#"
version: 1
name: from-form
services:
  ping:
    kind: spring-boot
    module: x
    port: 1
    health:
      type: none
  extra:
    kind: spring-boot
    module: y
    port: 2
    health:
      type: none
"#;
        eng.save_text(added, &again.hash).unwrap();
        assert!(eng.snapshot().unwrap().services.contains_key("extra"));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn open_close_acquires_and_releases_workspace_lock() {
        let root = write_ws_yaml(ping_yaml());
        let eng = Engine::new();
        eng.open(&root).unwrap();
        let info = crate::lock::query(&root).expect("lock exists after open");
        assert_eq!(info.pid, std::process::id());
        assert_eq!(info.holder, crate::lock::LockHolder::Desktop);
        eng.close().unwrap();
        assert!(
            crate::lock::query(&root).is_none(),
            "lock released on close"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn open_failed_workspace_releases_lock() {
        let root = write_ws_yaml("version: 1\nservices: {}\n");
        // services 为空等校验失败场景可能合法，改用不存在的 yaml 根目录
        let empty = crate::sandbox::test_temp_dir()
            .join(format!("st-eng-lock-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&empty);
        fs::create_dir_all(&empty).unwrap();
        let eng = Engine::new();
        assert!(eng.open(&empty).is_err());
        assert!(
            crate::lock::query(&empty).is_none(),
            "failed open must not leave a lock behind"
        );
        let _ = fs::remove_dir_all(&empty);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn holder_label_follows_engine_identity() {
        let root = write_ws_yaml(ping_yaml());
        let eng = Engine::with_holder(crate::lock::LockHolder::Cli);
        eng.open(&root).unwrap();
        let info = crate::lock::query(&root).expect("lock exists");
        assert_eq!(info.holder, crate::lock::LockHolder::Cli);
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn script_run_cancel_and_busy() {
        let root = write_ws_yaml(
            r#"
version: 1
services:
  ping:
    kind: spring-boot
    module: x
    port: 1
    health:
      type: none
scripts:
  hi:
    cmds: ["echo SUPER_TASK_OK"]
    timeout_secs: 15
  linger:
    cmds: ["ping -t 127.0.0.1"]
    timeout_secs: 30
"#,
        );
        let eng = Engine::new();
        eng.open(&root).unwrap();
        eng.subscribe_logs().unwrap();
        eng.run_script("hi").unwrap();
        let mut done = false;
        for _ in 0..80 {
            let st = eng.snapshot().unwrap().script.map(|s| s.state);
            if st == Some(ScriptState::Exited) {
                done = true;
                break;
            }
            thread::sleep(Duration::from_millis(40));
        }
        assert!(done, "script should exit");
        let src = LogSource {
            kind: LogSourceKind::Script,
            id: "hi".into(),
        };
        let (lines, _) = eng.logs_snapshot(Some(&src), 50).unwrap();
        assert!(
            lines.iter().any(|l| l.text.contains("SUPER_TASK_OK")),
            "{lines:?}"
        );
        eng.run_script("linger").unwrap();
        let e = eng.run_script("hi").unwrap_err();
        assert_eq!(e.code(), ErrorCode::ScriptBusy);
        eng.cancel_script().unwrap();
        let mut idle = false;
        for _ in 0..80 {
            let st = eng.snapshot().unwrap().script.map(|s| s.state);
            if st == Some(ScriptState::Exited) {
                idle = true;
                break;
            }
            thread::sleep(Duration::from_millis(40));
        }
        assert!(idle, "cancel should end linger");
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn start_all_and_dep_dead() {
        let root = write_ws_yaml(
            r#"
version: 1
services:
  a:
    kind: spring-boot
    module: x
    port: 1
    health:
      type: none
  b:
    kind: spring-boot
    module: y
    port: 2
    health:
      type: none
"#,
        );
        let eng = Engine::ping_for_test();
        eng.open(&root).unwrap();
        let order = eng.start_all().unwrap();
        assert_eq!(order, vec!["a", "b"]);
        assert!(wait_eq(&eng, "a", RtState::Running));
        assert!(wait_eq(&eng, "b", RtState::Running));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);

        let root = write_ws_yaml(
            r#"
version: 1
services:
  db:
    kind: spring-boot
    module: x
    port: 1
    health:
      type: tcp
    grace_secs: 1
  api:
    kind: spring-boot
    module: y
    port: 2
    health:
      type: none
    depends_on: [db]
"#,
        );
        let eng = Engine::fail_for_test();
        eng.open(&root).unwrap();
        let e = eng.start_one("api").unwrap_err();
        assert_eq!(e.code(), ErrorCode::DepDead);
        assert_ne!(eng.state_of("api"), Some(RtState::Running));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    // ---- 1.4 §5 Gradle 多模块 ----

    /// §5.3 gradle artifact：module/build/libs，排除 *-plain/-sources/-javadoc；
    /// 唯一直接用，零 ARTIFACT_MISSING，多 JAR_AMBIGUOUS，路径逃逸 PATH_ESCAPE。
    #[test]
    fn select_gradle_artifact_rules() {
        let root = crate::sandbox::test_temp_dir()
            .join(format!("st-eng-gradlejar-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let libs = root.join("mod/build/libs");
        fs::create_dir_all(&libs).unwrap();

        let only = libs.join("demo-api-1.0.0.jar");
        fs::write(&only, b"jar").unwrap();
        assert_eq!(select_gradle_artifact(&root, "mod").unwrap(), only);

        // 排除规则：plain / sources / javadoc / 非 jar 不参与
        fs::write(libs.join("demo-api-1.0.0-plain.jar"), b"x").unwrap();
        fs::write(libs.join("demo-api-1.0.0-sources.jar"), b"x").unwrap();
        fs::write(libs.join("demo-api-1.0.0-javadoc.jar"), b"x").unwrap();
        fs::write(libs.join("README.txt"), b"x").unwrap();
        assert_eq!(select_gradle_artifact(&root, "mod").unwrap(), only);

        // 只剩排除项 → 零候选
        fs::remove_file(&only).unwrap();
        assert_eq!(
            select_gradle_artifact(&root, "mod").unwrap_err().code(),
            ErrorCode::ArtifactMissing
        );

        // 多候选 → JAR_AMBIGUOUS（不按时间猜）
        fs::write(libs.join("demo-api-1.0.0.jar"), b"jar").unwrap();
        fs::write(libs.join("demo-web-2.0.0.jar"), b"jar").unwrap();
        assert_eq!(
            select_gradle_artifact(&root, "mod").unwrap_err().code(),
            ErrorCode::JarAmbiguous
        );

        // libs 目录不存在 → ARTIFACT_MISSING
        assert_eq!(
            select_gradle_artifact(&root, "nope").unwrap_err().code(),
            ErrorCode::ArtifactMissing
        );

        // 路径逃逸 → PATH_ESCAPE（复用 1.2 沙箱规则）
        assert_eq!(
            select_gradle_artifact(&root, "../esc").unwrap_err().code(),
            ErrorCode::PathEscape
        );
        let _ = fs::remove_dir_all(&root);
    }

    /// §5.1 打开时探测警告：并存 → BUILD_TOOL_AMBIGUOUS；显式 build_tool 跳过。
    #[test]
    fn open_warns_build_tool_ambiguous() {
        let root = write_ws_yaml(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: mod\n    port: 8080\n",
        );
        fs::create_dir_all(root.join("mod")).unwrap();
        fs::write(root.join("mod/pom.xml"), "<project/>").unwrap();
        fs::write(root.join("mod/build.gradle"), "").unwrap();
        let eng = Engine::new();
        let (warnings, _) = eng.open(&root).unwrap();
        assert!(
            warnings
                .iter()
                .any(|w| w.code == ErrorCode::BuildToolAmbiguous),
            "{warnings:?}"
        );
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);

        // 显式 build_tool 跳过探测：不产生警告
        let root2 = write_ws_yaml(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: mod\n    build_tool: gradle\n    port: 8080\n",
        );
        fs::create_dir_all(root2.join("mod")).unwrap();
        fs::write(root2.join("mod/pom.xml"), "<project/>").unwrap();
        fs::write(root2.join("mod/build.gradle"), "").unwrap();
        let eng2 = Engine::new();
        let (warnings2, _) = eng2.open(&root2).unwrap();
        assert!(
            !warnings2
                .iter()
                .any(|w| w.code == ErrorCode::BuildToolAmbiguous),
            "{warnings2:?}"
        );
        eng2.close().unwrap();
        let _ = fs::remove_dir_all(&root2);
    }

    // ---- 1.6 phase 4：网关托管（fake 校验桩 + TCP 监听桩，不拉真反代） ----

    use crate::gateway::validate::{ValidateOutcome, ValidateRunner};

    /// 校验桩：可配置退出码（0 = 校验通过；1 = 带 stderr 的校验失败）。
    struct FakeGatewayValidate {
        code: i32,
        stderr: &'static str,
    }

    impl FakeGatewayValidate {
        fn ok() -> Self {
            Self {
                code: 0,
                stderr: "",
            }
        }
        fn fail() -> Self {
            Self {
                code: 1,
                stderr: "[emerg] bind() to 127.0.0.1:PORT failed",
            }
        }
    }

    impl ValidateRunner for FakeGatewayValidate {
        fn run(
            &self,
            _program: &Path,
            _args: &[String],
            _timeout: Duration,
        ) -> Result<ValidateOutcome> {
            Ok(ValidateOutcome {
                code: self.code,
                stdout: String::new(),
                stderr: self.stderr.to_string(),
            })
        }
    }

    fn next_gateway_ports() -> (u16, u16) {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let base = 42000u32 + (std::process::id() % 300) * 70;
        let n = N.fetch_add(3, std::sync::atomic::Ordering::Relaxed);
        ((base + n) as u16, (base + n + 1) as u16)
    }

    /// 网关桩：cmd 包装的 PowerShell TCP 监听器（监听 gateway.port）；
    /// argv（-c/-p 等）全部忽略，模拟「反代进程在前台监听」。
    fn write_gateway_stub(dir: &Path, port: u16) -> PathBuf {
        let stub = dir.join("stub-gateway.cmd");
        let script = format!(
            "@echo off\r\npowershell -NoProfile -Command \"$ErrorActionPreference='SilentlyContinue';$l=[System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback,{port});$l.Start();while($true){{try{{$c=$l.AcceptTcpClient();$c.Close()}}catch{{break}}}}\"\r\n"
        );
        fs::write(&stub, script).unwrap();
        stub
    }

    fn gateway_ws_yaml(gw_port: u16, svc_port: u16, bin: &Path) -> String {
        format!(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: x\n    port: {svc_port}\n    health:\n      type: none\ngateway:\n  kind: nginx\n  port: {gw_port}\n  bin: {}\n  routes:\n    - path: /api\n      target: api\n    - path: /\n      target: api\n",
            bin.display()
        )
    }

    fn wait_gateway_state(eng: &Engine, want: &str, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if eng.gateway_status().unwrap().state.as_deref() == Some(want) {
                return true;
            }
            thread::sleep(Duration::from_millis(200));
        }
        false
    }

    #[test]
    fn gateway_status_unconfigured_and_preview_empty() {
        let root = write_ws_yaml(ping_yaml());
        let eng = Engine::new();
        eng.open(&root).unwrap();
        let st = eng.gateway_status().unwrap();
        assert!(!st.configured);
        assert!(st.kind.is_none() && st.routes.is_empty() && st.state.is_none());
        // 未配置时启动类命令 → GATEWAY_NOT_CONFIGURED
        let e = eng.gateway_start().unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayNotConfigured);
        // preview 无 kind → GATEWAY_NOT_CONFIGURED
        let e = eng.gateway_preview(None).unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayNotConfigured);
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gateway_preview_renders_in_memory() {
        let (gw, svc) = next_gateway_ports();
        let root =
            crate::sandbox::test_temp_dir().join(format!("st-gw-prev-{}-{gw}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("supertask.yaml"),
            format!(
                "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: x\n    port: {svc}\n    health:\n      type: none\ngateway:\n  kind: nginx\n  port: {gw}\n  routes:\n    - path: /api\n      target: api\n"
            ),
        )
        .unwrap();
        let eng = Engine::new();
        eng.open(&root).unwrap();
        let out = eng.gateway_preview(None).unwrap();
        assert_eq!(out.files.len(), 1);
        assert_eq!(out.files[0].name, "nginx.conf");
        assert!(
            out.files[0]
                .content
                .contains(&format!("proxy_pass http://127.0.0.1:{svc}")),
            "{}",
            out.files[0].content
        );
        // 纯内存：不落盘
        assert!(!root.join(".supertask/gateway/nginx.conf").exists());
        // status 路由解析 + 存活探测（服务未运行 → alive=false）
        let st = eng.gateway_status().unwrap();
        assert!(st.configured);
        assert_eq!(st.routes.len(), 1);
        assert_eq!(st.routes[0].target_port, Some(svc));
        assert_eq!(st.routes[0].upstream_alive, Some(false));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gateway_start_error_mapping() {
        let (gw, svc) = next_gateway_ports();
        let dir =
            crate::sandbox::test_temp_dir().join(format!("st-gw-err-{}-{gw}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // a) 校验失败 → GATEWAY_CONFIG_INVALID
        let stub = write_gateway_stub(&dir, gw);
        let root = write_ws_yaml(&gateway_ws_yaml(gw, svc, &stub));
        let eng = Engine::with_validator(Arc::new(FakeGatewayValidate::fail()));
        eng.open(&root).unwrap();
        let e = eng.gateway_start().unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayConfigInvalid);
        assert!(e.message().contains("[emerg]"), "{}", e);
        eng.close().unwrap();
        // b) 显式 bin 不存在 → GATEWAY_BINARY_MISSING（不回落 PATH）
        let root = write_ws_yaml(&gateway_ws_yaml(gw, svc, &dir.join("no-such-nginx.exe")));
        let eng = Engine::with_validator(Arc::new(FakeGatewayValidate::ok()));
        eng.open(&root).unwrap();
        let e = eng.gateway_start().unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayBinaryMissing);
        eng.close().unwrap();
        // c) enabled=false → GATEWAY_NOT_CONFIGURED（不启动）
        let stub2 = write_gateway_stub(&dir, gw);
        let base_yaml = gateway_ws_yaml(gw, svc, &stub2);
        let root = write_ws_yaml(&base_yaml.replacen(
            &format!("  port: {gw}\n"),
            &format!("  port: {gw}\n  enabled: false\n"),
            1,
        ));
        let eng = Engine::with_validator(Arc::new(FakeGatewayValidate::ok()));
        eng.open(&root).unwrap();
        let e = eng.gateway_start().unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayNotConfigured);
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gateway_full_lifecycle_with_stub() {
        let (gw, svc) = next_gateway_ports();
        let dir =
            crate::sandbox::test_temp_dir().join(format!("st-gw-life-{}-{gw}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let stub = write_gateway_stub(&dir, gw);
        let root = write_ws_yaml(&gateway_ws_yaml(gw, svc, &stub));
        let eng = Engine::with_validator(Arc::new(FakeGatewayValidate::ok()));
        eng.open(&root).unwrap();
        // 打开即有 slot（stopped），路由已解析
        let st = eng.gateway_status().unwrap();
        assert_eq!(st.state.as_deref(), Some("stopped"));
        assert_eq!(st.routes.len(), 2);
        assert!(st.routes.iter().all(|r| r.target_port == Some(svc)));
        // 启动 → 健康达标 Running（桩监听 TCP）
        eng.gateway_start().unwrap();
        assert!(
            wait_gateway_state(&eng, "running", 25),
            "{:?}",
            eng.gateway_status().unwrap()
        );
        // 上游存活仍 false（服务没跑），但网关端口在监听
        let st = eng.gateway_status().unwrap();
        assert!(st.pid.is_some());
        // 运行中重复启动 → AlreadyInProgress
        let e = eng.gateway_start().unwrap_err();
        assert_eq!(e.code(), ErrorCode::AlreadyInProgress);
        // 校验链产物落盘
        assert!(root.join(".supertask/gateway/nginx.conf").is_file());
        // 日志 source=gateway
        eng.subscribe_logs().unwrap();
        let src = LogSource {
            kind: LogSourceKind::Gateway,
            id: "gateway".into(),
        };
        eng.clear_logs(&src).unwrap();
        // 停止 → stopped，端口释放
        eng.gateway_stop().unwrap();
        assert_eq!(
            eng.gateway_status().unwrap().state.as_deref(),
            Some("stopped")
        );
        let mut released = false;
        for _ in 0..25 {
            if !crate::ports::is_serving(gw) {
                released = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        assert!(released, "网关端口应随进程树终止而释放");
        // 再启动 + stop_all 清场含网关
        eng.gateway_start().unwrap();
        assert!(wait_gateway_state(&eng, "running", 25));
        eng.stop_all().unwrap();
        assert_eq!(
            eng.gateway_status().unwrap().state.as_deref(),
            Some("stopped")
        );
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn gateway_apply_conflict_and_restart() {
        let (gw, svc) = next_gateway_ports();
        let dir = crate::sandbox::test_temp_dir()
            .join(format!("st-gw-apply-{}-{gw}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let stub = write_gateway_stub(&dir, gw);
        let root = write_ws_yaml(&gateway_ws_yaml(gw, svc, &stub));
        let eng = Engine::with_validator(Arc::new(FakeGatewayValidate::ok()));
        eng.open(&root).unwrap();
        eng.gateway_start().unwrap();
        assert!(wait_gateway_state(&eng, "running", 25));

        // a) base_hash 冲突 → YAML_CONFLICT（不写 yaml）
        let hash = eng.yaml_get().unwrap().hash;
        let mut new_conf = eng.spec().unwrap().gateway.clone().unwrap();
        new_conf.routes.push(crate::spec::GatewayRoute {
            host: Some("api.localhost".into()),
            path: "/".into(),
            target: Some("api".into()),
            upstream: None,
            strip_prefix: None,
            cors: None,
            redirect: None,
            redirect_status: None,
            static_dir: None,
            extra: Default::default(),
        });
        let e = eng.gateway_apply(new_conf.clone(), "deadbeef").unwrap_err();
        assert_eq!(e.code(), ErrorCode::YamlConflict);

        // b) 正常 apply：运行中 → 重启（stop→start），yaml 已更新
        let out = eng.gateway_apply(new_conf, &hash).unwrap();
        assert!(out.restarted);
        assert!(wait_gateway_state(&eng, "running", 25));
        let st = eng.gateway_status().unwrap();
        assert_eq!(st.routes.len(), 3, "apply 后路由来自新配置");
        let text = fs::read_to_string(root.join("supertask.yaml")).unwrap();
        assert!(text.contains("api.localhost"), "yaml 已写回");

        // c) 冲突语义由 save_form 承担：再次 apply 用旧 hash → YAML_CONFLICT
        let stale = "deadbeef".to_string();
        let conf2 = eng.spec().unwrap().gateway.clone().unwrap();
        let e = eng.gateway_apply(conf2, &stale).unwrap_err();
        assert_eq!(e.code(), ErrorCode::YamlConflict);
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&root);
    }

    // ---- 方向六：数据快照（ipc.md §10.18）----

    fn data_ws_yaml() -> &'static str {
        r#"
version: 1
services:
  db:
    kind: spring-boot
    module: db
    port: 2
    health:
      type: none
data:
  volumes:
    app-db:
      service: db
      dir: data/db
"#
    }

    /// 离线注入 Running 槽（真起服务不是离线测试的事）。
    fn running_slot_for_test(_id: &str, port: u16) -> Slot {
        Slot {
            state: RtState::Running,
            pid: None,
            port: Some(port),
            kind: "spring-boot".into(),
            job: None,
            stop_requested: false,
            started: None,
            started_at_ms: None,
            grace: Duration::from_secs(0),
            health: None,
            last_error: None,
            last_exit: None,
            cancel: Arc::new(AtomicBool::new(false)),
            managed: false,
            artifact: None,
            exit_reason: None,
            restart: crate::spec::RestartSpec::default(),
            restart_left: 0,
            restart_attempt: None,
            restart_cancel: Arc::new(AtomicBool::new(false)),
            restart_plan: None,
            compose: None,
            env_snapshot: None,
        }
    }

    /// 数据快照闭环：list → create → 预览（remove_count）→ restore → delete。
    #[test]
    fn data_snapshot_restore_round_trip_and_delete() {
        let root = write_ws_yaml(data_ws_yaml());
        fs::create_dir_all(root.join("data/db")).unwrap();
        fs::write(root.join("data/db/rows.txt"), "seed").unwrap();
        let eng = Engine::fail_for_test();
        eng.open(&root).unwrap();

        let list = eng.data_list().unwrap();
        assert_eq!(list.volumes.len(), 1);
        assert_eq!(list.volumes[0].id, "app-db");
        assert_eq!(list.volumes[0].service.as_deref(), Some("db"));
        assert_eq!(list.volumes[0].dir, "data/db");
        assert!(list.volumes[0].snapshots.is_empty());

        let created = eng.data_snapshot_create("app-db", "初次").unwrap();
        assert_eq!(created.snapshot.file_count, 1);
        assert_eq!(created.snapshot.note, "初次");

        // 修改数据后预览：stray 不在快照内，恢复将被删除
        fs::write(root.join("data/db/rows.txt"), "dirty").unwrap();
        fs::write(root.join("data/db/stray.log"), "junk").unwrap();
        let pv = eng
            .data_restore_preview("app-db", &created.snapshot.id)
            .unwrap();
        assert!(pv.ready);
        assert_eq!(pv.remove_count, 1);
        assert_eq!(pv.remove_sample, vec!["stray.log".to_string()]);

        let out = eng.data_restore("app-db", &created.snapshot.id).unwrap();
        assert_eq!(out.restored_files, 1);
        assert_eq!(out.removed_files, 1);
        assert_eq!(
            fs::read_to_string(root.join("data/db/rows.txt")).unwrap(),
            "seed"
        );
        assert!(!root.join("data/db/stray.log").exists());

        eng.data_snapshot_delete("app-db", &created.snapshot.id)
            .unwrap();
        assert!(eng.data_list().unwrap().volumes[0].snapshots.is_empty());
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    /// 离线守护：绑定服务运行中 → create/restore 报 SNAPSHOT_BUSY；预览不受限但 ready=false。
    #[test]
    fn data_snapshot_restore_blocked_while_bound_service_running() {
        let root = write_ws_yaml(data_ws_yaml());
        fs::create_dir_all(root.join("data/db")).unwrap();
        fs::write(root.join("data/db/rows.txt"), "seed").unwrap();
        let eng = Engine::fail_for_test();
        eng.open(&root).unwrap();
        let created = eng.data_snapshot_create("app-db", "").unwrap();

        {
            let mut g = eng.inner.lock().expect("engine lock");
            g.slots.insert("db".into(), running_slot_for_test("db", 2));
        }
        let e = eng.data_snapshot_create("app-db", "").unwrap_err();
        assert_eq!(e.code(), ErrorCode::SnapshotBusy);
        let e = eng
            .data_restore("app-db", &created.snapshot.id)
            .unwrap_err();
        assert_eq!(e.code(), ErrorCode::SnapshotBusy);
        let pv = eng
            .data_restore_preview("app-db", &created.snapshot.id)
            .unwrap();
        assert!(!pv.ready);
        assert!(pv.blockers.iter().any(|b| b.contains("db")));
        eng.close().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    /// 诊断面：卷不存在 → NOT_FOUND；快照 id 非法 → SNAPSHOT_INVALID；
    /// 合法 id 但文件缺失 → SNAPSHOT_NOT_FOUND；未打开工作区 → 报错。
    #[test]
    fn data_volume_and_snapshot_id_errors() {
        let root = write_ws_yaml(data_ws_yaml());
        let eng = Engine::fail_for_test();
        eng.open(&root).unwrap();
        assert_eq!(
            eng.data_snapshot_create("nope", "").unwrap_err().code(),
            ErrorCode::NotFound
        );
        assert_eq!(
            eng.data_snapshot_delete("app-db", "../x")
                .unwrap_err()
                .code(),
            ErrorCode::SnapshotInvalid
        );
        assert_eq!(
            eng.data_restore("app-db", "123").unwrap_err().code(),
            ErrorCode::SnapshotNotFound
        );
        eng.close().unwrap();
        let eng2 = Engine::fail_for_test();
        assert!(eng2.data_list().is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
