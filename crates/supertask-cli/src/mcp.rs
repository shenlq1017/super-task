//! `supertask mcp`（1.5 §5）：stdio 传输、tools only、10 个工具。
//! 业务走 supertask-core；tokio 只在本模块，引擎调用经 spawn_blocking 桥接（§3.2）。
//!
//! 生命周期：进程启动即就绪；**首个可变工具**触发取锁 + `engine.open`（holder=mcp）；
//! `supertask_status` / `supertask_logs` 只读、无需持锁。**断连即清场**：stdio 关闭
//! → stop_all → close（释放锁）→ 进程退出（防孤儿优先）。
//!
//! 方向七·AI 原生：`supertask_errors`（错误聚合）与 `supertask_wait_ready`（等待就绪，
//! outcome 区分 reached/failed/stopped/timeout）；`dispatch` 出口对所有工具返回值与
//! 错误信封统一脱敏（声明密钥值替换 + 敏感行整行掩码，幂等）。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use supertask_core::lock::LockHolder;
use supertask_core::{Engine, Error, ErrorCode};

pub const TOOL_STATUS: &str = "supertask_status";
pub const TOOL_START: &str = "supertask_start";
pub const TOOL_STOP: &str = "supertask_stop";
pub const TOOL_RESTART: &str = "supertask_restart";
pub const TOOL_LOGS: &str = "supertask_logs";
pub const TOOL_RUN_SCRIPT: &str = "supertask_run_script";
pub const TOOL_CANCEL_SCRIPT: &str = "supertask_cancel_script";
pub const TOOL_HOST_METRICS: &str = "supertask_host_metrics";
pub const TOOL_ERRORS: &str = "supertask_errors";
pub const TOOL_WAIT_READY: &str = "supertask_wait_ready";

/// 等待就绪的默认与钳制边界（毫秒）。
const WAIT_READY_DEFAULT_MS: u64 = 30_000;
const WAIT_READY_MIN_MS: u64 = 500;
const WAIT_READY_MAX_MS: u64 = 120_000;

