//! 1.1 Git 集成：只做 clone / status / pull，spawn `git.exe` CLI，不链任何 Git SDK。
//! Spec: `docs/spec/ipc.md` §10.2、`docs/plans/2026-08-27-v1-1-feature-spec.md` §5。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Error, ErrorCode, Result};
use crate::sandbox::strip_verbatim;

/// `git.status` 的摘要视图（§5.3）：只回计数，不送文件级明细给 UI。
#[derive(Debug, Clone, Serialize)]
pub struct GitStatus {
    pub workspace_id: String,
    pub is_repository: bool,
    /// detached 时为 None
    pub branch: Option<String>,
    pub detached: bool,
    pub dirty: bool,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    /// upstream 前缀（origin/main → origin）；无 upstream 时取 `git remote` 首行
    pub remote: Option<String>,
}

/// 一次 git 进程执行的结果。
#[derive(Debug, Clone)]
pub struct GitOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 可替换执行器：单测用 fake，不依赖用户机器上的远端与网络。
pub trait GitRunner: Send + Sync {
    fn run(&self, cwd: &Path, args: &[&str]) -> io::Result<GitOutput>;
}

/// 默认执行器：直接 spawn `git`（绝不经过 cmd.exe /C）。
pub struct ProcessRunner {
    pub program: String,
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self { program: "git".into() }
    }
}

