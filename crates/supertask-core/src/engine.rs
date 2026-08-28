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
use crate::launcher::{log_file_rel, plan_service, CommandSpec};
use crate::log::{LogBatcher, LogFile, LogHub, LogLine};
use crate::probe;
use crate::runtime::{apply, RtEvent, RtState};
use crate::sandbox;
use crate::spec::{parse_yaml, spec_hash, to_yaml, HealthType, ParseWarning, SuperTaskFile};

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
    Logs { workspace_id: String, items: Vec<LogLine> },
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

struct Slot {
    state: RtState,
    pid: Option<u32>,
    port: Option<u16>,
    kind: String,
    /// Arc：健康线程需要跨锁读取 Job 的进程树做端点发现
    job: Option<Arc<crate::job::Job>>,
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
}

struct ScriptSlot {
    id: String,
    state: ScriptState,
    pid: Option<u32>,
    job: Option<crate::job::Job>,
    cancel: Arc<AtomicBool>,
    last_exit: Option<ExitView>,
    last_error: Option<String>,
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
    job: Arc<crate::job::Job>,
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
    guard.get_or_insert_with(HashMap::new).insert(root_norm, slots);
}

pub struct Engine {
    inner: Arc<Mutex<Inner>>,
    events_rx: Mutex<Receiver<EngineEvent>>,
    spawner: SpawnerKind,
}

impl Engine {
    pub fn new() -> Self {
        Self::create(SpawnerKind::Real)
    }

    #[cfg(test)]
    pub fn ping_for_test() -> Self {
        Self::create(SpawnerKind::Ping)
    }

    #[cfg(test)]
    pub fn fail_for_test() -> Self {
        Self::create(SpawnerKind::Fail)
    }