/// 10 个工具的定义（名称 / 描述 / JSON Schema）。断连清场语义写进可变工具描述，
/// 提示编辑器重载会停止服务（§5.1 文档明示义务）。
pub fn tool_definitions() -> Vec<(&'static str, &'static str, Value)> {
    fn obj_schema(props: Value) -> Value {
        json!({ "type": "object", "properties": props, "additionalProperties": false })
    }
    vec![
        (
            TOOL_STATUS,
            "全服务快照：状态、端口、健康、脚本占用；含工作区根与锁持有者。只读，不取锁。",
            obj_schema(json!({})),
        ),
        (
            TOOL_START,
            "启动服务（缺省全部，拓扑顺序；依赖自动拉起）。注意：编辑器重载/断开 MCP 会停止全部服务。",
            obj_schema(json!({ "services": { "type": "array", "items": { "type": "string" }, "description": "服务 id 列表，缺省全部" } })),
        ),
        (
            TOOL_STOP,
            "停止服务（缺省全部）。注意：编辑器重载/断开 MCP 会停止全部服务。",
            obj_schema(json!({ "services": { "type": "array", "items": { "type": "string" } } })),
        ),
        (
            TOOL_RESTART,
            "停止再启动（缺省全部）。注意：编辑器重载/断开 MCP 会停止全部服务。",
            obj_schema(json!({ "services": { "type": "array", "items": { "type": "string" } } })),
        ),
        (
            TOOL_LOGS,
            "历史日志尾部/检索（读 .supertask/logs 文件，只读不持锁）。",
            obj_schema(json!({
                "service": { "type": "string", "description": "只看该服务，缺省全部源" },
                "lines": { "type": "integer", "description": "尾部行数，默认 200" },
                "grep": { "type": "string", "description": "literal 检索关键字（大小写不敏感）" },
            })),
        ),
        (
            TOOL_RUN_SCRIPT,
            "运行 supertask.yaml scripts 中的脚本（同工作区同时仅一个），等待结束后返回退出码。",
            obj_schema(json!({ "id": { "type": "string", "description": "脚本 id（必填）" } })),
        ),
        (
            TOOL_CANCEL_SCRIPT,
            "取消当前脚本。注意：编辑器重载/断开 MCP 会停止全部服务。",
            obj_schema(json!({})),
        ),
        (
            TOOL_HOST_METRICS,
            "主机指标只读快照（整机视角，与工作区无关，不取锁）：CPU 总占用与四分占比、\
             内存/交换空间、磁盘、CPU 温度（尽力采样）、网络上传下载速率。\
             用于判断「还能不能起一个服务 / 是否适合跑大模型」。\
             差分字段（CPU 占比、网络速率）首次调用为 null；取不到的字段为 null 而非 0；\
             不含 IP、路径、进程或环境信息，不持久化。",
            obj_schema(json!({})),
        ),
        (
            TOOL_ERRORS,
            "按服务聚合的错误摘要与就绪判定（不改动服务，但会取得工作区锁）：每个服务的状态、\
             是否就绪、错误来源（exit=进程退出 / health=健康检查失败 / generic=构建失败等）、\
             脱敏后的错误摘要与最近日志摘录。一次调用即可判断「当前栈哪里没就绪、为什么」。\
             返回 error 为 null 表示该服务当前没有捕获到的错误。",
            obj_schema(json!({})),
        ),
        (
            TOOL_WAIT_READY,
            "等待服务就绪（只等待不启动；会取得工作区锁并阻塞至多 timeout_ms）。返回 outcome：\
             reached=全部就绪；failed=有服务退出或不健康（附脱敏错误摘要）；\
             stopped=有服务未启动或被停止（先调 supertask_start）；timeout=超时（pending 列出\
             未就绪服务）。注意：编辑器重载/断开 MCP 会停止全部服务。",
            obj_schema(json!({
                "timeout_ms": { "type": "integer", "description": "最长等待毫秒数，默认 30000，钳到 [500, 120000]" },
                "services": { "type": "array", "items": { "type": "string" }, "description": "服务 id 列表，缺省全部 enabled 服务" },
            })),
        ),
    ]
}

/// MCP 服务器状态。Clone（Arc 字段）以便 spawn_blocking 持有。
#[derive(Clone)]
pub struct McpServer {
    root: Arc<PathBuf>,
    /// 惰性打开的引擎（holder=mcp）；None = 尚无可变工具调用过
    engine: Arc<Mutex<Option<Engine>>>,
    /// 工具调用串行化（引擎互斥外的调用级互斥，§5.1）
    dispatch_lock: Arc<Mutex<()>>,
}

impl McpServer {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root: Arc::new(root),
            engine: Arc::new(Mutex::new(None)),
            dispatch_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 断连清场：停止本进程启动过的全部服务并释放锁（幂等）。
    pub fn shutdown(&self) {
        if let Ok(mut g) = self.engine.lock() {
            if let Some(engine) = g.take() {
                let _ = engine.stop_all();
                let _ = engine.close();
            }
        }
    }

    /// 惰性取锁 + 打开引擎；WORKSPACE_LOCKED 原样传播（details 带 holder/pid）。
    fn ensure_open(&self) -> Result<Arc<Mutex<Option<Engine>>>, Error> {
        let mut g = self.engine.lock().expect("mcp engine lock");
        if g.is_none() {
            let engine = Engine::with_holder(LockHolder::Mcp);
            engine.open(self.root.as_path())?;
            *g = Some(engine);
        }
        Ok(Arc::clone(&self.engine))
    }

    fn with_engine<T>(&self, f: impl FnOnce(&Engine) -> Result<T, Error>) -> Result<T, Error> {
        let _guard = self.dispatch_lock.lock().expect("mcp dispatch lock");
        let g = self.ensure_open()?;
        let engine_guard = g.lock().expect("mcp engine lock");
        f(engine_guard.as_ref().expect("opened above"))
    }

