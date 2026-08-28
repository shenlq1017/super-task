//! Docker CLI spawn：固定程序 `docker` + 结构化 argv + 合计 2 MiB 输出上限 + 超时。
//!
//! 与 toolchain runner 同模式，但输出在读取侧按字节预算截尾（`logs --follow`
//! 等长输出场景由后续 phase 使用），并带 truncated 标记（规格 §4.2）。

use std::io::{self, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

pub const DOCKER_PROGRAM: &str = "docker";

/// 单次命令 stdout+stderr 合计读取上限，超出截尾并标记 `truncated`。
pub const OUTPUT_CAP_BYTES: usize = 2 * 1024 * 1024;

/// Spawn request. `args` 是离散 token；任何来自 YAML 的变量
/// （compose 文件路径、project/service 名、build context/dockerfile/tags）
/// 必须先过字符集与沙箱校验，禁止 `--` 前缀与空白。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerSpawn {
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    /// 合计输出超过 [`OUTPUT_CAP_BYTES`] 被截尾时为 true。
    pub truncated: bool,
}

/// 流式命令（`logs --follow`、`build`）的产物：两个输出流 + 终止句柄 + 退出码。
/// kill 必须幂等安全：调用方在取消时调用，杀掉 docker 进程；
/// wait 在两个流都 EOF 后调用才有意义（阻塞到进程退出）。
pub struct DockerStream {
    pub stdout: Box<dyn Read + Send>,
    pub stderr: Box<dyn Read + Send>,
    pub kill: Box<dyn FnOnce() + Send>,
    /// 退出码；`--follow` 场景调用方通常不需要。
    pub wait: Box<dyn FnMut() -> i32 + Send>,
}

pub trait DockerRunner: Send + Sync {
    fn run(&self, spec: &DockerSpawn) -> io::Result<DockerOutput>;

    /// 长输出 / 可取消命令：不设超时（规格 §4.2 build 无超时但可取消）。
    /// 输出由调用方逐行读取；结束后（含取消）调用 `kill` 收尾。
    fn run_stream(&self, spec: &DockerSpawn) -> io::Result<DockerStream>;
}

/// 真实 spawn。程序固定为 `docker`，不接受调用方指定程序名。
pub struct ProcessDockerRunner;

impl Default for ProcessDockerRunner {
    fn default() -> Self {
        Self
    }
}

impl DockerRunner for ProcessDockerRunner {
    fn run(&self, spec: &DockerSpawn) -> io::Result<DockerOutput> {
        let mut cmd = Command::new(DOCKER_PROGRAM);
        cmd.args(&spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        run_capped(&mut cmd, spec.timeout)
    }

    fn run_stream(&self, spec: &DockerSpawn) -> io::Result<DockerStream> {
        let mut cmd = Command::new(DOCKER_PROGRAM);
        cmd.args(&spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "docker stdout 不可读"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "docker stderr 不可读"))?;
        let child = Arc::new(Mutex::new(child));
        let kill_child = Arc::clone(&child);
        Ok(DockerStream {
            stdout: Box::new(stdout),
            stderr: Box::new(stderr),
            // kill 后必须 try_wait 回收，防止僵尸进程；轮询避免与 wait 抢锁死锁
            kill: Box::new(move || {
                let mut c = kill_child.lock().unwrap();
                let _ = c.kill();
                let _ = c.wait();
            }),
            wait: Box::new(move || loop {
                {
                    let mut c = child.lock().unwrap();
                    match c.try_wait() {
                        Ok(Some(st)) => return st.code().unwrap_or(-1),
                        Ok(None) => {}
                        Err(_) => return -1,
                    }
                }
                thread::sleep(Duration::from_millis(20));
            }),
        })
    }
}

