//! README 解析与命令分类（v2.1 spec §3）：确定性规则引擎，零网络、零 LLM。
//!
//! 流程：发现（大小写不敏感）→ 解码（UTF-8 → GBK 兜底）→ fenced/行内命令抽取
//! → 规则表分类（service / script / 忽略 + 章节加权 + 归一化去重）→ 与
//! [`crate::scan`] 草稿融合（scan 事实优先，README 只补全/新增）。

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
use crate::merge::{FieldMeta, FieldMetas};
use crate::scan;
use crate::spec::{PackageManager, ScriptSpec, ServiceSpec, SuperTaskFile};

// ---------------------------------------------------------------------------
// 置信度（spec §3.3：高/中/低）
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    Low,
    Medium,
    High,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
        }
    }
}

// ---------------------------------------------------------------------------
// 抽取结果
// ---------------------------------------------------------------------------

/// fenced code block 接受的语言标注；空标注与 text/plain 一并接受（命令行严格
/// 分类兜底，混入的配置片段只会进噪声计数）。
const FENCE_LANGS: &[&str] = &[
    "",
    "sh",
    "bash",
    "shell",
    "zsh",
    "console",
    "terminal",
    "powershell",
    "pwsh",
    "text",
    "plain",
    "plaintext",
];

/// 识别为可执行类命令的章节标题关键词（spec §3.3，中英同权）。
const RUN_SECTION_KEYS: &[&str] = &[
    "run",
    "getting started",
    "quick start",
    "development",
    "启动",
    "快速开始",
    "运行",
];
/// 识别为 script 类命令的章节标题关键词（Install 章节加权）。
const INSTALL_SECTION_KEYS: &[&str] = &["install", "安装"];

/// 忽略类命令（spec §3.3 忽略行示例 + 常见同族）。
const IGNORED_CMDS: &[&str] = &[
    "git", "cd", "mkdir", "curl", "wget", "echo", "rm", "cp", "mv", "ls", "cat", "touch", "chmod",
    "chown", "sudo", "source", "make", "npx", "node", "code", "open", "xdg-open", "start",
];

#[derive(Debug, Default)]
struct Parsed {
    services: Vec<ReadmeService>,
    scripts: Vec<ReadmeScript>,
    port_hints: Vec<String>,
    env_hints: Vec<String>,
    notes: Vec<String>,
    noise_count: usize,
    /// 归一化 argv 去重键（spec §3.3：相同者取首个上下文）
    seen: std::collections::HashSet<String>,
}

#[derive(Debug)]
struct ReadmeService {
    id_base: String,
    spec: ServiceSpec,
    confidence: Confidence,
    /// README 提供的字段名（用于 fields_meta 标注）
    readme_fields: Vec<&'static str>,
}

#[derive(Debug)]
struct ReadmeScript {
    id_base: String,
    spec: ScriptSpec,
    confidence: Confidence,
}

impl Parsed {
    fn feed(&mut self, lang: &str, line: &str, section: &str, inline: bool) {
        let line = strip_prompt(line, lang);
        if line.is_empty() || (!is_console(lang) && line.starts_with('#')) {
            return;
        }
        let line = strip_list_marker(line);
        for frag in split_chains(line) {
            if !looks_like_command(&frag) {
                continue; // 输出/散文行，不计噪声
            }
            let Some(tokens) = strip_env_prefixes(&frag, &mut self.port_hints, &mut self.env_hints)
            else {
                continue;
            };
            let Some(candidate) = classify(&tokens) else {
                self.noise_count += 1;
                continue;
            };
            // 章节加权与行内 code 上限（spec §3.3）
            let mut conf = candidate.confidence();
            if inline {
                conf = conf.min(Confidence::Medium);
            } else if is_run_section(section) && matches!(candidate, Classified::Service(_)) {
                conf = Confidence::High;
            } else if is_install_section(section) && matches!(candidate, Classified::Script(_)) {
                conf = Confidence::High;
            }
            let candidate = candidate.with_confidence(conf);
            match candidate {
                Classified::Service(s) => self.push_service(s),
                Classified::Script(s) => self.push_script(s),
            }
        }
    }

    /// 归一化 argv 去重：相同命令取首个上下文（spec §3.3）。
    fn push_service(&mut self, s: ReadmeService) {
        let key = normalize_argv_of(&s.spec);
        if self.seen.insert(key) {
            self.services.push(s);
        }
    }

    fn push_script(&mut self, s: ReadmeScript) {
        let key = s.spec.cmds.join(" ");
        if self.seen.insert(key) {
            self.scripts.push(s);
        }
    }
}

/// 归一化键：kind + 身份/入口字段（剥变量赋值与注释后由分类产出天然归一）。
fn normalize_argv_of(spec: &ServiceSpec) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}|{}|{}",
        spec.kind,
        spec.module.clone().unwrap_or_default(),
        spec.dir.clone().unwrap_or_default(),
        spec.entry.clone().unwrap_or_default(),
        spec.script.clone().unwrap_or_default(),
        spec.package.clone().unwrap_or_default(),
        spec.service.clone().unwrap_or_default(),
        spec.program.clone().unwrap_or_default(),
    )
}

#[derive(Debug)]
enum Classified {
    Service(ReadmeService),
    Script(ReadmeScript),
}

impl Classified {
    fn confidence(&self) -> Confidence {
        match self {
            Self::Service(s) => s.confidence,
            Self::Script(s) => s.confidence,
        }
    }

    fn with_confidence(self, conf: Confidence) -> Self {
        match self {
            Self::Service(mut s) => {
                s.confidence = conf;
                Self::Service(s)
            }
            Self::Script(mut s) => {
                s.confidence = conf;
                Self::Script(s)
            }
        }
    }
}