    /// 工具分发（业务真源，测试直接调用）。返回 data JSON 或引擎错误。
    /// 出口统一脱敏：所有工具的返回值与错误信封都过一遍声明密钥掩码（幂等）。
    pub fn dispatch(&self, tool: &str, args: Option<&Value>) -> Result<Value, Error> {
        let args = args.cloned().unwrap_or(Value::Null);
        let out = match tool {
            TOOL_STATUS => self.status(),
            // 主机指标：纯主机级只读采样，不取锁、不开引擎、与工作区有效性无关。
            TOOL_HOST_METRICS => Ok(supertask_core::host_metrics::HostMetrics::mcp_sample()),
            TOOL_LOGS => {
                let id = args["service"].as_str();
                let lines = args["lines"].as_u64().unwrap_or(200) as usize;
                let grep = args["grep"].as_str();
                crate::readonly::logs_data(self.root.as_path(), id, lines, grep)
            }
            TOOL_START | TOOL_STOP | TOOL_RESTART => self.lifecycle(tool, &args),
            TOOL_RUN_SCRIPT => {
                let id = args["id"]
                    .as_str()
                    .ok_or_else(|| Error::new(ErrorCode::SpecInvalid, "缺少必填参数 id"))?;
                self.run_script(id)
            }
            TOOL_CANCEL_SCRIPT => self.with_engine(|e| {
                e.cancel_script()?;
                Ok(json!({ "ok": true }))
            }),
            TOOL_ERRORS => self.errors(),
            TOOL_WAIT_READY => self.wait_ready(&args),
            _ => Err(Error::new(ErrorCode::NotFound, format!("未知工具: {tool}"))),
        };
        self.redact_out(out)
    }

    /// 方向七·AI 原生：出口统一脱敏——值替换来自 secrets 声明（主密钥文件 +
    /// 全部服务 env_file + env backend key），另加敏感行整行掩码；无声明时仅掩码。
    /// 引擎已打开时用引擎内值集合，否则从工作区文件 best-effort 收集。
    fn redact_out(&self, out: Result<Value, Error>) -> Result<Value, Error> {
        let values = {
            let g = self.engine.lock().expect("mcp engine lock");
            match g.as_ref() {
                Some(e) => e.redaction_values().unwrap_or_default(),
                None => supertask_core::ai::sanitize::collect_workspace_values(self.root.as_path()),
            }
        };
        let red = supertask_core::ai::sanitize::Redactor::from_values(values);
        match out {
            Ok(mut data) => {
                red.redact_json(&mut data);
                Ok(data)
            }
            Err(mut err) => {
                err.redact_with(&|s: &str| red.text(s));
                Err(err)
            }
        }
    }

    /// 错误聚合（方向七）：引擎诊断视图。会取锁打开引擎（不改动服务状态）。
    fn errors(&self) -> Result<Value, Error> {
        self.with_engine(|e| {
            let view = e.diagnostics()?;
            serde_json::to_value(view).map_err(|err| {
                Error::new(ErrorCode::Protocol, format!("诊断视图序列化失败: {err}"))
            })
        })
    }

