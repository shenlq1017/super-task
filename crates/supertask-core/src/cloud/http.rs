//! HttpCloudProvider：ureq + rustls 生产实现（v2.0 规格 §10）。
//!
//! 可测试性：HTTP 执行抽象为 [`HttpExecutor`]（status + body），错误映射矩阵用
//! 本地 fake executor 单测，**不访问外网**；生产 executor 为 [`UreqExecutor`]。
//! 端点可配置（自托管，spec §2.5）：默认官方占位端点，Phase 0.4 拍板后替换。

use super::{
    map_status, parse_entity, CloudProvider, Entity, EntityType, HttpResponse, LoginTokens,
    QuotaUsage,
};
use crate::error::{Error, ErrorCode, Result};

pub const DEFAULT_ENDPOINT: &str = "https://cloud.supertask.local.example";

/// Parse a list while retaining known entities and skipping unknown future types.
/// Servers may return either a bare array or `{ "entities": [...] }`.
pub fn parse_entity_list(body: &str) -> Result<Vec<Entity>> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| {
        Error::new(
            ErrorCode::CloudProtocolError,
            format!("实体列表解析失败: {e}"),
        )
    })?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(mut object) => object
            .remove("entities")
            .and_then(|v| v.as_array().cloned())
            .ok_or_else(|| {
                Error::new(ErrorCode::CloudProtocolError, "实体列表响应缺少 entities")
            })?,
        _ => {
            return Err(Error::new(
                ErrorCode::CloudProtocolError,
                "实体列表响应格式错误",
            ))
        }
    };
    let mut entities = Vec::new();
    for item in items {
        let kind = item.get("type").and_then(serde_json::Value::as_str);
        if kind.and_then(EntityType::parse).is_none() {
            continue;
        }
        entities.push(serde_json::from_value(item).map_err(|e| {
            Error::new(
                ErrorCode::CloudProtocolError,
                format!("实体列表项解析失败: {e}"),
            )
        })?);
    }
    Ok(entities)
}

pub trait HttpExecutor: Send + Sync {
    /// `method`/`url`/`bearer`(可选)/`body`(可选) → [`HttpResponse`]；传输失败返回 Err。
    fn execute(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        body: Option<&str>,
    ) -> Result<HttpResponse>;
}

pub struct UreqExecutor;

impl HttpExecutor for UreqExecutor {
    fn execute(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        body: Option<&str>,
    ) -> Result<HttpResponse> {
        let mut req = ureq::request(method, url);
        if let Some(t) = bearer {
            req = req.set("Authorization", &format!("Bearer {t}"));
        }
        let resp = if let Some(b) = body {
            req = req.set("Content-Type", "application/json");
            req.send_string(b)
        } else {
            req.call()
        };
        match resp {
            Ok(r) => {
                let status = r.status();
                let text = r.into_string().unwrap_or_default();
                Ok(HttpResponse { status, body: text })
            }
            Err(ureq::Error::Status(code, r)) => {
                let text = r.into_string().unwrap_or_default();
                Ok(HttpResponse {
                    status: code,
                    body: text,
                })
            }
            Err(_) => Err(Error::new(ErrorCode::CloudOffline, "网络不可达或超时")),
        }
    }
}

pub struct HttpCloudProvider {
    endpoint: String,
    executor: Box<dyn HttpExecutor>,
}