fn is_run_section(section: &str) -> bool {
    let lower = section.to_lowercase();
    RUN_SECTION_KEYS.iter().any(|k| lower.contains(k))
}

fn is_install_section(section: &str) -> bool {
    let lower = section.to_lowercase();
    INSTALL_SECTION_KEYS.iter().any(|k| lower.contains(k))
}

fn is_console(lang: &str) -> bool {
    matches!(lang, "console" | "terminal" | "powershell" | "pwsh")
}

/// 剥离 console / shell 提示符（`$ `、`> `、`PS C:\..>`、`# `）。
fn strip_prompt<'a>(line: &'a str, lang: &str) -> &'a str {
    let mut s = line.trim();
    if is_console(lang) {
        if let Some(r) = s.strip_prefix("PS ") {
            s = r;
        }
    }
    if let Some(r) = s.strip_prefix("$ ") {
        s = r;
    } else if let Some(r) = s.strip_prefix("> ") {
        s = r;
    }
    if is_console(lang) {
        // Windows 路径提示符 `C:\...>` 与 `# ` 提示符（console 语境）
        if let Some(i) = s.find('>') {
            let pre = &s[..i];
            let is_path_prompt = pre.len() < 60
                && pre
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_alphabetic())
                    .unwrap_or(false)
                && pre.contains(':');
            if is_path_prompt {
                s = &s[i + 1..];
            }
        }
        if let Some(r) = s.strip_prefix("# ") {
            s = r;
        }
    }
    s.trim()
}

/// 剥离 Markdown 列表标记（fenced 内偶发）。
fn strip_list_marker(line: &str) -> &str {
    let s = line;
    for p in ["- ", "* ", "+ "] {
        if let Some(r) = s.strip_prefix(p) {
            return r.trim_start();
        }
    }
    let bytes = s.as_bytes();
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits < 4 && bytes.len() > digits {
        let rest = &s[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return r.trim_start();
        }
    }
    s
}

/// 链式拆分：`&&` / `||` / `;` / `|` / 行尾 `&`（spec §3.2）。
fn split_chains(line: &str) -> Vec<String> {
    let line = line.trim().trim_end_matches('&').trim();
    let mut parts = vec![line.to_string()];
    for sep in ["&&", "||", ";", "|"] {
        let mut next = Vec::new();
        for p in parts {
            for frag in p.split(sep) {
                let f = frag.trim();
                if !f.is_empty() {
                    next.push(f.to_string());
                }
            }
        }
        parts = next;
    }
    parts
}

/// 剥离 `VAR=value` 前缀与 `export|set` 关键字；PORT 提示进端口建议，
/// 其余赋值进环境变量提示（只记变量名，不回显值）。
fn strip_env_prefixes(
    frag: &str,
    port_hints: &mut Vec<String>,
    env_hints: &mut Vec<String>,
) -> Option<Vec<String>> {
    let mut tokens: Vec<String> = frag.split_whitespace().map(str::to_string).collect();
    let mut record = |k: &str, v: &str, context: &str| {
        if k.eq_ignore_ascii_case("port") && v.parse::<u16>().is_ok() {
            port_hints.push(format!(
                "README 提示端口 {v}（{context}）；请确认后手填到服务 port"
            ));
        } else {
            env_hints.push(format!(
                "README 提示环境变量 {k}=…（{context}）；请确认后填入服务 env"
            ));
        }
    };
    loop {
        let Some(first) = tokens.first() else {
            return None;
        };
        if first == "export" || first == "set" {
            tokens.remove(0);
            continue;
        }
        let Some((k, v)) = split_assign(first) else {
            break;
        };
        tokens.remove(0);
        let context = if tokens.is_empty() {
            "独立赋值".to_string()
        } else {
            tokens.join(" ")
        };
        record(&k, &v, &context);
    }
    if tokens.is_empty() {
        return None;
    }
    Some(tokens)
}

fn split_assign(token: &str) -> Option<(String, String)> {
    let (k, v) = token.split_once('=')?;
    if k.is_empty()
        || !k
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return None;
    }
    Some((k.to_string(), v.to_string()))
}

/// 粗判是否命令行（排除输出/散文行）：首 token 像 binary 名且行不以句读结尾。
fn looks_like_command(frag: &str) -> bool {
    let first = frag.split_whitespace().next().unwrap_or("");
    if first.is_empty() || first.len() > 40 {
        return false;
    }
    if !first
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic() || c == '.' || c == '/')
        .unwrap_or(false)
    {
        return false;
    }
    if !first
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '_' | '-' | '@'))
    {
        return false;
    }
    if frag.contains("http://") || frag.contains("https://") {
        return false;
    }
    !matches!(
        frag.chars().last(),
        Some('.' | ':' | '，' | '。' | '；' | '）' | ')' | '"' | '”')
    )
}

// ---------------------------------------------------------------------------
// 分类规则表（spec §3.3）
// ---------------------------------------------------------------------------

fn base_service(kind: &str) -> ServiceSpec {
    let mut spec = ServiceSpec::default_service();
    spec.kind = kind.into();
    spec
}

fn script_spec(cmd: &str, args: &[&str], id_base: &str) -> Classified {
    Classified::Script(ReadmeScript {
        id_base: id_base.to_string(),
        spec: ScriptSpec {
            desc: None,
            cmds: vec![format!("{cmd} {}", args.join(" "))],
            cwd: None,
            env: IndexMap::new(),
            timeout_secs: None,
            depends_on: vec![],
        },
        confidence: Confidence::Medium,
    })
}

