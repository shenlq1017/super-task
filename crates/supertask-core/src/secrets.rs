//! 1.2 Secrets 与 `.env.local`（规格 §6）。
//!
//! 硬性边界：值绝不进入返回值、日志、事件、app data；`required` 只存 key 名；
//! 文件写回用临时文件 + 替换；`.env.local` 未被 Git 忽略时只报警，不自动改
//! `.gitignore`，不执行 `git rm --cached`。

use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
use crate::git::{GitOutput, GitRunner, ProcessRunner};
use crate::spec::{SecretsBackend, SuperTaskFile};

/// §6.2 dotenv 子集：`KEY=VALUE`、空行、`#` 注释、单双引号单行值。
/// 不支持：命令替换、插值、跨行、`export`、反引号。非法行报行号。
pub fn parse_dotenv(text: &str) -> Result<Vec<(usize, String, String)>> {
    let mut out = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.strip_prefix("export ").is_some() {
            return Err(Error::new(
                ErrorCode::SecretParse,
                format!("dotenv 第 {line_no} 行：不支持 export 语句"),
            )
            .details(serde_yaml::Value::Number(line_no.into())));
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(Error::new(
                ErrorCode::SecretParse,
                format!("dotenv 第 {line_no} 行：缺少 = ，无法解析"),
            )
            .details(serde_yaml::Value::Number(line_no.into())));
        };
        let key = key.trim();
        if !is_valid_secret_key(key) {
            return Err(Error::new(
                ErrorCode::SecretParse,
                format!("dotenv 第 {line_no} 行：非法 key {key:?}"),
            )
            .details(serde_yaml::Value::Number(line_no.into())));
        }
        let mut value = value.trim();
        // 单双引号包裹：去掉首尾引号；值内再出现同类引号按字面保留（无转义语义）
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = &value[1..value.len() - 1];
        }
        if value.contains('`') || value.contains('$') && value.contains('(') {
            return Err(Error::new(
                ErrorCode::SecretParse,
                format!("dotenv 第 {line_no} 行：不支持命令替换/插值"),
            )
            .details(serde_yaml::Value::Number(line_no.into())));
        }
        out.push((line_no, key.to_string(), value.to_string()));
    }
    Ok(out)
}

pub fn is_valid_secret_key(s: &str) -> bool {
    crate::spec::validate::is_valid_secret_key(s)
}

/// 顶层 secrets 指向的文件（backend: file/local）；backend: env 返回 None。
pub fn secret_file_rel(spec: &SuperTaskFile) -> Option<String> {
    let sec = spec.secrets.as_ref()?;
    let backend = sec.backend.unwrap_or(SecretsBackend::Local);
    if !backend.is_file() {
        return None;
    }
    Some(sec.file.clone().unwrap_or_else(|| ".env.local".to_string()))
}

fn secret_file_path(spec: &SuperTaskFile, root: &Path) -> Result<Option<PathBuf>> {
    let Some(rel) = secret_file_rel(spec) else {
        return Ok(None);
    };
    crate::sandbox::assert_rel_safe(&rel)?;
    Ok(Some(root.join(rel)))
}

/// 顶层 secrets 与服务 env_file 的 key → present 摘要。只返回 key 名与状态。
pub fn status(spec: &SuperTaskFile, root: &Path) -> Result<crate::ipc::SecretsStatusOutput> {
    let git = ProcessRunner::default();
    let mut keys: Vec<crate::ipc::SecretKeyStatus> = Vec::new();
    let mut git_ignored = true;

    if let Some(path) = secret_file_path(spec, root)? {
        git_ignored = is_git_ignored(&git, root, &path);
        let (entries, parse_ok) = read_dotenv_file(&path);
        for (key, _) in &entries {
            keys.push(crate::ipc::SecretKeyStatus {
                key: key.clone(),
                source: "file".into(),
                present: true,
                parse_ok: Some(parse_ok),
                git_tracked: Some(is_git_tracked(&git, root, &path)),
            });
        }
    }
    if let Some(sec) = &spec.secrets {
        for key in &sec.required {
            if !keys.iter().any(|k| k.key == *key) {
                keys.push(crate::ipc::SecretKeyStatus {
                    key: key.clone(),
                    source: "env".into(),
                    present: std::env::var(key).is_ok(),
                    parse_ok: None,
                    git_tracked: None,
                });
            }
        }
    }
    let backend = spec
        .secrets
        .as_ref()
        .map(|s| match s.backend.unwrap_or(SecretsBackend::Local) {
            SecretsBackend::Env => "env",
            _ => "file",
        })
        .unwrap_or("file")
        .to_string();
    Ok(crate::ipc::SecretsStatusOutput {
        backend,
        file: secret_file_rel(spec),
        keys,
        git_ignored,
    })
}