impl GitRunner for ProcessRunner {
    fn run(&self, cwd: &Path, args: &[&str]) -> io::Result<GitOutput> {
        let out = std::process::Command::new(&self.program)
            .args(args)
            .current_dir(cwd)
            // 快速失败而非挂起等输入：禁终端提示、禁交互 askpass
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_ASKPASS", "echo")
            .output()?;
        Ok(GitOutput {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// 启动失败（进程拉不起来，典型为 PATH 无 git.exe）统一映射 `GIT_NOT_FOUND`。
fn run_git(runner: &dyn GitRunner, cwd: &Path, args: &[&str]) -> Result<GitOutput> {
    runner.run(cwd, args).map_err(|e| {
        Error::new(
            ErrorCode::GitNotFound,
            format!("无法启动 git（{e}）：请确认 Git 已安装并在 PATH 中"),
        )
    })
}

// ---------- status ----------

/// 读取仓库状态摘要，不修改任何文件。
pub fn status(runner: &dyn GitRunner, workspace_id: &str, root: &Path) -> Result<GitStatus> {
    let probe = run_git(runner, root, &["rev-parse", "--is-inside-work-tree"])?;
    if probe.code != 0 || probe.stdout.trim() != "true" {
        return Err(Error::new(
            ErrorCode::GitNotRepository,
            format!("目录不是 Git 仓库: {}", root.display()),
        ));
    }
    let out = run_git(runner, root, &["status", "--porcelain=v2", "--branch"])?;
    if out.code != 0 {
        return Err(git_failure("status", ErrorCode::GitFailed, &out.stderr, &out.stdout));
    }
    let parsed = parse_porcelain_v2(&out.stdout);

    // remote：优先 upstream 前缀，否则 `git remote` 首行
    let remote = match &parsed.upstream {
        Some(up) => up.split('/').next().filter(|s| !s.is_empty()).map(str::to_string),
        None => None,
    };
    let remote = match remote {
        Some(r) => Some(r),
        None => first_remote(runner, root)?,
    };

    let dirty = parsed.staged + parsed.unstaged + parsed.untracked > 0;
    Ok(GitStatus {
        workspace_id: workspace_id.to_string(),
        is_repository: true,
        branch: parsed.branch,
        detached: parsed.detached,
        dirty,
        ahead: parsed.ahead,
        behind: parsed.behind,
        staged: parsed.staged,
        unstaged: parsed.unstaged,
        untracked: parsed.untracked,
        remote,
    })
}

#[derive(Default)]
struct ParsedStatus {
    branch: Option<String>,
    detached: bool,
    upstream: Option<String>,
    ahead: u32,
    behind: u32,
    staged: u32,
    unstaged: u32,
    untracked: u32,
}

/// 一次解析 `git status --porcelain=v2 --branch` 的全部字段。
fn parse_porcelain_v2(text: &str) -> ParsedStatus {
    let mut p = ParsedStatus::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            if let Some(head) = rest.strip_prefix("branch.head ") {
                if head == "(detached)" {
                    p.detached = true;
                } else {
                    p.branch = Some(head.to_string());
                }
            } else if let Some(up) = rest.strip_prefix("branch.upstream ") {
                p.upstream = Some(up.to_string());
            } else if let Some(ab) = rest.strip_prefix("branch.ab ") {
                for tok in ab.split_whitespace() {
                    if let Some(n) = tok.strip_prefix('+') {
                        p.ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = tok.strip_prefix('-') {
                        p.behind = n.parse().unwrap_or(0);
                    }
                }
            }
        } else if let Some(rest) = line.strip_prefix("1 ") {
            // 1 XY sub …：XY 为第一个 token
            if let Some(xy) = rest.split_whitespace().next() {
                count_xy(&mut p, xy);
            }
        } else if let Some(rest) = line.strip_prefix("2 ") {
            // 2 XY sub …：XY 同样在第二个 token
            if let Some(xy) = rest.split_whitespace().next() {
                count_xy(&mut p, xy);
            }
        } else if line.starts_with("u ") {
            // 未合并条目按未暂存计
            p.unstaged += 1;
        } else if line.starts_with("? ") {
            p.untracked += 1;
        }
        // `!`（ignored）与其他行忽略
    }
    p
}

fn count_xy(p: &mut ParsedStatus, xy: &str) {
    let mut chars = xy.chars();
    let (Some(x), Some(y)) = (chars.next(), chars.next()) else { return };
    if x != '.' {
        p.staged += 1;
    }
    if y != '.' {
        p.unstaged += 1;
    }
}

/// `git remote` 首行；无远端返回 None。
fn first_remote(runner: &dyn GitRunner, root: &Path) -> Result<Option<String>> {
    let out = run_git(runner, root, &["remote"])?;
    if out.code != 0 {
        return Err(git_failure("status", ErrorCode::GitFailed, &out.stderr, &out.stdout));
    }
    Ok(out
        .stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string))
}

// ---------- clone ----------

/// clone 到 target。target 不存在或为空目录均可；非空拒绝且不改动目录。
pub fn clone(runner: &dyn GitRunner, url: &str, target: &Path, branch: Option<&str>) -> Result<PathBuf> {
    check_url(url)?;
    if target.exists() {
        let not_empty = if target.is_dir() {
            fs::read_dir(target)
                .map(|mut rd| rd.next().is_some())
                .unwrap_or(true)
        } else {
            true
        };
        if not_empty {
            return Err(Error::new(
                ErrorCode::TargetNotEmpty,
                format!("目标目录非空，拒绝覆盖: {}", target.display()),
            ));
        }
    }
    let parent = target.parent().filter(|p| p.is_dir()).ok_or_else(|| {
        Error::new(
            ErrorCode::CwdMissing,
            format!("目标目录的父级不存在: {}", target.display()),
        )
    })?;

    let mut args: Vec<&str> = vec!["clone"];
    if let Some(b) = branch {
        args.push("--branch");
        args.push(b);
    }
    args.push(url);
    let target_str = target.to_string_lossy().into_owned();
    args.push(&target_str);

    let out = run_git(runner, parent, &args)?;
    if out.code != 0 {
        let code = classify_failure(&out.stderr, &out.stdout);
        return Err(git_failure("clone", code, &out.stderr, &out.stdout));
    }
    // canonicalize 在 Windows 产生 \\?\ 前缀，按仓库惯例还原
    let canonical = fs::canonicalize(target).map_err(|e| {
        Error::new(
            ErrorCode::GitFailed,
            format!("clone 成功但无法解析目标路径（{e}）: {}", target.display()),
        )
    })?;
    Ok(strip_verbatim(canonical))
}

// ---------- pull ----------

/// 拉取远端更新（merge 语义，绝不做 reset / checkout / clean / stash）。
pub fn pull(
    runner: &dyn GitRunner,
    root: &Path,
    remote: Option<&str>,
    branch: Option<&str>,
    allow_dirty: bool,
) -> Result<GitStatus> {
    let ws_id = root.to_string_lossy().into_owned();
    let st = status(runner, &ws_id, root)?;
    if st.dirty && !allow_dirty {
        return Err(Error::new(
            ErrorCode::GitDirty,
            "工作区有未提交修改，已阻止拉取；请先提交/暂存，或确认后带 allow_dirty 重试",
        ));
    }
    let remote_name = remote.unwrap_or("origin");

    // pull 前先确认远端存在，报错说人话而不是 git 的裸输出
    let remotes = run_git(runner, root, &["remote"])?;
    if remotes.code != 0 {
        return Err(git_failure("pull", ErrorCode::GitFailed, &remotes.stderr, &remotes.stdout));
    }
    let known = remotes.stdout.lines().any(|l| l.trim() == remote_name);
    if !known {
        return Err(Error::new(
            ErrorCode::GitRemote,
            format!("远端不存在: {remote_name}（可用远端: {}）", remotes.stdout.trim()),
        ));
    }

    // --no-rebase：固定 merge 语义；git ≥2.34 在分叉且无 pull.rebase 配置时会拒绝默认 merge
    let mut args: Vec<&str> = vec!["pull", "--no-rebase", remote_name];
    if let Some(b) = branch {
        args.push(b);
    }
    let out = run_git(runner, root, &args)?;
    let combined = format!("{}\n{}", out.stdout, out.stderr);
    if combined.contains("CONFLICT") {
        return Err(Error::new(
            ErrorCode::GitConflict,
            "拉取产生合并冲突：请解决冲突后提交。SuperTask 不会自动 reset / stash / checkout",
        )
        .details(serde_yaml::Value::String(sanitize_output(combined.trim()))));
    }
    if out.code != 0 {
        let code = classify_failure(&out.stderr, &out.stdout);
        return Err(git_failure("pull", code, &out.stderr, &out.stdout));
    }
    status(runner, &ws_id, root)
}

// ---------- URL 安全与输出脱敏 ----------

/// URL 校验：拒绝内嵌凭据（scheme://user:pass@ / token@ 形式），认证交给凭据管理器。
pub fn check_url(url: &str) -> Result<()> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::new(ErrorCode::GitFailed, "clone URL 不能为空"));
    }
    if url.chars().any(char::is_whitespace) {
        return Err(Error::new(ErrorCode::GitFailed, "clone URL 不能包含空白字符"));
    }
    if let Some(scheme_pos) = url.find("://") {
        let auth_start = scheme_pos + 3;
        let auth_end = url[auth_start..]
            .find('/')
            .map_or(url.len(), |i| auth_start + i);
        if url[auth_start..auth_end].contains('@') {
            return Err(Error::new(
                ErrorCode::GitFailed,
                "URL 中包含内嵌凭据（user:pass@ 或 token@ 形式），出于安全考虑已拒绝；请改用 Git Credential Manager 或系统已配置的凭据",
            ));
        }
    }
    Ok(())
}

