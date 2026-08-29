//! 可变命令（1.5 §4.1/§4.2）：up / down / restart / script。首个动作取锁（holder=cli）。

use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use supertask_core::lock::LockHolder;
use supertask_core::runtime::RtState;
use supertask_core::{Engine, Error, ErrorCode};

use crate::cli::Wait;
use crate::output;

/// 信号状态：首信号触发优雅清场，第二信号不再等待直接强杀（§4.2.5）。
static SIGNALS: AtomicUsize = AtomicUsize::new(0);
static STOP: AtomicBool = AtomicBool::new(false);
static SIGNAL_ENGINE: Mutex<Option<&'static Engine>> = Mutex::new(None);

fn install_signal_handler() {
    let _ = ctrlc::set_handler(|| {
        let n = SIGNALS.fetch_add(1, Ordering::SeqCst) + 1;
        STOP.store(true, Ordering::SeqCst);
        if n >= 2 {
            // 第二信号：同步停整棵树后立即退出；Windows Job kill-on-close、
            // Linux PDEATHSIG 兜底，macOS 崩溃残留为规格明示局限。
            if let Ok(g) = SIGNAL_ENGINE.lock() {
                if let Some(e) = g.as_ref() {
                    let _ = e.stop_all();
                }
            }
            std::process::exit(130);
        }
    });
}

/// 单服务观察值：wait 循环与失败报告共用（测试用闭包注入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvcObs {
    pub id: String,
    pub state: RtState,
    /// health 检查结果（type none 或未检查时为 None）
    pub health_ok: Option<bool>,
    pub detail: Option<String>,
}

pub fn observe(engine: &Engine, ids: &[String]) -> Vec<SvcObs> {
    let Ok(snap) = engine.snapshot() else {
        return ids
            .iter()
            .map(|id| SvcObs { id: id.clone(), state: RtState::Stopped, health_ok: None, detail: None })
            .collect();
    };
    ids.iter()
        .map(|id| match snap.services.get(id) {
            Some(s) => SvcObs {
                id: id.clone(),
                state: s.state,
                health_ok: s.health.as_ref().map(|h| h.ok),
                detail: s
                    .last_error
                    .clone()
                    .or_else(|| s.health.as_ref().filter(|h| !h.ok).map(|h| h.detail.clone())),
            },
            None => SvcObs { id: id.clone(), state: RtState::Stopped, health_ok: None, detail: Some("服务不存在".into()) },
        })
        .collect()
}

fn reached(mode: Wait, o: &SvcObs) -> bool {
    match mode {
        Wait::Never => true,
        Wait::Started => o.state == RtState::Running,
        Wait::Healthy => o.state == RtState::Running && o.health_ok.unwrap_or(true),
    }
}

/// 等待循环（§4.2.4）。失败/超时返回未达标清单；SIG 视为用户取消由调用方处理。
pub enum WaitOutcome {
    Reached,
    Failed(Vec<SvcObs>),
    Timeout(Vec<SvcObs>),
}