fn classify(tokens: &[String]) -> Option<Classified> {
    let cmd = tokens[0].rsplit(['/', '\\']).next().unwrap_or(&tokens[0]);
    let cmd = cmd.strip_suffix(".cmd").unwrap_or(cmd);
    let args: Vec<&str> = tokens[1..].iter().map(String::as_str).collect();
    match cmd {
        "mvn" | "mvnw" => classify_mvn(&args),
        "gradle" | "gradlew" => {
            if args.iter().any(|a| *a == "bootRun") {
                let mut spec = base_service("spring-boot");
                spec.build_tool = Some("gradle".into());
                Some(Classified::Service(ReadmeService {
                    id_base: "app".into(),
                    spec,
                    confidence: Confidence::High,
                    readme_fields: vec!["kind", "build_tool"],
                }))
            } else {
                None
            }
        }
        "npm" | "pnpm" | "yarn" => classify_pm(cmd, &args),
        "pip" | "pip3" => {
            if args.first().copied() == Some("install") {
                Some(script_spec(cmd, &args, "pip-install"))
            } else {
                None
            }
        }
        "python" | "python3" => classify_python(&args),
        "uvicorn" => {
            let mut spec = base_service("python");
            spec.module = Some("uvicorn".into());
            spec.extra_args = args.iter().map(|s| s.to_string()).collect();
            spec.dir = Some(".".into());
            let id_base = args
                .first()
                .and_then(|a| a.split(':').next())
                .map(|s| s.trim_end_matches(".py").to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "uvicorn".into());
            Some(Classified::Service(ReadmeService {
                id_base,
                spec,
                confidence: Confidence::High,
                readme_fields: vec!["kind", "dir", "module", "extra_args"],
            }))
        }
        "gunicorn" => {
            let mut spec = base_service("python");
            spec.module = Some("gunicorn".into());
            spec.extra_args = args.iter().map(|s| s.to_string()).collect();
            spec.dir = Some(".".into());
            Some(Classified::Service(ReadmeService {
                id_base: "gunicorn".into(),
                spec,
                confidence: Confidence::High,
                readme_fields: vec!["kind", "dir", "module", "extra_args"],
            }))
        }
        "go" => classify_go(&args),
        "deno" => {
            if args.first().copied() != Some("run") {
                return None;
            }
            let rest: Vec<String> = args[1..].iter().map(|s| s.to_string()).collect();
            let mut spec = base_service("generic");
            spec.program = Some("deno".into());
            spec.args = rest;
            spec.dir = Some(".".into());
            Some(Classified::Service(ReadmeService {
                id_base: "deno".into(),
                spec,
                confidence: Confidence::Medium,
                readme_fields: vec!["kind", "dir", "program", "args"],
            }))
        }
        "docker" | "docker-compose" => classify_compose(cmd, &args),
        _ => {
            if IGNORED_CMDS.contains(&cmd) {
                None
            } else {
                None
            }
        }
    }
}

fn classify_mvn(args: &[&str]) -> Option<Classified> {
    let has = |needle: &str| args.iter().any(|a| a.contains(needle));
    if has("spring-boot:run") {
        let module = pl_module(args);
        let mut spec = base_service("spring-boot");
        spec.module = module.clone();
        let id_base = module
            .as_deref()
            .and_then(|m| m.rsplit('/').next())
            .unwrap_or("app")
            .to_string();
        return Some(Classified::Service(ReadmeService {
            id_base,
            spec,
            confidence: Confidence::High,
            readme_fields: vec!["kind", "module"],
        }));
    }
    // script：最后一个 lifecycle goal 决定 id（clean 跳过）
    let goals = ["install", "package", "verify", "test", "deploy"];
    let goal = args.iter().rev().find(|a| goals.iter().any(|g| g == *a))?;
    let spec = ScriptSpec {
        desc: None,
        cmds: vec![format!("mvn {}", args.join(" "))],
        cwd: None,
        env: IndexMap::new(),
        timeout_secs: None,
        depends_on: vec![],
    };
    Some(Classified::Script(ReadmeScript {
        id_base: (*goal).to_string(),
        spec,
        confidence: Confidence::Medium,
    }))
}

/// `-pl <mod>` / `--projects <mod>` / `-pl=<mod>` 的模块提取。
fn pl_module(args: &[&str]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if *a == "-pl" || *a == "--projects" {
            return iter.next().map(|s| s.trim_start_matches('=').to_string());
        }
        if let Some(v) = a.strip_prefix("-pl=") {
            return Some(v.to_string());
        }
        if let Some(v) = a.strip_prefix("--projects=") {
            return Some(v.to_string());
        }
    }
    None
}

fn pm_of(cmd: &str) -> PackageManager {
    match cmd {
        "pnpm" => PackageManager::Pnpm,
        "yarn" => PackageManager::Yarn,
        _ => PackageManager::Npm,
    }
}

