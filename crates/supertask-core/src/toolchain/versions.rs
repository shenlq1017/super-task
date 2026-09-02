//! /env 深化（2026-08-31 评估 S1）：每工具可选版本列表。
//!
//! 数据源两级：
//! 1. winget 白名单（离线、肯定可装，[`super::manifest`]）；
//! 2. `mise ls-remote <tool>` 尾部 N 条（仅 mise 可用时；返回全量历史，只取近期版本）。
//!
//! 前端此前纯手输版本，撞上白名单外即 `TOOLCHAIN_VERSION_INVALID`；
//! 本模块把"哪些版本能装"变成可查询数据。

use super::manifest;
use super::runner::{SpawnSpec, ToolRunner};
use super::ToolKind;
use indexmap::IndexMap;
use std::time::Duration;

const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(10);
/// mise ls-remote 全量历史可达数千条，只取尾部 N 条作为可选项。
const REMOTE_TAIL: usize = 30;

/// 全部逻辑工具（顺序即输出 IndexMap 的键序）。
pub const ALL_TOOLS: [ToolKind; 9] = [
    ToolKind::Java,
    ToolKind::Maven,
    ToolKind::Node,
    ToolKind::Npm,
    ToolKind::Pnpm,
    ToolKind::Yarn,
    ToolKind::Bun,
    ToolKind::Python,
    ToolKind::Go,
];

pub fn ls_remote_spec(tool: ToolKind) -> SpawnSpec {
    SpawnSpec {
        program: "mise".into(),
        args: vec!["ls-remote".into(), manifest::mise_tool_name(tool).into()],
        cwd: None,
        env: IndexMap::new(),
        timeout: LS_REMOTE_TIMEOUT,
    }
}

/// 单工具版本列表：默认版本 + 白名单 +（mise 可用时）远端尾部。
/// 远端查询失败静默降级为白名单（可选列表是锦上添花，不是安装前置）。
pub fn versions_for(runner: &dyn ToolRunner, tool: ToolKind, mise_available: bool) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |v: &str| {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(v)) {
            out.push(v.to_string());
        }
    };
    push(manifest::default_version(tool));
    for v in manifest::winget_versions(tool) {
        push(v);
    }
    if mise_available {
        if let Ok(o) = runner.run(&ls_remote_spec(tool)) {
            if o.code == 0 {
                let remotes: Vec<&str> = o
                    .stdout
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect();
                for v in remotes.iter().rev().take(REMOTE_TAIL).rev() {
                    push(v);
                }
            }
        }
    }
    out
}

/// 8 工具并行查询，总耗时取决于最慢的单条 ls-remote。
pub fn all_versions(
    runner: &dyn ToolRunner,
    mise_available: bool,
) -> IndexMap<String, Vec<String>> {
    let results = std::thread::scope(|s| {
        let handles: Vec<_> = ALL_TOOLS
            .iter()
            .map(|t| s.spawn(move || versions_for(runner, *t, mise_available)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect::<Vec<_>>()
    });
    ALL_TOOLS
        .iter()
        .zip(results)
        .map(|(t, v)| (t.as_str().to_string(), v))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolchain::runner::FakeRunner;

    #[test]
    fn versions_union_default_whitelist_and_remote_tail() {
        let fake = FakeRunner::new();
        fake.push_ok("20.0.1\n21.0.1\n21.0.2\n22.0.0");
        let v = versions_for(&fake, ToolKind::Java, true);
        // 默认版本在最前；白名单与 lts 别名保留；远端按升序接尾
        assert_eq!(v[0], "21");
        assert!(v.contains(&"lts".to_string()));
        assert!(v.contains(&"17".to_string()));
        let i1 = v.iter().position(|x| x == "21.0.1").unwrap();
        let i2 = v.iter().position(|x| x == "22.0.0").unwrap();
        assert!(i1 < i2);
        // 远端与白名单重复的版本不重复出现（21 已在白名单）
        assert_eq!(v.iter().filter(|x| x.as_str() == "21").count(), 1);
        // 请求确实是 mise ls-remote <tool>
        let calls = fake.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "mise");
        assert_eq!(calls[0].args, vec!["ls-remote", "java"]);
    }

    #[test]
    fn versions_without_mise_never_spawn() {
        let fake = FakeRunner::new();
        let v = versions_for(&fake, ToolKind::Java, false);
        assert!(fake.calls().is_empty());
        assert_eq!(v, vec!["21", "17", "11", "lts"]);
    }

    #[test]
    fn remote_failure_degrades_to_whitelist() {
        let fake = FakeRunner::new();
        fake.push_fail(1, "network down");
        let v = versions_for(&fake, ToolKind::Go, true);
        assert_eq!(v, vec!["1.23", "1.22", "lts"]);
    }

    #[test]
    fn remote_tail_is_capped() {
        let fake = FakeRunner::new();
        let many = (0..200)
            .map(|i| format!("1.{i}.0"))
            .collect::<Vec<_>>()
            .join("\n");
        fake.push_ok(many);
        let v = versions_for(&fake, ToolKind::Node, true);
        // 200 条远端只取尾 30 条；且取的是最新段（1.199.0 在、1.0.0 不在）
        assert!(v.contains(&"1.199.0".to_string()));
        assert!(!v.contains(&"1.0.0".to_string()));
        assert!(v.len() <= 4 + REMOTE_TAIL);
    }

    #[test]
    fn all_versions_covers_every_tool_in_order() {
        let fake = FakeRunner::new();
        let map = all_versions(&fake, false);
        let keys: Vec<&str> = map.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            vec!["java", "maven", "node", "npm", "pnpm", "yarn", "bun", "python", "go"]
        );
        // mise 不可用 → 零 spawn
        assert!(fake.calls().is_empty());
        assert_eq!(map["python"][0], "3.12");
    }
}
