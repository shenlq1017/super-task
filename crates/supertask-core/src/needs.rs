//! 方向三·环境供给：声明式 `needs` 解析（resolve-only dry-run，ipc.md §10.17）。
//!
//! 工作区 YAML 顶层 `needs: ["node@20", "postgres@16"]` 声明高频工具/中间件需求；
//! 本模块把每条声明解析为四态之一，**纯只读、零副作用**（不安装、不下载、不写盘）：
//!
//! - `satisfied`     已存在：本机探测/安装枚举里有满足版本要求的安装；
//! - `installable`   可安装：已有供给来源（mise / winget，见 `toolchain::`）可补齐，
//!                    执行复用既有 `toolchain.install` 长操作链路；
//! - `archive`       可从归档供给：内置免安装归档目录有匹配版本（本切片仅声明
//!                    可供给性，下载/解压执行器是下一切片）；
//! - `unsatisfiable` 不可满足：`reason` 说明检查过什么、为什么不行、下一步做什么。
//!
//! 决策模型是纯函数：相同 (needs 声明, 探测结果, 平台) 输入两次调用结果完全一致，
//! 全部测试离线（探测数据手工构造，安装演示走 `FakeRunner`）。
//!
//! 供给语义（本切片明确的契约，spec 见 docs/spec/yaml.md §7.2）：
//! - 版本：`id` / `id@X` / `id@X.Y` / `id@X.Y.Z`（前缀匹配，非区间表达式）；
//! - 来源：satisfied 来自本机已有安装；installable 来自 mise/winget（机器级共享，
//!   安装目录由 provider 决定，不写工作区）；archive 来自内置目录（版本钉死）；
//! - 隔离：needs 只声明「要求」，不产生项目级隔离；两个工作区声明同一工具共用
//!   同一本机安装（项目级版本隔离是后续切片）；
//! - 失败回滚：resolve 无副作用；安装失败由 toolchain 链路只报错、不清场，
//!   YAML 与已有安装保持原样，重新 resolve 即回到 installable。

use std::collections::HashSet;

use serde::Serialize;

use crate::error::{Error, ErrorCode, Result};
use crate::probe::{ToolProbe, ToolchainProbeBundle};
use crate::toolchain::manifest;
use crate::toolchain::ToolKind;

/// needs 条目上限（`spec::file` 顶层段限额口径）。
pub const MAX_NEEDS: usize = 32;

// ---------------------------------------------------------------------------
// DTO（ipc.md §10.17，mirror frontend/src/ipc/protocol.ts）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedStatus {
    /// 已存在且满足版本要求。
    Satisfied,
    /// 已有供给来源（mise/winget）可补齐。
    Installable,
    /// 内置归档目录可供给（本切片仅报告，执行器未接入）。
    Archive,
    /// 不可满足，`reason` 说明原因与下一步。
    Unsatisfiable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeedItem {
    /// YAML 中的原始声明（如 `node@20`）。
    pub need: String,
    /// 解析出的需求 id（如 `node`）。
    pub id: String,
    /// 版本要求（无 `@` 时缺省）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_req: Option<String>,
    pub status: NeedStatus,
    /// satisfied：命中的安装版本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_version: Option<String>,
    /// satisfied：命中安装的可执行文件路径（PATH 命中）或安装根目录（枚举命中）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found_path: Option<String>,
    /// installable：建议供给器 `mise` | `winget`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via: Option<String>,
    /// installable：建议安装版本（无要求时为 manifest 默认钉扎）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_version: Option<String>,
    /// installable（winget）：manifest 包 ID，绝不由用户输入。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub winget_id: Option<String>,
    /// archive：目录可供给的版本。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archive_version: Option<String>,
    /// 结论解释：检查过什么、为什么是这个来源/为什么不行、下一步做什么。
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NeedsResolveOut {
    pub items: Vec<NeedItem>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// 声明解析
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedDecl {
    pub id: String,
    pub version_req: Option<String>,
}

/// 解析一条 needs 声明：`id` 或 `id@version-req`。
/// id：`^[a-z][a-z0-9_-]{0,31}$`；版本要求沿用工具链版本字符集（≤32），
/// 额外禁止 `lts` 别名与 `-` 前缀（防 argv 注入口径与 `toolchain::validate_version` 一致）。
pub fn parse_need(raw: &str) -> Result<NeedDecl> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(needs_invalid("需求声明不能为空"));
    }
    if s.len() > 64 {
        return Err(needs_invalid("需求声明过长（≤64 字符）"));
    }
    let (id, req) = match s.split_once('@') {
        None => (s, None),
        Some((id, req)) => (id, Some(req)),
    };
    if !is_valid_need_id(id) {
        return Err(needs_invalid(format!(
            "需求 id 非法：{id:?}（只允许小写字母开头的 a-z 0-9 - _，≤32 字符）"
        )));
    }
    if let Some(r) = req {
        if r.is_empty() {
            return Err(needs_invalid("@ 后缺少版本要求，如 node@20"));
        }
        if r.contains('@') {
            return Err(needs_invalid("版本要求中不允许再次出现 @"));
        }
        if r.eq_ignore_ascii_case("lts") {
            return Err(needs_invalid(
                "needs 请使用具体版本（如 node@20），不支持 lts 别名",
            ));
        }
        if r.starts_with('-') {
            return Err(needs_invalid("版本要求不能以 - 开头"));
        }
        if !crate::spec::validate::is_valid_toolchain_version(r) {
            return Err(needs_invalid(format!(
                "版本要求字符集非法：{r:?}（只允许数字、字母、. - _ +，≤32 字符）"
            )));
        }
    }
    Ok(NeedDecl {
        id: id.to_string(),
        version_req: req.map(str::to_string),
    })
}

