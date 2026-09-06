//! 方向三·E：归档供给执行器（ipc.md §10.17 增补）。
//!
//! 把 `needs` 的 `archive` 状态从「可供给性报告」变成可执行供给：
//! 下载官方发行包 → sha256 校验 → 解压到 app data 隔离目录 → PATH/解析可用。
//!
//! 设计约束（见 `docs/ROADMAP-NEXT-SLICES.md` 切片 E）：
//! - 确定性计划：相同（目录版本，平台）得到相同下载计划；
//! - 传输可注入：`ArchiveTransport` trait，测试用 `FakeTransport` 全离线；
//! - 安装目录不出沙箱：`<appdata>/SuperTask/archives/<id>/<version>/<platform>/`；
//! - 幂等可重试：`.complete` 标记 + sha256 命中的 `.part` 分片复用，中断后重跑收敛；
//! - 凭据/代理不进日志与事件：错误消息只含 URL 主机名，proxy userinfo 永不记录。
//!
//! 清单托管：**内置**（本文件 `ARCHIVE_DISTS`）。只有官方公布 sha256 的发行版才
//! 进入可执行目录——mysql 官方仅公布 MD5、postgres 无官方免安装包，两者保留
//! `ARCHIVE_CATALOG` 声明（`needs.rs`），执行器报 `ARCHIVE_UNAVAILABLE` 并说明原因。

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, ErrorCode, Result};

/// 单个下载包上限（mysql noinstall zip 约 240MB；上限 1GiB 防异常）。
pub const MAX_ARCHIVE_DOWNLOAD_BYTES: u64 = 1024 * 1024 * 1024;
/// 解压条目数上限（zip-bomb 防护第一层）。
pub const MAX_ARCHIVE_ENTRIES: usize = 100_000;
/// 解压总字节上限（zip-bomb 防护第二层）。
pub const MAX_ARCHIVE_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

/// 发行包形态：单文件（minio 可执行文件直存）或 zip（解压）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveKind {
    SingleFile,
    Zip,
}

/// 单平台发行版（url + sha256 必须来自官方公布渠道，见模块文档）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveDist {
    pub platform: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub kind: ArchiveKind,
    /// 落盘文件名（SingleFile 必填，如 `minio.exe`；Zip 为空串）。
    pub file_name: &'static str,
}

/// 可执行归档条目：版本钉死（与 `needs::ARCHIVE_CATALOG` 同 id/version 口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub id: &'static str,
    pub version: &'static str,
    /// 精确发行标识（写入安装 manifest，如 minio RELEASE…）。
    pub release: &'static str,
    pub dists: &'static [ArchiveDist],
    /// 安装根下 bin 相对目录（"." 或 "bin"）。
    pub bin_dir: &'static str,
    /// 校验用可执行名（不带 `.exe`，按平台补后缀）。
    pub executables: &'static [&'static str],
}

const MINIO_RELEASE: &str = "RELEASE.2024-12-18T13-15-44Z";

/// 可执行目录：只有官方公布 sha256 的发行版。mysql（官方仅 MD5）与 postgres
/// （无官方免安装包）不在此表，`plan` 对它们报 `ARCHIVE_UNAVAILABLE` 并说明门槛。
pub const ARCHIVE_DISTS: &[ArchiveEntry] = &[ArchiveEntry {
    id: "minio",
    version: "2024",
    release: MINIO_RELEASE,
    bin_dir: ".",
    executables: &["minio"],
    dists: &[
        ArchiveDist {
            platform: crate::needs::PLATFORM_WINDOWS_X64,
            url: "https://dl.min.io/server/minio/release/windows-amd64/archive/minio.RELEASE.2024-12-18T13-15-44Z",
            sha256: "5dd4fdc24e3f583f158bdf51671efdd11b4cc7a33d2bf47aa178403d9e5c03a8",
            kind: ArchiveKind::SingleFile,
            file_name: "minio.exe",
        },
        ArchiveDist {
            platform: crate::needs::PLATFORM_LINUX_X64,
            url: "https://dl.min.io/server/minio/release/linux-amd64/archive/minio.RELEASE.2024-12-18T13-15-44Z",
            sha256: "88182336ab5793488f2caad54846056b330f0dcb5c488b60d52434beeebae3ca",
            kind: ArchiveKind::SingleFile,
            file_name: "minio",
        },
        ArchiveDist {
            platform: crate::needs::PLATFORM_LINUX_ARM64,
            url: "https://dl.min.io/server/minio/release/linux-arm64/archive/minio.RELEASE.2024-12-18T13-15-44Z",
            sha256: "455c9b3093b7c1e6153c85cfd745c7c401a92497346ba24dbe45775c7b3ab1cd",
            kind: ArchiveKind::SingleFile,
            file_name: "minio",
        },
        ArchiveDist {
            platform: crate::needs::PLATFORM_DARWIN_X64,
            url: "https://dl.min.io/server/minio/release/darwin-amd64/archive/minio.RELEASE.2024-12-18T13-15-44Z",
            sha256: "6262efafdde3c61387245385da58a9c314aa582effdc75f39099b847ea2caca0",
            kind: ArchiveKind::SingleFile,
            file_name: "minio",
        },
        ArchiveDist {
            platform: crate::needs::PLATFORM_DARWIN_ARM64,
            url: "https://dl.min.io/server/minio/release/darwin-arm64/archive/minio.RELEASE.2024-12-18T13-15-44Z",
            sha256: "af079f5c4e2cb855f8dd0c86eea57d1412c81c458bd7fcb8421bec34a4143fef",
            kind: ArchiveKind::SingleFile,
            file_name: "minio",
        },
    ],
}];

