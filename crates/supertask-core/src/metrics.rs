//! 1.2 CPU / 内存 / 进程数指标（规格 §9）。
//!
//! 口径：只覆盖本引擎创建并加入 Job Object 的进程树。CPU 来自 Job
//! accounting（累计内核+用户时间，采样窗口差分折算，可超 100% 表示多核）；
//! 内存为 Job 内进程工作集之和（单进程查询失败可容忍，全部失败为 null）。
//! 不持久化、不进日志、不上传。

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub process_count: Option<u32>,
}

/// 由两次 Job accounting 差分计算一个采样点。
/// `prev_cpu_ms` 为上一窗口的累计 CPU 时间；`wall_ms` 为窗口时长。
pub fn cpu_percent(cur_cpu_ms: u64, prev_cpu_ms: Option<u64>, wall_ms: u64) -> Option<f64> {
    let prev = prev_cpu_ms?;
    if wall_ms == 0 {
        return None;
    }
    let delta = cur_cpu_ms.saturating_sub(prev);
    Some((delta as f64 * 100.0 / wall_ms as f64).clamp(0.0, 64.0 * 100.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_percent_window_diff() {
        assert_eq!(cpu_percent(1000, None, 1000), None, "首样本无 prev → None");
        assert_eq!(cpu_percent(1500, Some(1000), 1000), Some(50.0));
        // 多核可超 100%
        assert_eq!(cpu_percent(8000, Some(0), 1000), Some(800.0));
        // 时间回退（不可能但防御）
        assert_eq!(cpu_percent(500, Some(1000), 1000), Some(0.0));
    }
}
