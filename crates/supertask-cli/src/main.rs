//! SuperTask CLI 壳（1.5）：全部业务在 supertask-core，本 crate 只做适配与输出。
//! 退出码：0 成功；1 运行错误；2 用法错误（clap）。

mod cli;
mod mcp;
mod output;
mod pkg_cmd;
mod readonly;
mod resolve;
mod up;

#[cfg(test)]
mod test_stubs;

use clap::Parser as _;
use supertask_core::Error;

fn main() {
    let args = cli::Cli::parse();
    let code = match run(&args) {
        Ok(c) => c,
        Err(e) => output::fail(args.json, &e),
    };
    std::process::exit(code);
}

fn run(args: &cli::Cli) -> Result<i32, Error> {
    // 可变命令按需取锁；只读命令 resolve 后直读文件
    match &args.cmd {
        cli::Cmd::Up { ids, wait, wait_timeout, command } => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            up::run_up(&root, ids, *wait, *wait_timeout, command)
        }
        cli::Cmd::Down { ids } => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            up::run_down(&root, ids)
        }
        cli::Cmd::Restart { ids } => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            up::run_restart(&root, ids)
        }
        cli::Cmd::Script { cmd } => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            match cmd {
                cli::ScriptCmd::Run { id } => up::run_script_run(&root, id),
                cli::ScriptCmd::Cancel => up::run_script_cancel(&root),
            }
        }
        cli::Cmd::Status => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            readonly::run_status(args.json, &root)
        }
        cli::Cmd::Logs { id, lines, grep } => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            readonly::run_logs(args.json, &root, id.as_deref(), *lines, grep.as_deref())
        }
        cli::Cmd::Doctor => readonly::run_doctor(args.json),
        cli::Cmd::Mcp => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            mcp::run_mcp(root)
        }
        cli::Cmd::Version => readonly::run_version(args.json),
        cli::Cmd::Export { output: dest, with_secrets } => {
            let root = resolve::resolve(args.workspace.as_deref())?;
            pkg_cmd::run_export(args.json, &root, dest.as_deref(), *with_secrets)
        }
        cli::Cmd::Import { pkg, dest } => {
            let dest_dir = match dest {
                Some(d) => d.clone(),
                None => std::env::current_dir().map_err(|e| {
                    Error::new(supertask_core::ErrorCode::NoWorkspace, format!("无法读取 cwd: {e}"))
                })?,
            };
            pkg_cmd::run_import(args.json, pkg, &dest_dir)
        }
    }
}