fn run_capped(cmd: &mut Command, timeout: Duration) -> io::Result<DockerOutput> {
    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + timeout;
    let budget = Arc::new(AtomicUsize::new(OUTPUT_CAP_BYTES));
    let stdout_reader = spawn_reader(child.stdout.take(), budget.clone());
    let stderr_reader = spawn_reader(child.stderr.take(), budget.clone());

    let status = loop {
        match child.try_wait()? {
            Some(st) => break Some(st),
            None if Instant::now() >= deadline => break None,
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let status = match status {
        Some(st) => st,
        // 超时先杀进程，管道随之关闭，reader 线程自行结束（不 join，缓冲随线程回收）。
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "docker 命令执行超时"));
        }
    };

    let (stdout, stdout_trunc) = stdout_reader.join().unwrap_or_else(|_| (Vec::new(), false));
    let (stderr, stderr_trunc) = stderr_reader.join().unwrap_or_else(|_| (Vec::new(), false));
    Ok(DockerOutput {
        code: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        truncated: stdout_trunc || stderr_trunc,
    })
}

type ReaderResult = (Vec<u8>, bool);

/// 在共享字节预算内读流；预算耗尽后继续读但丢弃（必须持续排水，
/// 否则子进程写满管道会卡住）。返回 (字节, 是否发生截尾)。
fn spawn_reader(
    pipe: Option<impl Read + Send + 'static>,
    budget: Arc<AtomicUsize>,
) -> std::thread::JoinHandle<ReaderResult> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut truncated = false;
        if let Some(mut pipe) = pipe {
            let mut chunk = [0u8; 8192];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => {
                        let take = budget.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |b: usize| {
                            if b == 0 {
                                None
                            } else {
                                Some(b.saturating_sub(n))
                            }
                        });
                        match take {
                            Ok(prev) => {
                                let keep = prev.min(n);
                                buf.extend_from_slice(&chunk[..keep]);
                                if keep < n {
                                    truncated = true;
                                }
                            }
                            Err(_) => truncated = true,
                        }
                    }
                    Err(_) => break,
                }
            }
        }
        (buf, truncated)
    })
}