fn needs_invalid(msg: impl Into<String>) -> Error {
    Error::new(ErrorCode::NeedsInvalid, msg.into())
}

fn is_valid_need_id(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    s.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

// ---------------------------------------------------------------------------
// 版本匹配（前缀语义）
// ---------------------------------------------------------------------------

/// 提取数值版本段：`v20.11.1` → [20,11,1]，`go1.23.1` → [1,23,1]，
/// `21.0.4+9` / `21.0.4-9` / `1.2.3rc1` → 截到首个非数字段。
/// 解析不出任何数值段（如 "installed but version unknown"）→ None。
fn parse_version_nums(v: &str) -> Option<Vec<u64>> {
    let s = v.trim();
    let s = s.strip_prefix('v').unwrap_or(s);
    let s = s.strip_prefix("go").unwrap_or(s);
    let mut out: Vec<u64> = Vec::new();
    for part in s.split('.') {
        let digits: String = part.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            break;
        }
        // parse 不会失败：digits 全为 ASCII 数字，长度受输入长度限制
        out.push(digits.parse().unwrap_or(u64::MAX));
        if digits.len() != part.len() {
            break;
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// `prefix` 是 `full` 的数值前缀：[20] ⊑ [20,11,1]。
fn nums_prefix(prefix: &[u64], full: &[u64]) -> bool {
    prefix.len() <= full.len() && prefix.iter().zip(full).all(|(a, b)| a == b)
}

/// 版本要求是否被已装版本满足：req 的数值段是 found 的前缀；
/// 无要求时「存在即满足」。版本未知（解析不出）视为不满足具体要求。
fn version_matches(req: Option<&str>, found: Option<&str>) -> bool {
    match (req, found) {
        (None, Some(_)) => true,
        (Some(_), None) | (None, None) => false,
        (Some(r), Some(f)) => match (parse_version_nums(r), parse_version_nums(f)) {
            (Some(fr), Some(ff)) => nums_prefix(&fr, &ff),
            _ => false,
        },
    }
}

// ---------------------------------------------------------------------------
// 内置免安装归档目录（子集声明，非包仓库）
// ---------------------------------------------------------------------------

/// 单条归档目录项：官方免安装（zip/单文件）发行版的版本与平台。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub id: &'static str,
    pub version: &'static str,
    pub platforms: &'static [&'static str],
}

pub const PLATFORM_WINDOWS_X64: &str = "windows-x64";
pub const PLATFORM_LINUX_X64: &str = "linux-x64";
pub const PLATFORM_LINUX_ARM64: &str = "linux-arm64";
pub const PLATFORM_DARWIN_X64: &str = "darwin-x64";
pub const PLATFORM_DARWIN_ARM64: &str = "darwin-arm64";

/// 当前运行平台的目录键（由编译目标决定，运行期恒定）。
pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => PLATFORM_WINDOWS_X64,
        ("linux", "x86_64") => PLATFORM_LINUX_X64,
        ("linux", "aarch64") => PLATFORM_LINUX_ARM64,
        ("macos", "x86_64") => PLATFORM_DARWIN_X64,
        ("macos", "aarch64") => PLATFORM_DARWIN_ARM64,
        _ => "unknown",
    }
}