/// 确定性下载计划：相同（目录版本，平台，安装根）输入得到相同输出。
#[derive(Debug, Clone)]
pub struct ArchivePlan {
    pub id: String,
    pub version: String,
    pub release: String,
    pub url: String,
    pub sha256: String,
    pub kind: ArchiveKind,
    pub file_name: String,
    pub install_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub executables: Vec<String>,
}

/// 安装根目录：`<appdata>/SuperTask/archives`。
pub fn archives_root() -> PathBuf {
    crate::appdata::appdata_dir().join("archives")
}

/// 为 needs id + 版本要求做确定性计划。`version_req=None` 取目录最新版本；
/// 有要求时取满足前缀语义的最高版本。平台无构建 / 版本不满足 / 无可执行
/// 发行版 → `ARCHIVE_UNAVAILABLE`（附原因，不伪造计划）。
pub fn plan_with_root(
    root: &Path,
    id: &str,
    version_req: Option<&str>,
    platform: &str,
) -> Result<ArchivePlan> {
    let entry = ARCHIVE_DISTS.iter().find(|e| e.id == id).ok_or_else(|| {
        let why = if crate::needs::ARCHIVE_CATALOG.iter().any(|e| e.id == id) {
            "该中间件暂无官方公布 sha256 的免安装发行版（mysql 官方仅公布 MD5；\
             postgres 无官方免安装包），归档执行器拒绝无校验供给"
        } else {
            "未知归档 id"
        };
        Error::new(
            ErrorCode::ArchiveUnavailable,
            format!("归档 {id} 不可供给：{why}。"),
        )
    })?;
    // 版本门槛：目录版本须满足要求（前缀语义，与 needs 一致）
    if !crate::needs::version_matches(version_req, Some(entry.version)) {
        return Err(Error::new(
            ErrorCode::ArchiveUnavailable,
            format!(
                "归档 {id} 目录版本 {} 不满足要求 {}。",
                entry.version,
                version_req.unwrap_or("（无要求）")
            ),
        ));
    }
    let dist = entry
        .dists
        .iter()
        .find(|d| d.platform == platform)
        .ok_or_else(|| {
            let platforms: Vec<&str> = entry.dists.iter().map(|d| d.platform).collect();
            Error::new(
                ErrorCode::ArchiveUnavailable,
                format!(
                    "归档 {id} {} 暂无 {platform} 构建（支持：{}）。",
                    entry.version,
                    platforms.join("/")
                ),
            )
        })?;
    let install_dir = root.join(entry.id).join(entry.version).join(platform);
    let bin_dir = if entry.bin_dir == "." {
        install_dir.clone()
    } else {
        install_dir.join(entry.bin_dir)
    };
    let exe_suffix = if platform == crate::needs::PLATFORM_WINDOWS_X64 {
        ".exe"
    } else {
        ""
    };
    Ok(ArchivePlan {
        id: entry.id.to_string(),
        version: entry.version.to_string(),
        release: entry.release.to_string(),
        url: dist.url.to_string(),
        sha256: dist.sha256.to_string(),
        kind: dist.kind,
        file_name: dist.file_name.to_string(),
        install_dir,
        bin_dir,
        executables: entry
            .executables
            .iter()
            .map(|e| format!("{e}{exe_suffix}"))
            .collect(),
    })
}