fn classify_pm(cmd: &str, args: &[&str]) -> Option<Classified> {
    let sub = *args.first()?;
    let pm = pm_of(cmd);
    let node = |script: &str, conf: Confidence| {
        let mut spec = base_service("node");
        spec.dir = Some(".".into());
        spec.script = Some(script.into());
        spec.package_manager = Some(pm);
        let id_base = if matches!(script, "dev" | "start" | "serve") {
            "web".to_string()
        } else {
            script.to_string()
        };
        Classified::Service(ReadmeService {
            id_base,
            spec,
            confidence: conf,
            readme_fields: vec!["kind", "dir", "script", "package_manager"],
        })
    };
    let script = |id: &str, conf: Confidence| {
        let spec = ScriptSpec {
            desc: None,
            cmds: vec![format!("{cmd} {}", args.join(" "))],
            cwd: None,
            env: IndexMap::new(),
            timeout_secs: None,
            depends_on: vec![],
        };
        Classified::Script(ReadmeScript {
            id_base: id.to_string(),
            spec,
            confidence: conf,
        })
    };
    match sub {
        "run" => {
            let name = args.get(1)?;
            match *name {
                "dev" | "start" | "serve" => Some(node(name, Confidence::High)),
                "build" | "test" => Some(script(name, Confidence::Medium)),
                "install" | "ci" => None, // `pm run install` 非常规，忽略
                _ => Some(node(name, Confidence::Medium)),
            }
        }
        "install" | "ci" => Some(script(
            if sub == "ci" { "ci" } else { "install" },
            Confidence::Medium,
        )),
        "start" => Some(node("start", Confidence::High)),
        "test" => Some(script("test", Confidence::Medium)),
        _ => None,
    }
}

fn classify_python(args: &[&str]) -> Option<Classified> {
    if args.first().copied() == Some("-m") {
        let module = *args.get(1)?;
        let rest: Vec<String> = args[2..].iter().map(|s| s.to_string()).collect();
        match module {
            "venv" | "pip" => return None,
            "pytest" => {
                let spec = ScriptSpec {
                    desc: None,
                    cmds: vec![format!("python {}", args.join(" "))],
                    cwd: None,
                    env: IndexMap::new(),
                    timeout_secs: None,
                    depends_on: vec![],
                };
                return Some(Classified::Script(ReadmeScript {
                    id_base: "test".into(),
                    spec,
                    confidence: Confidence::Medium,
                }));
            }
            _ => {}
        }
        let mut spec = base_service("python");
        spec.dir = Some(".".into());
        spec.module = Some(module.to_string());
        spec.extra_args = rest;
        return Some(Classified::Service(ReadmeService {
            id_base: module.to_string(),
            spec,
            confidence: Confidence::Medium,
            readme_fields: vec!["kind", "dir", "module", "extra_args"],
        }));
    }
    // `python <file>.py [args]`
    let entry = args.iter().find(|a| a.ends_with(".py"))?;
    let idx = args.iter().position(|a| a == entry).unwrap_or(0);
    let rest: Vec<String> = args[idx + 1..].iter().map(|s| s.to_string()).collect();
    let mut spec = base_service("python");
    spec.dir = Some(".".into());
    spec.entry = Some(entry.to_string());
    spec.extra_args = rest;
    let id_base = entry
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(entry)
        .trim_end_matches(".py")
        .to_string();
    Some(Classified::Service(ReadmeService {
        id_base,
        spec,
        confidence: Confidence::Medium,
        readme_fields: vec!["kind", "dir", "entry", "extra_args"],
    }))
}

fn classify_go(args: &[&str]) -> Option<Classified> {
    let sub = *args.first()?;
    match sub {
        "run" => {
            let pkg = args.get(1)?;
            let package = if *pkg == "." {
                ".".to_string()
            } else if pkg.starts_with("./") {
                pkg.to_string()
            } else {
                format!("./{pkg}")
            };
            let mut spec = base_service("go");
            spec.dir = Some(".".into());
            spec.package = Some(package.clone());
            let id_base = package
                .rsplit('/')
                .next()
                .filter(|s| *s != ".")
                .unwrap_or("go-app")
                .to_string();
            Some(Classified::Service(ReadmeService {
                id_base,
                spec,
                confidence: Confidence::High,
                readme_fields: vec!["kind", "dir", "package"],
            }))
        }
        "build" | "test" => {
            let spec = ScriptSpec {
                desc: None,
                cmds: vec![format!("go {}", args.join(" "))],
                cwd: None,
                env: IndexMap::new(),
                timeout_secs: None,
                depends_on: vec![],
            };
            Some(Classified::Script(ReadmeScript {
                id_base: format!("go-{sub}"),
                spec,
                confidence: Confidence::Medium,
            }))
        }
        _ => None,
    }
}

