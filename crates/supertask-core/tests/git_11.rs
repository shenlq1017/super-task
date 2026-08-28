//! 1.1 Git 模块集成测试：真实 git.exe + 临时目录 + 本地 bare 远端。
//! 机器上没有 git.exe 时打印提示并整体跳过（不视为失败）。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use supertask_core::error::ErrorCode;
use supertask_core::git::{self, GitOutput, GitRunner, ProcessRunner};

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn unique_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("st-git11-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// 直接跑 git，失败即 panic（fixture 自身问题）。
fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .expect("无法启动 git");
    assert!(
        out.status.success(),
        "git {:?} 在 {} 失败: {}",
        args,
        cwd.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commit_all(repo: &Path, msg: &str) {
    git(repo, &["add", "-A"]);
    git(
        repo,
        &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", msg],
    );
}

/// 构造 fixture：seed 工作仓（分支 main）+ bare 远端 origin.git，seed 已挂 origin。
/// 返回 (root, seed, origin)。
fn setup_remote(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = unique_root(tag);
    let seed = root.join("seed");
    fs::create_dir_all(&seed).unwrap();
    git(&seed, &["init"]);
    // 不依赖 init.defaultBranch 配置，固定分支名 main
    git(&seed, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fs::write(seed.join("file.txt"), "line1\n").unwrap();
    commit_all(&seed, "init");

    let origin = root.join("origin.git");
    let seed_str = seed.to_string_lossy().into_owned();
    let origin_str = origin.to_string_lossy().into_owned();
    git(&root, &["clone", "--bare", &seed_str, &origin_str]);

    let origin_fwd = origin_str.replace('\\', "/");
    git(&seed, &["remote", "add", "origin", &origin_fwd]);
    (root, seed, origin)
}

/// seed 里改文件并推到 bare 远端。
fn push_seed_file(seed: &Path, name: &str, content: &str) {
    fs::write(seed.join(name), content).unwrap();
    commit_all(seed, "update from seed");
    git(seed, &["push", "origin", "main"]);
}

fn runner() -> ProcessRunner {
    ProcessRunner::default()
}

// ---- clone ----

#[test]
fn clone_into_missing_dir_succeeds() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, _seed, origin) = setup_remote("clone-missing");
    let target = root.join("work1");
    let got = git::clone(&runner(), &origin.to_string_lossy(), &target, None).unwrap();
    assert!(target.join(".git").is_dir());
    assert_eq!(got, target);

    let st = git::status(&runner(), "ws1", &target).unwrap();
    assert!(st.is_repository);
    assert_eq!(st.branch.as_deref(), Some("main"));
    assert!(!st.detached);
    assert!(!st.dirty);
    assert_eq!((st.ahead, st.behind), (0, 0));
    assert_eq!((st.staged, st.unstaged, st.untracked), (0, 0, 0));
    assert_eq!(st.remote.as_deref(), Some("origin"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn clone_into_empty_dir_succeeds() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, _seed, origin) = setup_remote("clone-empty");
    let target = root.join("work2");
    fs::create_dir_all(&target).unwrap();
    git::clone(&runner(), &origin.to_string_lossy(), &target, None).unwrap();
    assert!(target.join(".git").is_dir());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn clone_into_nonempty_dir_rejected_without_touching_it() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, _seed, origin) = setup_remote("clone-nonempty");
    let target = root.join("work3");
    fs::create_dir_all(&target).unwrap();
    fs::write(target.join("keep.txt"), "precious\n").unwrap();

    let err = git::clone(&runner(), &origin.to_string_lossy(), &target, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TargetNotEmpty);
    // 原目录内容未被改动，也没有混入 clone 产物
    assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "precious\n");
    assert_eq!(fs::read_dir(&target).unwrap().count(), 1);
    let _ = fs::remove_dir_all(&root);
}

// ---- pull ----

