//! 2.1 §4.3 数据卫生：进 prompt 前的掩码（sanitize）。
//!
//! 三类掩码对象（spec §4.3）：
//! 1. workspace secret 值 / `supertask.ai` key（调用方通过 `secret_values` 传入，精确子串替换）；
//! 2. 形似 token / password / authorization 的行 → 整行替换为 [`REDACTED`]；
//! 3. 短值（< 4 字符）不参与精确替换，避免把端口号等普通数字打穿文本。
//!
//! 方向七·AI 原生：[`Redactor`] 把值集合缓存起来，供引擎诊断与 MCP/CLI 出口
//! 统一脱敏复用（同一套掩码语义，幂等）。

use std::path::Path;

/// 掩码后的占位文本（spec §4.3）。
pub const REDACTED: &str = "<redacted>";

/// 参与精确子串替换的最小值长度：更短的值（如 `1`）会误伤普通数字。
const MIN_EXACT_LEN: usize = 4;

fn is_sensitive_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some((lhs, _)) = trimmed.split_once(['=', ':']) else {
        // Bearer 头单独识别：`Authorization: Bearer xxx` 已被上面覆盖；
        // 这里兜底裸 `Bearer <token>` 行。
        return trimmed.len() > 16 && trimmed[..6].eq_ignore_ascii_case("bearer");
    };
    let name = lhs.trim();
    let name = name.strip_prefix("export ").map(str::trim).unwrap_or(name);
    let lower = name.to_lowercase();
    // 名字本身带敏感词（按词边界）才整行掩码（如 `password: hunter2`、`AUTH_TOKEN=xx`）；
    // 普通行（如 `port: 8080`、`tokenizer: ik`）保持原样。
    let sensitive_name = [
        "password", "passwd", "pwd", "token", "secret", "apikey", "api_key",
    ]
    .iter()
    .any(|w| contains_keyword(&lower, w));
    // 值里出现 Bearer 凭据（`Authorization: Bearer eyJ...`）时同样视为敏感行
    let bearer_value = trimmed.len() >= lhs.len() + 8
        && trimmed[lhs.len() + 1..]
            .trim()
            .to_lowercase()
            .starts_with("bearer ");
    sensitive_name || bearer_value
}

