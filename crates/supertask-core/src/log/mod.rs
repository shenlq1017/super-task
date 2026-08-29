mod batch;
mod file;
mod ring;
pub mod search;

pub use batch::{should_flush_batch, LogBatcher};
pub use file::LogFile;
pub use ring::{truncate_line, LogHub, LogLine};
pub use search::{
    export_logs, is_expired, run_retention, search_logs, tail_logs, RetentionSummary, SearchHit,
    SearchResult, TailHit, TailResult,
};