/// 默认安装根（appdata）下的确定性计划。
pub fn plan(id: &str, version_req: Option<&str>, platform: &str) -> Result<ArchivePlan> {
    plan_with_root(&archives_root(), id, version_req, platform)
}

// ---------------------------------------------------------------------------
// 传输（可注入，测试全离线）
// ---------------------------------------------------------------------------

/// 下载传输：只做 GET 取字节，代理/超时由实现方处理，错误消息必须已脱敏
/// （只含主机名，绝不含 proxy userinfo 或 token）。
pub trait ArchiveTransport: Send + Sync {
    fn fetch(&self, url: &str) -> Result<Vec<u8>>;
}

/// 生产传输：ureq + rustls。代理取 HTTPS_PROXY → HTTP_PROXY → ALL_PROXY
/// （大小写均可）；凭据只进内存 header，永不进错误消息与日志。
pub struct UreqTransport {
    agent: ureq::Agent,
    proxy_desc: Option<String>,
}

impl UreqTransport {
    pub fn new(env: &IndexMap<String, String>) -> Self {
        let mut builder = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(120));
        let mut proxy_desc = None;
        if let Some(raw) = proxy_of(env) {
            match ureq::Proxy::new(raw.as_str()) {
                Ok(proxy) => {
                    builder = builder.proxy(proxy);
                    proxy_desc = Some(describe_proxy(&raw));
                }
                Err(_) => {}
            }
        }
        Self {
            agent: builder.build(),
            proxy_desc,
        }
    }
}

/// 从环境取代理地址（键大小写均可），只返回裸值（调用方负责脱敏描述）。
fn proxy_of(env: &IndexMap<String, String>) -> Option<String> {
    for key in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Some(v) = env.get(key).filter(|v| !v.trim().is_empty()) {
            return Some(v.clone());
        }
    }
    None
}

/// 代理描述（脱敏）：只保留 scheme + host + 端口，丢弃 userinfo 与 path。
fn describe_proxy(raw: &str) -> String {
    let (scheme, after_scheme) = match raw.split_once("://") {
        Some((s, rest)) => (s.to_string(), rest),
        None => ("proxy".to_string(), raw),
    };
    let host_port = after_scheme
        .split('@')
        .next_back()
        .unwrap_or(after_scheme)
        .split('/')
        .next()
        .unwrap_or(after_scheme);
    format!("{scheme}://{host_port}")
}

/// URL 主机名（脱敏：错误消息只用它，不用完整 URL）。
fn url_host(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
}

impl ArchiveTransport for UreqTransport {
    fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(Error::new(
                ErrorCode::ArchiveFetch,
                format!("拒绝非 http(s) 归档地址（主机 {}）。", url_host(url)),
            ));
        }
        let mut bytes = Vec::new();
        let resp = self.agent.get(url).call().map_err(|e| {
            Error::new(
                ErrorCode::ArchiveFetch,
                match e {
                    ureq::Error::Status(code, _) => {
                        format!("下载失败：主机 {} 返回 HTTP {code}。", url_host(url))
                    }
                    ureq::Error::Transport(_) => {
                        let via = self
                            .proxy_desc
                            .as_deref()
                            .map(|d| format!("（代理 {d}）"))
                            .unwrap_or_default();
                        format!("下载失败：主机 {} 不可达或超时{via}。", url_host(url))
                    }
                },
            )
        })?;
        resp.into_reader()
            .take(MAX_ARCHIVE_DOWNLOAD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| {
                Error::new(
                    ErrorCode::ArchiveFetch,
                    format!("下载失败：读取主机 {} 响应失败：{e}。", url_host(url)),
                )
            })?;
        if bytes.len() as u64 > MAX_ARCHIVE_DOWNLOAD_BYTES {
            return Err(Error::new(
                ErrorCode::ArchiveFetch,
                format!(
                    "下载失败：主机 {} 的包超过 {} 字节上限，拒绝写入。",
                    url_host(url),
                    MAX_ARCHIVE_DOWNLOAD_BYTES
                ),
            ));
        }
        Ok(bytes)
    }
}

// ---------------------------------------------------------------------------
// 安装（幂等可重试）
// ---------------------------------------------------------------------------

/// 已安装归档（`scan_installed` 扫描结果；needs 解析与 PATH 注入共用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstalledArchive {
    pub id: String,
    pub version: String,
    pub release: String,
    pub bin_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct InstallManifest {
    id: String,
    version: String,
    release: String,
    url: String,
    sha256: String,
    bin_dir: String,
    installed_at_ms: u64,
}

