//! 孤儿进程纳管：把发现页看到的本机监听进程反推成 `kind: generic` 服务草稿。
//!
//! 与 Taskfile / scan merge 同一套导入机制：`preview` 是纯内存 dry-run（拟新增 /
//! 已被现有服务覆盖 / id 冲突 / 无法纳入 + 逐项警告），`apply` 是纯函数合并；
//! 写盘由调用方走 `yaml.saveForm`（base_hash 乐观锁，冲突 → `YAML_CONFLICT`），
//! 且 apply 前重新发现进程——预览到确认之间退出的进程按警告跳过，保证幂等。
//!
//! 边界（ROADMAP 方向二首轮切片）：
//! - 草稿一律 `generic` 忠实复刻原命令（program + args），不伪造 spring-boot/node
//!   专用字段（module/entry 推断不可靠，错值会被 spec 校验硬拒）；
//! - 环境变量 OS 层读不到且不允许回显：草稿 env 留空，继承工作区环境；
//! - 命令行里形似密钥的参数值脱敏为 `<redacted>`（预览与草稿都脱敏），不进 IPC 返回值；
//! - 只写用户确认的 pid 对应草稿，`labels` 保留来源（origin / adopted-from）。
//!
//! 契约：`docs/spec/ipc.md` §10.16。

use std::collections::BTreeSet;
use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::ai::sanitize::{contains_keyword, REDACTED};
use crate::discover::ForeignService;
use crate::error::Result;
use crate::scan::{sanitize_id, unique_id};
use crate::spec::{ServiceSpec, SuperTaskFile};