pub fn wait_until<F>(targets: &[String], mode: Wait, timeout: Duration, mut obs: F) -> WaitOutcome
where
    F: FnMut() -> Vec<SvcObs>,
{
    if mode == Wait::Never || targets.is_empty() {
        return WaitOutcome::Reached;
    }
    let deadline = Instant::now() + timeout;
    loop {
        if STOP.load(Ordering::SeqCst) {
            return WaitOutcome::Timeout(targets.iter().map(|id| SvcObs { id: id.clone(), state: RtState::Starting, health_ok: None, detail: Some("被信号中断".into()) }).collect());
        }
        let list = obs();
        let bad: Vec<SvcObs> = list
            .iter()
            .filter(|o| matches!(o.state, RtState::Exited | RtState::Unhealthy))
            .cloned()
            .collect();
        if !bad.is_empty() {
            return WaitOutcome::Failed(bad);
        }
        if list.iter().all(|o| reached(mode, o)) {
            return WaitOutcome::Reached;
        }
        if Instant::now() >= deadline {
            return WaitOutcome::Timeout(
                list.into_iter().filter(|o| !reached(mode, o)).collect(),
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn report_pending(kind: &str, list: &[SvcObs]) {
    eprintln!("以下服务未达标（{kind}）：");
    for o in list {
        eprintln!(
            "  {}  state={:?}  {}",
            o.id,
            o.state,
            o.detail.as_deref().unwrap_or("-")
        );
    }
}

/// 打开工作区（holder=cli，取锁；WORKSPACE_LOCKED 原样传播）。
fn open_engine(root: &Path) -> Result<Engine, Error> {
    let engine = Engine::with_holder(LockHolder::Cli);
    let (warnings, _) = engine.open(root)?;
    for w in warnings {
        eprintln!("[警告 {:?}] {}", w.code, w.message);
    }
    Ok(engine)
}

/// 拓扑目标清单：显式 ids 优先；缺省全量 start_order 且跳过 profile 禁用服务。
fn targets_of(engine: &Engine, ids: &[String]) -> Result<Vec<String>, Error> {
    if !ids.is_empty() {
        return Ok(ids.to_vec());
    }
    let spec = engine.spec()?;
    let order = supertask_core::graph::start_order(&spec)?;
    Ok(order
        .into_iter()
        .filter(|id| {
            supertask_core::profiles::effective_service(&spec, id)
                .map(|s| s.enabled)
                .unwrap_or(true)
        })
        .collect())
}

/// 已注册信号引擎（第二信号强杀路径用）。
fn register_for_signals(engine: &'static Engine) {
    if let Ok(mut g) = SIGNAL_ENGINE.lock() {
        *g = Some(engine);
    }
}

pub fn run_up(
    root: &Path,
    ids: &[String],
    wait: Wait,
    wait_timeout_secs: u64,
    wrapper: &[std::ffi::OsString],
) -> Result<i32, Error> {
    install_signal_handler();
    let engine = Box::leak(Box::new(open_engine(root)?));
    register_for_signals(engine);

    let targets = targets_of(engine, ids)?;
    for id in &targets {
        if let Err(e) = engine.start_one(id) {
            if e.code() == ErrorCode::AlreadyInProgress {
                continue; // up 幂等：已在运行/构建的服务跳过
            }
            let _ = engine.stop_all();
            let _ = engine.close();
            return Err(e);
        }
    }

    match wait_until(&targets, wait, Duration::from_secs(wait_timeout_secs), || observe(engine, &targets)) {
        WaitOutcome::Reached => {}
        WaitOutcome::Failed(bad) => {
            let _ = engine.stop_all();
            let _ = engine.close();
            report_pending("启动失败", &bad);
            return Err(Error::new(
                ErrorCode::HealthTimeout,
                "服务启动失败，已停止全部服务",
            ));
        }
        WaitOutcome::Timeout(pending) => {
            let _ = engine.stop_all();
            let _ = engine.close();
            report_pending("健康等待超时", &pending);
            return Err(Error::new(
                ErrorCode::HealthTimeout,
                format!(
                    "健康等待超时（{}s），已停止全部服务",
                    wait_timeout_secs
                ),
            ));
        }
    }

    if !wrapper.is_empty() {
        // §4.2.5 包装形态：健康达标后 spawn 子命令（继承 stdio），退出码透传
        let (prog, args) = wrapper.split_first().expect("wrapper non-empty");
        let status = std::process::Command::new(prog).args(args).status();
        let _ = engine.stop_all();
        let _ = engine.close();
        return Ok(match status {
            Ok(s) => s.code().unwrap_or(1),
            Err(e) => {
                eprintln!("包装命令无法启动: {e}");
                1
            }
        });
    }

    // 交互附加形态：聚合输出各服务日志，行前缀 [<service>]
    let _ = engine.subscribe_logs();
    loop {
        match engine.recv_event_timeout(Duration::from_millis(150)) {
            Some(supertask_core::EngineEvent::Logs { items, .. }) => {
                for line in items {
                    let text = &line.text;
                    match line.stream {
                        supertask_core::ipc::LogStream::Stderr => {
                            eprintln!("[{}] {text}", line.source.id);
                        }
                        _ => println!("[{}] {text}", line.source.id),
                    }
                }
            }
            Some(_) => {}
            None => {}
        }
        if STOP.load(Ordering::SeqCst) {
            // 排空一轮日志后清场
            while let Some(supertask_core::EngineEvent::Logs { items, .. }) =
                engine.recv_event_timeout(Duration::from_millis(50))
            {
                for line in items {
                    println!("[{}] {}", line.source.id, line.text);
                }
            }
            let _ = engine.stop_all();
            let _ = engine.close();
            return Ok(output::EXIT_OK);
        }
    }
}

pub fn run_down(root: &Path, ids: &[String]) -> Result<i32, Error> {
    let engine = open_engine(root)?;
    if ids.is_empty() {
        engine.stop_all()?;
    } else {
        for id in ids {
            engine.stop_one(id)?;
        }
    }
    let _ = engine.close();
    println!("已停止。");
    Ok(output::EXIT_OK)
}

pub fn run_restart(root: &Path, ids: &[String]) -> Result<i32, Error> {
    let engine = open_engine(root)?;
    let targets = targets_of(&engine, ids)?;
    for id in &targets {
        if let Err(e) = engine.restart_one(id) {
            if e.code() == ErrorCode::AlreadyInProgress {
                continue;
            }
            let _ = engine.close();
            return Err(e);
        }
    }
    let _ = engine.close();
    println!("已重启 {} 个服务。", targets.len());
    Ok(output::EXIT_OK)
}

pub fn run_script_run(root: &Path, id: &str) -> Result<i32, Error> {
    install_signal_handler();
    let engine = open_engine(root)?;
    engine.subscribe_logs()?;
    engine.run_script(id)?;
    loop {
        if let Some(supertask_core::EngineEvent::Logs { items, .. }) =
            engine.recv_event_timeout(Duration::from_millis(150))
        {
            for line in items {
                let text = &line.text;
                match line.stream {
                    supertask_core::ipc::LogStream::Stderr => eprintln!("[{id}] {text}"),
                    _ => println!("[{id}] {text}"),
                }
            }
        }
        let running = engine
            .snapshot()
            .ok()
            .and_then(|s| s.script.map(|sc| sc.state == supertask_core::engine::ScriptState::Running))
            .unwrap_or(false);
        if !running {
            break;
        }
        if STOP.load(Ordering::SeqCst) {
            let _ = engine.cancel_script();
        }
    }
    let exit_code = engine
        .snapshot()
        .ok()
        .and_then(|s| s.script.and_then(|sc| sc.last_exit.map(|e| e.code)))
        .unwrap_or(1);
    let _ = engine.close();
    if exit_code == 0 {
        println!("脚本 {id} 完成。");
        Ok(output::EXIT_OK)
    } else {
        println!("脚本 {id} 退出码 {exit_code}。");
        Ok(output::EXIT_RUNTIME)
    }
}

pub fn run_script_cancel(root: &Path) -> Result<i32, Error> {
    let engine = open_engine(root)?;
    engine.cancel_script()?;
    let _ = engine.close();
    println!("已请求取消脚本。");
    Ok(output::EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn obs(id: &str, state: RtState, health_ok: Option<bool>) -> SvcObs {
        SvcObs { id: id.into(), state, health_ok, detail: None }
    }

    #[test]
    fn wait_until_reaches_when_all_running_and_healthy() {
        let seq = [vec![
            obs("a", RtState::Starting, None),
            obs("b", RtState::Running, Some(false)),
        ], vec![
            obs("a", RtState::Running, Some(true)),
            obs("b", RtState::Running, Some(true)),
        ]];
        let i = AtomicUsize::new(0);
        let out = wait_until(
            &["a".into(), "b".into()],
            Wait::Healthy,
            Duration::from_secs(2),
            || {
                let n = i.fetch_add(1, Ordering::SeqCst);
                seq[n.min(seq.len() - 1)].clone()
            },
        );
        assert!(matches!(out, WaitOutcome::Reached));
    }

    #[test]
    fn wait_until_fails_on_exited_service() {
        let out = wait_until(
            &["a".into()],
            Wait::Healthy,
            Duration::from_secs(2),
            || vec![obs("a", RtState::Exited, None)],
        );
        match out {
            WaitOutcome::Failed(bad) => assert_eq!(bad[0].id, "a"),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn wait_until_timeout_lists_pending() {
        let out = wait_until(
            &["a".into(), "b".into()],
            Wait::Healthy,
            Duration::from_millis(50),
            || vec![obs("a", RtState::Running, Some(true)), obs("b", RtState::Starting, None)],
        );
        match out {
            WaitOutcome::Timeout(pending) => {
                assert_eq!(pending.len(), 1);
                assert_eq!(pending[0].id, "b");
            }
            _ => panic!("expected Timeout"),
        }
    }

    #[test]
    fn wait_started_mode_ignores_missing_health() {
        let out = wait_until(
            &["a".into()],
            Wait::Started,
            Duration::from_secs(1),
            || vec![obs("a", RtState::Running, None)],
        );
        assert!(matches!(out, WaitOutcome::Reached));
    }

    #[test]
    fn wait_never_is_immediate() {
        let out = wait_until(
            &["a".into()],
            Wait::Never,
            Duration::from_secs(1),
            || vec![obs("a", RtState::Stopped, None)],
        );
        assert!(matches!(out, WaitOutcome::Reached));
    }

    // ---- §13.2 桩进程集成测试（node 桩；无 node 环境自动跳过）----

    use crate::test_stubs::node_stub;

    /// `up --wait healthy -- <cmd>`：健康达标后执行包装命令并透传退出码，结束清场。
    #[test]
    fn up_wrapper_passthrough_exit_code() {
        if !node_stub::node_available() {
            eprintln!("skip: node 不可用");
            return;
        }
        let ws = node_stub::write_ws("up-passthrough", 18211, true);
        let wrapper: Vec<std::ffi::OsString> = if cfg!(windows) {
            vec!["cmd".into(), "/C".into(), "exit 5".into()]
        } else {
            vec!["sh".into(), "-c".into(), "exit 5".into()]
        };
        let code = run_up(&ws.root, &[], Wait::Healthy, 60, &wrapper).unwrap();
        assert_eq!(code, 5, "wrapper exit code must pass through");
        // 清场断言：桩服务端口已释放
        assert!(!supertask_core::ports::is_serving(ws.port), "no stub process may survive up");
        node_stub::cleanup(&ws);
    }

    /// 健康永不达标（tcp 检查失败）→ 停止全部 + HEALTH_TIMEOUT，锁已释放。
    #[test]
    fn up_health_failure_stops_all_and_reports() {
        if !node_stub::node_available() {
            eprintln!("skip: node 不可用");
            return;
        }
        let ws = node_stub::write_ws("up-timeout", 18212, false);
        let err = run_up(&ws.root, &[], Wait::Healthy, 15, &[]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::HealthTimeout);
        assert!(
            supertask_core::lock::query(&ws.root).is_none(),
            "lock must be released after failed up"
        );
        node_stub::cleanup(&ws);
    }
}