/// 脱敏：`scheme://user:pass@` → `scheme://***@`、`scheme://token@` → `scheme://***@`；
/// `git@host:path`（scp 形式，无 `://`）保持原样；其余文本不动。
pub fn sanitize_output(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        let Some(rel) = text[i..].find("://") else {
            out.push_str(&text[i..]);
            break;
        };
        let start = i + rel;
        let auth_start = start + 3;
        out.push_str(&text[i..auth_start]);
        // authority 到 '/'、'?'、'#' 或空白为止
        let mut auth_end = text.len();
        for (off, c) in text[auth_start..].char_indices() {
            if c == '/' || c == '?' || c == '#' || c.is_whitespace() {
                auth_end = auth_start + off;
                break;
            }
        }
        let authority = &text[auth_start..auth_end];
        match authority.rfind('@') {
            Some(at) => {
                out.push_str("***");
                out.push_str(&authority[at..]); // 保留 @ 及其后的 host
            }
            None => out.push_str(authority),
        }
        i = auth_end;
    }
    out
}

// ---------- 错误分类 ----------

/// 按 git 的 stderr/stdout 关键词把非零退出归类到稳定错误码。
fn classify_failure(stderr: &str, stdout: &str) -> ErrorCode {
    let lower = format!("{stderr}\n{stdout}").to_lowercase();
    if lower.contains("authentication failed")
        || lower.contains("could not read username")
        || (lower.contains("terminal prompts disabled") && (lower.contains("401") || lower.contains("403")))
    {
        return ErrorCode::GitAuth;
    }
    // 分支类先于远端类判定（"Remote branch … not found in upstream origin"）
    if (lower.contains("remote branch") && lower.contains("not found"))
        || lower.contains("not found in upstream")
    {
        return ErrorCode::GitBranch;
    }
    if lower.contains("does not appear to be a git repository")
        || lower.contains("could not resolve host")
        || (lower.contains("repository") && lower.contains("not found"))
    {
        return ErrorCode::GitRemote;
    }
    ErrorCode::GitFailed
}

