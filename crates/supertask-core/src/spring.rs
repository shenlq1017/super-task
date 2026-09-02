//! `spring.inspect` —— Spring Boot 项目自身配置的静态扫描（只读）。
//!
//! 背景：运行页「生效环境」只覆盖 supertask 注入链（§6.3 五层），项目里的
//! `application.yml` / `application.properties` 对它不可见。本模块解析服务目录
//! `src/main/resources` 下 application 基础文件与 `application-<profile>.*`，
//! flatten 成点分键供 UI 展示。
//!
//! 局限（前端需明示「静态解析」）：不解析 `${}` 占位符的运行时取值、
//! spring.config.import、profile 激活规则；同名键跨文件并存（各带来源文件），
//! 不模拟 Spring 的覆盖顺序。

use std::fs;
use std::path::Path;

use serde::Serialize;

/// 展示条数上限（防超大配置灌满 IPC 载荷）。
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpringConfigEntry {
    pub key: String,
    pub value: String,
    /// 相对工作区根的文件路径（`/` 分隔），供来源徽章。
    pub file: String,
    /// 敏感键（password/secret/token 等）值已遮蔽。
    pub masked: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpringConfigOutput {
    pub id: String,
    /// 基础文件（非 profile）中的 `server.port`；缺省/非数字/占位符 → None。
    pub server_port: Option<u16>,
    pub entries: Vec<SpringConfigEntry>,
    pub warnings: Vec<String>,
}

/// 在服务可能的目录（launch 的 cwd、maven/gradle 的 module、根）里找第一个
/// `src/main/resources` 目录并解析其中全部 application 配置文件。
pub fn inspect(id: &str, root: &Path, search_dirs: &[String]) -> SpringConfigOutput {
    let mut out = SpringConfigOutput {
        id: id.to_string(),
        server_port: None,
        entries: Vec::new(),
        warnings: Vec::new(),
    };
    let Some(res_dir) = search_dirs
        .iter()
        .map(|d| root.join(d).join("src").join("main").join("resources"))
        .find(|p| p.is_dir())
    else {
        return out;
    };
    let Ok(rd) = fs::read_dir(&res_dir) else {
        return out;
    };
    let mut files: Vec<(String, std::path::PathBuf)> = rd
        .filter_map(|e| e.ok())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .filter(|(name, _)| is_config_file(name))
        .collect();
    // 基础文件在前、profile 文件其后，各自按名排序，输出稳定
    files.sort_by(|a, b| base_rank(&a.0).cmp(&base_rank(&b.0)).then(a.0.cmp(&b.0)));
    for (name, path) in &files {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = fs::read_to_string(path) else {
            out.warnings.push(format!("{name}: 读取失败"));
            continue;
        };
        let pairs = if name.ends_with(".properties") {
            parse_properties(&text)
        } else {
            parse_yaml_docs(&text)
        };
        let Ok(pairs) = pairs else {
            out.warnings.push(format!(
                "{name}: 解析失败（{}），该文件已跳过",
                pairs.unwrap_err()
            ));
            continue;
        };
        let is_base = base_rank(name) == 0;
        for (key, value) in pairs {
            if is_base && out.server_port.is_none() && key == "server.port" {
                out.server_port = value.trim().parse::<u16>().ok();
            }
            if out.entries.len() >= MAX_ENTRIES {
                out.warnings
                    .push(format!("配置项超过 {MAX_ENTRIES} 条，已截断"));
                return out;
            }
            let masked = is_sensitive(&key);
            out.entries.push(SpringConfigEntry {
                key,
                value: if masked {
                    "••••••".into()
                } else {
                    value
                },
                file: rel.clone(),
                masked,
            });
        }
    }
    out
}

/// application / application-<profile> × yml|yaml|properties。
fn is_config_file(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("application") else {
        return false;
    };
    let Some((stem, ext)) = rest.rsplit_once('.') else {
        return false;
    };
    matches!(ext, "yml" | "yaml" | "properties") && (stem.is_empty() || stem.starts_with('-'))
}

/// 0 = 基础文件，1 = profile 文件。
fn base_rank(name: &str) -> u8 {
    if name.starts_with("application-") {
        1
    } else {
        0
    }
}

fn is_sensitive(key: &str) -> bool {
    let k = key.to_lowercase();
    [
        "password",
        "secret",
        "token",
        "credential",
        "privatekey",
        "private-key",
        "accesskey",
        "access-key",
    ]
    .iter()
    .any(|needle| k.contains(needle))
}

fn parse_properties(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        } else if let Some((k, v)) = line.split_once(':') {
            // properties 也接受 key: value 形式
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    Ok(out)
}

/// yml 多文档（`---` 分隔）逐段解析；段间合并（少见用法，静态展示够用）。
fn parse_yaml_docs(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut docs: Vec<String> = Vec::new();
    let mut cur = String::new();
    for line in text.lines() {
        let t = line.trim_end();
        if t == "---" {
            docs.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push_str(line);
        cur.push('\n');
    }
    docs.push(cur);
    let mut out = Vec::new();
    for doc in docs {
        if doc.trim().is_empty() {
            continue;
        }
        let value: serde_yaml::Value = serde_yaml::from_str(&doc).map_err(|e| e.to_string())?;
        flatten_value("", &value, &mut out);
    }
    Ok(out)
}

fn flatten_value(prefix: &str, v: &serde_yaml::Value, out: &mut Vec<(String, String)>) {
    match v {
        serde_yaml::Value::Null => {}
        serde_yaml::Value::Bool(b) => push_kv(prefix, b.to_string(), out),
        serde_yaml::Value::Number(n) => push_kv(prefix, n.to_string(), out),
        serde_yaml::Value::String(s) => push_kv(prefix, s.clone(), out),
        serde_yaml::Value::Sequence(seq) => {
            for (i, item) in seq.iter().enumerate() {
                flatten_value(&format!("{prefix}[{i}]"), item, out);
            }
        }
        serde_yaml::Value::Mapping(map) => {
            for (k, val) in map {
                let key = match k {
                    serde_yaml::Value::String(s) => s.clone(),
                    serde_yaml::Value::Number(n) => n.to_string(),
                    serde_yaml::Value::Bool(b) => b.to_string(),
                    other => format!("{other:?}"),
                };
                let joined = if prefix.is_empty() {
                    key
                } else {
                    format!("{prefix}.{key}")
                };
                // 空 map/空 sequence 也要露一行，提示「该节点存在但无内容」
                match val {
                    serde_yaml::Value::Mapping(m) if m.is_empty() => {
                        push_kv(&joined, "{}".into(), out)
                    }
                    serde_yaml::Value::Sequence(s) if s.is_empty() => {
                        push_kv(&joined, "[]".into(), out)
                    }
                    _ => flatten_value(&joined, val, out),
                }
            }
        }
        serde_yaml::Value::Tagged(tagged) => flatten_value(prefix, &tagged.value, out),
    }
}

fn push_kv(prefix: &str, value: String, out: &mut Vec<(String, String)>) {
    if !prefix.is_empty() {
        out.push((prefix.to_string(), value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("st-spring-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn flattens_yml_properties_and_profiles() {
        let root = tmpdir("flat");
        let res = root.join("backend").join("src/main/resources");
        fs::create_dir_all(&res).unwrap();
        fs::write(
            res.join("application.yml"),
            "server:\n  port: 9090\nspring:\n  application:\n    name: demo\n  datasource:\n    password: hunter2\ndepends-on: [a, b]\nempty: {}\n",
        )
        .unwrap();
        fs::write(
            res.join("application.properties"),
            "# comment\nmanagement.endpoints.web.exposure.include=health,info\n",
        )
        .unwrap();
        fs::write(
            res.join("application-prod.yml"),
            "server:\n  port: 8080\nlog: prod-only\n",
        )
        .unwrap();

        let out = inspect("api", &root, &["backend".into(), ".".into()]);
        assert_eq!(
            out.server_port,
            Some(9090),
            "server.port 只取基础文件，profile 不覆盖"
        );
        assert_eq!(out.warnings, Vec::<String>::new());
        let get = |k: &str| out.entries.iter().find(|e| e.key == k);
        assert_eq!(get("spring.application.name").unwrap().value, "demo");
        let pw = get("spring.datasource.password").unwrap();
        assert!(pw.masked && pw.value == "••••••");
        assert_eq!(get("depends-on[1]").unwrap().value, "b");
        assert_eq!(get("empty").unwrap().value, "{}");
        assert_eq!(
            get("management.endpoints.web.exposure.include")
                .unwrap()
                .value,
            "health,info"
        );
        // 同名键跨文件并存，各带来源；基础在前
        let ports: Vec<_> = out
            .entries
            .iter()
            .filter(|e| e.key == "server.port")
            .collect();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].file, "backend/src/main/resources/application.yml");
        assert_eq!(
            ports[1].file,
            "backend/src/main/resources/application-prod.yml"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn multi_doc_bad_yaml_and_missing_dir() {
        let root = tmpdir("docs");
        let res = root.join("src/main/resources");
        fs::create_dir_all(&res).unwrap();
        fs::write(res.join("application.yml"), "a: 1\n---\nb: two\n").unwrap();
        fs::write(res.join("application-bad.yml"), "\tbroken: [unclosed\n").unwrap();

        let out = inspect("api", &root, &[".".into()]);
        assert_eq!(out.server_port, None);
        assert!(out.entries.iter().any(|e| e.key == "a" && e.value == "1"));
        assert!(
            out.entries.iter().any(|e| e.key == "b" && e.value == "two"),
            "多文档第二段应并入"
        );
        assert!(out
            .warnings
            .iter()
            .any(|w| w.contains("application-bad.yml")));

        // 目录不存在 → 干净空结果（非错误）
        let missing = inspect("api", &root.join("nowhere"), &[".".into()]);
        assert!(missing.entries.is_empty() && missing.warnings.is_empty());
        let _ = fs::remove_dir_all(&root);
    }
}
