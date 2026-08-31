//! 2.1 真实端点冒烟（Phase 6.2 自动化可重复部分，**opt-in**）：
//! 读取本机真实 appdata（`%APPDATA%/SuperTask/app.json`）的默认 AI 配置与
//! secrets key，各发一次 `explain_logs` / `config_suggest` 最小请求。
//! 消耗真实配额，默认 ignored，仅人工运行：
//!
//! ```text
//! cargo test -p supertask-core --test real_ai_smoke -- --ignored --nocapture
//! ```
//!
//! 未配置 AI（无默认配置或缺 key）时打印提示并直接通过——不作为失败。

use std::path::PathBuf;

use supertask_core::appdata;
use supertask_core::ai::{
    complete, read_key, AiTask, CompleteRequest, UreqAiHttp,
};

fn load_real_appdata() -> Option<(appdata::AppData, PathBuf)> {
    let base = std::env::var("APPDATA").ok()?;
    let dir = PathBuf::from(base).join("SuperTask");
    let path = dir.join("app.json");
    if !path.is_file() {
        return None;
    }
    Some((appdata::load_at(&path), path))
}

#[test]
#[ignore = "真实端点冒烟：消耗真实配额，仅人工 opt-in 运行（--ignored --nocapture）"]
fn real_endpoint_smoke_explain_and_suggest() {
    let Some((mut app, path)) = load_real_appdata() else {
        println!("[skip] 未找到真实 appdata（app.json），本机未跑过桌面端；跳过冒烟");
        return;
    };
    let Some(cfg) = supertask_core::ai::default_config(&app) else {
        println!("[skip] appdata 无默认 AI 配置；请先在桌面端 /ai 页配置后再跑");
        return;
    };
    let has_key = read_key().unwrap_or(None).is_some() || cfg.config.key_optional();
    if !has_key {
        println!("[skip] secrets 未设置 supertask.ai key 且配置不免鉴权；跳过冒烟");
        return;
    }
    println!(
        "[info] 使用默认配置「{}」→ {}（model={}）",
        cfg.name, cfg.config.base_url, cfg.config.model
    );

    let http = UreqAiHttp;

    // 场景 1：解释日志（最小输入）
    let out1 = complete(
        &http,
        &mut app,
        None,
        CompleteRequest {
            task: AiTask::ExplainLogs,
            payload: &serde_json::json!({
                "service": { "id": "smoke", "kind": "spring-boot", "port": 8080, "state": "running" },
                "lines": [
                    "2026-08-29 10:00:00.001  INFO 1 --- [main] com.example.SmokeApplication : Started SmokeApplication",
                    "2026-08-29 10:00:00.002 ERROR 1 --- [main] o.s.b.SpringApplication : Application run failed",
                    "2026-08-29 10:00:00.003 Caused by: java.net.BindException: Address already in use: bind",
                ],
            }),
            extra_redact: &[],
            config_id: None,
        },
        None::<fn(&str)>,
    )
    .expect("explain_logs 真实请求失败");
    println!("[ok] explain_logs model={} usage={:?}\n{}\n", out1.model, out1.usage.count, out1.text);
    assert!(!out1.text.trim().is_empty(), "explain_logs 返回空文本");

    // 场景 2：配置建议（最小 yaml；断言返回包含建议内容）
    let out2 = complete(
        &http,
        &mut app,
        None,
        CompleteRequest {
            task: AiTask::ConfigSuggest,
            payload: &serde_json::json!({
                "yaml": "version: 1\nservices:\n  api:\n    kind: spring-boot\n    port: 8080\n",
                "problems": ["8080 端口与本机另一进程冲突"],
            }),
            extra_redact: &[],
            config_id: None,
        },
        None::<fn(&str)>,
    )
    .expect("config_suggest 真实请求失败");
    println!("[ok] config_suggest model={}\n{}\n", out2.model, out2.text);
    assert!(!out2.text.trim().is_empty(), "config_suggest 返回空文本");

    // 用量：两次业务调用 → 当日计数 ≥2（重试成功也只计 1 次/调用）
    assert!(app.ai_usage.as_ref().map(|u| u.count).unwrap_or(0) >= 2, "按日用量未累计");

    // 不写回 appdata：冒烟不落盘（用量计数只留在内存）
    let _ = path;
}
