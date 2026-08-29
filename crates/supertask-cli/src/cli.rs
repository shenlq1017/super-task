//! 1.5 §4：CLI 参数定义（clap derive）。壳层只做适配，业务在 supertask-core。

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "supertask",
    version,
    about = "SuperTask 工作台命令行：起停、观察、脚本、导出包"
)]
pub struct Cli {
    /// 机器可读 JSON 输出（结构快照由测试锁定）
    #[arg(long, global = true)]
    pub json: bool,
    /// 关闭带色输出（当前版本输出为纯文本，保留开关兼容脚本）
    #[arg(long, global = true)]
    pub no_color: bool,
    /// 工作区目录（缺省：SUPERTASK_WORKSPACE > cwd 向上搜索 supertask.yaml）
    #[arg(short, long, global = true)]
    pub workspace: Option<PathBuf>,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand)]
pub enum Cmd {
    /// 按拓扑启动服务并等待健康，随后保持附加（或 `--` 包装子命令）
    Up {
        /// 只启动这些服务（缺省全部；依赖自动拉起）
        ids: Vec<String>,
        /// 等待目标：healthy（默认）/ started / none
        #[arg(long, value_enum, default_value_t = Wait::Healthy)]
        wait: Wait,
        /// 等待健康超时秒数
        #[arg(long, default_value_t = 300)]
        wait_timeout: u64,
        /// 健康达标后执行的包装命令（`--` 后全部原样透传；继承 stdio，退出码透传）
        #[arg(last = true, allow_hyphen_values = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// 停止所选/全部服务（本进程非 owner 且锁被持有 → 拒绝）
    Down {
        /// 只停这些服务（缺省全部）
        ids: Vec<String>,
    },
    /// 停止再启动（缺省全部，按拓扑顺序）
    Restart {
        ids: Vec<String>,
    },
    /// 服务快照 + 工作区锁持有者（只读，不取锁）
    Status,
    /// 历史日志尾部/检索（只读 `.supertask/logs`，不取锁）
    Logs {
        /// 只看这个服务的日志（缺省全部源）
        id: Option<String>,
        /// 尾部行数
        #[arg(long, default_value_t = 200)]
        lines: usize,
        /// literal 检索关键字（大小写不敏感）
        #[arg(long)]
        grep: Option<String>,
    },
    /// 工作区脚本：run / cancel
    Script {
        #[command(subcommand)]
        cmd: ScriptCmd,
    },
    /// 导出工作区包（zip：manifest + supertask.yaml + 可选密钥文件）
    Export {
        /// 输出文件路径（缺省 supertask-<目录名>-<时间>.zip 到 cwd）
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 把 secrets.file 与 env_file 一并打包（明文密钥，需自行保管）
        #[arg(long)]
        with_secrets: bool,
    },
    /// 导入工作区包（只落盘，不打开不启动）
    Import {
        /// 导出包路径
        pkg: PathBuf,
        /// 目标目录（缺省 cwd；已有 supertask.yaml 时拒绝）
        #[arg(long)]
        dest: Option<PathBuf>,
    },
    /// 工具链与 docker 探测摘要（CI 排障）
    Doctor,
    /// stdio MCP 服务器（Cursor/Claude 等编辑器接入；断开即停止全部服务）
    Mcp,
    /// 版本与协议信息
    Version,
}

#[derive(Subcommand)]
pub enum ScriptCmd {
    /// 运行脚本（cmds 只来自 supertask.yaml；同工作区同时仅一个脚本）
    Run { id: String },
    /// 取消当前脚本
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Wait {
    Healthy,
    Started,
    /// 不等待（value 名 none）
    #[value(name = "none")]
    Never,
}

impl Wait {
    pub fn label(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Started => "started",
            Self::Never => "none",
        }
    }
}
