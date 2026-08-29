//! `--format json` 输出解析（规格 §4.2/§5.3/§9）：
//! - `docker compose ps --format json <svc>` → 容器状态（状态轮询、docker.ps）
//! - `docker images --format json` → 镜像列表（docker.images，只读）
//!
//! compose v2 新版输出 JSON 数组，旧版输出 NDJSON（每行一个对象）；两者都容忍。

use serde_json::Value;

use crate::ipc::{ContainerSummary, ImageSummary};

/// `compose ps` 里的单个容器。`exit_code` 供崩溃通知（外部退出 → exited/crash）使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsContainer {
    /// compose 服务名（旧版输出无 Service 字段时从 Name 推导）。
    pub service: String,
    pub container_id: String,
    pub name: String,
    pub image: String,
    /// 小写：running / exited / created / paused / restarting …
    pub state: String,
    pub health: Option<String>,
    pub ports: Vec<u16>,
    pub exit_code: Option<i32>,
}

impl PsContainer {
    pub fn exited(&self) -> bool {
        self.state.eq_ignore_ascii_case("exited")
    }

    /// IPC 载荷（§9 ContainerSummary）。
    pub fn summary(&self) -> ContainerSummary {
        ContainerSummary {
            service: self.service.clone(),
            container_id: self.container_id.clone(),
            image: self.image.clone(),
            state: self.state.to_ascii_lowercase(),
            health: self.health.clone(),
            ports: self.ports.clone(),
        }
    }
}

pub fn parse_ps(stdout: &str) -> Vec<PsContainer> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let items: Vec<Value> = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<Value>>(trimmed).unwrap_or_default()
    } else {
        // NDJSON：每行一个对象；解析失败的行跳过
        trimmed
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .collect()
    };
    items.into_iter().filter_map(parse_ps_item).collect()
}

fn parse_ps_item(v: Value) -> Option<PsContainer> {
    let name = str_of(&v, "Name").unwrap_or_default();
    let id = str_of(&v, "ID").unwrap_or_default();
    if name.is_empty() && id.is_empty() {
        return None;
    }
    let service = str_of(&v, "Service").unwrap_or_else(|| derive_service(&name));
    let ports = v
        .get("Publishers")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|p| {
                    p.get("PublishedPort")
                        .and_then(Value::as_u64)
                        .and_then(|x| u16::try_from(x).ok())
                })
                .collect()
        })
        .unwrap_or_default();
    Some(PsContainer {
        container_id: id,
        name,
        service,
        image: str_of(&v, "Image").unwrap_or_default(),
        state: str_of(&v, "State").unwrap_or_else(|| "unknown".into()),
        health: str_of(&v, "Health"),
        ports,
        exit_code: v.get("ExitCode").and_then(Value::as_i64).map(|x| x as i32),
    })
}

/// 容器名 `<project>-<service>-<replica>` → 服务名：先去掉尾部副本序号，
/// 再剥掉首段 project。无分隔符时整个名字兜底（不影响状态判断）。
fn derive_service(name: &str) -> String {
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = base.strip_suffix('-').unwrap_or(base);
    if let Some(idx) = base.find('-') {
        let rest = &base[idx + 1..];
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    name.to_string()
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

/// `docker images --format json` → ImageSummary（NDJSON，每行一个镜像对象）。
/// 两种真实形状都容忍：
/// - 旧/inspect 形状：`RepoTags: ["r:t", …]`、`Size: 123`（字节）、`CreatedAt` RFC3339；
/// - docker 29+（2026 实测）：`Repository`/`Tag` 单值字段、`Size: "194MB"`（人类可读
///   字符串）、`CreatedAt: "2026-08-26 16:46:11 +0800 CST"`。`<none>` 原样保留。
pub fn parse_images(stdout: &str) -> Vec<ImageSummary> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id = str_of(&v, "ID").unwrap_or_default();
        let size = size_bytes_of(&v);
        let created_ms = v
            .get("CreatedAt")
            .and_then(Value::as_str)
            .and_then(created_ms_of);
        // 形状 A：RepoTags 数组；形状 B：Repository/Tag 单字段
        let mut tags: Vec<String> = v
            .get("RepoTags")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        if tags.is_empty() {
            if let (Some(r), Some(t)) = (str_of(&v, "Repository"), str_of(&v, "Tag")) {
                tags.push(format!("{r}:{t}"));
            }
        }
        if tags.is_empty() {
            out.push(ImageSummary {
                repository: "<none>".into(),
                tag: "<none>".into(),
                id: id.clone(),
                size_bytes: size,
                created_ms,
            });
            continue;
        }
        for tag in tags {
            // docker 29 的 <none>:<none>（悬空镜像）直接保留
            if tag == "<none>:<none>" {
                out.push(ImageSummary {
                    repository: "<none>".into(),
                    tag: "<none>".into(),
                    id: id.clone(),
                    size_bytes: size,
                    created_ms,
                });
                continue;
            }
            let (repository, t) = match tag.rsplit_once(':') {
                // registry 带端口时（host:5000/img:tag）按最后一个 ':' 拆，
                // 拆出的 "tag" 含 '/' 视为仓库一部分
                Some((r, t)) if !t.contains('/') => (r.to_string(), t.to_string()),
                _ => (tag.clone(), "latest".into()),
            };
            out.push(ImageSummary {
                repository,
                tag: t,
                id: id.clone(),
                size_bytes: size,
                created_ms,
            });
        }
    }
    out
}