/// 构造带中文说明 + 脱敏摘要的失败错误。
fn git_failure(op: &str, code: ErrorCode, stderr: &str, stdout: &str) -> Error {
    let raw = if stderr.trim().is_empty() { stdout } else { stderr };
    let sanitized = sanitize_output(raw);
    let joined = sanitized
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" | ");
    let mut summary: String = joined.chars().take(300).collect();
    if summary.len() < joined.len() {
        summary.push('…');
    }
    let what = match code {
        ErrorCode::GitAuth => "Git 认证失败",
        ErrorCode::GitRemote => "远端仓库不可访问",
        ErrorCode::GitBranch => "分支不存在",
        _ => "git 命令执行失败",
    };
    Error::new(code, format!("git {op} 失败：{what}：{summary}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2_full() {
        let text = "\
# branch.oid 6f1a2b
# branch.head main
# branch.upstream origin/main
# branch.ab +2 -1
1 M. N... 000000 100644 100644 aaa bbb file-a
1 .D N... 100644 000000 bbb aaa file-b
2 R. N... 100644 100644 aaa bbb R100 old.txt -> new.txt
u 1 0 0 0 c1 c2 c3 x
? untracked.txt
! ignored.txt
";
        let p = parse_porcelain_v2(text);
        assert_eq!(p.branch.as_deref(), Some("main"));
        assert!(!p.detached);
        assert_eq!(p.upstream.as_deref(), Some("origin/main"));
        assert_eq!(p.ahead, 2);
        assert_eq!(p.behind, 1);
        assert_eq!(p.staged, 2); // M. + R.
        assert_eq!(p.unstaged, 2); // .D + u
        assert_eq!(p.untracked, 1);
    }

    #[test]
    fn parses_detached_and_empty() {
        let p = parse_porcelain_v2("# branch.head (detached)\n");
        assert!(p.detached);
        assert!(p.branch.is_none());
        assert_eq!(p.staged + p.unstaged + p.untracked, 0);

        let clean = parse_porcelain_v2(
            "# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n",
        );
        assert_eq!(clean.branch.as_deref(), Some("main"));
        assert_eq!((clean.ahead, clean.behind), (0, 0));
    }

    #[test]
    fn sanitize_masks_userinfo_urls() {
        assert_eq!(
            sanitize_output("https://user:pass@github.com/a/b.git"),
            "https://***@github.com/a/b.git"
        );
        assert_eq!(
            sanitize_output("https://token-placeholder@github.com/a/b.git"),
            "https://***@github.com/a/b.git"
        );
        // 同一段文本里的多个 URL 都要脱敏
        assert_eq!(
            sanitize_output("fatal: https://u:p@a.com/x and https://t@b.com/y"),
            "fatal: https://***@a.com/x and https://***@b.com/y"
        );
    }

    #[test]
    fn sanitize_keeps_scp_and_plain_text() {
        assert_eq!(sanitize_output("git@github.com:a/b.git"), "git@github.com:a/b.git");
        assert_eq!(sanitize_output("no urls here"), "no urls here");
        assert_eq!(
            sanitize_output("see https://github.com/a/b.git for docs"),
            "see https://github.com/a/b.git for docs"
        );
    }

    #[test]
    fn check_url_accepts_safe_forms() {
        check_url("https://github.com/a/b.git").unwrap();
        check_url("git@github.com:a/b.git").unwrap();
        check_url("file:///C:/repos/x.git").unwrap();
        check_url("  https://example.com/x  ").unwrap(); // 去空白后放行
    }

    #[test]
    fn check_url_rejects_credentials_and_junk() {
        for bad in [
            "https://user:pass@github.com/a/b.git",
            "https://token@github.com/a/b.git",
            "",
            "https://a b.com/x",
        ] {
            let err = check_url(bad).unwrap_err();
            assert_eq!(err.code(), ErrorCode::GitFailed, "case: {bad}");
        }
    }

    #[test]
    fn classifies_failures() {
        assert_eq!(
            classify_failure("fatal: Authentication failed for 'https://x'", ""),
            ErrorCode::GitAuth
        );
        assert_eq!(
            classify_failure("fatal: could not read Username for 'https://x': terminal prompts disabled", ""),
            ErrorCode::GitAuth
        );
        assert_eq!(
            classify_failure("", "fatal: Remote branch nope not found in upstream origin"),
            ErrorCode::GitBranch
        );
        assert_eq!(
            classify_failure("fatal: 'C:/x' does not appear to be a git repository", ""),
            ErrorCode::GitRemote
        );
        assert_eq!(
            classify_failure("ERROR: Repository not found.", ""),
            ErrorCode::GitRemote
        );
        assert_eq!(
            classify_failure("ssh: Could not resolve hostname none.invalid", ""),
            ErrorCode::GitRemote
        );
        assert_eq!(classify_failure("fatal: something else", ""), ErrorCode::GitFailed);
    }

    #[test]
    fn status_maps_io_error_to_git_not_found() {
        struct Dead;
        impl GitRunner for Dead {
            fn run(&self, _cwd: &Path, _args: &[&str]) -> io::Result<GitOutput> {
                Err(io::Error::new(io::ErrorKind::NotFound, "no git"))
            }
        }
        let err = status(&Dead, "ws", Path::new(".")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::GitNotFound);
    }
}