/// 内置归档目录。按 id 分组内保持升序（解析取最后一个匹配 = 最高版本）。
/// 只声明可供给性；下载/解压执行器与 sha256 清单在归档供给切片落地。
pub const ARCHIVE_CATALOG: &[ArchiveEntry] = &[
    ArchiveEntry {
        id: "postgres",
        version: "16.4",
        platforms: &[
            PLATFORM_WINDOWS_X64,
            PLATFORM_LINUX_X64,
            PLATFORM_DARWIN_X64,
            PLATFORM_DARWIN_ARM64,
        ],
    },
    ArchiveEntry {
        id: "mysql",
        version: "8.0",
        platforms: &[
            PLATFORM_WINDOWS_X64,
            PLATFORM_LINUX_X64,
            PLATFORM_DARWIN_X64,
            PLATFORM_DARWIN_ARM64,
        ],
    },
    ArchiveEntry {
        id: "minio",
        version: "2024",
        platforms: &[
            PLATFORM_WINDOWS_X64,
            PLATFORM_LINUX_X64,
            PLATFORM_LINUX_ARM64,
            PLATFORM_DARWIN_X64,
            PLATFORM_DARWIN_ARM64,
        ],
    },
];

// ---------------------------------------------------------------------------
// 解析主流程
// ---------------------------------------------------------------------------

/// 对当前平台解析 needs 列表。`resolve` 是唯一生产入口；
/// `resolve_with` 显式传入平台键，供离线测试固定平台差异。
pub fn resolve(needs: &[String], bundle: &ToolchainProbeBundle) -> NeedsResolveOut {
    resolve_with(needs, bundle, platform_key())
}

pub fn resolve_with(
    needs: &[String],
    bundle: &ToolchainProbeBundle,
    platform: &str,
) -> NeedsResolveOut {
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for raw in needs {
        let key = raw.trim().to_string();
        if !seen.insert(key) {
            warnings.push(format!(
                "needs 中「{}」重复声明，已忽略重复项。",
                raw.trim()
            ));
            continue;
        }
        items.push(resolve_one(raw, bundle, platform));
    }
    NeedsResolveOut { items, warnings }
}

fn resolve_one(raw: &str, bundle: &ToolchainProbeBundle, platform: &str) -> NeedItem {
    let decl = match parse_need(raw) {
        Ok(d) => d,
        // validate() 在加载期已拦截非法项；resolve 防御性兜底，绝不 panic
        Err(e) => {
            return NeedItem {
                need: raw.to_string(),
                id: raw.trim().split('@').next().unwrap_or("").to_string(),
                version_req: None,
                status: NeedStatus::Unsatisfiable,
                found_version: None,
                found_path: None,
                via: None,
                install_version: None,
                winget_id: None,
                archive_version: None,
                reason: e.message().to_string(),
            };
        }
    };
    if let Some(kind) = ToolKind::parse(&decl.id) {
        resolve_tool(raw, decl, kind, bundle)
    } else if ARCHIVE_CATALOG.iter().any(|e| e.id == decl.id) {
        resolve_archive(raw, decl, platform)
    } else {
        let catalog_ids: Vec<&str> = ARCHIVE_CATALOG.iter().map(|e| e.id).collect();
        let reason = format!(
            "未知需求 id「{}」：当前支持语言工具 java/maven/node/npm/pnpm/yarn/bun/python/go，\
             归档目录 {}。",
            decl.id,
            catalog_ids.join("/")
        );
        NeedItem {
            need: raw.to_string(),
            id: decl.id,
            version_req: decl.version_req,
            status: NeedStatus::Unsatisfiable,
            found_version: None,
            found_path: None,
            via: None,
            install_version: None,
            winget_id: None,
            archive_version: None,
            reason,
        }
    }
}

fn probe_of(kind: ToolKind, tools: &crate::probe::ToolchainProbe) -> &ToolProbe {
    match kind {
        ToolKind::Java => &tools.java,
        ToolKind::Maven => &tools.maven,
        ToolKind::Node => &tools.node,
        ToolKind::Npm => &tools.npm,
        ToolKind::Pnpm => &tools.pnpm,
        ToolKind::Yarn => &tools.yarn,
        ToolKind::Bun => &tools.bun,
        ToolKind::Python => &tools.python,
        ToolKind::Go => &tools.go,
    }
}