impl HttpCloudProvider {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            executor: Box::new(UreqExecutor),
        }
    }

    pub fn with_executor(endpoint: impl Into<String>, executor: Box<dyn HttpExecutor>) -> Self {
        Self {
            endpoint: endpoint.into(),
            executor,
        }
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.endpoint.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn map(&self, r: HttpResponse) -> Result<String> {
        if (200..300).contains(&r.status) {
            Ok(r.body)
        } else {
            Err(Error::new(
                map_status(r.status),
                format!("云端响应 {status}", status = r.status),
            ))
        }
    }

    /// Execute an authenticated request, refreshing and replaying exactly once
    /// after a 401. The refreshed token is persisted before replay so a crash
    /// cannot leave the session using an already-rotated refresh token.
    pub fn authenticated_call<F>(&self, tokens: &mut LoginTokens, call: F) -> Result<String>
    where
        F: FnMut(&dyn HttpExecutor, &str) -> Result<HttpResponse>,
    {
        self.authenticated_call_with(tokens, call, super::session::save_session)
    }

    fn authenticated_call_with<F, S>(
        &self,
        tokens: &mut LoginTokens,
        mut call: F,
        save: S,
    ) -> Result<String>
    where
        F: FnMut(&dyn HttpExecutor, &str) -> Result<HttpResponse>,
        S: FnOnce(&LoginTokens) -> Result<()>,
    {
        let response = call(self.executor.as_ref(), &tokens.access_token)?;
        if response.status != 401 {
            return self.map(response);
        }
        let refreshed = self.refresh(&tokens.refresh_token)?;
        save(&refreshed)?;
        *tokens = refreshed;
        self.map(call(self.executor.as_ref(), &tokens.access_token)?)
    }
}

impl CloudProvider for HttpCloudProvider {
    fn login(&self, email: &str, password: &str) -> Result<LoginTokens> {
        let body = serde_json::json!({ "email": email, "password": password }).to_string();
        let r = self
            .executor
            .execute("POST", &self.url("/auth/login"), None, Some(&body))?;
        let text = self.map(r)?;
        serde_json::from_str(&text).map_err(|e| {
            Error::new(
                ErrorCode::CloudProtocolError,
                format!("登录响应解析失败: {e}"),
            )
        })
    }

    fn refresh(&self, refresh_token: &str) -> Result<LoginTokens> {
        let body = serde_json::json!({ "refresh_token": refresh_token }).to_string();
        let r = self
            .executor
            .execute("POST", &self.url("/auth/refresh"), None, Some(&body))?;
        let text = self.map(r)?;
        serde_json::from_str(&text).map_err(|e| {
            Error::new(
                ErrorCode::CloudProtocolError,
                format!("刷新响应解析失败: {e}"),
            )
        })
    }

    fn list(&self, token: &str, entity_type: Option<EntityType>) -> Result<Vec<Entity>> {
        let suffix = entity_type
            .map(|t| format!("?type={}", t.as_str()))
            .unwrap_or_default();
        let r = self.executor.execute(
            "GET",
            &self.url(&format!("/entities{suffix}")),
            Some(token),
            None,
        )?;
        let text = self.map(r)?;
        parse_entity_list(&text)
    }

    fn get(&self, token: &str, id: &str) -> Result<Entity> {
        let r = self.executor.execute(
            "GET",
            &self.url(&format!("/entities/{id}")),
            Some(token),
            None,
        )?;
        parse_entity(&self.map(r)?)
    }

    fn put(&self, token: &str, entity: &Entity, base_rev: u64) -> Result<Entity> {
        let body = serde_json::json!({
            "type": entity.entity_type.as_str(),
            "data": entity.data,
            "base_rev": base_rev,
            "updated_by": entity.updated_by,
        })
        .to_string();
        let r = self.executor.execute(
            "PUT",
            &self.url(&format!("/entities/{}", entity.id)),
            Some(token),
            Some(&body),
        )?;
        parse_entity(&self.map(r)?)
    }

    fn delete(&self, token: &str, id: &str) -> Result<()> {
        let r = self.executor.execute(
            "DELETE",
            &self.url(&format!("/entities/{id}")),
            Some(token),
            None,
        )?;
        self.map(r)?;
        Ok(())
    }

    fn telemetry_batch(&self, token: &str, events: &str) -> Result<()> {
        let r = self.executor.execute(
            "POST",
            &self.url("/telemetry/batch"),
            Some(token),
            Some(events),
        )?;
        self.map(r)?;
        Ok(())
    }