#[test]
fn clean_pull_fast_forwards_and_behind_resets() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, seed, origin) = setup_remote("pull-clean");
    let work = root.join("work");
    git::clone(&runner(), &origin.to_string_lossy(), &work, None).unwrap();

    push_seed_file(&seed, "file.txt", "line1\nline2\n");
    // ahead/behind 对比的是 remote-tracking ref，需 fetch 后才可见 behind=1
    git(&work, &["fetch"]);

    let before = git::status(&runner(), "ws", &work).unwrap();
    assert_eq!(before.behind, 1);
    assert_eq!(before.ahead, 0);

    let after = git::pull(&runner(), &work, None, None, false).unwrap();
    assert_eq!(after.behind, 0);
    assert!(!after.dirty);
    assert!(fs::read_to_string(work.join("file.txt")).unwrap().contains("line2"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn dirty_pull_blocked_then_allowed() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, seed, origin) = setup_remote("pull-dirty");
    let work = root.join("work");
    git::clone(&runner(), &origin.to_string_lossy(), &work, None).unwrap();
    // 本地未提交修改（改 file.txt）；上游改另一个文件，保证 allow_dirty 后 merge 能干净完成
    fs::write(work.join("file.txt"), "line1\nlocal\n").unwrap();
    push_seed_file(&seed, "readme.md", "up\n");

    let err = git::pull(&runner(), &work, None, None, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitDirty);
    // 阻止时确实没有执行 pull：readme.md 还没下来
    assert!(!work.join("readme.md").exists());

    let after = git::pull(&runner(), &work, None, None, true).unwrap();
    assert!(fs::read_to_string(work.join("readme.md")).unwrap().contains("up"));
    assert!(after.dirty); // 本地修改仍在，SuperTask 不动它
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn divergent_pull_conflict_keeps_worktree() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, seed, origin) = setup_remote("pull-conflict");
    let work = root.join("work");
    git::clone(&runner(), &origin.to_string_lossy(), &work, None).unwrap();

    // 双方各提交一行互相冲突的修改
    push_seed_file(&seed, "file.txt", "upstream\n");
    fs::write(work.join("file.txt"), "local\n").unwrap();
    commit_all(&work, "local change");

    let err = git::pull(&runner(), &work, None, None, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitConflict);

    // 冲突现场保留：文件仍在且带冲突标记
    let body = fs::read_to_string(work.join("file.txt")).unwrap();
    assert!(body.contains("<<<<<<<"), "缺少冲突标记: {body}");
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn pull_unknown_remote_rejected() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, _seed, origin) = setup_remote("pull-remote");
    let work = root.join("work");
    git::clone(&runner(), &origin.to_string_lossy(), &work, None).unwrap();
    let err = git::pull(&runner(), &work, Some("nope"), None, false).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitRemote);
    let _ = fs::remove_dir_all(&root);
}

// ---- clone 失败分类 ----

#[test]
fn clone_missing_branch_maps_to_git_branch() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let (root, _seed, origin) = setup_remote("clone-branch");
    let target = root.join("work-b");
    let err = git::clone(
        &runner(),
        &origin.to_string_lossy(),
        &target,
        Some("no-such-branch"),
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitBranch);
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn clone_missing_path_maps_to_git_remote() {
    if !git_available() {
        eprintln!("跳过：本机无 git.exe");
        return;
    }
    let root = unique_root("clone-missing-path");
    let url = format!("file:///{}/no-such-repo.git", root.to_string_lossy().replace('\\', "/"));
    let err = git::clone(&runner(), &url, &root.join("work-c"), None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitRemote);
    let _ = fs::remove_dir_all(&root);
}

// ---- runner 不可用 ----

#[test]
fn dead_runner_maps_to_git_not_found() {
    struct Dead;
    impl GitRunner for Dead {
        fn run(&self, _cwd: &Path, _args: &[&str]) -> io::Result<GitOutput> {
            Err(io::Error::new(io::ErrorKind::NotFound, "no git"))
        }
    }
    let err = git::status(&Dead, "ws", Path::new(".")).unwrap_err();
    assert_eq!(err.code(), ErrorCode::GitNotFound);
}
