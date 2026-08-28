//! Fixed-program + structured-argv spawn. Never `cmd /c` concatenated user strings.

use std::io;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};

/// Structured spawn request. `program` is a fixed binary name/path; `args` are
/// discrete tokens. Callers must never put user-controlled flags in `args`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: IndexMap<String, String>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait ToolRunner: Send + Sync {
    fn run(&self, spec: &SpawnSpec) -> io::Result<ToolOutput>;
}

/// Real Windows/Unix spawn. Uses `Command::new(program).args(args)` only.
pub struct ProcessRunner;

impl Default for ProcessRunner {
    fn default() -> Self {
        Self
    }
}

impl ToolRunner for ProcessRunner {
    fn run(&self, spec: &SpawnSpec) -> io::Result<ToolOutput> {
        if is_generic_shell(&spec.program) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "refusing to spawn a generic shell as a toolchain provider",
            ));
        }
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &spec.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        run_with_timeout(&mut cmd, spec.timeout)
    }
}

fn is_generic_shell(program: &str) -> bool {
    let name = std::path::Path::new(program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    matches!(
        name.as_str(),
        "cmd" | "cmd.exe" | "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" | "bash" | "sh"
    )
}

fn run_with_timeout(cmd: &mut Command, timeout: Duration) -> io::Result<ToolOutput> {
    let mut child = cmd.spawn()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait()? {
            Some(_) => {
                let out = child.wait_with_output()?;
                return Ok(ToolOutput {
                    code: out.status.code().unwrap_or(-1),
                    stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                });
            }
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "toolchain provider timed out",
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

/// Records every spawn and returns scripted outputs. Used by unit tests.
pub struct FakeRunner {
    pub calls: Mutex<Vec<SpawnSpec>>,
    /// FIFO of scripted results. If empty, a default success is returned.
    pub script: Mutex<Vec<io::Result<ToolOutput>>>,
}

impl FakeRunner {
    pub fn new() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            script: Mutex::new(Vec::new()),
        }
    }

    pub fn push_ok(&self, stdout: impl Into<String>) {
        self.script.lock().unwrap().push(Ok(ToolOutput {
            code: 0,
            stdout: stdout.into(),
            stderr: String::new(),
        }));
    }

    pub fn push_fail(&self, code: i32, stderr: impl Into<String>) {
        self.script.lock().unwrap().push(Ok(ToolOutput {
            code,
            stdout: String::new(),
            stderr: stderr.into(),
        }));
    }

    pub fn calls(&self) -> Vec<SpawnSpec> {
        self.calls.lock().unwrap().clone()
    }
}

impl Default for FakeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRunner for FakeRunner {
    fn run(&self, spec: &SpawnSpec) -> io::Result<ToolOutput> {
        if is_generic_shell(&spec.program) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "fake runner refused generic shell",
            ));
        }
        self.calls.lock().unwrap().push(spec.clone());
        let mut script = self.script.lock().unwrap();
        if script.is_empty() {
            return Ok(ToolOutput {
                code: 0,
                stdout: "ok".into(),
                stderr: String::new(),
            });
        }
        script.remove(0)
    }
}

pub fn map_spawn_io(err: io::Error, program: &str) -> Error {
    if err.kind() == io::ErrorKind::NotFound {
        Error::new(
            ErrorCode::ToolchainManagerMissing,
            format!("未找到 {program}。请安装 mise 或 winget。"),
        )
    } else if err.kind() == io::ErrorKind::TimedOut {
        Error::new(
            ErrorCode::ToolchainInstallFailed,
            format!("{program} 执行超时"),
        )
    } else {
        Error::new(
            ErrorCode::ToolchainInstallFailed,
            format!("无法启动 {program}: {err}"),
        )
    }
}

pub fn run_mapped(runner: &dyn ToolRunner, spec: &SpawnSpec) -> Result<ToolOutput> {
    runner.run(spec).map_err(|e| map_spawn_io(e, &spec.program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_refuses_generic_shell() {
        let fake = FakeRunner::new();
        let spec = SpawnSpec {
            program: "cmd.exe".into(),
            args: vec!["/c".into(), "echo hi".into()],
            cwd: None,
            env: IndexMap::new(),
            timeout: Duration::from_secs(4),
        };
        assert!(fake.run(&spec).is_err());
    }
}