/// 安装收据（op 结果与 needs 刷新共用）。
#[derive(Debug, Clone, Serialize)]
pub struct InstallReceipt {
    pub id: String,
    pub version: String,
    pub release: String,
    pub bin_dir: PathBuf,
    /// 已安装且校验通过，直接复用（无下载）。
    pub reused: bool,
}

fn complete_marker(plan: &ArchivePlan) -> PathBuf {
    plan.install_dir.join(".complete")
}

fn part_path(plan: &ArchivePlan) -> PathBuf {
    plan.install_dir.with_extension("part")
}

fn stage_path(plan: &ArchivePlan) -> PathBuf {
    plan.install_dir.with_extension("stage")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex_of(h.finalize())
}

fn hex_of(digest: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(64);
    for b in digest.as_ref() {
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 可执行文件是否齐备（安装目录损坏/半截时为 false，触发重装）。
fn executables_present(plan: &ArchivePlan) -> bool {
    plan.executables
        .iter()
        .all(|e| plan.bin_dir.join(e).is_file())
}

/// 安装归档：`.complete` + 可执行文件齐备 → 直接复用；否则下载（`.part` 复用）
/// → sha256 → 暂存解压 → 校验 → 原子改名 → 标记。`report` 收进度文案（进 op）。
pub fn install(
    transport: &dyn ArchiveTransport,
    plan: &ArchivePlan,
    report: &dyn Fn(&str),
) -> Result<InstallReceipt> {
    if complete_marker(plan).is_file() && executables_present(plan) {
        return Ok(InstallReceipt {
            id: plan.id.clone(),
            version: plan.version.clone(),
            release: plan.release.clone(),
            bin_dir: plan.bin_dir.clone(),
            reused: true,
        });
    }
    if let Some(parent) = plan.install_dir.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            Error::new(
                ErrorCode::ArchiveExtract,
                format!("无法创建归档目录 {}: {e}", parent.display()),
            )
        })?;
    }
    // 下载：`.part` 的 sha256 命中即复用（上次下载完成但未及解压），否则重下。
    let bytes = match fs::read(part_path(plan)) {
        Ok(cached) if sha256_hex(&cached) == plan.sha256 => {
            report("复用已下载的分片（sha256 命中），跳过下载");
            cached
        }
        Ok(_) => {
            let _ = fs::remove_file(part_path(plan));
            fetch_and_stage(transport, plan, report)?
        }
        Err(_) => fetch_and_stage(transport, plan, report)?,
    };
    // 校验（part 复用路径同样必经此处）。
    if sha256_hex(&bytes) != plan.sha256 {
        let _ = fs::remove_file(part_path(plan));
        return Err(Error::new(
            ErrorCode::ArchiveHash,
            format!(
                "归档 {} 校验失败：sha256 与官方公布不一致（主机 {}），已删除分片，请重试。",
                plan.id,
                url_host(&plan.url)
            ),
        ));
    }
    report("校验通过，正在解压到隔离目录");
    // 暂存解压：stage 先清场，成功后改名（Windows 改名前删 stale 目标）。
    let stage = stage_path(plan);
    let _ = fs::remove_dir_all(&stage);
    fs::create_dir_all(&stage).map_err(|e| {
        Error::new(
            ErrorCode::ArchiveExtract,
            format!("无法创建暂存目录 {}: {e}", stage.display()),
        )
    })?;
    let extract_result = match plan.kind {
        ArchiveKind::SingleFile => stage_single(&stage, plan, &bytes),
        ArchiveKind::Zip => stage_zip(&stage, plan, &bytes),
    };
    if let Err(e) = extract_result {
        let _ = fs::remove_dir_all(&stage);
        return Err(e);
    }
    // manifest 落 stage，随改名一起生效
    let manifest = InstallManifest {
        id: plan.id.clone(),
        version: plan.version.clone(),
        release: plan.release.clone(),
        url: plan.url.clone(),
        sha256: plan.sha256.clone(),
        bin_dir: plan.bin_dir.to_string_lossy().into_owned(),
        installed_at_ms: now_ms(),
    };
    let manifest_text = serde_json::to_string(&manifest).map_err(|e| {
        Error::new(
            ErrorCode::ArchiveExtract,
            format!("安装清单序列化失败：{e}"),
        )
    })?;
    fs::write(stage.join("manifest.json"), manifest_text)
        .map_err(|e| Error::new(ErrorCode::ArchiveExtract, format!("安装清单写入失败：{e}")))?;
    let _ = fs::remove_dir_all(&plan.install_dir);
    fs::rename(&stage, &plan.install_dir).map_err(|e| {
        let _ = fs::remove_dir_all(&stage);
        Error::new(
            ErrorCode::ArchiveExtract,
            format!("安装目录落盘失败 {}: {e}", plan.install_dir.display()),
        )
    })?;
    fs::write(complete_marker(plan), b"")
        .map_err(|e| Error::new(ErrorCode::ArchiveExtract, format!("完成标记写入失败：{e}")))?;
    let _ = fs::remove_file(part_path(plan));
    report(&format!("归档 {} {} 安装完成", plan.id, plan.version));
    Ok(InstallReceipt {
        id: plan.id.clone(),
        version: plan.version.clone(),
        release: plan.release.clone(),
        bin_dir: plan.bin_dir.clone(),
        reused: false,
    })
}