    fn quota(&self, token: &str) -> Result<QuotaUsage> {
        let r = self
            .executor
            .execute("GET", &self.url("/quota"), Some(token), None)?;
        let text = self.map(r)?;
        serde_json::from_str(&text).map_err(|e| {
            Error::new(
                ErrorCode::CloudProtocolError,
                format!("配额响应解析失败: {e}"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU16;
    use std::sync::Mutex;

    struct FakeHttp {
        status: AtomicU16,
        offline: Mutex<bool>,
    }
    impl FakeHttp {
        fn new(status: u16) -> Self {
            Self {
                status: AtomicU16::new(status),
                offline: Mutex::new(false),
            }
        }
    }
    impl HttpExecutor for FakeHttp {
        fn execute(
            &self,
            _m: &str,
            _u: &str,
            _b: Option<&str>,
            _body: Option<&str>,
        ) -> Result<HttpResponse> {
            if *self.offline.lock().unwrap() {
                return Err(Error::new(ErrorCode::CloudOffline, "offline"));
            }
            Ok(HttpResponse {
                status: self.status.load(std::sync::atomic::Ordering::Relaxed),
                body: "{}".into(),
            })
        }
    }

    #[test]
    fn error_mapping_matrix_no_network() {
        let cases: [(u16, ErrorCode); 5] = [
            (401, ErrorCode::CloudAuthFailed),
            (403, ErrorCode::CloudAuthFailed),
            (409, ErrorCode::CloudSyncConflict),
            (429, ErrorCode::CloudQuotaExceeded),
            (500, ErrorCode::CloudProtocolError),
        ];
        for (status, code) in cases {
            let p = HttpCloudProvider::with_executor("https://x", Box::new(FakeHttp::new(status)));
            assert_eq!(
                p.list("t", None).unwrap_err().code(),
                code,
                "status={status}"
            );
        }
    }

    #[test]
    fn transport_error_maps_offline() {
        let exec = FakeHttp::new(200);
        *exec.offline.lock().unwrap() = true;
        let p = HttpCloudProvider::with_executor("https://x", Box::new(exec));
        assert_eq!(
            p.list("t", None).unwrap_err().code(),
            ErrorCode::CloudOffline
        );
    }

    #[test]
    fn list_skips_unknown_types_individually() {
        let known = serde_json::json!({"id":"w","type":"workspace","rev":1,"data":{"name":"w"}});
        let unknown = serde_json::json!({"id":"k","type":"kind.python","rev":1,"data":{}});
        let result = parse_entity_list(&serde_json::json!([known, unknown]).to_string()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "w");
    }

    #[test]
    fn endpoint_join_has_one_separator() {
        let p = HttpCloudProvider::with_executor(
            "https://example.test///",
            Box::new(FakeHttp::new(200)),
        );
        assert_eq!(p.url("/entities"), "https://example.test/entities");
    }

    #[test]
    fn authenticated_call_refreshes_once_and_replays() {
        let refreshed = LoginTokens {
            account_id: "acc".into(),
            email: "a@b.c".into(),
            access_token: "new-access".into(),
            refresh_token: "new-refresh".into(),
            expires_in_secs: 900,
        };
        let responses = vec![
            HttpResponse {
                status: 401,
                body: String::new(),
            },
            HttpResponse {
                status: 200,
                body: serde_json::to_string(&refreshed).unwrap(),
            },
            HttpResponse {
                status: 200,
                body: "replayed".into(),
            },
        ];
        let p = HttpCloudProvider::with_executor(
            "https://example.test",
            Box::new(SequenceHttp::new(responses)),
        );
        let mut tokens = LoginTokens {
            account_id: "acc".into(),
            email: "a@b.c".into(),
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            expires_in_secs: 60,
        };
        let body = p
            .authenticated_call_with(
                &mut tokens,
                |exec, token| {
                    exec.execute("GET", "https://example.test/protected", Some(token), None)
                },
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(body, "replayed");
        assert_eq!(tokens.access_token, "new-access");
        assert_eq!(tokens.refresh_token, "new-refresh");
    }

    struct SequenceHttp {
        responses: Mutex<Vec<HttpResponse>>,
    }
    impl SequenceHttp {
        fn new(responses: Vec<HttpResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
            }
        }
    }
    impl HttpExecutor for SequenceHttp {
        fn execute(
            &self,
            _m: &str,
            _u: &str,
            _b: Option<&str>,
            _body: Option<&str>,
        ) -> Result<HttpResponse> {
            self.responses.lock().unwrap().remove(0).pipe(Ok)
        }
    }

    trait Pipe: Sized {
        fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
            f(self)
        }
    }
    impl<T> Pipe for T {}
}