fn classify_compose(cmd: &str, args: &[&str]) -> Option<Classified> {
    // docker compose <sub>… / docker-compose <sub>…
    let rest: &[&str] = if cmd == "docker-compose" {
        args
    } else if args.first().copied() == Some("compose") {
        &args[1..]
    } else {
        return None;
    };
    let sub = *rest.first()?;
    let cmd_text = if cmd == "docker-compose" {
        format!("docker-compose {}", args.join(" "))
    } else {
        format!("docker {}", args.join(" "))
    };
    match sub {
        "up" => {
            let names: Vec<String> = rest[1..]
                .iter()
                .filter(|a| !a.starts_with('-'))
                .map(|s| s.to_string())
                .collect();
            if names.is_empty() {
                let spec = base_service("compose");
                return Some(Classified::Service(ReadmeService {
                    id_base: "compose".into(),
                    spec,
                    confidence: Confidence::Low,
                    readme_fields: vec!["kind"],
                }));
            }
            // 多服务名：取首个生成草稿，其余丢弃（归一化去重以命令整体为键）
            let mut spec = base_service("compose");
            spec.service = Some(names[0].clone());
            Some(Classified::Service(ReadmeService {
                id_base: names[0].clone(),
                spec,
                confidence: Confidence::High,
                readme_fields: vec!["kind", "service"],
            }))
        }
        "build" | "down" => {
            let spec = ScriptSpec {
                desc: None,
                cmds: vec![cmd_text],
                cwd: None,
                env: IndexMap::new(),
                timeout_secs: None,
                depends_on: vec![],
            };
            Some(Classified::Script(ReadmeScript {
                id_base: format!("compose-{sub}"),
                spec,
                confidence: Confidence::Medium,
            }))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 解析入口
// ---------------------------------------------------------------------------

fn inline_segments(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, seg) in line.split('`').enumerate() {
        if i % 2 == 1 {
            let s = seg.trim();
            if !s.is_empty() && s.contains(' ') {
                out.push(s.to_string());
            }
        }
    }
    out
}

fn parse_readme(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut section = String::new();
    let mut in_fence = false;
    let mut lang = String::new();
    let mut cont: Option<String> = None;
    for raw in text.lines() {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            if !in_fence {
                in_fence = true;
                let l = rest.trim().to_lowercase();
                lang = if FENCE_LANGS.contains(&l.as_str()) {
                    l
                } else {
                    String::new()
                };
            } else {
                in_fence = false;
                lang = String::new();
            }
            cont = None;
            continue;
        }
        if in_fence {
            if lang.is_empty() {
                continue; // 非命令类 fence（yaml/json dump 等）
            }
            // 续行拼接（行尾 \）
            let line = match cont.take() {
                Some(prev) => format!("{prev} {}", trimmed),
                None => trimmed.to_string(),
            };
            if line.ends_with('\\') {
                cont = Some(line.trim_end_matches('\\').trim().to_string());
                continue;
            }
            if line.is_empty() {
                continue;
            }
            parsed.feed(&lang, &line, &section, false);
        } else if trimmed.starts_with('#') {
            section = trimmed.trim_start_matches('#').trim().to_string();
        } else if trimmed.contains('`') {
            for seg in inline_segments(trimmed) {
                parsed.feed("", &seg, &section, true);
            }
        }
    }
    parsed
}

/// 大小写不敏感发现工作区根的 README；`.md` 优先于 `.markdown`，
/// 同扩展名按文件名字节序（`README.md` 先于 `readme.md`）。
pub fn find_readme(root: &Path) -> Option<PathBuf> {
    let mut cands: Vec<(String, PathBuf)> = Vec::new();
    for e in fs::read_dir(root).ok()?.flatten() {
        let p = e.path();
        if !p.is_file() {
            continue;
        }
        if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            let lower = name.to_lowercase();
            if lower == "readme.md" || lower == "readme.markdown" {
                cands.push((lower, p));
            }
        }
    }
    cands.sort();
    cands.into_iter().map(|(_, p)| p).next()
}

fn decode_bytes(bytes: &[u8]) -> String {
    if std::str::from_utf8(bytes).is_ok() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let (cow, _, _) = encoding_rs::GBK.decode(bytes);
    cow.into_owned()
}

// ---------------------------------------------------------------------------
// 与 scan 融合（spec §3.4）
// ---------------------------------------------------------------------------

/// README 导入产物：scan 骨架 + README 补全/新增（未写盘），附字段来源元数据。
#[derive(Debug)]
pub struct ReadmeImport {
    pub draft: SuperTaskFile,
    /// service_id → 字段来源（scan/readme + 置信度；冲突时附 readme_value）
    pub service_sources: FieldMetas,
    /// script_id → 字段来源
    pub script_sources: FieldMetas,
    pub warnings: Vec<String>,
    /// 实际使用的 README（相对工作区根）；未发现为 None
    pub readme_path: Option<String>,
}

/// 显式指定路径不存在 → `README_NOT_FOUND`；未指定且未发现 → scan 骨架 +
/// 人话提示（非错误）。
pub fn import_readme(root: &Path, explicit: Option<&str>) -> Result<ReadmeImport> {
    let (mut draft, mut warnings) = match scan::scan_draft(root) {
        Ok((f, w)) => (f, w),
        Err(e) if e.code() == ErrorCode::NoYaml => {
            let mut w = Vec::new();
            w.push(
                "文件系统扫描未识别已知工程（pom.xml / package.json / pyproject.toml / go.mod / compose）；以下草稿仅来自 README。"
                    .to_string(),
            );
            (empty_file(root), w)
        }
        Err(e) => return Err(e),
    };

    let readme = match explicit {
        Some(p) => {
            let path = root.join(p.replace('/', std::path::MAIN_SEPARATOR_STR));
            if !path.is_file() {
                return Err(Error::new(
                    ErrorCode::ReadmeNotFound,
                    format!("README 不存在: {p}"),
                ));
            }
            Some((path, p.to_string()))
        }
        None => find_readme(root).map(|p| {
            let rel = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            (p, rel)
        }),
    };
    let Some((path, rel)) = readme else {
        warnings.push(
            "未在工作区根目录发现 README.md（大小写不敏感）；可直接使用「扫描工作区」，或显式指定 README 路径。"
                .to_string(),
        );
        return Ok(ReadmeImport {
            draft,
            service_sources: IndexMap::new(),
            script_sources: IndexMap::new(),
            warnings,
            readme_path: None,
        });
    };

    let bytes = fs::read(&path)
        .map_err(|e| Error::new(ErrorCode::ReadmeNotFound, format!("README 读取失败: {e}")))?;
    let text = decode_bytes(&bytes);
    let parsed = parse_readme(&text);

    let mut service_sources: FieldMetas = IndexMap::new();
    let mut script_sources: FieldMetas = IndexMap::new();
    let mut claimed: Vec<String> = Vec::new();

    for svc in parsed.services {
        let match_id = draft
            .services
            .iter()
            .find(|(id, cur)| {
                !claimed.iter().any(|c| c == *id)
                    && cur.kind == svc.spec.kind
                    && identity_key(cur) == identity_key(&svc.spec)
            })
            .map(|(id, _)| id.clone());
        match match_id {
            Some(id) => {
                claimed.push(id.clone());
                let metas = merge_into(&mut draft.services, &id, &svc);
                service_sources.insert(id, metas);
            }
            None => {
                let id = scan::unique_id(&scan::sanitize_id(&svc.id_base), &draft.services);
                let mut metas = vec![meta("kind", svc.confidence)];
                for f in &svc.readme_fields {
                    if *f != "kind" {
                        metas.push(meta(f, svc.confidence));
                    }
                }
                draft.services.insert(id.clone(), svc.spec.clone());
                service_sources.insert(id, metas);
            }
        }
    }

    for script in parsed.scripts {
        let id_base = scan::sanitize_id(&script.id_base);
        let mut id = id_base.clone();
        let mut i = 2;
        while draft.scripts.contains_key(&id) {
            id = format!("{id_base}-{i}");
            i += 1;
        }
        script_sources.insert(
            id.clone(),
            vec![FieldMeta {
                field: "cmds".into(),
                source: "readme".into(),
                confidence: Some(script.confidence.as_str().into()),
                readme_value: None,
            }],
        );
        draft.scripts.insert(id, script.spec);
    }

    warnings.extend(parsed.port_hints);
    warnings.extend(parsed.env_hints);
    warnings.extend(parsed.notes);
    if parsed.noise_count > 0 {
        warnings.push(format!("{} 条命令未识别，已忽略", parsed.noise_count));
    }

    draft.apply_defaults();
    Ok(ReadmeImport {
        draft,
        service_sources,
        script_sources,
        warnings,
        readme_path: Some(rel),
    })
}