    fn create(spawner: SpawnerKind) -> Self {
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
        }
    }

    pub fn open(&self, path: &Path) -> Result<(Vec<ParseWarning>, RuntimeSnapshot)> {
        let root = sandbox::strip_verbatim(fs::canonicalize(path).map_err(|e| {
            Error::new(ErrorCode::CwdMissing, format!("无法打开目录: {e}"))
        })?);
        let (yaml_path, text, file, mut warnings) = load_yaml_at(&root)?;
        let workspace_id = root.to_string_lossy().into_owned();
        let mut slots = HashMap::new();
        let _ = crate::log::run_retention(&root, file.log_retention.as_ref());
        let mut files = HashMap::new();
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
            // 端口已被监听 → 外部已在运行（用户手动起的或第三方工具），识别为非托管服务
            let (state, managed) = match svc.port {
                Some(p) if port_is_serving(p) => {
                    warnings.push(ParseWarning {
                        code: ErrorCode::PortDup,
                        message: format!("{id}: 端口 {p} 已被占用，按外部已运行服务显示（仅监控）"),
                    });
                    (RtState::Running, false)
                }
                _ => (RtState::Stopped, true),
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
                    last_error: None,
                    last_exit: None,
                    cancel: Arc::new(AtomicBool::new(false)),
                    managed,
                    artifact: None,
                    exit_reason: None,
                },
            );
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
    fn adopt_detached(&self, root: &Path, workspace_id: &str) {        let Some(detached) = detached_take(&norm_root(root)) else {
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

    /// Write a fresh `supertask.yaml` from a spec, then open the workspace.
    /// Used by the scan wizard: confirm a draft → persist → open in one step.
    pub fn init(&self, path: &Path, mut file: SuperTaskFile) -> Result<(Vec<ParseWarning>, RuntimeSnapshot)> {
        let root = sandbox::strip_verbatim(fs::canonicalize(path).map_err(|e| {
            Error::new(ErrorCode::CwdMissing, format!("无法打开目录: {e}"))
        })?);
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
        fs::write(&yaml_path, text).map_err(|e| {
            Error::new(ErrorCode::YamlParse, format!("写 YAML 失败: {e}"))
        })?;
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
        let _ = self.cancel_script();
        self.wait_script_idle(Duration::from_secs(8));
        let mut g = self.inner.lock().expect("engine lock");
        g.workspace_id.clear();
        g.slots.clear();
        g.files.clear();
        g.script = None;
        g.script_file = None;
        g.yaml_path = PathBuf::new();
        g.root = PathBuf::new();
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
        if !detached.is_empty() {
            let root_norm = norm_root(&root_norm_path);
            detached_put(root_norm, detached);
        }
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

    pub fn save_text(&self, text: &str, base_hash: &str) -> Result<(SuperTaskFile, String, Vec<ParseWarning>)> {
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
        fs::write(&g.yaml_path, text).map_err(|e| {
            Error::new(ErrorCode::YamlParse, format!("写入 YAML 失败: {e}"))
        })?;
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

    pub fn logs_snapshot(&self, source: Option<&LogSource>, limit: usize) -> Result<(Vec<LogLine>, u64)> {
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
                RtState::Starting | RtState::Running | RtState::Unhealthy | RtState::Stopping | RtState::Building
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
        Ok(order)
    }

    pub fn stop_one(&self, id: &str) -> Result<()> {
        {
            let mut g = self.inner.lock().expect("engine lock");
            let slot = g
                .slots
                .get_mut(id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
            if matches!(slot.state, RtState::Stopped) {
                return Ok(());
            }
            // 外部进程：无 Job 可 terminate，按端口找到 pid 后 taskkill 整棵树
            if !slot.managed {
                let port = slot.port;
                slot.stop_requested = true;
                slot.cancel.store(true, Ordering::SeqCst);
                slot.state = RtState::Stopped;
                emit_runtime(&g);
                drop(g);
                match port.and_then(crate::discover::port_to_pid) {
                    Some(pid) => kill_foreign_by_pid(pid)?,
                    None => {} // 端口已无人监听：按已停止处理
                }
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
        self.wait_state(id, &[RtState::Stopped, RtState::Exited], Duration::from_secs(8))
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
        Ok(())
    }

    pub fn restart_one(&self, id: &str) -> Result<()> {
        self.stop_one(id)?;
        self.start_one(id)
    }

    pub fn run_script(&self, id: &str) -> Result<()> {
        let (cmds, cwd, env, timeout) = {
            let mut g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            if g.script.as_ref().is_some_and(|s| s.state == ScriptState::Running) {
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
            let job = crate::job::Job::create()?;
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
            (cmds, cwd, env, timeout)
        };
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
        let (run_spec, build_spec, root, health_spec, health_none, port, kind, pkg, svc_grace, is_jar) = {
            let g = self.inner.lock().expect("engine lock");
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
            let is_jar = eff_svc.kind == "spring-boot" && eff_svc.launch.as_deref() == Some("jar");
            // §6.3 环境链：ws+profile < secrets/env_file < 服务+profile env < 端口注入
            let env = build_service_env(&eff_spec, id, &g.root)?;
            let (planned, build_spec) = if is_jar {
                let build = crate::launcher::plan_jar_build(&eff_svc, env.clone())?;
                let run = crate::launcher::plan_jar_run(&eff_svc, env);
                (run, Some(build))
            } else {
                let mut planned = plan_service(&eff_spec, id)?;
                planned.env = env;
                (planned, None)
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
            });
            (
                planned,
                build_spec,
                g.root.clone(),
                health_spec,
                health_none,
                eff_svc.port,
                eff_svc.kind.clone(),
                pkg,
                eff_svc.grace_secs.unwrap_or(0) as u64,
                is_jar,
            )
        };

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
                        spawner,
                    );
                    if let Err(e) = r {
                        jar_flow_fail(&inner, &id2, e);
                    }
                });
            return Ok(());
        }

        if matches!(self.spawner, SpawnerKind::Real) {
            probe::require_tools_for_kind(&kind, pkg)?;
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
            self.spawner,
        )
    }

    // -------------------------------------------------------------------
    // 1.2 phase 3–6：端口 / secrets / 日志 / 指标 / profile / build
    // -------------------------------------------------------------------

    /// §5.1 端口检查：本机 TCP 监听表 + 引擎托管 PID 对照。
    pub fn ports_inspect(&self) -> Result<Vec<crate::ipc::PortInspection>> {
        let (spec, managed) = {
            let g = self.inner.lock().expect("engine lock");
            require_ws(&g)?;
            let managed: std::collections::HashSet<u32> = g
                .slots
                .values()
                .filter(|s| s.managed)
                .filter_map(|s| s.pid)
                .collect();
            (g.spec.clone(), managed)
        };
        let listeners = crate::ports::tcp_listeners()?;
        Ok(crate::ports::inspect(&spec, &listeners, &managed))
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
            return Ok(PortsAssignView { spec, hash, notes, restart_required: false });
        }
        let (spec, hash, _) = saved?;
        Ok(PortsAssignView { spec, hash, notes, restart_required: false })
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
        Ok(crate::ipc::MetricsSnapshotOutput { services: g.metrics.clone() })
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
        let (build_spec, root) = {
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
                return Err(Error::new(ErrorCode::BuildBusy, format!("{id} 运行中，停止后再构建")));
            }
            let eff_spec = crate::profiles::overlay_spec(&g.spec, id)?;
            let eff_svc = eff_spec.services.get(id).unwrap().clone();
            if !(eff_svc.kind == "spring-boot" && eff_svc.launch.as_deref() == Some("jar")) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id} 不是 launch: jar 的 spring-boot 服务"),
                ));
            }
            let env = build_service_env(&eff_spec, id, &g.root)?;
            (crate::launcher::plan_jar_build(&eff_svc, env)?, g.root.clone())
        };
        jar_build_phase(Arc::clone(&self.inner), id, build_spec, &root)
    }

    /// 工作区引用（spec 快照 + root），供 secrets/日志等文件型操作使用。
    fn ws_ref(&self) -> Result<(SuperTaskFile, PathBuf)> {
        let g = self.inner.lock().expect("engine lock");
        require_ws(&g)?;
        Ok((g.spec.clone(), g.root.clone()))
    }

    fn state_of(&self, id: &str) -> Option<RtState> {
        self.inner
            .lock()
            .ok()?
            .slots
            .get(id)
            .map(|s| s.state)
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
            &[RtState::Running, RtState::Unhealthy, RtState::Exited, RtState::Stopped],
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
                return Err(Error::new(
                    ErrorCode::JobKill,
                    format!("{id} 等待状态超时"),
                ));
            }
            thread::sleep(Duration::from_millis(40));
        }
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
                log_seq: g.logs.next_seq().saturating_sub(1),
                managed: slot.managed,
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
    }
}