/// 词边界的关键词匹配：前后不能是字母/数字（`tokenizer` 不算 `token`）。
pub(crate) fn contains_keyword(haystack: &str, keyword: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = haystack[from..].find(keyword) {
        let start = from + pos;
        let end = start + keyword.len();
        let before_ok = start == 0
            || !haystack[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        let after_ok = end == haystack.len()
            || !haystack[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// 掩码一整段文本：先做值精确替换，再按行做敏感行整行替换。
/// 确定性纯函数；断言「原始值不出现在结果」的单测见下方。
pub fn sanitize_text(text: &str, secret_values: &[String]) -> String {
    // 值替换一次编译替换表：长值优先，避免短值先替换破坏长值
    let mut values: Vec<&String> = secret_values
        .iter()
        .filter(|v| v.len() >= MIN_EXACT_LEN)
        .collect();
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    let mut out = text.to_string();
    for v in values {
        if !v.is_empty() {
            out = out.replace(v.as_str(), REDACTED);
        }
    }
    out.lines()
        .map(|line| {
            if is_sensitive_line(line) {
                REDACTED.to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 日志尾部截断（spec §4.2 场景 1）：最多 `max_lines` 行且最多 `max_bytes` 字节，
/// 超限保尾部（最新的日志在尾部）。
pub fn tail_truncate(lines: &[String], max_lines: usize, max_bytes: usize) -> Vec<String> {
    let mut out: Vec<&String> = lines.iter().rev().take(max_lines).collect();
    out.reverse();
    let mut total = 0usize;
    let mut start = out.len();
    for (i, line) in out.iter().enumerate().rev() {
        total += line.len() + 1;
        if total > max_bytes {
            start = i + 1;
            break;
        }
        start = i;
    }
    out[start..].iter().map(|s| (*s).clone()).collect()
}

/// 方向七·AI 原生：可复用的输出脱敏器。缓存声明密钥值集合，供引擎诊断与
/// MCP/CLI 出口统一调用；`text` 语义与 [`sanitize_text`] 完全一致（幂等）。
pub struct Redactor {
    values: Vec<String>,
}

impl Redactor {
    pub fn from_values(values: Vec<String>) -> Self {
        Self { values }
    }

    /// best-effort：从工作区根加载 yaml 并收集声明密钥值。
    /// yaml 缺失/非法 → 空值集（敏感行整行掩码仍然生效）。
    pub fn for_workspace(root: &Path) -> Self {
        Self::from_values(collect_workspace_values(root))
    }

    pub fn text(&self, s: &str) -> String {
        sanitize_text(s, &self.values)
    }

    /// 递归脱敏 JSON 里的所有字符串值（MCP 出口统一调用；幂等）。
    pub fn redact_json(&self, v: &mut serde_json::Value) {
        match v {
            serde_json::Value::String(s) => *s = self.text(s),
            serde_json::Value::Array(items) => {
                for item in items {
                    self.redact_json(item);
                }
            }
            serde_json::Value::Object(map) => {
                for val in map.values_mut() {
                    self.redact_json(val);
                }
            }
            _ => {}
        }
    }
}

/// 工作区级密钥值收集（引擎未打开时的出口兜底）：加载根目录 yaml 后交给
/// [`crate::secrets::collect_redaction_values`]；任何失败 → 空集合。
pub fn collect_workspace_values(root: &Path) -> Vec<String> {
    match crate::engine::load_yaml_at(root) {
        Ok((_, _, spec, _)) => crate::secrets::collect_redaction_values(&spec, root),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_secret_values_exactly() {
        let values = vec!["hunter2secret".to_string(), "sk-abc123def456".to_string()];
        let out = sanitize_text(
            "DB_PASSWORD=hunter2secret\nkey: sk-abc123def456\nport: 8080\n",
            &values,
        );
        assert!(out.contains(REDACTED));
        assert!(!out.contains("hunter2secret"));
        assert!(!out.contains("sk-abc123def456"));
        assert!(out.contains("port: 8080"), "普通行不受影响");
    }

    #[test]
    fn short_values_are_not_replaced() {
        let out = sanitize_text(
            "a=1\nb=22\nc=333\n",
            &["1".to_string(), "22".to_string(), "333".to_string()],
        );
        assert_eq!(out, "a=1\nb=22\nc=333");
    }

    #[test]
    fn sensitive_lines_masked_whole() {
        let out = sanitize_text(
            "password: hunter2\nAPI_TOKEN=abcd1234\nport: 8080\nAuthorization: Bearer eyJhbGc\n",
            &[],
        );
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], REDACTED);
        assert_eq!(lines[1], REDACTED);
        assert_eq!(lines[2], "port: 8080");
        assert_eq!(lines[3], REDACTED);
        assert!(!out.contains("hunter2"));
        assert!(!out.contains("abcd1234"));
        assert!(!out.contains("eyJhbGc"));
    }

    #[test]
    fn normal_names_not_masked() {
        let out = sanitize_text("port: 8080\ntokenizer: ik-max-word\n", &[]);
        assert_eq!(out, "port: 8080\ntokenizer: ik-max-word");
    }

    #[test]
    fn tail_truncate_respects_lines_and_bytes() {
        let lines: Vec<String> = (0..300).map(|i| format!("line-{i:04}")).collect();
        let out = tail_truncate(&lines, 200, 32 * 1024);
        assert_eq!(out.len(), 200);
        assert_eq!(out[0], "line-0100");
        assert_eq!(out[199], "line-0299");

        let long: Vec<String> = (0..100).map(|_| "x".repeat(200)).collect();
        let out2 = tail_truncate(&long, 200, 1000);
        assert!(out2.len() < 100, "字节上限生效");
        assert_eq!(out2.last().unwrap().len(), 200, "保尾部");
        let total: usize = out2.iter().map(|l| l.len() + 1).sum();
        assert!(total <= 1000 + 200, "最后一行允许保留，总字节受控");
    }

    #[test]
    fn redactor_is_idempotent_and_masks_values() {
        let red = Redactor::from_values(vec!["abcd1234xyz".to_string()]);
        let once = red.text("connecting API_TOKEN=abcd1234xyz ok");
        assert!(!once.contains("abcd1234xyz"));
        assert!(red.text(&once) == once, "重复脱敏应幂等");
    }

    #[test]
    fn redactor_json_walk_masks_nested_strings() {
        let red = Redactor::from_values(vec!["abcd1234xyz".to_string()]);
        let mut v = serde_json::json!({
            "a": "token=abcd1234xyz",
            "list": ["plain", { "deep": "abcd1234xyz" }],
            "n": 42,
        });
        red.redact_json(&mut v);
        assert!(!v.to_string().contains("abcd1234xyz"));
        assert_eq!(v["list"][0], "plain");
        assert_eq!(v["n"], 42);
    }

    #[test]
    fn collect_workspace_values_tolerates_missing_yaml() {
        let dir = crate::sandbox::test_temp_dir().join("st-sanitize-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert!(Redactor::for_workspace(&dir)
            .text("password: whatever")
            .contains(REDACTED));
        assert_eq!(
            Redactor::for_workspace(&dir).text("port: 8080"),
            "port: 8080"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
