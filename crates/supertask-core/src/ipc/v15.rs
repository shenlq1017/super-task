//! 1.5 IPC 契约增量（ipc.md §10.9）：导出包两条命令的输出结构。
//! protocol 保持 1；输入结构在 Tauri command 签名中内联（与既有命令一致）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgEntryView {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPackageOut {
    pub path: String,
    pub entries: Vec<PkgEntryView>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPackageOut {
    pub root: String,
    pub warnings: Vec<String>,
}