/// 预览项状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdoptStatus {
    /// cwd 在工作区内、可安全生成草稿（id 冲突除外）。
    Adoptable,
    /// 监听端口已被现有服务声明：大概率是该服务的外部实例，无需纳管。
    Matched,
    /// 推导 id 与现有服务冲突；候选 id 在 `candidate_id`，默认不勾。
    IdConflict,
    /// 无法安全纳入（cwd 不可用 / 端口被占且进程在工作区外等）；`reason` 说明。
    Unadoptable,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdoptItem {
    pub pid: u32,
    pub process_name: String,
    /// 运行时归类提示（discover::classify_process）：java/node/…；草稿 kind 恒为 generic。
    pub process_kind: String,
    pub ports: Vec<u16>,
    /// 脱敏后的完整命令行（展示用；未命中敏感参数时与原值一致）。
    pub cmd_line: Option<String>,
    pub cwd: Option<String>,
    /// 父进程 pid（平台读不到为 None）；`parent_name` 仅当父进程也在本次发现列表中。
    pub parent_pid: Option<u32>,
    pub parent_name: Option<String>,
    pub status: AdoptStatus,
    /// matched / unadoptable 的原因说明。
    pub reason: Option<String>,
    /// 目标服务 id（已合法化）。IdConflict 时为被占用的 id，候选在 `candidate_id`。
    pub service_id: String,
    pub candidate_id: Option<String>,
    /// generic 服务草稿；matched / unadoptable 为 None。
    pub draft: Option<ServiceSpec>,
    pub warnings: Vec<String>,
    /// 默认动作：干净 adoptable = 勾选；有警告/冲突/不可纳入 = 不勾。
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdoptPreview {
    pub items: Vec<AdoptItem>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdoptAction {
    Add,
    Keep,
}

/// apply 输入：用户在预览里逐项确认的结果（pid 是发现列表内的进程）。
#[derive(Debug, Clone, Deserialize)]
pub struct AdoptChoice {
    pub pid: u32,
    pub action: AdoptAction,
}

/// 纳管草稿的来源标签（spec `services.*.labels`，用户可在配置页看到）。
pub const ORIGIN_LABEL: &str = "origin";
pub const ORIGIN_ADOPTED: &str = "adopted";
pub const ADOPTED_FROM_LABEL: &str = "adopted-from";

// ---------------------------------------------------------------------------
// 路径归一化与相对化（与 discover 的 cwd 判定同口径：Windows 大小写不敏感）
// ---------------------------------------------------------------------------

fn norm_path(s: &str) -> String {
    let mut out = s.replace('\\', "/");
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    #[cfg(windows)]
    {
        out.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        out
    }
}

/// path 是否落在 root 下；命中时返回相对 root 的路径（path == root → "."）。
/// 纯字符串运算不触盘，无法 canonicalize 符号链接等别名列——按 discover
/// 归属判定的同口径处理，足够解释且与「外部实例识别」一致。
fn rel_under_root(path: &str, root: &Path) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let root_s = norm_path(&root.to_string_lossy());
    if root_s.is_empty() {
        return None;
    }
    let path_s = norm_path(path);
    if path_s == root_s {
        return Some(".".into());
    }
    let rest = path_s.strip_prefix(&format!("{root_s}/"))?;
    // 还原原始大小写：在原始串（仅替换分隔符）上按前缀长度切片。
    let root_raw = root.to_string_lossy().replace('\\', "/");
    let path_raw = path.replace('\\', "/");
    let cut = root_raw.len() + 1;
    if path_raw.len() > cut
        && path_raw[..root_raw.len()].eq_ignore_ascii_case(&root_raw)
        && path_raw[cut - 1..].starts_with('/')
    {
        return Some(path_raw[cut..].to_string());
    }
    // 非 ASCII 大小写差异等罕见情况：退回归一化形式（Windows 下为小写）。
    Some(rest.to_string())
}

fn cmdline_hits_root(cmd: Option<&str>, root: &Path) -> bool {
    let Some(c) = cmd else { return false };
    if c.is_empty() {
        return false;
    }
    let root_s = norm_path(&root.to_string_lossy());
    if root_s.len() < 4 {
        return false;
    }
    norm_path(c).contains(&root_s)
}

// ---------------------------------------------------------------------------
// 命令行切分与脱敏
// ---------------------------------------------------------------------------

/// 命令行 → argv。Windows 按 MSVCRT 引号/反斜杠规则（PEB 拿到的是原始命令行）；
/// Unix 是 `ps command` / `/proc/cmdline` 的空格拼接，引号分组尽力还原，有损已知。
fn split_cmdline(input: &str) -> Vec<String> {
    #[cfg(windows)]
    {
        split_cmdline_windows(input)
    }
    #[cfg(not(windows))]
    {
        split_cmdline_unix(input)
    }
}

#[cfg(windows)]
fn split_cmdline_windows(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has_token = false;
    let mut in_quotes = false;
    for c in input.chars() {
        match c {
            '"' => {
                // 结尾连续反斜杠：2n→n 个字面量；2n+1→n 个 + 转义引号
                let trailing = cur.chars().rev().take_while(|&b| b == '\\').count();
                if trailing > 0 {
                    let keep = cur.len() - trailing + trailing / 2;
                    cur.truncate(keep);
                }
                if trailing % 2 == 1 {
                    cur.push('"');
                    has_token = true;
                } else {
                    in_quotes = !in_quotes;
                    has_token = true;
                }
            }
            ' ' | '\t' if !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            _ => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(not(windows))]
fn split_cmdline_unix(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has_token = false;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in input.chars() {
        if escaped {
            cur.push(c);
            has_token = true;
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '\'' | '"' if quote.is_none() => {
                quote = Some(c);
                has_token = true;
            }
            '\'' | '"' if quote == Some(c) => quote = None,
            ' ' | '\t' if quote.is_none() => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            _ => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// 形似密钥的参数（`--password=x` / `-Dsecret=x` / `key=token` / `=Bearer …`）：
/// 值替换为 [`REDACTED`]。只处理 `=` 连接形式——值在独立参数里的无法可靠配对，
/// 宁可放过也不误伤普通参数。
fn redact_token(tok: &str) -> Option<(String, String)> {
    let (name, val) = tok.split_once('=')?;
    if val.is_empty() {
        return None;
    }
    let n = name.trim_start_matches('-');
    let n = n.strip_prefix('D').unwrap_or(n);
    let lower = n.to_lowercase();
    let sensitive = [
        "password", "passwd", "pwd", "token", "secret", "apikey", "api_key",
    ]
    .iter()
    .any(|w| contains_keyword(&lower, w));
    let bearer = val.trim_start().to_lowercase().starts_with("bearer ");
    if sensitive || bearer {
        Some((format!("{name}={REDACTED}"), n.to_string()))
    } else {
        None
    }
}

/// 展示用命令行脱敏：逐 token 掩码；未命中时保留原文（不重排引号）。
fn mask_cmdline(cmd: &str) -> (String, bool) {
    let toks = split_cmdline(cmd);
    let mut changed = false;
    let mut masked = Vec::with_capacity(toks.len());
    for t in &toks {
        match redact_token(t) {
            Some((m, _)) => {
                changed = true;
                masked.push(m);
            }
            None => masked.push(t.clone()),
        }
    }
    if changed {
        (masked.join(" "), true)
    } else {
        (cmd.to_string(), false)
    }
}

// ---------------------------------------------------------------------------
// 草稿推导
// ---------------------------------------------------------------------------

fn strip_exe(name: &str) -> String {
    name.strip_suffix(".exe")
        .or_else(|| name.strip_suffix(".EXE"))
        .unwrap_or(name)
        .to_string()
}

fn file_name_of(p: &str) -> &str {
    p.rsplit(['/', '\\']).next().unwrap_or(p)
}

fn is_absolute_path(p: &str) -> bool {
    p.starts_with('/') || p.starts_with('\\') || {
        let b = p.as_bytes();
        b.len() >= 2 && b[1] == b':'
    }
}

/// argv[0] → generic `program`：
/// - 工作区内相对 dir 的路径：保留（转 `/`），启动无需 PATH；
/// - 工作区内但不在 dir 下 / 工作区外 / 相对路径歧义：退回裸程序名（PATH 解析），警告说明；
/// - 无命令行：退回进程名（去 .exe），强警告。
fn derive_program(
    argv0: Option<&str>,
    process_name: &str,
    root: &Path,
    dir_rel: Option<&str>,
    warnings: &mut Vec<String>,
) -> String {
    let Some(raw) = argv0.map(str::trim).filter(|s| !s.is_empty()) else {
        warnings.push(
            "未取得命令行参数：草稿仅含程序名，原启动参数未知，启动行为可能与原进程不同".into(),
        );
        return strip_exe(process_name);
    };
    if !raw.contains('/') && !raw.contains('\\') {
        return raw.to_string();
    }
    let name = strip_exe(file_name_of(raw));
    if !is_absolute_path(raw) {
        warnings.push(format!(
            "命令行程序 {raw:?} 为相对路径，无法可靠还原原 cwd，已取程序名 {name:?}（启动时按 PATH 解析）"
        ));
        return name;
    }
    match rel_under_root(raw, root) {
        Some(rel) => {
            let rel = rel.replace('\\', "/");
            match dir_rel {
                None | Some(".") => rel,
                Some(d) if d != "." && rel.starts_with(&format!("{d}/")) => {
                    rel[d.len() + 1..].to_string()
                }
                _ => {
                    warnings.push(format!(
                        "程序 {name:?} 在工作区内但不在服务目录下，已改用程序名（启动时按 PATH 解析）"
                    ));
                    name
                }
            }
        }
        None => {
            warnings.push(format!(
                "原程序为工作区外绝对路径 {raw:?}，已取程序名 {name:?}（启动时按 PATH 解析，缺失将报缺失工具错误）"
            ));
            name
        }
    }
}

fn derive_id(cwd_rel: Option<&str>, process_name: &str) -> String {
    let raw = match cwd_rel {
        Some(rel) if rel != "." => file_name_of(rel),
        _ => process_name,
    };
    let raw = strip_exe(raw);
    // 先截断再清洗，保证长目录名仍能产出合法 id（sanitize 失败会退化成 "svc"）
    let truncated: String = raw.chars().take(40).collect();
    sanitize_id(&truncated)
}

/// 从单个发现进程构建草稿项。cwd_rel 由调用方按包含规则算好。
#[allow(clippy::too_many_arguments)]
fn build_item(
    fsvc: &ForeignService,
    parent_name: Option<String>,
    cwd_rel: Option<String>,
    root: &Path,
    current: &SuperTaskFile,
    used_ids: &mut Vec<String>,
    used_ports: &mut BTreeSet<(u16, String)>,
) -> AdoptItem {
    let mut warnings: Vec<String> = Vec::new();

    // 父进程也在监听列表：提示确认端口归属（常见于壳进程 + 子进程双监听）。
    if let (Some(ppid), Some(pname)) = (fsvc.parent_pid, parent_name.as_deref()) {
        warnings.push(format!(
            "父进程 {pname}（PID {ppid}）也在监听进程列表中，请确认纳管的是持有目标端口的进程"
        ));
    }

    let (cmd_display, cmd_masked) = match &fsvc.cmd_line {
        Some(cmd) => {
            let (m, changed) = mask_cmdline(cmd);
            (Some(m), changed)
        }
        None => (None, false),
    };
    if cmd_masked {
        warnings
            .push("命令行含疑似敏感参数，预览与草稿中已脱敏，启动前请补填或改用 env_file".into());
    }

    let argv = fsvc.cmd_line.as_deref().map(split_cmdline);
    let dir_rel = cwd_rel.clone().filter(|r| r != ".");
    let port = fsvc.ports.first().copied();

    // 命令行其余参数 → args（脱敏）
    let mut args: Vec<String> = Vec::new();
    if let Some(argv) = &argv {
        for a in argv.iter().skip(1) {
            match redact_token(a) {
                Some((masked, name)) => {
                    let w = format!(
                        "参数 {name} 含疑似敏感值，已脱敏为 {REDACTED}，启动前请在配置页补填或改用 env_file"
                    );
                    if !warnings.contains(&w) {
                        warnings.push(w);
                    }
                    args.push(masked);
                }
                None => args.push(a.clone()),
            }
        }
    }

    let program = derive_program(
        argv.as_ref().and_then(|a| a.first()).map(String::as_str),
        &fsvc.name,
        root,
        dir_rel.as_deref(),
        &mut warnings,
    );

    // id：目录名优先，进程名兜底；与现有服务冲突 → IdConflict + 候选 id
    let base = derive_id(cwd_rel.as_deref(), &fsvc.name);
    let id_conflict = current.services.contains_key(&base);
    let final_id = {
        let candidate = unique_id(&base, &current.services);
        // unique_id 只避开 current；再避开本次预览内已分配的 id
        if !used_ids.iter().any(|u| u == &candidate) {
            candidate
        } else {
            let mut i = 2;
            loop {
                let c = format!("{base}-{i}");
                if !current.services.contains_key(&c) && !used_ids.iter().any(|u| u == &c) {
                    break c;
                }
                i += 1;
            }
        }
    };
    used_ids.push(final_id.clone());

    // 端口：首个监听端口写入 port；与本次预览内其他草稿撞端口 → 警告 + 默认不勾
    let mut port_conflict_with: Option<String> = None;
    if let Some(p) = port {
        if let Some((_, other)) = used_ports.iter().find(|(used, _)| *used == p) {
            port_conflict_with = Some(other.clone());
        } else {
            used_ports.insert((p, final_id.clone()));
        }
    }
    if fsvc.ports.len() > 1 {
        let rest: Vec<String> = fsvc.ports[1..].iter().map(|p| p.to_string()).collect();
        warnings.push(format!(
            "该进程还监听 {}：仅 {} 写入 port，其余端口请保存后在配置页手工补 ports/health",
            rest.join("、"),
            fsvc.ports[0]
        ));
    }
    if let Some(other) = &port_conflict_with {
        warnings.push(format!(
            "端口 {} 与候选 {other:?} 冲突，同时添加将无法通过校验（端口重复），请二选一",
            port.unwrap_or(0)
        ));
    }

    let mut labels = IndexMap::new();
    labels.insert(ORIGIN_LABEL.to_string(), ORIGIN_ADOPTED.to_string());
    labels.insert(
        ADOPTED_FROM_LABEL.to_string(),
        format!("pid {} ({})", fsvc.pid, fsvc.name),
    );

    let draft = ServiceSpec {
        kind: "generic".into(),
        service: None,
        enabled: true,
        group: None,
        labels,
        port,
        ports: Vec::new(),
        env: IndexMap::new(),
        env_file: Vec::new(),
        depends_on: Vec::new(),
        depends_on_ex: None,
        grace_secs: None,
        health: None,
        restart: None,
        max_retries: None,
        extra_args: Vec::new(),
        build_args: Vec::new(),
        cwd: None,
        launch: None,
        module: None,
        build_tool: None,
        jvm_args: Vec::new(),
        dir: dir_rel.clone(),
        package_manager: None,
        script: None,
        entry: None,
        package: None,
        program: Some(program),
        args,
        logging: None,
        resources: None,
        extra: IndexMap::new(),
    };

    let selected = !(id_conflict || !warnings.is_empty());
    AdoptItem {
        pid: fsvc.pid,
        process_name: fsvc.name.clone(),
        process_kind: fsvc.kind.clone(),
        ports: fsvc.ports.clone(),
        cmd_line: cmd_display,
        cwd: fsvc.cwd.clone(),
        parent_pid: fsvc.parent_pid,
        parent_name,
        status: if id_conflict {
            AdoptStatus::IdConflict
        } else {
            AdoptStatus::Adoptable
        },
        reason: id_conflict
            .then(|| format!("服务 id {base:?} 已存在，将以候选 id 添加（或改用现有服务）")),
        service_id: if id_conflict { base } else { final_id.clone() },
        candidate_id: id_conflict.then_some(final_id),
        draft: Some(draft),
        warnings,
        selected,
    }
}

/// dry-run 预览：纯内存、确定性（同输入两次调用结果一致）。
///
/// 包含规则（其余进程计入 warnings 的「未列入」计数，不展示明细）：
/// ① cwd 在工作区内；② 命令行命中工作区根但 cwd 不可用；③ 端口被现有服务声明。
pub fn preview(current: &SuperTaskFile, root: &Path, procs: &[ForeignService]) -> AdoptPreview {
    let mut items: Vec<AdoptItem> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut excluded = 0usize;

    // 现有服务声明的端口（port + ports）→ id
    let mut declared: Vec<(u16, &str)> = Vec::new();
    for (id, svc) in &current.services {
        if let Some(p) = svc.port {
            declared.push((p, id));
        }
        for p in &svc.ports {
            declared.push((*p, id));
        }
    }

    let mut used_ids: Vec<String> = Vec::new();
    let mut used_ports: BTreeSet<(u16, String)> = BTreeSet::new();
    let by_pid: std::collections::HashMap<u32, &ForeignService> =
        procs.iter().map(|p| (p.pid, p)).collect();

    let mut sorted: Vec<&ForeignService> = procs.iter().collect();
    sorted.sort_by_key(|p| p.pid);

    for fsvc in sorted {
        let cwd_rel = fsvc.cwd.as_deref().and_then(|c| rel_under_root(c, root));
        let port_hit = fsvc.ports.iter().find_map(|p| {
            declared
                .iter()
                .find(|(dp, _)| dp == p)
                .map(|(_, id)| (*id, *p))
        });
        let parent_name = fsvc
            .parent_pid
            .and_then(|pp| by_pid.get(&pp))
            .map(|p| p.name.clone());

        let item = if let Some((owner, port)) = port_hit {
            let (status, reason) = if cwd_rel.is_some() {
                (
                    AdoptStatus::Matched,
                    format!(
                        "监听端口 {port} 已由服务 {owner:?} 声明（外部实例在打开工作区时自动识别，无需纳管）"
                    ),
                )
            } else {
                (
                    AdoptStatus::Unadoptable,
                    format!(
                        "端口 {port} 已被服务 {owner:?} 声明，且进程工作目录不在工作区内——可能为该服务的外部实例（如 compose 宿主进程），也可能是端口冲突"
                    ),
                )
            };
            AdoptItem {
                pid: fsvc.pid,
                process_name: fsvc.name.clone(),
                process_kind: fsvc.kind.clone(),
                ports: fsvc.ports.clone(),
                cmd_line: fsvc.cmd_line.as_deref().map(|c| mask_cmdline(c).0),
                cwd: fsvc.cwd.clone(),
                parent_pid: fsvc.parent_pid,
                parent_name,
                status,
                reason: Some(reason),
                service_id: owner.to_string(),
                candidate_id: None,
                draft: None,
                warnings: Vec::new(),
                selected: false,
            }
        } else if let Some(rel) = cwd_rel {
            build_item(
                fsvc,
                parent_name,
                Some(rel),
                root,
                current,
                &mut used_ids,
                &mut used_ports,
            )
        } else if cmdline_hits_root(fsvc.cmd_line.as_deref(), root) {
            AdoptItem {
                pid: fsvc.pid,
                process_name: fsvc.name.clone(),
                process_kind: fsvc.kind.clone(),
                ports: fsvc.ports.clone(),
                cmd_line: fsvc.cmd_line.as_deref().map(|c| mask_cmdline(c).0),
                cwd: fsvc.cwd.clone(),
                parent_pid: fsvc.parent_pid,
                parent_name,
                status: AdoptStatus::Unadoptable,
                reason: Some(
                    "命令行指向工作区，但无法读取进程工作目录（权限受限或已退出），无法确定服务目录"
                        .into(),
                ),
                service_id: derive_id(None, &fsvc.name),
                candidate_id: None,
                draft: None,
                warnings: Vec::new(),
                selected: false,
            }
        } else {
            excluded += 1;
            continue;
        };
        items.push(item);
    }

    if excluded > 0 {
        warnings.push(format!(
            "另有 {excluded} 个监听进程与当前工作区无关（工作目录在工作区外且端口无交集），未列入"
        ));
    }
    if items
        .iter()
        .any(|i| matches!(i.status, AdoptStatus::Adoptable | AdoptStatus::IdConflict))
    {
        warnings.push(
            "运行中进程的环境变量无法读取：纳管后服务将以工作区环境启动，需要的变量请用 env 补齐"
                .into(),
        );
    }

    AdoptPreview { items, warnings }
}

/// 按用户确认合并草稿。与 Taskfile 导入同一机制：**apply 前用传入的最新发现
/// 快照重算预览**（预览到确认之间退出的进程按警告跳过，不报错），只新增所选
/// 服务，其余字段不动；写盘由调用方走 `yaml.saveForm`。
pub fn apply(
    current: &SuperTaskFile,
    root: &Path,
    procs: &[ForeignService],
    choices: &[AdoptChoice],
) -> Result<(SuperTaskFile, Vec<String>)> {
    let pv = preview(current, root, procs);
    let mut out = current.clone();
    let mut warnings: Vec<String> = Vec::new();
    let mut applied = 0usize;

    for choice in choices {
        if !matches!(choice.action, AdoptAction::Add) {
            continue;
        }
        let Some(item) = pv.items.iter().find(|i| i.pid == choice.pid) else {
            warnings.push(format!(
                "进程 {} 已退出或不在当前发现列表中，已跳过",
                choice.pid
            ));
            continue;
        };
        match item.status {
            AdoptStatus::Unadoptable => {
                let reason = item.reason.as_deref().unwrap_or("原因未知");
                warnings.push(format!("进程 {} 无法纳入：{reason}", item.pid));
            }
            AdoptStatus::Matched => {
                warnings.push(format!(
                    "进程 {} 的端口已由服务 {:?} 声明，未重复添加",
                    item.pid, item.service_id
                ));
            }
            AdoptStatus::Adoptable | AdoptStatus::IdConflict => {
                let key = item
                    .candidate_id
                    .clone()
                    .unwrap_or_else(|| item.service_id.clone());
                if out.services.contains_key(&key) {
                    warnings.push(format!(
                        "服务 {key} 已存在，跳过进程 {}（如需覆盖请在配置页操作）",
                        item.pid
                    ));
                    continue;
                }
                let spec = item
                    .draft
                    .clone()
                    .expect("adoptable / id_conflict 项必带草稿");
                out.services.insert(key.clone(), spec);
                applied += 1;
                warnings.push(format!(
                    "已添加服务 {key}（来源：进程 {} {}）",
                    item.process_name, item.pid
                ));
            }
        }
    }
    if applied == 0 {
        warnings.push("未添加任何服务".into());
    }
    Ok((out, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(
        pid: u32,
        name: &str,
        kind: &str,
        ports: Vec<u16>,
        cwd: Option<&str>,
        cmd: Option<&str>,
    ) -> ForeignService {
        ForeignService {
            pid,
            name: name.into(),
            kind: kind.into(),
            ports,
            cwd: cwd.map(str::to_string),
            cmd_line: cmd.map(str::to_string),
            parent_pid: None,
            cpu_percent: None,
            memory_bytes: None,
        }
    }

    fn empty_svc(kind: &str) -> ServiceSpec {
        ServiceSpec {
            kind: kind.into(),
            service: None,
            enabled: true,
            group: None,
            labels: IndexMap::new(),
            port: None,
            ports: Vec::new(),
            env: IndexMap::new(),
            env_file: Vec::new(),
            depends_on: Vec::new(),
            depends_on_ex: None,
            grace_secs: None,
            health: None,
            restart: None,
            max_retries: None,
            extra_args: Vec::new(),
            build_args: Vec::new(),
            cwd: None,
            launch: None,
            module: None,
            build_tool: None,
            jvm_args: Vec::new(),
            dir: None,
            package_manager: None,
            script: None,
            entry: None,
            package: None,
            program: None,
            args: Vec::new(),
            logging: None,
            resources: None,
            extra: IndexMap::new(),
        }
    }

    fn base_file(services: IndexMap<String, ServiceSpec>) -> SuperTaskFile {
        SuperTaskFile {
            version: 1,
            kind: None,
            name: None,
            description: None,
            root: ".".into(),
            env: IndexMap::new(),
            services,
            scripts: IndexMap::new(),
            logging: None,
            secrets: None,
            profiles: None,
            toolchain: None,
            needs: None,
            data: None,
            network: None,
            log_retention: None,
            templates: None,
            git: None,
            docker: None,
            gateway: None,
            cloud: None,
            ai: None,
            extra: IndexMap::new(),
        }
    }

    fn empty_file() -> SuperTaskFile {
        base_file(IndexMap::new())
    }

    #[cfg(windows)]
    const ROOT: &str = "C:\\ws\\demo";
    #[cfg(not(windows))]
    const ROOT: &str = "/tmp/ws/demo";
    #[cfg(windows)]
    const SUB: &str = "C:\\ws\\demo\\services\\api";
    #[cfg(not(windows))]
    const SUB: &str = "/tmp/ws/demo/services/api";

    #[test]
    fn draft_is_generic_with_program_args_port_and_dir() {
        let procs = vec![proc(
            101,
            "node.exe",
            "node",
            vec![3000],
            Some(SUB),
            Some("node server.js --port 3000"),
        )];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        assert_eq!(pv.items.len(), 1);
        let it = &pv.items[0];
        assert_eq!(it.status, AdoptStatus::Adoptable);
        assert!(it.selected);
        assert_eq!(it.service_id, "api");
        let d = it.draft.as_ref().unwrap();
        assert_eq!(d.kind, "generic");
        assert_eq!(d.program.as_deref(), Some("node"));
        assert_eq!(d.args, vec!["server.js", "--port", "3000"]);
        assert_eq!(d.port, Some(3000));
        assert_eq!(d.dir.as_deref(), Some("services/api"));
        assert_eq!(
            d.labels.get(ORIGIN_LABEL).map(String::as_str),
            Some(ORIGIN_ADOPTED)
        );
        assert!(d.labels.get(ADOPTED_FROM_LABEL).unwrap().contains("101"));
    }

    #[test]
    fn cwd_equal_root_gives_no_dir() {
        let procs = vec![proc(
            7,
            "app-server.exe",
            "other",
            vec![9000],
            Some(ROOT),
            Some("app-server --listen 9000"),
        )];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        let d = pv.items[0].draft.as_ref().unwrap();
        assert_eq!(d.dir, None, "cwd == root 时 dir 缺省即根目录");
        assert_eq!(d.program.as_deref(), Some("app-server"));
    }

    #[test]
    fn absolute_program_under_root_becomes_relative() {
        let exe = if cfg!(windows) {
            format!("{ROOT}\\target\\release\\app.exe")
        } else {
            format!("{ROOT}/target/release/app")
        };
        let cmd = format!("{exe} serve");
        let procs = vec![proc(
            9,
            "app.exe",
            "other",
            vec![8000],
            Some(ROOT),
            Some(&cmd),
        )];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        let d = pv.items[0].draft.as_ref().unwrap();
        let expected = if cfg!(windows) {
            "target/release/app.exe"
        } else {
            "target/release/app"
        };
        assert_eq!(d.program.as_deref(), Some(expected));
        assert!(
            pv.items[0].warnings.is_empty(),
            "无降级警告：{:?}",
            pv.items[0].warnings
        );
    }

    #[test]
    fn program_outside_root_falls_back_to_name_with_warning() {
        // 真实 Windows 命令行里带空格的绝对路径必须带引号（同时覆盖切词器）
        let cmd = if cfg!(windows) {
            r#""C:\Program Files\Java\jdk-17\bin\java.exe" -jar app.jar"#.to_string()
        } else {
            "/usr/lib/jvm/jdk-17/bin/java -jar app.jar".to_string()
        };
        let procs = vec![proc(
            11,
            "java.exe",
            "java",
            vec![8080],
            Some(ROOT),
            Some(&cmd),
        )];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        let it = &pv.items[0];
        let d = it.draft.as_ref().unwrap();
        assert_eq!(d.program.as_deref(), Some("java"));
        assert!(
            it.warnings.iter().any(|w| w.contains("PATH")),
            "{:?}",
            it.warnings
        );
    }

    #[test]
    fn sensitive_args_are_redacted_everywhere() {
        let cmd = "java -jar app.jar --db.password=hunter2 -Dapi.token=sk-abcd1234 --port 8080";
        let procs = vec![proc(
            12,
            "java.exe",
            "java",
            vec![8080],
            Some(ROOT),
            Some(cmd),
        )];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        let it = &pv.items[0];
        // 预览 JSON（IPC 返回值口径）不出现明文
        let json = serde_json::to_string(&pv).unwrap();
        assert!(!json.contains("hunter2"), "明文泄漏进预览: {json}");
        assert!(!json.contains("sk-abcd1234"));
        assert!(json.contains(REDACTED));
        let d = it.draft.as_ref().unwrap();
        assert!(
            d.args.contains(&format!("--db.password={REDACTED}")),
            "args: {:?}",
            d.args
        );
        assert!(d.args.contains(&format!("-Dapi.token={REDACTED}")));
        assert!(
            d.args.iter().any(|a| a == "--port"),
            "普通参数不受影响: {:?}",
            d.args
        );
        assert!(it.cmd_line.as_deref().unwrap().contains(REDACTED));
        assert!(it.warnings.iter().any(|w| w.contains("脱敏")));
    }

    #[test]
    fn missing_cmdline_yields_name_only_draft_unselected() {
        let procs = vec![proc(
            13,
            "mytool.exe",
            "other",
            vec![7777],
            Some(ROOT),
            None,
        )];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        let it = &pv.items[0];
        assert_eq!(it.status, AdoptStatus::Adoptable);
        assert!(!it.selected, "参数未知时默认不勾选");
        let d = it.draft.as_ref().unwrap();
        assert_eq!(d.program.as_deref(), Some("mytool"));
        assert!(d.args.is_empty());
        assert!(it.warnings.iter().any(|w| w.contains("未取得命令行")));
    }

    #[test]
    fn matched_when_port_declared_by_existing_service() {
        let mut services = IndexMap::new();
        let mut api = empty_svc("generic");
        api.port = Some(8080);
        services.insert("api".into(), api);
        let file = base_file(services);
        let procs = vec![proc(
            14,
            "java.exe",
            "java",
            vec![8080],
            Some(ROOT),
            Some("java -jar app.jar"),
        )];
        let pv = preview(&file, Path::new(ROOT), &procs);
        let it = &pv.items[0];
        assert_eq!(it.status, AdoptStatus::Matched);
        assert!(it.draft.is_none());
        assert!(!it.selected);
        assert!(it.reason.as_deref().unwrap().contains("api"));
    }

    #[test]
    fn unadoptable_when_port_declared_but_cwd_outside() {
        let mut services = IndexMap::new();
        let mut db = empty_svc("compose");
        db.port = Some(5432);
        services.insert("db".into(), db);
        let file = base_file(services);
        let cwd = if cfg!(windows) {
            r"C:\docker"
        } else {
            "/opt/docker"
        };
        let procs = vec![proc(
            15,
            "com.docker.backend.exe",
            "other",
            vec![5432],
            Some(cwd),
            None,
        )];
        let pv = preview(&file, Path::new(ROOT), &procs);
        let it = &pv.items[0];
        assert_eq!(it.status, AdoptStatus::Unadoptable);
        assert!(it.draft.is_none());
        assert!(it.reason.as_deref().unwrap().contains("db"));
    }

    #[test]
    fn outside_root_without_port_hit_is_excluded_with_count() {
        let cwd = if cfg!(windows) {
            r"C:\elsewhere"
        } else {
            "/opt/elsewhere"
        };
        let procs = vec![
            proc(16, "nginx.exe", "other", vec![80], Some(cwd), None),
            proc(
                17,
                "node.exe",
                "node",
                vec![3000],
                Some(ROOT),
                Some("node server.js"),
            ),
        ];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        assert_eq!(pv.items.len(), 1, "只展示与工作区相关的进程");
        assert!(pv.warnings.iter().any(|w| w.contains("1 个监听进程")));
    }

    #[test]
    fn cmdline_hits_root_but_no_cwd_is_unadoptable() {
        let cmd = if cfg!(windows) {
            format!("node {ROOT}\\server.js")
        } else {
            format!("node {ROOT}/server.js")
        };
        let procs = vec![proc(18, "node.exe", "node", vec![3000], None, Some(&cmd))];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        let it = &pv.items[0];
        assert_eq!(it.status, AdoptStatus::Unadoptable);
        assert!(it.draft.is_none());
        assert!(it.reason.as_deref().unwrap().contains("工作目录"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_cwd_case_insensitive_preserves_original_case() {
        let procs = vec![proc(
            19,
            "node.exe",
            "node",
            vec![3000],
            Some(r"C:\WS\DEMO\Services\API"),
            Some("node index.js"),
        )];
        let pv = preview(&empty_file(), Path::new("c:\\ws\\demo"), &procs);
        let d = pv.items[0].draft.as_ref().unwrap();
        assert_eq!(d.dir.as_deref(), Some("Services/API"), "保留原始大小写");
    }

    #[test]
    fn duplicate_derived_id_gets_suffix_or_conflict() {
        // 两个同目录 java 进程：第一个干净新增，第二个在预览内加后缀
        let procs = vec![
            proc(
                20,
                "java.exe",
                "java",
                vec![8081],
                Some(ROOT),
                Some("java -jar a.jar"),
            ),
            proc(
                21,
                "java.exe",
                "java",
                vec![8082],
                Some(ROOT),
                Some("java -jar b.jar"),
            ),
        ];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        assert_eq!(pv.items[0].service_id, "java");
        assert_eq!(pv.items[1].service_id, "java-2");
        assert_eq!(pv.items[1].status, AdoptStatus::Adoptable);

        // 与现有服务 id 冲突：IdConflict + 候选 id + 默认不勾
        let mut services = IndexMap::new();
        services.insert("java".into(), empty_svc("node"));
        let file = base_file(services);
        let procs = vec![proc(
            22,
            "java.exe",
            "java",
            vec![8083],
            Some(ROOT),
            Some("java -jar c.jar"),
        )];
        let pv = preview(&file, Path::new(ROOT), &procs);
        let it = &pv.items[0];
        assert_eq!(it.status, AdoptStatus::IdConflict);
        assert_eq!(it.service_id, "java");
        assert_eq!(it.candidate_id.as_deref(), Some("java-2"));
        assert!(!it.selected);
    }

    #[test]
    fn two_drafts_on_same_port_second_warns_and_unselected() {
        let procs = vec![
            proc(
                23,
                "a.exe",
                "other",
                vec![9000],
                Some(ROOT),
                Some("a --port 9000"),
            ),
            proc(
                24,
                "b.exe",
                "other",
                vec![9000],
                Some(SUB),
                Some("b --port 9000"),
            ),
        ];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        assert!(pv.items[0].selected);
        let second = &pv.items[1];
        assert!(!second.selected);
        assert!(second
            .warnings
            .iter()
            .any(|w| w.contains("端口 9000") && w.contains("冲突")));
    }

    #[test]
    fn parent_in_snapshot_is_reported_with_warning() {
        let mut parent = proc(
            40,
            "npm.exe",
            "node",
            vec![4002],
            Some(ROOT),
            Some("npm run dev"),
        );
        let mut child = proc(
            41,
            "node.exe",
            "node",
            vec![4003],
            Some(ROOT),
            Some("node srv.js"),
        );
        child.parent_pid = Some(40);
        parent.parent_pid = None;
        let pv = preview(&empty_file(), Path::new(ROOT), &[parent, child]);
        let c = pv.items.iter().find(|i| i.pid == 41).unwrap();
        assert_eq!(c.parent_pid, Some(40));
        assert_eq!(c.parent_name.as_deref(), Some("npm.exe"));
        assert!(c.warnings.iter().any(|w| w.contains("父进程")));
    }

    #[test]
    fn extra_listen_ports_warned_but_only_first_written() {
        let procs = vec![proc(
            50,
            "node.exe",
            "node",
            vec![5100, 5200, 5300],
            Some(ROOT),
            Some("node srv.js"),
        )];
        let pv = preview(&empty_file(), Path::new(ROOT), &procs);
        let d = pv.items[0].draft.as_ref().unwrap();
        assert_eq!(d.port, Some(5100));
        assert!(pv.items[0].warnings.iter().any(|w| w.contains("5200")));
        assert!(d.ports.is_empty(), "reserved ports 不由纳管填写");
    }

    #[test]
    fn apply_merges_selected_and_skips_gone_pids() {
        let procs = vec![
            proc(
                60,
                "node.exe",
                "node",
                vec![6000],
                Some(ROOT),
                Some("node a.js"),
            ),
            proc(
                61,
                "node.exe",
                "node",
                vec![6001],
                Some(SUB),
                Some("node b.js"),
            ),
        ];
        let choices = vec![
            AdoptChoice {
                pid: 60,
                action: AdoptAction::Add,
            },
            AdoptChoice {
                pid: 61,
                action: AdoptAction::Add,
            },
            AdoptChoice {
                pid: 999,
                action: AdoptAction::Add,
            },
            AdoptChoice {
                pid: 61,
                action: AdoptAction::Keep,
            },
        ];
        let (merged, warnings) = apply(&empty_file(), Path::new(ROOT), &procs, &choices).unwrap();
        assert_eq!(merged.services.len(), 2);
        assert!(merged.services.contains_key("node"));
        assert!(merged.services.contains_key("api"));
        assert!(warnings
            .iter()
            .any(|w| w.contains("999") && w.contains("跳过")));
    }

    #[test]
    fn apply_is_noop_for_matched_and_duplicate() {
        let mut services = IndexMap::new();
        let mut api = empty_svc("generic");
        api.port = Some(7000);
        services.insert("api".into(), api);
        let file = base_file(services);
        let procs = vec![
            proc(
                70,
                "java.exe",
                "java",
                vec![7000],
                Some(ROOT),
                Some("java -jar x.jar"),
            ), // matched
            proc(
                71,
                "node.exe",
                "node",
                vec![7001],
                Some(ROOT),
                Some("node s.js"),
            ),
        ];
        // 同一 pid 加两次：第二次撞 contains_key → 跳过警告
        let choices = vec![
            AdoptChoice {
                pid: 70,
                action: AdoptAction::Add,
            },
            AdoptChoice {
                pid: 71,
                action: AdoptAction::Add,
            },
            AdoptChoice {
                pid: 71,
                action: AdoptAction::Add,
            },
        ];
        let (merged, warnings) = apply(&file, Path::new(ROOT), &procs, &choices).unwrap();
        assert_eq!(merged.services.len(), 2, "只应新增 1 个服务");
        assert!(merged.services.contains_key("node"));
        assert!(warnings
            .iter()
            .any(|w| w.contains("api") && w.contains("声明")));
        assert!(warnings.iter().any(|w| w.contains("已存在")));
    }

    #[test]
    fn repreview_after_apply_is_idempotent() {
        let procs = vec![proc(
            80,
            "node.exe",
            "node",
            vec![8100],
            Some(ROOT),
            Some("node server.js"),
        )];
        let choices = vec![AdoptChoice {
            pid: 80,
            action: AdoptAction::Add,
        }];
        let (merged, _) = apply(&empty_file(), Path::new(ROOT), &procs, &choices).unwrap();
        // 纳管后再次预览：同一进程变成 matched（端口已声明），apply 再跑也不新增
        let pv2 = preview(&merged, Path::new(ROOT), &procs);
        assert_eq!(pv2.items[0].status, AdoptStatus::Matched);
        let (_, warnings) = apply(&merged, Path::new(ROOT), &procs, &choices).unwrap();
        assert!(warnings.iter().any(|w| w.contains("声明")));
    }

    #[test]
    fn merged_draft_passes_spec_validation_roundtrip() {
        let procs = vec![
            proc(
                90,
                "node.exe",
                "node",
                vec![9000],
                Some(SUB),
                Some("node server.js --db.password=hunter2"),
            ),
            proc(91, "app-server.exe", "other", vec![9001], Some(ROOT), None),
        ];
        let choices = vec![
            AdoptChoice {
                pid: 90,
                action: AdoptAction::Add,
            },
            AdoptChoice {
                pid: 91,
                action: AdoptAction::Add,
            },
        ];
        let (merged, _) = apply(&empty_file(), Path::new(ROOT), &procs, &choices).unwrap();
        let text = crate::spec::to_yaml(&merged).unwrap();
        let (_, parse_warnings) = crate::spec::parse_yaml(&text).unwrap();
        let hard = parse_warnings
            .iter()
            .filter(|w| {
                matches!(
                    w.code,
                    crate::error::ErrorCode::SpecInvalid | crate::error::ErrorCode::PathEscape
                )
            })
            .count();
        assert_eq!(hard, 0, "草稿不应产生硬校验错误: {parse_warnings:?}");
        // 脱敏占位写进 yaml，明文绝不落盘
        assert!(!text.contains("hunter2"));
    }

    #[test]
    fn preview_is_deterministic() {
        let procs = vec![
            proc(
                95,
                "node.exe",
                "node",
                vec![9500],
                Some(SUB),
                Some("node a.js"),
            ),
            proc(
                94,
                "node.exe",
                "node",
                vec![9400],
                Some(ROOT),
                Some("node b.js"),
            ),
        ];
        let a = preview(&empty_file(), Path::new(ROOT), &procs);
        let b = preview(&empty_file(), Path::new(ROOT), &procs);
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
        assert_eq!(a.items[0].pid, 94, "按 pid 升序，输入乱序不影响结果");
    }

    #[cfg(windows)]
    #[test]
    fn split_cmdline_handles_windows_quoting() {
        assert_eq!(
            split_cmdline(r#""C:\Program Files\app.exe" --flag "a b" esc\"quot"#),
            vec![r"C:\Program Files\app.exe", "--flag", "a b", r#"esc"quot"#]
        );
        assert_eq!(split_cmdline("  spaced\targs  "), vec!["spaced", "args"]);
        assert_eq!(split_cmdline(r#""""#), vec![""]);
    }

    #[cfg(not(windows))]
    #[test]
    fn split_cmdline_handles_unix_quoting() {
        assert_eq!(
            split_cmdline("node 'a b' \"c d\" plain"),
            vec!["node", "a b", "c d", "plain"]
        );
    }
}