fn fetch_and_stage(
    transport: &dyn ArchiveTransport,
    plan: &ArchivePlan,
    report: &dyn Fn(&str),
) -> Result<Vec<u8>> {
    report(&format!(
        "正在下载归档 {}（主机 {}）",
        plan.id,
        url_host(&plan.url)
    ));
    let bytes = transport.fetch(&plan.url)?;
    if bytes.len() as u64 > MAX_ARCHIVE_DOWNLOAD_BYTES {
        return Err(Error::new(
            ErrorCode::ArchiveFetch,
            format!(
                "下载失败：归档 {} 超过 {} 字节上限，拒绝写入。",
                plan.id, MAX_ARCHIVE_DOWNLOAD_BYTES
            ),
        ));
    }
    // 先落 part：此后任何中断都可由 sha256 复用/重下收敛
    fs::write(part_path(plan), &bytes)
        .map_err(|e| Error::new(ErrorCode::ArchiveExtract, format!("分片写入失败：{e}")))?;
    Ok(bytes)
}

/// 单文件形态：直接落盘到 stage 根（文件名即 file_name）。
fn stage_single(stage: &Path, plan: &ArchivePlan, bytes: &[u8]) -> Result<()> {
    if plan.file_name.is_empty() {
        return Err(Error::new(
            ErrorCode::ArchiveExtract,
            format!("归档 {} 未声明落盘文件名。", plan.id),
        ));
    }
    let dest = stage.join(&plan.file_name);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            Error::new(
                ErrorCode::ArchiveExtract,
                format!("无法创建目录 {}: {e}", parent.display()),
            )
        })?;
    }
    fs::write(&dest, bytes).map_err(|e| {
        Error::new(
            ErrorCode::ArchiveExtract,
            format!("写入失败 {}: {e}", dest.display()),
        )
    })?;
    verify_executables_in(stage, plan)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for exe in &plan.executables {
            let p = stage.join(exe);
            if p.is_file() {
                let mut perm = fs::metadata(&p)
                    .map_err(|e| {
                        Error::new(ErrorCode::ArchiveExtract, format!("读取权限失败：{e}"))
                    })?
                    .permissions();
                perm.set_mode(0o755);
                fs::set_permissions(&p, perm).map_err(|e| {
                    Error::new(ErrorCode::ArchiveExtract, format!("设置可执行位失败：{e}"))
                })?;
            }
        }
    }
    Ok(())
}

/// zip 形态：zip-slip 拒绝（`enclosed_name` 为 None 即拒）+ 条目数/总字节上限。
/// bin 校验锚定 stage 根（`bin_dir` 为安装根相对路径，stage 即未来安装根）。
fn stage_zip(stage: &Path, plan: &ArchivePlan, bytes: &[u8]) -> Result<()> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| {
        Error::new(
            ErrorCode::ArchiveExtract,
            format!("归档 {} 不是合法 zip：{e}。", plan.id),
        )
    })?;
    if zip.len() > MAX_ARCHIVE_ENTRIES {
        return Err(Error::new(
            ErrorCode::ArchiveExtract,
            format!(
                "归档 {} 条目 {} 超过上限 {MAX_ARCHIVE_ENTRIES}，拒绝解压。",
                plan.id,
                zip.len()
            ),
        ));
    }
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| {
            Error::new(
                ErrorCode::ArchiveExtract,
                format!("归档 {} 条目读取失败：{e}。", plan.id),
            )
        })?;
        let rel = entry.enclosed_name().ok_or_else(|| {
            Error::new(
                ErrorCode::ArchiveExtract,
                format!("归档 {} 含不安全路径，已拒绝解压。", plan.id),
            )
        })?;
        total = total.saturating_add(entry.size());
        if total > MAX_ARCHIVE_TOTAL_BYTES {
            return Err(Error::new(
                ErrorCode::ArchiveExtract,
                format!(
                    "归档 {} 解压超过 {MAX_ARCHIVE_TOTAL_BYTES} 字节上限，拒绝解压。",
                    plan.id
                ),
            ));
        }
        let dest = stage.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| {
                Error::new(
                    ErrorCode::ArchiveExtract,
                    format!("无法创建目录 {}: {e}", dest.display()),
                )
            })?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    Error::new(
                        ErrorCode::ArchiveExtract,
                        format!("无法创建目录 {}: {e}", parent.display()),
                    )
                })?;
            }
            let mut out = fs::File::create(&dest).map_err(|e| {
                Error::new(
                    ErrorCode::ArchiveExtract,
                    format!("写入失败 {}: {e}", dest.display()),
                )
            })?;
            std::io::copy(&mut entry, &mut out).map_err(|e| {
                Error::new(
                    ErrorCode::ArchiveExtract,
                    format!("解压写入失败 {}: {e}", dest.display()),
                )
            })?;
        }
    }
    verify_executables_in(stage, plan)?;
    Ok(())
}

