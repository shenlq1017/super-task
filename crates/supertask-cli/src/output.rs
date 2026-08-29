//! 输出信封与退出码（1.5 §4.3）：`--json` 直接序列化 core 结构，不另造模型。
//! 错误码与 IPC 同表。退出码：0 成功；1 运行错误；2 用法错误（clap 自带）。

use std::io::Write as _;

use supertask_core::Error;

pub const EXIT_OK: i32 = 0;
pub const EXIT_RUNTIME: i32 = 1;

/// 稳定错误码字符串（serde SCREAMING_SNAKE_CASE，与 IPC 码表一致）
pub fn code_str(code: &supertask_core::ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{code:?}"))
}

/// 错误信封（serde 序列化，与 IPC error 字段同名）
pub fn error_value(e: &Error) -> serde_json::Value {
    let details = match e {
        supertask_core::Error::App { details, .. } => details.clone(),
    };
    let mut obj = serde_json::json!({
        "code": code_str(&e.code()),
        "message": e.message(),
    });
    if let Some(d) = details {
        // serde_yaml::Value 与 serde_json::Value 同构（映射/标量），借道序列化
        obj["details"] = serde_json::to_value(&d).unwrap_or(serde_json::Value::Null);
    }
    obj
}

fn stdout_json(v: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = serde_json::to_writer_pretty(&mut out, v);
    let _ = out.write_all(b"\n");
    let _ = out.flush();
}

/// 成功：`--json` 输出 `{ok:true,data}`；否则 human 文本已在调用方打印。
pub fn ok(json: bool, data: serde_json::Value) {
    if json {
        stdout_json(&serde_json::json!({ "ok": true, "data": data }));
    }
}

/// 失败：`--json` 输出 `{ok:false,error}` 到 stdout；否则中文 message 到 stderr。
pub fn fail(json: bool, e: &Error) -> i32 {
    if json {
        stdout_json(&serde_json::json!({ "ok": false, "error": error_value(e) }));
    } else {
        eprintln!("错误 [{}]: {}", code_str(&e.code()), e.message());
        // holder/pid 等结构化细节人读时并入提示
        if let supertask_core::Error::App { details: Some(d), .. } = e {
            eprintln!("  详情: {d:?}");
        }
    }
    EXIT_RUNTIME
}