fn resolve_tool(
    raw: &str,
    decl: NeedDecl,
    kind: ToolKind,
    bundle: &ToolchainProbeBundle,
) -> NeedItem {
    let id = kind.as_str();
    let probe = probe_of(kind, &bundle.tools);

    // 1) 已存在：PATH 命中优先，其次安装枚举（未激活也满足「已存在」）
    if probe.found && version_matches(decl.version_req.as_deref(), probe.version.as_deref()) {
        return NeedItem {
            need: raw.to_string(),
            id: id.to_string(),
            version_req: decl.version_req,
            status: NeedStatus::Satisfied,
            found_version: probe.version.clone(),
            found_path: probe.path.clone(),
            via: None,
            install_version: None,
            winget_id: None,
            archive_version: None,
            reason: format!(
                "本机 PATH 已检测到 {id} {}，满足 {}。",
                probe.version.as_deref().unwrap_or("（版本未知）"),
                raw.trim()
            ),
        };
    }
    let inst =
        bundle.tools.installs.iter().find(|i| {
            i.tool == kind && version_matches(decl.version_req.as_deref(), Some(&i.version))
        });
    if let Some(inst) = inst {
        let reason = if probe.found {
            format!(
                "本机已安装 {id} {}（{}），满足 {}；PATH 上的是 {}，\
                 启动服务前请确认版本解析顺序。",
                inst.version,
                inst.home,
                raw.trim(),
                probe.version.as_deref().unwrap_or("（版本未知）")
            )
        } else {
            format!(
                "本机已安装 {id} {}（{}），满足 {}；该安装未在 PATH 激活，\
                 启动服务前请确认 PATH。",
                inst.version,
                inst.home,
                raw.trim()
            )
        };
        return NeedItem {
            need: raw.to_string(),
            id: id.to_string(),
            version_req: decl.version_req,
            status: NeedStatus::Satisfied,
            found_version: Some(inst.version.clone()),
            found_path: Some(inst.home.clone()),
            via: None,
            install_version: None,
            winget_id: None,
            archive_version: None,
            reason,
        };
    }

    // 2) 已有供给来源：mise 优先（与 provider::select_manager 的 auto 顺序一致）
    let mismatch_prefix = if probe.found {
        format!(
            "已检测到 {id} {}，不满足 {}。",
            probe.version.as_deref().unwrap_or("（版本未知）"),
            raw.trim()
        )
    } else {
        format!("本机未发现满足 {} 的安装。", raw.trim())
    };
    let avail = bundle.managers;
    if avail.mise {
        let ver = decl
            .version_req
            .clone()
            .unwrap_or_else(|| manifest::default_version(kind).to_string());
        return NeedItem {
            need: raw.to_string(),
            id: id.to_string(),
            version_req: decl.version_req,
            status: NeedStatus::Installable,
            found_version: None,
            found_path: None,
            via: Some("mise".to_string()),
            install_version: Some(ver.clone()),
            winget_id: None,
            archive_version: None,
            reason: format!(
                "{mismatch_prefix}可用 mise 安装 {id}@{ver}\
                 （具体小版本由 mise 在安装时解析，执行复用工具链安装链路）。"
            ),
        };
    }
    if avail.winget {
        let target = decl
            .version_req
            .clone()
            .unwrap_or_else(|| manifest::default_version(kind).to_string());
        // 白名单按「目录版本 ⊑ 要求」匹配：要求 20.11.1 可由逻辑版本 20 的包满足
        let logical = manifest::winget_versions(kind).into_iter().find(|v| {
            match (parse_version_nums(v), parse_version_nums(&target)) {
                (Some(ev), Some(tr)) => nums_prefix(&ev, &tr),
                _ => false,
            }
        });
        // logical 来自白名单自身，winget_id 不会失败；一旦失败按「不在列表」口径收尾
        if let Some(logical) =
            logical.and_then(|v| manifest::winget_id(kind, v).map(|w| (v, w)).ok())
        {
            return NeedItem {
                need: raw.to_string(),
                id: id.to_string(),
                version_req: decl.version_req,
                status: NeedStatus::Installable,
                found_version: None,
                found_path: None,
                via: Some("winget".to_string()),
                install_version: Some(logical.0.to_string()),
                winget_id: Some(logical.1.to_string()),
                archive_version: None,
                reason: format!(
                    "{mismatch_prefix}可用 winget 安装 {id}\
                     （包 {}，逻辑版本 {}，安装包取该版本最新补丁）。",
                    logical.1, logical.0
                ),
            };
        }
        let supported = manifest::winget_versions(kind).join("/");
        return NeedItem {
            need: raw.to_string(),
            id: id.to_string(),
            version_req: decl.version_req,
            status: NeedStatus::Unsatisfiable,
            found_version: None,
            found_path: None,
            via: None,
            install_version: None,
            winget_id: None,
            archive_version: None,
            reason: format!(
                "{mismatch_prefix}winget 清单支持 {id}：{supported}，\
                 {target} 不在列表内；可安装 mise 以获得更宽的版本范围。"
            ),
        };
    }
    NeedItem {
        need: raw.to_string(),
        id: id.to_string(),
        version_req: decl.version_req,
        status: NeedStatus::Unsatisfiable,
        found_version: None,
        found_path: None,
        via: None,
        install_version: None,
        winget_id: None,
        archive_version: None,
        reason: format!(
            "{mismatch_prefix}且 mise / winget 均不可用；\
             请先安装 mise（推荐）或 winget，或手动安装后重新检查。"
        ),
    }
}