/// 脚本化流式输出的单条回放项。
pub struct FakeStream {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// 记录每次 spawn 并按脚本回放输出。单元测试用，不真调 docker。
pub struct FakeDockerRunner {
    pub calls: Mutex<Vec<DockerSpawn>>,
    /// FIFO 脚本；为空时返回默认成功（stdout "ok"）。
    pub script: Mutex<Vec<io::Result<DockerOutput>>>,
    /// `run_stream` 专用 FIFO；为空时返回空流（立即 EOF、退出码 0）。
    pub stream_script: Mutex<Vec<io::Result<FakeStream>>>,
}

impl FakeDockerRunner {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            script: Mutex::new(Vec::new()),
            stream_script: Mutex::new(Vec::new()),
        }
    }

    pub fn push_ok(&self, stdout: impl Into<String>) {
        self.script.lock().unwrap().push(Ok(DockerOutput {
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
            truncated: false,
        }));
    }

    pub fn push_fail(&self, code: i32, stderr: impl Into<String>) {
        self.script.lock().unwrap().push(Ok(DockerOutput {
            code,
            stdout: String::new(),
            stderr: stderr.into(),
            truncated: false,
        }));
    }

    /// 脚本化一个 spawn 失败（如 PATH 无 docker → NotFound）。
    pub fn push_err(&self, kind: io::ErrorKind) {
        self.script
            .lock()
            .unwrap()
            .push(Err(io::Error::new(kind, "scripted spawn error")));
    }

    /// 脚本化一次流式输出（stdout 文本，退出码 0，EOF 即结束）。
    pub fn push_stream_ok(&self, stdout: impl Into<String>) {
        self.push_stream_parts(stdout, "");
    }

    /// 脚本化一次流式输出（stdout + stderr 文本，退出码 0）。
    pub fn push_stream_parts(&self, stdout: impl Into<String>, stderr: impl Into<String>) {
        self.push_stream_full(0, stdout, stderr);
    }

    /// 脚本化一次流式输出（自定义退出码，模拟构建失败）。
    pub fn push_stream_full(&self, code: i32, stdout: impl Into<String>, stderr: impl Into<String>) {
        self.stream_script
            .lock()
            .unwrap()
            .push(Ok(FakeStream {
                code,
                stdout: stdout.into().into_bytes(),
                stderr: stderr.into().into_bytes(),
            }));
    }

    pub fn calls(&self) -> Vec<DockerSpawn> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for FakeDockerRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerRunner for FakeDockerRunner {
    fn run(&self, spec: &DockerSpawn) -> io::Result<DockerOutput> {
        self.calls.lock().unwrap().push(spec.clone());
        let mut script = self.script.lock().unwrap();
        if script.is_empty() {
            return Ok(DockerOutput {
                code: 0,
                stdout: "ok".into(),
                stderr: String::new(),
                truncated: false,
            });
        }
        script.remove(0)
    }

    fn run_stream(&self, spec: &DockerSpawn) -> io::Result<DockerStream> {
        self.calls.lock().unwrap().push(spec.clone());
        let scripted = {
            let mut q = self.stream_script.lock().unwrap();
            if q.is_empty() {
                None
            } else {
                Some(q.remove(0))
            }
        };
        let scripted = match scripted {
            Some(Ok(s)) => s,
            Some(Err(e)) => return Err(e),
            // 默认空流：立即 EOF、退出码 0（模拟容器停止后 --follow 自然结束）
            None => FakeStream { code: 0, stdout: Vec::new(), stderr: Vec::new() },
        };
        let code = scripted.code;
        Ok(DockerStream {
            stdout: Box::new(io::Cursor::new(scripted.stdout)),
            stderr: Box::new(io::Cursor::new(scripted.stderr)),
            kill: Box::new(|| {}),
            wait: Box::new(move || code),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn drain(bytes: &[u8], budget: usize) -> (Vec<u8>, bool) {
        let handle = spawn_reader(
            Some(Cursor::new(bytes.to_vec())),
            Arc::new(AtomicUsize::new(budget)),
        );
        handle.join().expect("reader thread")
    }

    #[test]
    fn reader_keeps_within_budget_and_flags_truncation() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let (out, truncated) = drain(&data, 1_000);
        assert!(truncated);
        assert_eq!(out.len(), 1_000);
        assert_eq!(&out[..], &data[..1_000]);
    }

    #[test]
    fn reader_under_budget_no_truncation() {
        let (out, truncated) = drain(b"hello docker", 1_000);
        assert!(!truncated);
        assert_eq!(out, b"hello docker");
    }

    #[test]
    fn reader_zero_budget_discards_but_flags() {
        let (out, truncated) = drain(b"hello docker", 0);
        assert!(truncated);
        assert!(out.is_empty());
    }

    #[test]
    fn fake_records_calls_and_defaults_to_ok() {
        let fake = FakeDockerRunner::new();
        let out = fake
            .run(&DockerSpawn {
                args: vec!["version".into()],
                cwd: None,
                timeout: Duration::from_secs(1),
            })
            .expect("default ok");
        assert_eq!(out.code, 0);
        assert_eq!(fake.calls().len(), 1);
        assert_eq!(fake.calls()[0].args, vec!["version"]);
    }

    #[test]
    fn fake_scripts_errors() {
        let fake = FakeDockerRunner::new();
        fake.push_err(io::ErrorKind::NotFound);
        let err = fake
            .run(&DockerSpawn {
                args: vec![],
                cwd: None,
                timeout: Duration::from_secs(1),
            })
            .expect_err("scripted");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn fake_run_stream_scripts_and_records() {
        let fake = FakeDockerRunner::new();
        fake.push_stream_parts("line1\nline2\n", "err1\n");
        let spec = DockerSpawn {
            args: vec!["compose".into(), "logs".into()],
            cwd: None,
            timeout: Duration::from_secs(1),
        };
        let mut stream = fake.run_stream(&spec).expect("stream");
        let mut out = String::new();
        stream.stdout.read_to_string(&mut out).unwrap();
        let mut err = String::new();
        stream.stderr.read_to_string(&mut err).unwrap();
        assert_eq!(out, "line1\nline2\n");
        assert_eq!(err, "err1\n");
        assert_eq!((stream.wait)(), 0);
        (stream.kill)();
        // 默认空流：立即 EOF
        let mut stream = fake.run_stream(&spec).expect("stream default");
        let mut out = String::new();
        stream.stdout.read_to_string(&mut out).unwrap();
        assert_eq!(out, "");
        // 调用被记录（argv 断言用）
        assert_eq!(fake.calls().len(), 2);
        assert_eq!(fake.calls()[0].args, vec!["compose", "logs"]);
    }
}