/// 可执行文件齐备校验（缺失 = 包损坏/布局变更，拒绝落盘）。
fn verify_executables_in(stage: &Path, plan: &ArchivePlan) -> Result<()> {
    // bin_dir 是安装根相对路径；stage 即未来安装根
    let anchor = if plan.bin_dir == plan.install_dir {
        stage.to_path_buf()
    } else {
        let rel = plan
            .bin_dir
            .strip_prefix(&plan.install_dir)
            .unwrap_or(&plan.bin_dir);
        stage.join(rel)
    };
    let missing: Vec<&str> = plan
        .executables
        .iter()
        .filter(|e| !anchor.join(e).is_file())
        .map(|e| e.as_str())
        .collect();
    if !missing.is_empty() {
        return Err(Error::new(
            ErrorCode::ArchiveExtract,
            format!(
                "归档 {} 解压后缺少可执行文件 {}，拒绝落盘（包布局可能变更）。",
                plan.id,
                missing.join(", ")
            ),
        ));
    }
    Ok(())
}

/// 扫描已安装归档（读 `.complete` + manifest，损坏跳过不报错）。
/// needs 解析与 PATH 注入共用。
pub fn scan_installed(root: &Path) -> Vec<InstalledArchive> {
    let mut out = Vec::new();
    let Ok(ids) = fs::read_dir(root) else {
        return out;
    };
    for id_entry in ids.flatten() {
        let Ok(versions) = fs::read_dir(id_entry.path()) else {
            continue;
        };
        for ver_entry in versions.flatten() {
            let Ok(platforms) = fs::read_dir(ver_entry.path()) else {
                continue;
            };
            for pf_entry in platforms.flatten() {
                let dir = pf_entry.path();
                if !dir.join(".complete").is_file() {
                    continue;
                }
                let Ok(text) = fs::read_to_string(dir.join("manifest.json")) else {
                    continue;
                };
                let Ok(m): std::result::Result<InstallManifest, _> = serde_json::from_str(&text)
                else {
                    continue;
                };
                out.push(InstalledArchive {
                    id: m.id,
                    version: m.version,
                    release: m.release,
                    bin_dir: PathBuf::from(m.bin_dir),
                });
            }
        }
    }
    out.sort_by(|a, b| (&a.id, &a.version).cmp(&(&b.id, &b.version)));
    out
}

/// 默认安装根下的已安装归档。
pub fn installed() -> Vec<InstalledArchive> {
    scan_installed(&archives_root())
}