/// `Size`：数字（字节）或人类可读字符串（"194MB"、"1.23GB"，SI 千进制；docker 29）。
fn size_bytes_of(v: &Value) -> Option<u64> {
    match v.get("Size") {
        Some(Value::Number(n)) => n.as_u64(),
        Some(Value::String(s)) => parse_human_size(s),
        _ => None,
    }
}

/// "194MB" → 194_000_000。SI（kB/MB/GB/TB）与二进制（KiB/MiB/GiB/TiB）都认。
fn parse_human_size(s: &str) -> Option<u64> {
    let s = s.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(s.len());
    let val: f64 = s[..split].trim().parse().ok()?;
    let mult = match s[split..].trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1.0,
        "kb" => 1e3,
        "mb" => 1e6,
        "gb" => 1e9,
        "tb" => 1e12,
        "pb" => 1e15,
        "kib" => 1_024.0,
        "mib" => 1_048_576.0,
        "gib" => 1_073_741_824.0,
        "tib" => 1_099_511_627_776.0,
        _ => return None,
    };
    Some((val * mult) as u64)
}

/// `CreatedAt`：RFC3339（inspect 形状）或 docker 人类格式
/// "2026-08-26 16:46:11 +0800 CST"（docker 29）。换算为 epoch ms。
fn created_ms_of(s: &str) -> Option<u64> {
    parse_rfc3339_ms(s).or_else(|| parse_docker_created_at_ms(s))
}

/// "2026-08-26 16:46:11 +0800 CST" → epoch ms。时区 `±HHMM`/`±HH:MM`，时区名忽略。
fn parse_docker_created_at_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b' '
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let minute: i64 = s.get(14..16)?.parse().ok()?;
    let second: i64 = s.get(17..19)?.parse().ok()?;
    let offset = s.get(19..)?.trim();
    let offset_min = match offset.split_ascii_whitespace().next() {
        Some("Z") | Some("z") => 0,
        Some(tok) => parse_offset_min(tok)?,
        None => return None,
    };
    epoch_ms_from_ymd_hms(year, month, day, hour, minute, second, offset_min)
}

fn parse_offset_min(tok: &str) -> Option<i64> {
    let b = tok.as_bytes();
    if b.len() < 5 || (b[0] != b'+' && b[0] != b'-') {
        return None;
    }
    let sign: i64 = if b[0] == b'+' { 1 } else { -1 };
    let oh: i64 = tok.get(1..3)?.parse().ok()?;
    let om: i64 = if b[3] == b':' {
        tok.get(4..6)?.parse().ok()?
    } else {
        tok.get(3..5)?.parse().ok()?
    };
    Some(sign * (oh * 60 + om))
}

fn epoch_ms_from_ymd_hms(
    year: i64,
    month: u32,
    day: u32,
    hour: i64,
    minute: i64,
    second: i64,
    offset_min: i64,
) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // 天数（civil → days since epoch，Howard Hinnant 算法）
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let secs = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_min * 60;
    if secs < 0 {
        return None;
    }
    Some((secs * 1000) as u64)
}

