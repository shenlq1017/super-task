//! 2.1 README 导入器：确定性规则引擎，把 README 中的启动/构建命令解析为
//! `supertask.yaml` 草稿（与文件系统扫描融合），经 merge 向导人确认后写盘。
//! 纯函数、零网络、零 LLM；契约见 `docs/spec/ipc.md` §10.13、spec §3。

pub mod readme;
