//! 2.2 IPC 契约增量（ipc.md §10.18）：数据快照（方向六·数据与备份）命令的输出结构。
//! protocol 保持 1；输入结构在 Tauri command 签名中内联（与既有命令一致）。

use serde::{Deserialize, Serialize};

/// 单个快照的元信息（mirror `snapshot.rs::SnapshotMeta`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSnapshotView {
    /// created_at 毫秒字符串（文件名 stem）
    pub id: String,
    /// epoch 毫秒
    pub created_at: u64,
    /// zip 文件大小
    pub bytes: u64,
    /// 快照内文件数
    pub file_count: u64,
    /// 解压后总字节
    pub total_bytes: u64,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataVolumeView {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// 工作区相对路径（spec 声明原文）
    pub dir: String,
    /// created_at 降序
    pub snapshots: Vec<DataSnapshotView>,
}

/// `workspace.dataList` 输出：数据卷与各自快照。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataListOut {
    pub volumes: Vec<DataVolumeView>,
    pub warnings: Vec<String>,
}

/// `workspace.dataSnapshotCreate` 输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSnapshotCreatedOut {
    pub volume_id: String,
    pub snapshot: DataSnapshotView,
    pub warnings: Vec<String>,
}

/// `workspace.dataRestorePreview` 输出（纯只读；覆盖面陈述）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRestorePreviewOut {
    pub volume_id: String,
    pub snapshot_id: String,
    /// false 时 blockers 说明原因，UI 据此禁用确认
    pub ready: bool,
    pub blockers: Vec<String>,
    pub target_exists: bool,
    pub current_files: u64,
    pub snapshot_files: u64,
    pub total_bytes: u64,
    /// 快照外现存文件数（恢复后被删除）
    pub remove_count: u64,
    /// 最多 20 条相对路径
    pub remove_sample: Vec<String>,
    pub warnings: Vec<String>,
}

/// `workspace.dataRestore` 输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataRestoreOut {
    pub volume_id: String,
    pub snapshot_id: String,
    pub restored_files: u64,
    pub removed_files: u64,
    pub warnings: Vec<String>,
}

/// `workspace.dataSnapshotDelete` 输出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSnapshotDeletedOut {
    pub volume_id: String,
    pub snapshot_id: String,
}