    /// 等待就绪（方向七）：只等待不启动；超时是结果不是错误。
    /// 持 dispatch_lock 阻塞至多 timeout_ms（与 run_script 的等待语义一致）。
    fn wait_ready(&self, args: &Value) -> Result<Value, Error> {
        let timeout_ms = args["timeout_ms"].as_u64().unwrap_or(WAIT_READY_DEFAULT_MS);
        let timeout_ms = timeout_ms.clamp(WAIT_READY_MIN_MS, WAIT_READY_MAX_MS);
        let services: Option<Vec<String>> = args["services"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        });
        self.with_engine(|e| {
            let view =
                e.wait_workspace_ready(services, std::time::Duration::from_millis(timeout_ms))?;
            serde_json::to_value(view).map_err(|err| {
                Error::new(ErrorCode::Protocol, format!("等待视图序列化失败: {err}"))
            })
        })
    }

    /// 只读快照：引擎未打开时退化为文件级只读（不取锁）。
    fn status(&self) -> Result<Value, Error> {
        let opened = self.engine.lock().expect("mcp engine lock").is_some();
        if opened {
            return self.with_engine(|e| {
                let snap = e.snapshot()?;
                let mut services = serde_json::Map::new();
                for (id, s) in &snap.services {
                    services.insert(
                        id.clone(),
                        json!({
                            "state": s.state,
                            "pid": s.pid,
                            "port": s.port,
                            "kind": s.kind,
                            "health": s.health,
                            "last_error": s.last_error,
                            "managed": s.managed,
                        }),
                    );
                }
                Ok(json!({
                    "workspace": self.root.display().to_string(),
                    "holder": LockHolder::Mcp.as_str(),
                    "script": snap.script,
                    "services": services,
                }))
            });
        }
        let mut data = crate::readonly::status_data(self.root.as_path())?;
        data["holder"] = json!("无（只读视图）");
        Ok(data)
    }

    /// start / stop / restart（可变路径，取锁）。
    fn lifecycle(&self, tool: &str, args: &Value) -> Result<Value, Error> {
        let selected: Option<Vec<String>> = args["services"].as_array().map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        });
        self.with_engine(|e| {
            let targets = match selected {
                Some(ids) => ids,
                None => {
                    let spec = e.spec()?;
                    match tool {
                        TOOL_STOP => e.snapshot()?.services.keys().cloned().collect(),
                        _ => supertask_core::graph::start_order(&spec)?,
                    }
                }
            };
            for id in &targets {
                let result = match tool {
                    TOOL_START => e.start_one(id),
                    TOOL_STOP => e.stop_one(id),
                    TOOL_RESTART => e.restart_one(id),
                    _ => unreachable!(),
                };
                if let Err(err) = result {
                    // start/restart 幂等：已在运行/构建的服务跳过
                    if !(matches!(tool, TOOL_START | TOOL_RESTART)
                        && err.code() == ErrorCode::AlreadyInProgress)
                    {
                        return Err(err);
                    }
                }
            }
            let snap = e.snapshot()?;
            let mut services = serde_json::Map::new();
            for (id, s) in &snap.services {
                if targets.is_empty() || targets.contains(id) {
                    services.insert(
                        id.clone(),
                        json!({ "state": s.state, "pid": s.pid, "port": s.port }),
                    );
                }
            }
            Ok(json!({ "ok": true, "services": services }))
        })
    }

    /// 运行脚本并等待结束（Agent 高频闭环；日志走 supertask_logs）。
    fn run_script(&self, id: &str) -> Result<Value, Error> {
        self.with_engine(|e| {
            e.subscribe_logs()?;
            e.run_script(id)?;
            loop {
                let running = e
                    .snapshot()
                    .ok()
                    .and_then(|s| {
                        s.script
                            .map(|sc| sc.state == supertask_core::engine::ScriptState::Running)
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(150));
            }
            let snap = e.snapshot()?;
            let script = snap.script;
            Ok(json!({
                "id": id,
                "state": script.as_ref().map(|s| s.state),
                "exit_code": script.as_ref().and_then(|s| s.last_exit.as_ref().map(|e| e.code)),
                "last_error": script.as_ref().and_then(|s| s.last_error.clone()),
            }))
        })
    }
}

// ---------------------------------------------------------------------------
// rmcp 壳层（薄封装：分发/清场逻辑全部在 McpServer，可直接测试）
// ---------------------------------------------------------------------------

#[allow(unused_imports)]
/// `supertask mcp` 入口。stdio 关闭（编辑器退出/重载/崩溃）后：
/// stop_all → close（释放锁）→ 返回（main 以退出码 0 结束）。
pub fn run_mcp(root: PathBuf) -> Result<i32, Error> {
    let server = McpServer::new(root);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("tokio runtime 构建失败: {e}")))?;
    let res = rt.block_on(async {
        let running = rmcp::ServiceExt::serve(server.clone(), rmcp::transport::stdio())
            .await
            .map_err(|e| Error::new(ErrorCode::Protocol, format!("MCP 服务启动失败: {e}")))?;
        // waiting() 在 stdio 关闭（编辑器退出/重载）时返回
        let _quit = running.waiting().await;
        Ok(())
    });
    // 断连即清场（§5.1）：无论正常断开还是错误路径
    server.shutdown();
    res.map(|_| crate::output::EXIT_OK)
}