fn read_dotenv_file(path: &Path) -> (Vec<(String, String)>, bool) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (Vec::new(), false);
    };
    match parse_dotenv(&text) {
        Ok(entries) => (entries.into_iter().map(|(_, k, v)| (k, v)).collect(), true),
        Err(_) => (Vec::new(), false),
    }
}

fn is_git_tracked(git: &dyn GitRunner, root: &Path, file: &Path) -> bool {
    let rel = rel_to_root(root, file);
    git.run(root, &["ls-files", "--", &rel])
        .map(|o| o.code == 0 && !o.stdout.trim().is_empty())
        .unwrap_or(false)
}

fn is_git_ignored(git: &dyn GitRunner, root: &Path, file: &Path) -> bool {
    let rel = rel_to_root(root, file);
    git.run(root, &["check-ignore", "-q", &rel])
        .map(|o: GitOutput| o.code == 0)
        .unwrap_or(false)
}

fn rel_to_root(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 写入单个 key（存在则原位替换，不存在则追加）。临时文件 + 替换；失败保留原文件。
pub fn set_key(spec: &SuperTaskFile, root: &Path, key: &str, value: &str) -> Result<()> {
    if !is_valid_secret_key(key) {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            format!("非法 key {key:?}"),
        ));
    }
    if value.contains('\n') || value.contains('\r') {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            "secret 值只允许单行（dotenv 子集不支持跨行）",
        ));
    }
    let Some(path) = secret_file_path(spec, root)? else {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            "secrets.backend 为 env，没有可写的 secret 文件",
        ));
    };
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = if existing.is_empty() {
        Vec::new()
    } else {
        // 未解析成功的行原样保留（set 不做全文件校验，避免破坏用户手写内容）
        existing.lines().map(str::to_string).collect()
    };
    let replaced = lines
        .iter_mut()
        .find(|l| {
            l.trim()
                .split_once('=')
                .map(|(k, _)| k.trim() == key)
                .unwrap_or(false)
        })
        .is_some_and(|slot| {
            *slot = format!("{key}={value}");
            true
        });
    if !replaced {
        if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("{key}={value}"));
    }
    let mut body = lines.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    atomic_write(&path, body.as_bytes())
}

pub fn delete_key(spec: &SuperTaskFile, root: &Path, key: &str) -> Result<()> {
    let Some(path) = secret_file_path(spec, root)? else {
        return Err(Error::new(ErrorCode::SpecInvalid, "secrets.backend 为 env"));
    };
    if !path.exists() {
        return Err(Error::new(
            ErrorCode::SecretFileMissing,
            format!("secret 文件不存在: {}", path.display()),
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Error::new(
            ErrorCode::SecretFileMissing,
            format!("无法读取 secret 文件: {e}"),
        )
    })?;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let is_target = line
            .trim()
            .split_once('=')
            .map(|(k, _)| k.trim() == key)
            .unwrap_or(false);
        if !is_target {
            out.push_str(line);
            out.push('\n');
        }
    }
    atomic_write(&path, out.as_bytes())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("无法创建目录: {e}")))?;
    }
    let tmp = path.with_extension("tmp-st");
    std::fs::write(&tmp, bytes)
        .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("写入临时文件失败: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::new(
            ErrorCode::TemplateWrite,
            format!("替换 secret 文件失败: {e}"),
        )
    })?;
    Ok(())
}