/// README 与 scan 的身份键：spring → module、compose → service、其余 → dir。
fn identity_key(spec: &ServiceSpec) -> (String, String) {
    match spec.kind.as_str() {
        "spring-boot" => ("module".into(), spec.module.clone().unwrap_or_default()),
        "compose" => ("service".into(), spec.service.clone().unwrap_or_default()),
        _ => ("dir".into(), spec.dir.clone().unwrap_or_default()),
    }
}

/// scan 值优先：README 补全 scan 缺失字段；字段冲突时保留 scan 值、README 值进
/// 建议列（fields_meta 附 readme_value，向导双值可见）。返回合并后的 metas。
fn merge_into(
    services: &mut IndexMap<String, ServiceSpec>,
    id: &str,
    svc: &ReadmeService,
) -> Vec<crate::merge::FieldMeta> {
    use crate::merge::FieldMeta;
    let fields = [
        "module",
        "dir",
        "entry",
        "script",
        "package_manager",
        "package",
        "program",
        "service",
        "extra_args",
    ];
    let mut metas = Vec::new();
    let cur = services.get_mut(id).expect("matched id 必在表内");
    for f in fields {
        let read = field_of(&svc.spec, f);
        if read.is_none() {
            continue;
        }
        let have = field_of(cur, f);
        match have {
            None => {
                set_field(cur, f, &svc.spec);
                metas.push(FieldMeta {
                    field: f.into(),
                    source: "readme".into(),
                    confidence: Some(svc.confidence.as_str().into()),
                    readme_value: None,
                });
            }
            Some(hv) if Some(&hv) == read.as_ref() => {
                metas.push(FieldMeta {
                    field: f.into(),
                    source: "scan".into(),
                    confidence: None,
                    readme_value: None,
                });
            }
            Some(_) => {
                // 冲突：scan 值保留，README 值进建议列
                metas.push(FieldMeta {
                    field: f.into(),
                    source: "scan".into(),
                    confidence: None,
                    readme_value: Some(read.unwrap_or_default()),
                });
            }
        }
    }
    metas
}

fn field_of(spec: &ServiceSpec, field: &str) -> Option<String> {
    match field {
        "module" => spec.module.clone(),
        "dir" => spec.dir.clone(),
        "entry" => spec.entry.clone(),
        "script" => spec.script.clone(),
        "package_manager" => spec.package_manager.map(|pm| match pm {
            PackageManager::Npm => "npm".into(),
            PackageManager::Pnpm => "pnpm".into(),
            PackageManager::Yarn => "yarn".into(),
        }),
        "package" => spec.package.clone(),
        "program" => spec.program.clone(),
        "service" => spec.service.clone(),
        "extra_args" => (!spec.extra_args.is_empty()).then(|| spec.extra_args.join(" ")),
        _ => None,
    }
}

fn set_field(dst: &mut ServiceSpec, field: &str, src: &ServiceSpec) {
    match field {
        "module" => dst.module = src.module.clone(),
        "dir" => dst.dir = src.dir.clone(),
        "entry" => dst.entry = src.entry.clone(),
        "script" => dst.script = src.script.clone(),
        "package_manager" => dst.package_manager = src.package_manager,
        "package" => dst.package = src.package.clone(),
        "program" => dst.program = src.program.clone(),
        "service" => dst.service = src.service.clone(),
        "extra_args" => dst.extra_args = src.extra_args.clone(),
        _ => {}
    }
}

fn meta(field: &str, conf: Confidence) -> crate::merge::FieldMeta {
    crate::merge::FieldMeta {
        field: field.into(),
        source: "readme".into(),
        confidence: Some(conf.as_str().into()),
        readme_value: None,
    }
}