/// 默认安装根下的已安装 bin 目录（launcher PATH 注入用）。
pub fn installed_bins() -> Vec<PathBuf> {
    installed()
        .into_iter()
        .map(|a| a.bin_dir)
        .filter(|p| p.is_dir())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write as _;
    use std::sync::Mutex;

    /// 离线假传输：url → 字节（缺键即断网口径）；记录调用。
    pub struct FakeTransport {
        calls: Mutex<Vec<String>>,
        payloads: Mutex<HashMap<String, Vec<u8>>>,
    }

    impl FakeTransport {
        pub fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                payloads: Mutex::new(HashMap::new()),
            }
        }

        pub fn push(&self, url: &str, bytes: Vec<u8>) {
            self.payloads.lock().unwrap().insert(url.to_string(), bytes);
        }

        pub fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ArchiveTransport for FakeTransport {
        fn fetch(&self, url: &str) -> Result<Vec<u8>> {
            self.calls.lock().unwrap().push(url.to_string());
            self.payloads
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::ArchiveFetch,
                        format!("下载失败：主机 {} 不可达或超时。", url_host(url)),
                    )
                })
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-arch-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn minio_plan(root: &Path) -> ArchivePlan {
        plan_with_root(root, "minio", Some("2024"), crate::needs::platform_key()).unwrap()
    }

    /// 内存组一个单文件负载的“发行包”。
    fn single_bytes() -> Vec<u8> {
        b"fake-minio-binary".to_vec()
    }

    /// 内存组一个 zip 包（可执行 + 嵌套 dir/nested.txt，文件名按平台后缀）。
    fn zip_bytes() -> Vec<u8> {
        let exe = exe_name("ok");
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            w.start_file(&exe, opts).unwrap();
            w.write_all(b"exe").unwrap();
            w.start_file("dir/nested.txt", opts).unwrap();
            w.write_all(b"nested").unwrap();
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    /// 内存组恶意 zip（`../escape.txt`）。
    fn evil_zip_bytes() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            w.start_file("../escape.txt", opts).unwrap();
            w.write_all(b"evil").unwrap();
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    fn zip_plan(root: &Path, bytes: &[u8]) -> ArchivePlan {
        // 用内存包的真实 sha256 构造计划（目录数据不可伪造，测试走注入计划）
        let install_dir = root
            .join("ziptool")
            .join("1.0")
            .join(crate::needs::platform_key());
        ArchivePlan {
            id: "ziptool".into(),
            version: "1.0".into(),
            release: "test".into(),
            url: "https://example.invalid/ziptool.zip".into(),
            sha256: sha256_hex(bytes),
            kind: ArchiveKind::Zip,
            file_name: String::new(),
            install_dir: install_dir.clone(),
            bin_dir: install_dir,
            executables: vec![exe_name("ok")],
        }
    }

    fn exe_name(base: &str) -> String {
        if crate::needs::platform_key() == crate::needs::PLATFORM_WINDOWS_X64 {
            format!("{base}.exe")
        } else {
            base.to_string()
        }
    }

    #[test]
    fn plan_is_deterministic_and_pinned() {
        let root = temp_root("plan");
        let a = minio_plan(&root);
        let b = minio_plan(&root);
        assert_eq!(a.url, b.url);
        assert!(a.url.starts_with("https://dl.min.io/"), "{}", a.url);
        assert_eq!(a.sha256.len(), 64);
        assert!(a.install_dir.starts_with(&root), "安装目录不出沙箱");
        // 版本门槛：2025 不满足目录 2024
        let e =
            plan_with_root(&root, "minio", Some("2025"), crate::needs::platform_key()).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ArchiveUnavailable);
        // mysql：无可执行发行版（仅 MD5 门槛）
        let e =
            plan_with_root(&root, "mysql", Some("8.0"), crate::needs::platform_key()).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ArchiveUnavailable);
        assert!(e.message().contains("MD5"), "{}", e.message());
        // 未知 id
        let e = plan_with_root(&root, "nope", None, crate::needs::platform_key()).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ArchiveUnavailable);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn single_file_install_verify_reuse_and_resume() {
        let root = temp_root("single");
        let t = FakeTransport::new();
        let mut plan = minio_plan(&root);
        // 测试注入：payload 固定，sha256 按 payload 重算（目录值不可伪造输入）
        let payload = single_bytes();
        plan.sha256 = sha256_hex(&payload);
        plan.executables = vec![exe_name("minio")];
        plan.file_name = exe_name("minio");
        t.push(&plan.url, payload.clone());

        let r = install(&t, &plan, &|_| {}).unwrap();
        assert!(!r.reused);
        assert!(plan.bin_dir.join(exe_name("minio")).is_file());
        assert_eq!(t.calls().len(), 1);
        // 成功后 part 已清理（省磁盘），只留安装目录 + .complete
        assert!(!part_path(&plan).exists());

        // 幂等：已安装直接复用，不再下载
        let r2 = install(&t, &plan, &|_| {}).unwrap();
        assert!(r2.reused);
        assert_eq!(t.calls().len(), 1);

        // 中断恢复：删 .complete 并手写上次残留的 part（sha256 命中）→ 不重新下载
        fs::remove_file(complete_marker(&plan)).unwrap();
        fs::write(part_path(&plan), &payload).unwrap();
        let r3 = install(&t, &plan, &|_| {}).unwrap();
        assert!(!r3.reused);
        assert_eq!(t.calls().len(), 1, "part 命中不得重新下载");

        // 损坏恢复：part 被篡改 → 重新下载
        fs::remove_file(complete_marker(&plan)).unwrap();
        fs::write(part_path(&plan), b"corrupted").unwrap();
        let r4 = install(&t, &plan, &|_| {}).unwrap();
        assert!(!r4.reused);
        assert_eq!(t.calls().len(), 2);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn hash_mismatch_deletes_part_and_fails() {
        let root = temp_root("hash");
        let t = FakeTransport::new();
        let mut plan = minio_plan(&root);
        // 计划 sha256 与 payload 不一致 → ARCHIVE_HASH
        t.push(&plan.url, b"tampered".to_vec());
        plan.executables = vec![exe_name("minio")];
        plan.file_name = exe_name("minio");
        let e = install(&t, &plan, &|_| {}).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ArchiveHash);
        assert!(!part_path(&plan).exists(), "坏分片必须删除");
        assert!(!plan.install_dir.exists(), "失败不落半成品");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fetch_failure_is_sanitized() {
        let root = temp_root("fetch");
        let t = FakeTransport::new();
        let plan = minio_plan(&root);
        let e = install(&t, &plan, &|_| {}).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ArchiveFetch);
        assert!(e.message().contains("dl.min.io"), "{}", e.message());
        assert!(!e.message().contains("https://"), "{}", e.message());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn zip_install_guards_and_layout() {
        let root = temp_root("zip");
        let t = FakeTransport::new();
        let payload = zip_bytes();
        let plan = zip_plan(&root, &payload);
        t.push(&plan.url, payload);
        let r = install(&t, &plan, &|_| {}).unwrap();
        assert!(!r.reused);
        assert!(plan.bin_dir.join(exe_name("ok")).is_file());
        assert!(plan.install_dir.join("dir/nested.txt").is_file());
        // 恶意 zip：zip-slip 拒绝，不落盘
        let evil = evil_zip_bytes();
        let mut evil_plan = zip_plan(&root, &evil);
        evil_plan.id = "eviltool".into();
        evil_plan.install_dir = root
            .join("eviltool")
            .join("1.0")
            .join(crate::needs::platform_key());
        evil_plan.bin_dir = evil_plan.install_dir.clone();
        t.push(&evil_plan.url, evil);
        let e = install(&t, &evil_plan, &|_| {}).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ArchiveExtract);
        assert!(!evil_plan.install_dir.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_executable_rejected() {
        let root = temp_root("noexe");
        let t = FakeTransport::new();
        // zip 里没有 ok.exe → 布局校验拒绝
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            w.start_file("other.txt", opts).unwrap();
            w.write_all(b"x").unwrap();
            w.finish().unwrap();
        }
        let payload = buf.into_inner();
        let plan = zip_plan(&root, &payload);
        t.push(&plan.url, payload);
        let e = install(&t, &plan, &|_| {}).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ArchiveExtract);
        assert!(e.message().contains("ok"), "{}", e.message());
        assert!(!plan.install_dir.exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn scan_lists_completed_only() {
        let root = temp_root("scan");
        let t = FakeTransport::new();
        let payload = single_bytes();
        let mut plan = minio_plan(&root);
        plan.sha256 = sha256_hex(&payload);
        plan.executables = vec![exe_name("minio")];
        plan.file_name = exe_name("minio");
        t.push(&plan.url, payload);
        install(&t, &plan, &|_| {}).unwrap();
        // 半截目录（无 .complete）不计入
        fs::create_dir_all(root.join("junk").join("1").join("x")).unwrap();
        let found = scan_installed(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "minio");
        assert_eq!(found[0].bin_dir, plan.bin_dir);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn proxy_and_url_descriptions_stay_sanitized() {
        assert_eq!(
            describe_proxy("http://user:pass@proxy:8080/x"),
            "http://proxy:8080"
        );
        assert_eq!(describe_proxy("proxy:8080"), "proxy://proxy:8080");
        assert_eq!(url_host("https://dl.min.io/a/b"), "dl.min.io");
        assert_eq!(url_host("not a url"), "not a url");
        let env: IndexMap<String, String> =
            [("HTTP_PROXY".to_string(), "http://u:p@h:1".to_string())]
                .into_iter()
                .collect();
        // transport 构造不 panic；错误路径不回显凭据由 fetch_failure 覆盖
        let _ = UreqTransport::new(&env);
        assert_eq!(proxy_of(&env), Some("http://u:p@h:1".to_string()));
    }
}