/// §6.3 环境链中的文件层：secrets 文件 + 服务 env_file（按声明顺序）。
/// `service` 限定只加载该服务的 env_file（None = 全部，validate 用）。
/// 服务 env / profile env / 端口注入由调用方继续叠加。文件缺失：secrets 主文件
/// 缺失按 warnings 返回；服务 env_file 缺失是配置错误 → `SECRET_FILE_MISSING`。
pub fn load_file_layers(
    spec: &SuperTaskFile,
    root: &Path,
    service: Option<&str>,
) -> Result<(IndexMap<String, String>, Vec<String>)> {
    let mut env = IndexMap::new();
    let mut warnings = Vec::new();
    if let Some(path) = secret_file_path(spec, root)? {
        if path.exists() {
            let text = std::fs::read_to_string(&path).map_err(|e| {
                Error::new(ErrorCode::SecretParse, format!("无法读取 secret 文件: {e}"))
            })?;
            for (_, k, v) in parse_dotenv(&text)? {
                env.insert(k, v);
            }
        } else {
            warnings.push(format!("secret 文件不存在: {}", rel_to_root(root, &path)));
        }
    }
    for (id, svc) in spec
        .services
        .iter()
        .filter(|(sid, _)| service.is_none_or(|s| *sid == s))
    {
        for rel in &svc.env_file {
            crate::sandbox::assert_rel_safe(rel)?;
            let p = root.join(rel);
            if !p.exists() {
                return Err(Error::new(
                    ErrorCode::SecretFileMissing,
                    format!("{id}: env_file 不存在: {rel}"),
                ));
            }
            let text = std::fs::read_to_string(&p).map_err(|e| {
                Error::new(ErrorCode::SecretParse, format!("{id}: 无法读取 {rel}: {e}"))
            })?;
            for (_, k, v) in parse_dotenv(&text)? {
                env.insert(k, v);
            }
        }
    }
    Ok((env, warnings))
}

/// 方向七·AI 原生：收集「输出脱敏」用的密钥值集合——主密钥文件 + 全部服务
/// env_file 的全部值 + `required` 声明 key 的用户环境变量值。
/// best-effort：单个文件缺失/解析失败不影响其余来源；去重返回（≥4 字符的
/// 长度过滤与替换语义由 `ai::sanitize` 负责）。只用于出口掩码，不做他途。
pub fn collect_redaction_values(spec: &SuperTaskFile, root: &Path) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    fn push(out: &mut Vec<String>, v: String) {
        if !v.is_empty() && !out.contains(&v) {
            out.push(v);
        }
    }
    if let Ok(Some(path)) = secret_file_path(spec, root) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(entries) = parse_dotenv(&text) {
                for (_, _, v) in entries {
                    push(&mut out, v);
                }
            }
        }
    }
    for svc in spec.services.values() {
        for rel in &svc.env_file {
            if crate::sandbox::assert_rel_safe(rel).is_err() {
                continue;
            }
            if let Ok(text) = std::fs::read_to_string(root.join(rel)) {
                if let Ok(entries) = parse_dotenv(&text) {
                    for (_, _, v) in entries {
                        push(&mut out, v);
                    }
                }
            }
        }
    }
    if let Some(sec) = &spec.secrets {
        for key in &sec.required {
            if let Ok(v) = std::env::var(key) {
                push(&mut out, v);
            }
        }
    }
    out
}