#[allow(unused_imports)]
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, ErrorData, JsonObject, ListToolsResult,
    PaginatedRequestParams, ServerInfo, Tool,
};
use rmcp::service::{RequestContext, RoleServer};

impl ServerHandler for McpServer {
    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<rmcp::model::CallToolResponse, ErrorData>> + Send + '_
    {
        let name = request.name.clone();
        let arguments = request
            .arguments
            .map(|a| Value::Object(a.into_iter().collect()));
        let this = self.clone();
        async move {
            let res = tokio::task::spawn_blocking(move || this.dispatch(&name, arguments.as_ref()))
                .await
                .map_err(|e| {
                    rmcp::model::ErrorData::internal_error(format!("join error: {e}"), None)
                })?;
            match res {
                Ok(data) => Ok(CallToolResult::success(vec![ContentBlock::text(
                    serde_json::to_string_pretty(&data).unwrap_or_default(),
                )])
                .into()),
                Err(err) => {
                    // 工具级失败：调用方可见的统一错误信封（§5.2）
                    let envelope = json!({
                        "ok": false,
                        "error": {
                            "code": crate::output::code_str(&err.code()),
                            "message": err.message(),
                            "details": match &err {
                                Error::App { details: Some(d), .. } => {
                                    serde_json::to_value(d).unwrap_or(Value::Null)
                                }
                                _ => Value::Null,
                            },
                        },
                    });
                    Ok(CallToolResult::error(vec![ContentBlock::text(
                        serde_json::to_string_pretty(&envelope).unwrap_or_default(),
                    )])
                    .into())
                }
            }
        }
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, ErrorData>> + Send + '_ {
        async move {
            let tools = tool_definitions()
                .into_iter()
                .map(|(name, desc, schema)| {
                    Tool::new(
                        name,
                        desc,
                        serde_json::from_value::<JsonObject>(schema)
                            .expect("tool schema is a JSON object"),
                    )
                })
                .collect();
            Ok(ListToolsResult::with_all_items(tools))
        }
    }

    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::default();
        info.capabilities.tools = Some(rmcp::model::ToolsCapability::default());
        info.server_info.name = "supertask".into();
        info.server_info.version = env!("CARGO_PKG_VERSION").into();
        info.instructions = Some("SuperTask 工作区起停/观察/脚本工具。首个可变工具会取得工作区锁；断开 MCP 会停止全部服务。".into());
        info
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use supertask_core::lock;
    use supertask_core::ErrorCode;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-mcp-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("supertask.yaml"),
            // 声明一个 spring 服务（不 spawn：stop 对 stopped 幂等、锁冲突在启动前发生）
            "version: 1\nname: t\nservices:\n  api:\n    kind: spring-boot\n    module: m\n    port: 18099\n",
        )
        .unwrap();
        dir
    }

    fn foreign_alive_pid() -> u32 {
        if cfg!(windows) {
            4
        } else {
            1
        }
    }

    #[test]
    fn status_readonly_does_not_take_lock() {
        let root = temp_root("status-ro");
        let server = McpServer::new(root.clone());
        let out = server.dispatch(TOOL_STATUS, None).unwrap();
        assert!(out["workspace"].is_string());
        assert!(out["lock"].is_null());
        assert!(
            lock::query(&root).is_none(),
            "readonly must not create lock"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn unknown_tool_is_not_found() {
        let root = temp_root("unknown");
        let server = McpServer::new(root.clone());
        let err = server.dispatch("nope", None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn mutable_tool_takes_lock_and_shutdown_releases() {
        let root = temp_root("lock-cycle");
        let server = McpServer::new(root.clone());
        // 空工作区 stop：可变路径 → 取锁打开（无服务，不 spawn）
        let out = server.dispatch(TOOL_STOP, None).unwrap();
        assert_eq!(out["ok"], true);
        let info = lock::query(&root).expect("mutable tool acquires lock");
        assert_eq!(info.holder, LockHolder::Mcp);
        // 断连清场：锁释放
        server.shutdown();
        assert!(lock::query(&root).is_none(), "shutdown releases lock");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn mutable_tool_propagates_workspace_locked() {
        let root = temp_root("locked");
        let dir = root.join(".supertask");
        std::fs::create_dir_all(&dir).unwrap();
        let info = lock::LockInfo {
            pid: foreign_alive_pid(),
            holder: LockHolder::Desktop,
            started_at_ms: 0,
        };
        std::fs::write(lock::lock_path(&root), serde_json::to_vec(&info).unwrap()).unwrap();
        let server = McpServer::new(root.clone());
        let err = server.dispatch(TOOL_START, None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::WorkspaceLocked);
        // 只读工具在被持有工作区仍可用（§2.4.4）
        server.dispatch(TOOL_STATUS, None).unwrap();
        let _ = std::fs::remove_file(lock::lock_path(&root));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn run_script_requires_id() {
        let root = temp_root("script-id");
        let server = McpServer::new(root.clone());
        let err = server
            .dispatch(TOOL_RUN_SCRIPT, Some(&json!({})))
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::SpecInvalid);
        std::fs::remove_dir_all(&root).unwrap();
    }

    // ---- 方向七·AI 原生：错误聚合 / 等待就绪 / 出口统一脱敏 ----

    /// supertask_errors：取锁打开引擎（不改动服务），未启动服务的错误为 null、
    /// ready=false；断连清场释放锁。
    #[test]
    fn errors_opens_engine_and_reports_stopped_without_error() {
        let root = temp_root("errors-stopped");
        let server = McpServer::new(root.clone());
        let out = server.dispatch(TOOL_ERRORS, None).unwrap();
        assert_eq!(out["ready"], false);
        assert_eq!(out["ready_count"], 0);
        assert_eq!(out["total_count"], 1);
        let svc = &out["services"][0];
        assert_eq!(svc["id"], "api");
        assert_eq!(svc["state"], "stopped");
        assert_eq!(svc["ready"], false);
        assert!(
            svc.get("error").is_none() || svc["error"].is_null(),
            "无错误与有错误可区分"
        );
        assert!(
            lock::query(&root).is_some(),
            "errors opens engine and holds lock"
        );
        server.shutdown();
        assert!(lock::query(&root).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// supertask_wait_ready：目标从未启动 → stopped 早退（不空等）；
    /// timeout_ms=0 钳制后同样立即返回，未知服务 → NOT_FOUND。
    #[test]
    fn wait_ready_stopped_immediately_and_validates_targets() {
        let root = temp_root("wait-stopped");
        let server = McpServer::new(root.clone());
        let out = server
            .dispatch(
                TOOL_WAIT_READY,
                Some(&json!({ "timeout_ms": 500, "services": ["api"] })),
            )
            .unwrap();
        assert_eq!(out["outcome"], "stopped");
        assert_eq!(out["targets"][0], "api");
        let out = server
            .dispatch(TOOL_WAIT_READY, Some(&json!({ "timeout_ms": 0 })))
            .unwrap();
        assert_eq!(out["outcome"], "stopped");
        let err = server
            .dispatch(
                TOOL_WAIT_READY,
                Some(&json!({ "timeout_ms": 500, "services": ["nope"] })),
            )
            .unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        server.shutdown();
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 出口统一脱敏：声明密钥值从日志工具输出中掩掉，敏感行整行掩码，
    /// 无声明密钥的普通行不受影响。
    #[test]
    fn logs_output_redacts_declared_secrets_and_sensitive_lines() {
        let root = temp_root("logs-redact");
        std::fs::write(root.join(".env.local"), "API_TOKEN=abcd1234xyz\n").unwrap();
        let logs = root.join(".supertask").join("logs");
        std::fs::create_dir_all(&logs).unwrap();
        std::fs::write(
            logs.join("api.log"),
            "connecting with API_TOKEN=abcd1234xyz\npassword: hunter2x\nport: 18099\n",
        )
        .unwrap();
        let server = McpServer::new(root.clone());
        let out = server
            .dispatch(TOOL_LOGS, Some(&json!({ "service": "api", "lines": 10 })))
            .unwrap();
        let text = out.to_string();
        assert!(!text.contains("abcd1234xyz"), "{text}");
        assert!(!text.contains("hunter2x"), "{text}");
        assert!(text.contains("<redacted>"));
        assert!(text.contains("port: 18099"), "普通行保持原样");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn tool_definitions_cover_ten_tools() {
        let names: Vec<&str> = tool_definitions().into_iter().map(|(n, _, _)| n).collect();
        assert_eq!(names.len(), 10);
        for expected in [
            TOOL_STATUS,
            TOOL_START,
            TOOL_STOP,
            TOOL_RESTART,
            TOOL_LOGS,
            TOOL_RUN_SCRIPT,
            TOOL_CANCEL_SCRIPT,
            TOOL_HOST_METRICS,
            TOOL_ERRORS,
            TOOL_WAIT_READY,
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
    }

    /// 主机指标：只读、不取锁，且在无 supertask.yaml 的目录同样可用（与工作区无关）。
    /// 输出脱敏：除 platform（枚举值）外不含任何字符串字段。
    #[test]
    fn host_metrics_readonly_without_lock_or_workspace() {
        let dir = std::env::temp_dir().join(format!("st-mcp-hostm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let server = McpServer::new(dir.clone());
        let out = server.dispatch(TOOL_HOST_METRICS, None).unwrap();
        let obj = out.as_object().expect("host metrics must be an object");
        assert_eq!(out["platform"], std::env::consts::OS);
        assert!(out["sampledAtMs"].as_u64().unwrap() > 0);
        assert!(out.get("netLocalIp").is_none(), "local IP must be dropped");
        for (k, v) in obj {
            if *k != "platform" {
                assert!(!v.is_string(), "{k} must not carry free text");
            }
        }
        assert!(
            lock::query(&dir).is_none(),
            "host metrics must not create a lock"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// §13.3 断连清场：start 起真实 node 桩 → shutdown（等价 stdio 关闭）→
    /// 桩进程归零（端口释放）+ 锁释放。
    #[test]
    fn start_then_shutdown_kills_stub_and_releases_lock() {
        use crate::test_stubs::node_stub;
        if !node_stub::node_available() {
            eprintln!("skip: node 不可用");
            return;
        }
        let ws = node_stub::write_ws("mcp-disconnect", 18213, true);
        let server = McpServer::new(ws.root.clone());
        server.dispatch(TOOL_START, None).unwrap();
        // start 返回 = 引擎已受理并 wait_ready；Agent 语义是随后轮询 status
        let mut running = false;
        for _ in 0..60 {
            let status = server.dispatch(TOOL_STATUS, None).unwrap();
            if status["services"]["web"]["state"] == serde_json::json!("running") {
                running = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        assert!(running, "web should reach running after start");
        assert!(
            supertask_core::ports::is_serving(ws.port),
            "stub must be listening before shutdown"
        );
        assert!(lock::query(&ws.root).is_some(), "mutable tool holds lock");

        // 模拟编辑器断开：stdio 关闭路径调用的同一 shutdown()
        server.shutdown();

        // 桩进程归零：轮询等待端口释放（terminate 同步，留少量余量）
        let mut released = false;
        for _ in 0..20 {
            if !supertask_core::ports::is_serving(ws.port) {
                released = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        assert!(released, "stub process must be killed on disconnect");
        assert!(
            lock::query(&ws.root).is_none(),
            "lock must be released on disconnect"
        );
        node_stub::cleanup(&ws);
    }
}