fn empty_file(root: &Path) -> SuperTaskFile {
    SuperTaskFile {
        version: 1,
        kind: Some("workspace".into()),
        name: Some(
            root.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "workspace".into()),
        ),
        description: None,
        root: ".".into(),
        env: IndexMap::new(),
        services: IndexMap::new(),
        scripts: IndexMap::new(),
        logging: None,
        secrets: None,
        profiles: None,
        toolchain: None,
        templates: None,
        git: None,
        docker: None,
        gateway: None,
        cloud: None,
        ai: None,
        network: None,
        log_retention: None,
        extra: IndexMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    fn svc_kind(c: Option<Classified>) -> String {
        match c {
            Some(Classified::Service(s)) => s.spec.kind.clone(),
            Some(Classified::Script(s)) => format!("script:{}", s.id_base),
            None => "none".into(),
        }
    }

    #[test]
    fn split_chains_and_trailing_amp() {
        let parts = split_chains("cd web && npm run dev");
        assert_eq!(parts, ["cd web", "npm run dev"]);
        let parts = split_chains("npm run dev &");
        assert_eq!(parts, ["npm run dev"]);
        let parts = split_chains("npm install; go build ./...");
        assert_eq!(parts, ["npm install", "go build ./..."]);
    }

    #[test]
    fn env_prefixes_split_to_hints() {
        let mut ports = Vec::new();
        let mut envs = Vec::new();
        let tokens = strip_env_prefixes("PORT=3000 npm run dev", &mut ports, &mut envs).unwrap();
        assert_eq!(tokens, ["npm", "run", "dev"]);
        assert_eq!(ports.len(), 1);
        assert!(ports[0].contains("3000"));
        let tokens =
            strip_env_prefixes("export DEBUG=1 npm run dev", &mut ports, &mut envs).unwrap();
        assert_eq!(tokens, ["npm", "run", "dev"]);
        assert!(envs[0].contains("DEBUG"));
        // 纯赋值
        assert!(strip_env_prefixes("export FOO=bar", &mut ports, &mut envs).is_none());
        // `-Dx=y` 不是 VAR= 前缀
        let tokens = strip_env_prefixes("-Dx=y mvn package", &mut ports, &mut envs).unwrap();
        assert_eq!(tokens, ["-Dx=y", "mvn", "package"]);
    }

    #[test]
    fn strip_prompt_console_and_shell() {
        assert_eq!(strip_prompt("$ npm run dev", "sh"), "npm run dev");
        assert_eq!(
            strip_prompt("PS C:\\proj> npm run dev", "console"),
            "npm run dev"
        );
        assert_eq!(strip_prompt("# npm run dev", "console"), "npm run dev");
    }

    #[test]
    fn classify_matrix() {
        // service 候选
        assert_eq!(
            svc_kind(classify(&tok("mvn -pl user-api spring-boot:run -am"))),
            "spring-boot"
        );
        assert_eq!(svc_kind(classify(&tok("./mvnw spring-boot:run"))), "spring-boot");
        assert_eq!(svc_kind(classify(&tok("gradle bootRun"))), "spring-boot");
        assert_eq!(svc_kind(classify(&tok("npm run dev"))), "node");
        assert_eq!(svc_kind(classify(&tok("yarn start"))), "node");
        assert_eq!(svc_kind(classify(&tok("pnpm run storybook"))), "node");
        assert_eq!(svc_kind(classify(&tok("python app.py --port 8000"))), "python");
        assert_eq!(svc_kind(classify(&tok("python -m http.server"))), "python");
        assert_eq!(svc_kind(classify(&tok("uvicorn app:app --reload"))), "python");
        assert_eq!(svc_kind(classify(&tok("gunicorn -w 2 main:app"))), "python");
        assert_eq!(svc_kind(classify(&tok("go run ."))), "go");
        assert_eq!(svc_kind(classify(&tok("go run ./cmd/server"))), "go");
        assert_eq!(svc_kind(classify(&tok("deno run -A main.ts"))), "generic");
        assert_eq!(svc_kind(classify(&tok("docker compose up -d"))), "compose");
        match classify(&tok("docker compose up db")) {
            Some(Classified::Service(s)) => assert_eq!(s.spec.service.as_deref(), Some("db")),
            other => panic!("{other:?}"),
        }
        // script 候选
        assert_eq!(svc_kind(classify(&tok("mvn clean package"))), "script:package");
        assert_eq!(svc_kind(classify(&tok("./mvnw install"))), "script:install");
        assert_eq!(svc_kind(classify(&tok("npm install"))), "script:install");
        assert_eq!(svc_kind(classify(&tok("pnpm run build"))), "script:build");
        assert_eq!(svc_kind(classify(&tok("yarn test"))), "script:test");
        assert_eq!(
            svc_kind(classify(&tok("pip install -r requirements.txt"))),
            "script:pip-install"
        );
        assert_eq!(svc_kind(classify(&tok("go build ./..."))), "script:go-build");
        assert_eq!(svc_kind(classify(&tok("go test ./..."))), "script:go-test");
        assert_eq!(
            svc_kind(classify(&tok("docker-compose build"))),
            "script:compose-build"
        );
        assert_eq!(svc_kind(classify(&tok("docker compose down"))), "script:compose-down");
        // 忽略
        assert!(classify(&tok("git clone ./repo.git")).is_none());
        assert!(classify(&tok("python -m venv .venv")).is_none());
        assert_eq!(svc_kind(classify(&tok("python -m pytest -q"))), "script:test");
        assert!(classify(&tok("cargo build --release")).is_none());
        assert!(classify(&tok("curl -X POST http://x")).is_none());
    }

    #[test]
    fn section_weighting_upgrade() {
        let p = parse_readme("# T\n\n```sh\nnpm run start\n```\n");
        assert_eq!(p.services[0].confidence, Confidence::High);
        let p = parse_readme("# T\n\n```sh\npnpm run storybook\n```\n");
        assert_eq!(p.services[0].confidence, Confidence::Medium);
        let p = parse_readme("## 快速开始\n\n```sh\npnpm run storybook\n```\n");
        assert_eq!(p.services[0].confidence, Confidence::High);
        let p = parse_readme("## Install\n\n```sh\nnpm run build\n```\n");
        assert_eq!(p.scripts[0].confidence, Confidence::High);
        let p = parse_readme("## Build\n\n```sh\nnpm run build\n```\n");
        assert_eq!(p.scripts[0].confidence, Confidence::Medium);
    }

    #[test]
    fn inline_code_capped_and_deduped() {
        let p = parse_readme("## 快速开始\n\n用 `pnpm run storybook` 预览。\n");
        assert_eq!(p.services[0].confidence, Confidence::Medium); // inline 上限 medium
        // fenced + inline 相同命令 → 只留首个
        let p = parse_readme("## Run\n\n```sh\nnpm run dev\n```\n\n或直接 `npm run dev`。\n");
        assert_eq!(p.services.len(), 1);
    }

    #[test]
    fn continuation_and_console_blocks() {
        let p = parse_readme("## Run\n\n```sh\nmvn -pl user-api \\\n  spring-boot:run\n```\n");
        assert_eq!(p.services.len(), 1);
        let p =
            parse_readme("## Run\n\n```console\n$ docker compose up db\nPS C:\\x> npm run dev\n```\n");
        assert_eq!(p.services.len(), 2);
    }

    #[test]
    fn port_and_env_hints() {
        let p = parse_readme("## Run\n\n```sh\nexport PORT=8000\nuvicorn app:app\n```\n");
        assert_eq!(p.services.len(), 1);
        assert!(p.port_hints[0].contains("8000"));
        assert_eq!(p.noise_count, 0);
    }

    #[test]
    fn non_command_fence_skipped() {
        let p = parse_readme(
            "# T\n\n```yaml\nservices:\n  api:\n    image: demo\n```\n\n```text\nnpm run dev\n```\n",
        );
        assert_eq!(p.services.len(), 1);
        assert_eq!(p.noise_count, 0);
    }

    #[test]
    fn explicit_path_missing_is_readme_not_found() {
        let root = temp_root("nf");
        let err = import_readme(&root, Some("docs/NOPE.md")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ReadmeNotFound);
    }

    #[test]
    fn discovery_case_insensitive_markdown() {
        let root = temp_root("disc");
        fs::write(root.join("readme.MARKDOWN"), "## Run\n\n```sh\nnpm run dev\n```\n").unwrap();
        let imp = import_readme(&root, None).unwrap();
        assert_eq!(imp.readme_path.as_deref(), Some("readme.MARKDOWN"));
        assert_eq!(imp.draft.services.len(), 1);
    }

    #[test]
    fn no_readme_is_hint_not_error() {
        let root = temp_root("empty");
        let imp = import_readme(&root, None).unwrap();
        assert!(imp.readme_path.is_none());
        assert!(imp.draft.services.is_empty());
        assert!(imp.warnings.iter().any(|w| w.contains("未在工作区根目录发现")));
    }

    #[test]
    fn gbk_fallback_decode() {
        let text = "# 启动\n\n```text\nnpm run dev\n```\n";
        let (bytes, _, _) = encoding_rs::GBK.encode(text);
        let decoded = decode_bytes(&bytes);
        assert!(decoded.contains("启动"));
        let p = parse_readme(&decoded);
        assert_eq!(p.services.len(), 1);
    }

    #[test]
    fn fusion_scan_value_wins_readme_value_to_suggestion() {
        let root = temp_root("fusion");
        fs::write(
            root.join("package.json"),
            r#"{"name":"demo","scripts":{"dev":"vite"}}"#,
        )
        .unwrap();
        fs::write(
            root.join("README.md"),
            "## Run\n\n```sh\nnpm run start\nuvicorn app:app\n```\n",
        )
        .unwrap();
        let imp = import_readme(&root, None).unwrap();
        // scan 的 node 服务（dir "."）被 README 命中：script 冲突 → scan 值保留
        let web = imp.draft.services.get("web").expect("web");
        assert_eq!(web.script.as_deref(), Some("dev"));
        let metas = imp.service_sources.get("web").expect("web metas");
        let script_meta = metas.iter().find(|m| m.field == "script").expect("script meta");
        assert_eq!(script_meta.source, "scan");
        assert_eq!(script_meta.readme_value.as_deref(), Some("start"));
        // README-only 服务进入草稿
        let app = imp.draft.services.get("app").expect("app");
        assert_eq!(app.kind, "python");
        assert_eq!(app.module.as_deref(), Some("uvicorn"));
    }

    #[test]
    fn golden_fixtures() {
        for name in ["spring-node", "python", "go", "zh", "noise"] {
            let dir = PathBuf::from(format!("tests/fixtures/readme/{name}"));
            let imp = import_readme(&dir, None).unwrap();
            let yaml = serde_yaml::to_string(&imp.draft).unwrap();
            golden(name, &yaml);
        }
    }

    fn golden(name: &str, text: &str) {
        let path = format!("tests/golden/readme/{name}.yaml");
        match std::fs::read_to_string(&path) {
            Ok(want) => assert_eq!(text, want, "golden {name} 不一致（更新请重写 golden 文件）"),
            Err(_) => {
                let p = PathBuf::from(&path);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, text).unwrap();
                panic!("golden {name} 已生成，请重跑确认");
            }
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("st-readme-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }
}