/// secrets.validate：required key 是否可解析（文件或用户环境）；env_file 语法检查。
pub fn validate(
    spec: &SuperTaskFile,
    root: &Path,
    service: Option<&str>,
) -> Result<crate::ipc::SecretsValidateOutput> {
    let git = ProcessRunner::default();
    let mut missing = Vec::new();
    let mut warnings = Vec::new();

    let (file_env, file_warnings) = load_file_layers(spec, root, None)?;
    warnings.extend(file_warnings);

    if let Some(sec) = &spec.secrets {
        for key in &sec.required {
            let in_file = file_env.contains_key(key);
            let in_env = std::env::var(key).is_ok();
            if !in_file && !in_env {
                missing.push(key.clone());
            }
        }
    }

    // env_file 范围：指定服务时只检查该服务
    let scope: Vec<(&String, &Vec<String>)> = match service {
        Some(id) => spec
            .services
            .get_key_value(id)
            .map(|(k, v)| vec![(k, &v.env_file)])
            .unwrap_or_default(),
        None => spec
            .services
            .iter()
            .map(|(k, v)| (k, &v.env_file))
            .collect(),
    };
    for (id, files) in scope {
        for rel in files {
            let p = root.join(rel);
            if !p.exists() {
                warnings.push(format!("{id}: env_file 不存在: {rel}"));
                continue;
            }
            let text = std::fs::read_to_string(&p)
                .map_err(|e| Error::new(ErrorCode::SecretParse, format!("无法读取 {rel}: {e}")))?;
            if let Err(e) = parse_dotenv(&text) {
                warnings.push(format!("{id}: {rel} 解析失败: {e}"));
            }
            if is_git_tracked(&git, root, &p) {
                warnings.push(format!(
                    "{id}: {rel} 已被 Git 跟踪，请尽快移出版本库（git rm --cached）"
                ));
            }
        }
    }
    Ok(crate::ipc::SecretsValidateOutput {
        ok: missing.is_empty(),
        missing,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn spec_with_secrets(yaml: &str) -> SuperTaskFile {
        crate::spec::parse_yaml(yaml).unwrap().0
    }

    fn ws_yaml(secrets: &str) -> String {
        format!("version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n{secrets}\n")
    }

    #[test]
    fn dotenv_subset_parse() {
        let text = "# comment\n\nA=1\nB = hello world \nC=\"x=y\"\nD='sq'\n";
        let out = parse_dotenv(text).unwrap();
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], (3, "A".into(), "1".into()));
        assert_eq!(out[1], (4, "B".into(), "hello world".into()));
        assert_eq!(out[2], (5, "C".into(), "x=y".into()));
        assert_eq!(out[3].1, "D");
    }

    #[test]
    fn dotenv_rejects_export_and_interpolation_with_line_no() {
        let e = parse_dotenv("A=1\nexport B=2\n").unwrap_err();
        assert_eq!(e.code(), ErrorCode::SecretParse);
        assert!(e.to_string().contains("2"));
        let e2 = parse_dotenv("A=$(rm -rf)\n").unwrap_err();
        assert_eq!(e2.code(), ErrorCode::SecretParse);
        let e3 = parse_dotenv("no equals sign\n").unwrap_err();
        assert_eq!(e3.code(), ErrorCode::SecretParse);
    }

    #[test]
    fn collect_redaction_values_unions_files_best_effort() {
        let dir = std::env::temp_dir().join(format!("st-sec-redact-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".env.local"), "API_KEY=abcd1234xyz\n").unwrap();
        fs::write(
            dir.join(".env.api"),
            "DB_PASSWORD=hunter2pass\nLOG_LEVEL=info\n",
        )
        .unwrap();
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n    env_file: [.env.api]\nsecrets:\n  backend: file\n  file: .env.local\n";
        let spec = spec_with_secrets(y);
        let vals = collect_redaction_values(&spec, &dir);
        assert!(vals.contains(&"abcd1234xyz".to_string()));
        assert!(vals.contains(&"hunter2pass".to_string()));
        assert!(
            vals.contains(&"info".to_string()),
            "env_file 值一并纳入（安全优先口径）"
        );
        let red = crate::ai::sanitize::Redactor::from_values(vals);
        let out = red.text("connect API_KEY=abcd1234xyz\nDB_PASSWORD=hunter2pass\nport: 8080");
        assert!(!out.contains("abcd1234xyz"));
        assert!(!out.contains("hunter2pass"));
        assert!(out.contains("port: 8080"));
        // env_file 缺失不致命：主密钥文件仍然生效
        let y2 = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n    env_file: [.env.missing]\nsecrets:\n  backend: file\n  file: .env.local\n";
        let vals2 = collect_redaction_values(&spec_with_secrets(y2), &dir);
        assert!(vals2.contains(&"abcd1234xyz".to_string()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn set_replaces_in_place_and_appends() {
        let dir = std::env::temp_dir().join(format!("st-sec-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let y = ws_yaml("secrets:\n  backend: file\n  file: .env.local\n");
        let spec = spec_with_secrets(&y);
        set_key(&spec, &dir, "DB_PASSWORD", "hunter2").unwrap();
        set_key(&spec, &dir, "OTHER", "v1").unwrap();
        set_key(&spec, &dir, "DB_PASSWORD", "hunter3").unwrap(); // 原位替换
        let text = fs::read_to_string(dir.join(".env.local")).unwrap();
        assert!(text.contains("DB_PASSWORD=hunter3"));
        assert!(text.contains("OTHER=v1"));
        assert_eq!(text.matches("DB_PASSWORD").count(), 1);
        // 跨行值拒绝
        assert!(set_key(&spec, &dir, "X", "a\nb").is_err());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn delete_removes_line_only() {
        let dir = std::env::temp_dir().join(format!("st-sec-del-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".env.local"), "A=1\nB=2\nC=3\n").unwrap();
        let y = ws_yaml("secrets:\n  backend: file\n  file: .env.local\n");
        let spec = spec_with_secrets(&y);
        delete_key(&spec, &dir, "B").unwrap();
        let text = fs::read_to_string(dir.join(".env.local")).unwrap();
        assert_eq!(text, "A=1\nC=3\n");
        // 文件不存在 → SECRET_FILE_MISSING
        fs::remove_file(dir.join(".env.local")).unwrap();
        assert_eq!(
            delete_key(&spec, &dir, "A").unwrap_err().code(),
            ErrorCode::SecretFileMissing
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn file_layers_service_env_file_missing_is_error() {
        let dir = std::env::temp_dir().join(format!("st-sec-lay-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".env.local"), "S=secret\n").unwrap();
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n    env_file:\n      - .env.local\n      - missing.env\nsecrets:\n  backend: file\n  file: .env.local\n";
        let spec = spec_with_secrets(y);
        assert_eq!(
            load_file_layers(&spec, &dir, Some("api"))
                .unwrap_err()
                .code(),
            ErrorCode::SecretFileMissing
        );
        // 去掉缺失文件后可加载且顺序正确（后文件覆盖）
        fs::write(dir.join(".env.local"), "S=secret\nDB=hunter\n").unwrap();
        let y2 = y.replace("      - missing.env\n", "");
        let spec2 = spec_with_secrets(&y2);
        let (env, _) = load_file_layers(&spec2, &dir, Some("api")).unwrap();
        assert_eq!(env.get("S").map(String::as_str), Some("secret"));
        assert_eq!(env.get("DB").map(String::as_str), Some("hunter"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn validate_reports_missing_required_names_only() {
        let dir = std::env::temp_dir().join(format!("st-sec-val-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".env.local"), "DB_PASSWORD=hunter\n").unwrap();
        let y = ws_yaml(
            "secrets:\n  backend: file\n  file: .env.local\n  required:\n    - DB_PASSWORD\n    - JWT_SECRET\n",
        );
        let spec = spec_with_secrets(&y);
        let out = validate(&spec, &dir, None).unwrap();
        assert!(!out.ok);
        assert_eq!(out.missing, vec!["JWT_SECRET".to_string()]);
        // 错误信息只有 key 名，没有值
        assert!(!format!("{out:?}").contains("hunter"));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn status_never_returns_values() {
        let dir = std::env::temp_dir().join(format!("st-sec-st-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(".env.local"), "DB_PASSWORD=hunter\n").unwrap();
        let y = ws_yaml(
            "secrets:\n  backend: file\n  file: .env.local\n  required:\n    - JWT_SECRET\n",
        );
        let spec = spec_with_secrets(&y);
        let out = status(&spec, &dir).unwrap();
        assert_eq!(out.file.as_deref(), Some(".env.local"));
        assert!(out.keys.iter().any(|k| k.key == "DB_PASSWORD" && k.present));
        assert!(!format!("{out:?}").contains("hunter"), "状态不得包含值");
        fs::remove_dir_all(&dir).ok();
    }
}