/// RFC3339（`2026-01-02T03:04:05.123456789Z` / `+08:00`）→ epoch ms。
/// 不引时间库（依赖最小化）；解析失败返回 None。
fn parse_rfc3339_ms(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b't') {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: u32 = s.get(5..7)?.parse().ok()?;
    let day: u32 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let minute: i64 = s.get(14..16)?.parse().ok()?;
    let second: i64 = s.get(17..19)?.parse().ok()?;
    let mut idx = 19;
    let mut frac_ms: i64 = 0;
    if b.get(idx) == Some(&b'.') {
        let start = idx + 1;
        let mut end = start;
        while b.get(end).is_some_and(u8::is_ascii_digit) {
            end += 1;
        }
        if end == start {
            return None;
        }
        // 取前 3 位毫秒
        let digits = &s[start..end];
        let mut padded = digits.to_string();
        while padded.len() < 3 {
            padded.push('0');
        }
        frac_ms = padded[..3].parse().ok()?;
        idx = end;
    }
    // 时区：Z 或 ±HH:MM
    let offset_min: i64 = match b.get(idx) {
        Some(&b'Z') | Some(&b'z') => 0,
        Some(&b'+') | Some(&b'-') => {
            let sign: i64 = if b[idx] == b'+' { 1 } else { -1 };
            let oh: i64 = s.get(idx + 1..idx + 3)?.parse().ok()?;
            if b.get(idx + 3) != Some(&b':') {
                return None;
            }
            let om: i64 = s.get(idx + 4..idx + 6)?.parse().ok()?;
            sign * (oh * 60 + om)
        }
        _ => return None,
    };
    let base = epoch_ms_from_ymd_hms(year, month, day, hour, minute, second, offset_min)?;
    Some(base + frac_ms as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ps_array_output() {
        let out = r#"[
          {"ID":"abc123","Name":"mall-redis-1","Image":"redis:7","State":"running","Health":"healthy",
           "Publishers":[{"URL":"0.0.0.0","Target":6379,"PublishedPort":6379,"Protocol":"tcp"}]},
          {"ID":"def456","Name":"mall-mysql-1","Image":"mysql:8","State":"exited","ExitCode":137}
        ]"#;
        let items = parse_ps(out);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].service, "redis");
        assert_eq!(items[0].container_id, "abc123");
        assert_eq!(items[0].state, "running");
        assert_eq!(items[0].health.as_deref(), Some("healthy"));
        assert_eq!(items[0].ports, vec![6379]);
        assert!(!items[0].exited());
        assert_eq!(items[1].service, "mysql");
        assert!(items[1].exited());
        assert_eq!(items[1].exit_code, Some(137));
        assert_eq!(items[1].ports, Vec::<u16>::new());
    }

    #[test]
    fn parse_ps_ndjson_output() {
        let out = concat!(
            r#"{"ID":"a1","Name":"mall-redis-1","Image":"redis:7","State":"exited","ExitCode":0}"#,
            "\n",
            r#"{"ID":"b2","Service":"mysql","Name":"db","Image":"mysql:8","State":"running"}"#,
        );
        let items = parse_ps(out);
        assert_eq!(items.len(), 2);
        assert!(items[0].exited());
        assert_eq!(items[1].service, "mysql");
        assert_eq!(items[1].state, "running");
    }

    #[test]
    fn parse_ps_tolerates_garbage_and_empty() {
        assert!(parse_ps("").is_empty());
        assert!(parse_ps("ok").is_empty()); // FakeDockerRunner 默认输出
        assert!(parse_ps("not json at all").is_empty());
    }

    #[test]
    fn ps_summary_matches_ipc_shape() {
        let items = parse_ps(
            r#"[{"ID":"abc","Name":"mall-redis-1","Service":"redis","Image":"redis:7","State":"RUNNING","Publishers":[{"PublishedPort":6379}]}]"#,
        );
        let s = items[0].summary();
        assert_eq!(s.service, "redis");
        assert_eq!(s.container_id, "abc");
        assert_eq!(s.image, "redis:7");
        assert_eq!(s.state, "running");
        assert_eq!(s.ports, vec![6379]);
        assert!(s.health.is_none());
    }

    #[test]
    fn parse_images_ndjson_with_tags() {
        let out = concat!(
            r#"{"CreatedAt":"2026-01-02T03:04:05.123456789Z","ID":"sha256:aaa","RepoTags":["mall-user:local","mall-user:1.0"],"Size":123456789}"#,
            "\n",
            r#"{"CreatedAt":"2026-02-03T00:00:00Z","ID":"sha256:bbb","RepoTags":[],"Size":1}"#,
        );
        let images = parse_images(out);
        assert_eq!(images.len(), 3);
        assert_eq!(images[0].repository, "mall-user");
        assert_eq!(images[0].tag, "local");
        assert_eq!(images[1].tag, "1.0");
        assert_eq!(images[0].size_bytes, Some(123456789));
        assert!(images[0].created_ms.is_some());
        // 无 tag → <none>
        assert_eq!(images[2].repository, "<none>");
        assert_eq!(images[2].id, "sha256:bbb");
    }

    #[test]
    fn parse_images_registry_port_tag() {
        let out = r#"{"ID":"sha256:ccc","RepoTags":["host:5000/img:dev"],"Size":9}"#;
        let images = parse_images(out);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].repository, "host:5000/img");
        assert_eq!(images[0].tag, "dev");
    }

    #[test]
    fn parse_images_docker29_shape_real_output() {
        // 2026-08 真机实测（docker 29.7.2）：Repository/Tag 单字段、Size 人类可读、
        // CreatedAt 人类时间格式；\u003cnone\u003e 是 docker 输出的字面转义
        let out = concat!(
            r#"{"Containers":"0","CreatedAt":"2026-08-26 16:46:11 +0800 CST","CreatedSince":"2 days ago","#,
            r#""Digest":"\u003cnone\u003e","ID":"3caf5504371b","Repository":"nest-py-isbn","SharedSize":"N/A","#,
            r#""Size":"194MB","Tag":"local","UniqueSize":"N/A"}"#,
            "\n",
            r#"{"Containers":"0","CreatedAt":"2026-08-25 21:51:48 +0800 CST","Digest":"\u003cnone\u003e","#,
            r#""ID":"cff6bc3ed1a6","Repository":"weishaw/sub2api","Size":"345MB","Tag":"latest"}"#,
        );
        let images = parse_images(out);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].repository, "nest-py-isbn");
        assert_eq!(images[0].tag, "local");
        assert_eq!(images[0].id, "3caf5504371b");
        assert_eq!(images[0].size_bytes, Some(194_000_000));
        assert!(images[0].created_ms.is_some());
        assert_eq!(images[1].repository, "weishaw/sub2api");
        assert_eq!(images[1].tag, "latest");
        assert_eq!(images[1].size_bytes, Some(345_000_000));
    }

    #[test]
    fn parse_images_docker29_dangling_is_none_none() {
        let out = r#"{"CreatedAt":"2026-08-19 00:58:27 +0000 UTC","ID":"becdda6c7f4b","Repository":"<none>","Size":"12.3MB","Tag":"<none>"}"#;
        let images = parse_images(out);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].repository, "<none>");
        assert_eq!(images[0].tag, "<none>");
        assert_eq!(images[0].size_bytes, Some(12_300_000));
    }

    #[test]
    fn parse_human_size_units() {
        assert_eq!(parse_human_size("194MB"), Some(194_000_000));
        assert_eq!(parse_human_size("1.23GB"), Some(1_230_000_000));
        assert_eq!(parse_human_size("568B"), Some(568));
        assert_eq!(parse_human_size("12KiB"), Some(12 * 1024));
        assert_eq!(parse_human_size("1.5MiB"), Some((1.5 * 1_048_576.0) as u64));
        assert_eq!(parse_human_size("N/A"), None);
        assert_eq!(parse_human_size(""), None);
    }

    #[test]
    fn created_at_human_format_matches_rfc3339() {
        // 人类格式与等价 RFC3339 应换算到同一时刻
        let human = parse_docker_created_at_ms("2026-08-26 16:46:11 +0800 CST");
        let rfc = parse_rfc3339_ms("2026-08-26T16:46:11+08:00");
        assert_eq!(human, rfc);
        assert!(human.is_some());
        // UTC 时区名
        assert_eq!(
            parse_docker_created_at_ms("2026-01-02 03:04:05 +0000 UTC"),
            parse_rfc3339_ms("2026-01-02T03:04:05Z")
        );
        // 2026-08-26 16:46:11 +0800 → 08:46:11 UTC = 1787733971
        assert_eq!(human, Some(1_787_733_971_000));
        // 非法输入
        assert_eq!(parse_docker_created_at_ms("garbage"), None);
        assert_eq!(parse_docker_created_at_ms("2026-08-26T16:46:11Z"), None); // 留给 RFC3339
    }
}