fn resolve_archive(raw: &str, decl: NeedDecl, platform: &str) -> NeedItem {
    let id = decl.id.as_str();
    let entries: Vec<&ArchiveEntry> = ARCHIVE_CATALOG.iter().filter(|e| e.id == id).collect();
    let matching: Vec<&&ArchiveEntry> = entries
        .iter()
        .filter(|e| version_matches(decl.version_req.as_deref(), Some(e.version)))
        .collect();
    // 目录按版本升序维护：最后一个匹配 = 最高满足版本
    if let Some(e) = matching.last() {
        let platforms = e.platforms.join("/");
        if e.platforms.contains(&platform) {
            return NeedItem {
                need: raw.to_string(),
                id: id.to_string(),
                version_req: decl.version_req,
                status: NeedStatus::Archive,
                found_version: None,
                found_path: None,
                via: None,
                install_version: None,
                winget_id: None,
                archive_version: Some(e.version.to_string()),
                reason: format!(
                    "可从免安装归档供给 {id} {}（{platform}）；\
                     归档下载/解压执行器尚未接入，本切片仅报告可供给性。",
                    e.version
                ),
            };
        }
        return NeedItem {
            need: raw.to_string(),
            id: id.to_string(),
            version_req: decl.version_req,
            status: NeedStatus::Unsatisfiable,
            found_version: None,
            found_path: None,
            via: None,
            install_version: None,
            winget_id: None,
            archive_version: None,
            reason: format!(
                "归档目录中 {id} {} 暂无 {platform} 构建（支持：{platforms}）。",
                e.version
            ),
        };
    }
    let platform_hit = entries.iter().any(|e| e.platforms.contains(&platform));
    let highest = entries.last().map(|e| e.version).unwrap_or("（目录为空）");
    let reason = if platform_hit {
        format!(
            "归档目录中 {id} 最高版本 {highest}，不满足 {}。",
            raw.trim()
        )
    } else {
        let platforms: Vec<&str> = entries
            .iter()
            .flat_map(|e| e.platforms.iter().copied())
            .collect();
        format!(
            "归档目录中 {id} 暂无 {platform} 构建（支持：{}），且最高版本 {highest} 不满足 {}。",
            platforms.join("/"),
            raw.trim()
        )
    };
    NeedItem {
        need: raw.to_string(),
        id: id.to_string(),
        version_req: decl.version_req,
        status: NeedStatus::Unsatisfiable,
        found_version: None,
        found_path: None,
        via: None,
        install_version: None,
        winget_id: None,
        archive_version: None,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::toolchain::discover::{DiscoveredInstall, InstallSource};
    use crate::toolchain::runner::FakeRunner;
    use crate::toolchain::{discover, ManagerAvailability};

    fn avail(mise: bool, winget: bool) -> ManagerAvailability {
        ManagerAvailability { mise, winget }
    }

    fn probe_of(kind: ToolKind, found: bool, version: Option<&str>) -> ToolProbe {
        ToolProbe {
            found,
            version: version.map(str::to_string),
            path: found.then(|| format!("C:\\fake\\bin\\{}.exe", kind.as_str())),
        }
    }

    fn bundle(
        node: ToolProbe,
        managers: ManagerAvailability,
        installs: Vec<DiscoveredInstall>,
    ) -> ToolchainProbeBundle {
        let mut tools = crate::probe::ToolchainProbe::default();
        tools.node = node;
        tools.installs = installs;
        ToolchainProbeBundle { tools, managers }
    }

    fn install(tool: ToolKind, version: &str, home: &str, active: bool) -> DiscoveredInstall {
        DiscoveredInstall {
            tool,
            version: version.to_string(),
            home: home.to_string(),
            source: InstallSource::Directory,
            active,
        }
    }

    // ---- 声明解析 ----

    #[test]
    fn parse_accepts_id_and_prefix_versions() {
        for (raw, id, req) in [
            ("node", "node", None),
            ("node@20", "node", Some("20")),
            ("node@20.11", "node", Some("20.11")),
            ("node@20.11.1", "node", Some("20.11.1")),
            ("postgres@16", "postgres", Some("16")),
            ("  node@20  ", "node", Some("20")),
        ] {
            let d = parse_need(raw).unwrap();
            assert_eq!(d.id, id, "{raw}");
            assert_eq!(d.version_req.as_deref(), req, "{raw}");
        }
    }

    #[test]
    fn parse_rejects_malformed_entries() {
        for bad in [
            "",
            "   ",
            "@20",
            "node@",
            "node@20@21",
            "Node@20",
            "1node@20",
            "no de@20",
            "node@lts",
            "node@LTS",
            "node@-20",
            "node@20;rm -rf /",
            "node@20 21",
        ] {
            let e = parse_need(bad).unwrap_err();
            assert_eq!(e.code(), ErrorCode::NeedsInvalid, "{bad}");
        }
    }

    #[test]
    fn parse_rejects_overlong_entries() {
        let long_id = format!("{}@20", "a".repeat(33));
        assert_eq!(
            parse_need(&long_id).unwrap_err().code(),
            ErrorCode::NeedsInvalid
        );
        let long_req = format!("node@{}", "1".repeat(33));
        assert_eq!(
            parse_need(&long_req).unwrap_err().code(),
            ErrorCode::NeedsInvalid
        );
    }

    // ---- 版本匹配 ----

    #[test]
    fn version_parse_handles_platform_quirks() {
        assert_eq!(parse_version_nums("v20.11.1"), Some(vec![20, 11, 1]));
        assert_eq!(parse_version_nums("go1.23.1"), Some(vec![1, 23, 1]));
        assert_eq!(parse_version_nums("21.0.4+9"), Some(vec![21, 0, 4]));
        assert_eq!(parse_version_nums("21.0.4-9"), Some(vec![21, 0, 4]));
        assert_eq!(parse_version_nums("3.9"), Some(vec![3, 9]));
        assert_eq!(parse_version_nums("2024-08-17"), Some(vec![2024]));
        assert_eq!(parse_version_nums("installed but version unknown"), None);
        assert_eq!(parse_version_nums(""), None);
    }

    #[test]
    fn version_match_prefix_semantics() {
        assert!(version_matches(Some("20"), Some("v20.11.1")));
        assert!(version_matches(Some("20.11"), Some("20.11.5")));
        assert!(version_matches(Some("20.11.1"), Some("20.11.1")));
        assert!(!version_matches(Some("20.12"), Some("20.11.5")));
        assert!(!version_matches(Some("21"), Some("20.11.5")));
        // 20 ≠ 2：按数值段比较，不是字符串前缀
        assert!(!version_matches(Some("2"), Some("20.11.5")));
        assert!(version_matches(None, Some(" anything ")));
        assert!(!version_matches(Some("20"), None));
        assert!(!version_matches(Some("20"), Some("unknown-version")));
    }

    // ---- satisfied：已存在不重复安装 ----

    #[test]
    fn satisfied_via_probe_when_version_matches() {
        let b = bundle(
            probe_of(ToolKind::Node, true, Some("v20.11.1")),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["node@20".into()], &b, "windows-x64");
        assert_eq!(out.items.len(), 1);
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Satisfied);
        assert_eq!(item.found_version.as_deref(), Some("v20.11.1"));
        assert!(item.via.is_none());
        assert!(item.reason.contains("PATH"), "{}", item.reason);
        // 无供给器也不影响「已存在」结论
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn satisfied_via_discovered_install_even_when_probe_has_other_version() {
        // PATH 上是 22，但 nvm 目录里有 20.18.1 → 已存在（不重复安装），并提示 PATH 指向
        let b = bundle(
            probe_of(ToolKind::Node, true, Some("v22.11.0")),
            avail(true, false),
            vec![install(
                ToolKind::Node,
                "20.18.1",
                "C:\\nvm\\v20.18.1",
                false,
            )],
        );
        let out = resolve_with(&["node@20".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Satisfied);
        assert_eq!(item.found_version.as_deref(), Some("20.18.1"));
        assert_eq!(item.found_path.as_deref(), Some("C:\\nvm\\v20.18.1"));
        assert!(item.reason.contains("PATH"), "{}", item.reason);
    }

    #[test]
    fn satisfied_without_version_req_means_any_install() {
        let b = bundle(
            probe_of(ToolKind::Node, true, Some("v22.11.0")),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["node".into()], &b, "windows-x64");
        assert_eq!(out.items[0].status, NeedStatus::Satisfied);
    }

    // ---- installable：已有供给来源 ----

    #[test]
    fn mismatch_with_mise_reports_installable_with_version() {
        let b = bundle(
            probe_of(ToolKind::Node, true, Some("v22.11.0")),
            avail(true, false),
            vec![],
        );
        let out = resolve_with(&["node@20".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Installable);
        assert_eq!(item.via.as_deref(), Some("mise"));
        assert_eq!(item.install_version.as_deref(), Some("20"));
        assert!(item.reason.contains("22.11.0"), "{}", item.reason);
    }

    #[test]
    fn no_req_uses_manifest_default_version() {
        let b = bundle(
            probe_of(ToolKind::Go, false, None),
            avail(true, true),
            vec![],
        );
        let out = resolve_with(&["go".into()], &b, "windows-x64");
        assert_eq!(out.items[0].install_version.as_deref(), Some("1.23"));
    }

    #[test]
    fn winget_only_maps_whitelist_and_package_id() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, true),
            vec![],
        );
        let out = resolve_with(&["node@20.11.1".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Installable);
        assert_eq!(item.via.as_deref(), Some("winget"));
        assert_eq!(item.install_version.as_deref(), Some("20"));
        assert_eq!(item.winget_id, Some("OpenJS.NodeJS.LTS".to_string()));
    }

    #[test]
    fn winget_only_rejects_out_of_whitelist_with_actionable_reason() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, true),
            vec![],
        );
        let out = resolve_with(&["node@19".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Unsatisfiable);
        assert!(item.reason.contains("mise"), "{}", item.reason);
        assert!(item.reason.contains("19"), "{}", item.reason);
    }

    #[test]
    fn no_manager_is_unsatisfiable_with_next_step() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["node@20".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Unsatisfiable);
        assert!(item.reason.contains("mise"), "{}", item.reason);
        assert!(item.reason.contains("winget"), "{}", item.reason);
    }

    // ---- archive：免安装归档目录 ----

    #[test]
    fn archive_entry_matches_version_and_platform() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["postgres@16".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Archive);
        assert_eq!(item.archive_version.as_deref(), Some("16.4"));
        assert!(item.reason.contains("windows-x64"), "{}", item.reason);
    }

    #[test]
    fn archive_without_req_takes_highest_version() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["minio".into()], &b, "linux-arm64");
        assert_eq!(out.items[0].status, NeedStatus::Archive);
        assert_eq!(out.items[0].archive_version.as_deref(), Some("2024"));
    }

    #[test]
    fn archive_platform_gap_is_explicit() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        // mysql 目录不含 linux-arm64
        let out = resolve_with(&["mysql@8".into()], &b, "linux-arm64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Unsatisfiable);
        assert!(item.reason.contains("linux-arm64"), "{}", item.reason);
    }

    #[test]
    fn archive_version_gap_names_highest_available() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["postgres@17".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Unsatisfiable);
        assert!(item.reason.contains("16.4"), "{}", item.reason);
    }

    // ---- unsatisfiable：未知 id / 兜底 ----

    #[test]
    fn unknown_id_lists_supported_ids() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["gradle@8".into()], &b, "windows-x64");
        let item = &out.items[0];
        assert_eq!(item.status, NeedStatus::Unsatisfiable);
        assert!(item.reason.contains("java/maven/node"), "{}", item.reason);
        assert!(item.reason.contains("postgres"), "{}", item.reason);
    }

    #[test]
    fn defensive_garbage_does_not_panic_and_reports_reason() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["Node@20".into()], &b, "windows-x64");
        assert_eq!(out.items[0].status, NeedStatus::Unsatisfiable);
    }

    // ---- 确定性与去重 ----

    #[test]
    fn same_input_yields_identical_results() {
        let b = bundle(
            probe_of(ToolKind::Node, true, Some("v22.11.0")),
            avail(true, false),
            vec![install(
                ToolKind::Node,
                "20.18.1",
                "C:\\nvm\\v20.18.1",
                false,
            )],
        );
        let needs = vec![
            "node@20".to_string(),
            "postgres@16".to_string(),
            "java@21".to_string(),
        ];
        let a = resolve_with(&needs, &b, "windows-x64");
        let c = resolve_with(&needs, &b, "windows-x64");
        assert_eq!(a, c);
        // 声明顺序保持
        assert_eq!(
            a.items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["node", "postgres", "java"]
        );
    }

    #[test]
    fn duplicates_warn_and_keep_first() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(true, false),
            vec![],
        );
        let out = resolve_with(&["node@20".into(), "node@20".into()], &b, "windows-x64");
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.warnings.len(), 1);
        assert!(out.warnings[0].contains("node@20"), "{}", out.warnings[0]);
    }

    #[test]
    fn empty_needs_yield_empty_plan() {
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve(&[], &b);
        assert!(out.items.is_empty());
        assert!(out.warnings.is_empty());
    }

    #[test]
    fn reasons_never_echo_environment_values() {
        // reason 只含 id/版本/路径类事实，不允许出现形如 KEY=VALUE 的环境回显
        let b = bundle(
            probe_of(ToolKind::Node, false, None),
            avail(false, false),
            vec![],
        );
        let out = resolve_with(&["node@20".into(), "db@1".into()], &b, "windows-x64");
        for item in &out.items {
            assert!(!item.reason.contains("PATH="), "{}", item.reason);
            assert!(!item.reason.contains("SECRET"), "{}", item.reason);
        }
    }

    // ---- 端到端演示：resolve → 既有安装链 → 重新 resolve ----

    #[test]
    fn e2e_node20_resolve_install_reresolve() {
        // 1) 初始：PATH 上 node 22 不满足 node@20，mise 可用 → 可安装
        let b1 = bundle(
            probe_of(ToolKind::Node, true, Some("v22.11.0")),
            avail(true, false),
            vec![],
        );
        let plan = resolve_with(&["node@20".into()], &b1, "windows-x64");
        let item = &plan.items[0];
        assert_eq!(item.status, NeedStatus::Installable);
        assert_eq!(item.via.as_deref(), Some("mise"));
        let version = item.install_version.clone().unwrap();

        // 2) 走既有 toolchain 安装链（FakeRunner 脚本，零真实网络/零真实 spawn）
        let fake = FakeRunner::new();
        fake.push_ok("mise 2024.1"); // mise --version
        fake.push_ok("winget v1.6"); // winget --version
        fake.push_ok("installed node@20"); // mise install node@20
        fake.push_ok("C:\\mise\\node\\20\\node.exe"); // mise which node
        let ws = std::path::PathBuf::from("C:/work/mall");
        let outcome = crate::toolchain::install(
            &fake,
            crate::toolchain::InstallRequest {
                tool: ToolKind::Node,
                version: &version,
                requested: None,
                workspace_manager: None,
                workspace: &ws,
                env: Default::default(),
                path_probe: |name| Some(std::path::PathBuf::from(name)),
            },
        )
        .unwrap();
        assert_eq!(outcome.manager, crate::toolchain::ProviderKind::Mise);
        let calls = fake.calls();
        assert_eq!(
            calls[2].args,
            vec!["install".to_string(), "node@20".to_string()]
        );

        // 3) 安装成功后的新探测 → satisfied（已存在不重复安装）
        let b2 = bundle(
            probe_of(ToolKind::Node, true, Some("v20.18.1")),
            avail(true, false),
            vec![discover::DiscoveredInstall {
                tool: ToolKind::Node,
                version: "20.18.1".to_string(),
                home: "C:\\mise\\node\\20".to_string(),
                source: InstallSource::Directory,
                active: true,
            }],
        );
        let plan2 = resolve_with(&["node@20".into()], &b2, "windows-x64");
        assert_eq!(plan2.items[0].status, NeedStatus::Satisfied);
        assert_eq!(plan2.items[0].found_version.as_deref(), Some("v20.18.1"));
    }
}