fn emit_runtime(g: &Inner) {
    let snap = build_snapshot(g);
    let _ = g.events.try_send(EngineEvent::Runtime(snap));
}

/// loopback:port 是否已有服务在监听（250ms 上限，open 时批量调用要快）。
/// 双栈：Node/Vite 默认常只监听 [::1]，仅探 IPv4 会把外部运行的服务误判为未启动。
fn port_is_serving(port: u16) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    [IpAddr::V4(Ipv4Addr::LOCALHOST), IpAddr::V6(Ipv6Addr::LOCALHOST)]
        .iter()
        .any(|ip| {
            TcpStream::connect_timeout(&SocketAddr::new(*ip, port), Duration::from_millis(250))
                .is_ok()
        })
}

/// 按 LISTEN 端口找到外部进程 pid 并 `taskkill /T /F`（等效杀整棵树）。
fn kill_foreign_by_pid(pid: u32) -> Result<()> {
    crate::discover::taskkill_tree(pid)
}

fn spawn_real(planned: &CommandSpec, cwd: &Path) -> Result<(Child, crate::job::Job)> {
    let program = probe::resolve_program(&planned.program)?;
    let mut cmd = Command::new(&program);
    cmd.args(&planned.args)
        .current_dir(cwd)
        .envs(planned.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::job::Job::create()?;
    let child = crate::job::spawn_in_job(&mut cmd, &job)?;
    Ok((child, job))
}

#[cfg(test)]
fn spawn_ping() -> Result<(Child, crate::job::Job)> {
    let mut cmd = Command::new("ping");
    cmd.args(["-t", "127.0.0.1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::job::Job::create()?;
    let child = crate::job::spawn_in_job(&mut cmd, &job)?;
    Ok((child, job))
}

#[cfg(test)]
fn spawn_fail() -> Result<(Child, crate::job::Job)> {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "exit 1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let job = crate::job::Job::create()?;
    let child = crate::job::spawn_in_job(&mut cmd, &job)?;
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
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", line])
            .current_dir(&cwd)
            .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let spawned = {
            let g = inner.lock().expect("engine lock");
            let Some(slot) = g.script.as_ref() else { break };
            let Some(job) = slot.job.as_ref() else { break };
            crate::job::spawn_in_job(&mut cmd, job)
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
            spawn_pump(Arc::clone(&inner), src.clone(), LogStream::Stderr, errp, cancel);
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
fn build_service_env(
    eff_spec: &SuperTaskFile,
    id: &str,
    root: &Path,
) -> Result<IndexMap<String, String>> {
    let svc = eff_spec
        .services
        .get(id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
    let (file_env, _warnings) = crate::secrets::load_file_layers(eff_spec, root, Some(id))?;
    let mut env = eff_spec.env.clone();
    for (k, v) in &file_env {
        env.insert(k.clone(), v.clone());
    }
    for (k, v) in &svc.env {
        env.insert(k.clone(), v.clone());
    }
    if let Some(p) = svc.port {
        if let Some(key) = crate::ports::port_env_key(&svc.kind) {
            env.entry(key.to_string()).or_insert_with(|| p.to_string());
        }
    }
    Ok(env)
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
    spawner: SpawnerKind,
) -> Result<()> {
    if matches!(spawner, SpawnerKind::Real) {
        probe::require_tools_for_kind(&kind, pkg)?;
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
        slot.job = Some(Arc::new(job));
        slot.pid = Some(pid);
        slot.started = Some(Instant::now());
        slot.started_at_ms = Some(now_ms());
        slot.last_error = None;
        slot.exit_reason = None;
        match apply(slot.state, RtEvent::Spawned { health_none }) {
            Ok(s) => slot.state = s,
            Err(e) => {
                slot.last_error = Some(e.to_string());
            }
        }
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

/// 1.2 §11 launch: jar 编排：构建（若无 artifact）→ java -jar。
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
    spawner: SpawnerKind,
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
            None => jar_build_phase(inner.clone(), id, build_spec, &root)?,
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
        spawner,
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

/// package 阶段：Building 状态 + 输出进服务日志；成功解析 artifact。
fn jar_build_phase(
    inner: Arc<Mutex<Inner>>,
    id: &str,
    build_spec: CommandSpec,
    root: &Path,
) -> Result<PathBuf> {
    let (module, cancel) = {
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
        (module, slot.cancel.clone())
    };
    {
        let mut g = inner.lock().expect("engine lock");
        let slot = g.slots.get_mut(id).unwrap();
        slot.state = apply(slot.state, RtEvent::BuildStarted)?;
        slot.last_error = None;
        slot.exit_reason = None;
        emit_runtime(&g);
    }
    let cwd = resolve_cwd(root, &build_spec.cwd_rel)?;
    let (mut child, job) = spawn_real(&build_spec, &cwd)?;
    let job = Arc::new(job);
    {
        let mut g = inner.lock().expect("engine lock");
        if let Some(slot) = g.slots.get_mut(id) {
            slot.job = Some(Arc::clone(&job));
            slot.pid = Some(child.id());
        }
        emit_runtime(&g);
    }
    let src = LogSource {
        kind: LogSourceKind::Service,
        id: id.to_string(),
    };
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
                            if let Ok(s) = apply(slot.state, RtEvent::ProcessExited { stop_requested: true }) {
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
                    jar_build_fail(&inner, id, Some("package 超时（20 分钟）".to_string()));
                    return Err(Error::new(ErrorCode::BuildFailed, "package 超时（20 分钟）"));
                }
                thread::sleep(Duration::from_millis(60));
            }
        }
    };
    let code = status.code().unwrap_or(-1);
    if code != 0 {
        jar_build_fail(&inner, id, Some(format!("mvn package 退出码 {code}")));
        return Err(Error::new(
            ErrorCode::BuildFailed,
            format!("mvn package 退出码 {code}：已保留 package 日志，服务未启动"),
        ));
    }
    let artifact = select_jar_artifact(root, &module)?;
    {
        let mut g = inner.lock().expect("engine lock");
        if let Some(slot) = g.slots.get_mut(id) {
            slot.state = apply(slot.state, RtEvent::BuildFinished { ok: true })?;
            slot.artifact = Some(artifact.clone());
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
            slot.last_error = Some(
                detail
                    .unwrap_or_else(|| "BUILD_FAILED: package 失败，服务未启动".to_string()),
            );
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
        if name.starts_with("original-") || name.ends_with("-sources.jar") || name.ends_with("-javadoc.jar") {
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
        return Err(Error::new(ErrorCode::ArtifactMissing, "target 中没有可执行 jar"));
    }
    let names = names_of(&jars);
    Err(Error::new(
        ErrorCode::JarAmbiguous,
        format!("多个候选 jar 且 pom 未提供 artifactId: {}", names.join(", ")),
    ))
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
                    .try_send(EngineEvent::Metrics(crate::ipc::MetricsPayload { services: map }));
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
            let err_msg = if !g
                .slots
                .get(&id)
                .map(|s| s.stop_requested)
                .unwrap_or(true)
                && code != 0
            {
                Some(exit_error_from_logs(&g, &src, code))
            } else {
                None
            };
            let Some(slot) = g.slots.get_mut(&id) else { return };
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
            slot.exit_reason = if slot.stop_requested { None } else { Some("crash") };
            let ev = RtEvent::ProcessExited {
                stop_requested: slot.stop_requested,
            };
            if let Ok(next) = apply(slot.state, ev) {
                slot.state = next;
            }
            emit_runtime(&g);
        })
        .ok();
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
                let Some(slot) = g.slots.get_mut(&id) else { break };
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

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
        let root = std::env::temp_dir().join(format!("st-eng-{}-{n}", std::process::id()));
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

    #[test]
    fn ping_start_stop_and_logs() {
        let root = write_ws_yaml(ping_yaml());
        let eng = Engine::ping_for_test();
        eng.open(&root).unwrap();
        eng.subscribe_logs().unwrap();
        eng.start_one("ping").unwrap();
        assert!(wait_eq(&eng, "ping", RtState::Running), "{:?}", eng.state_of("ping"));
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
}
