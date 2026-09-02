//! 2.1 AI IPC 命令层（v2.1 规格 §5 + 截图对齐升级；业务在 core `ai/`）。
//!
//! - 配置：appdata 命名多配置（`ai_configs` + `ai_default_config`；壳层托管
//!   `AppDataHandle`，磁盘真源 app.json；旧单配置 `ai` 由 core 视图迁移）；
//! - key：core `ai::read_key/write_key/clear_key`（appdata `secrets.env`，
//!   逻辑 id `supertask.ai`；值绝不进入任何返回值）；
//! - 全局指令 / Prompt 模板：appdata（限额与校验在 core）；
//! - complete/models：每次用户显式触发一次；gather workspace secret 值做掩码后走 core。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use supertask_core::ai::{self, AiCompleteOut, AiTask, NamedAiConfig, UreqAiHttp};
use supertask_core::appdata;
use supertask_core::error::Result;
use supertask_core::ipc::{AiStreamPayload, PROTOCOL};
use supertask_core::Engine;

use crate::state::AppDataHandle;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Serialize)]
struct AiStreamEnvelope {
    protocol: u32,
    event: &'static str,
    workspace_id: Option<String>,
    ts_ms: u64,
    payload: AiStreamPayload,
}

fn emit_ai_chunk(app: &AppHandle, request_id: &str, delta: &str) {
    if request_id.is_empty() || delta.is_empty() {
        return;
    }
    let envelope = AiStreamEnvelope {
        protocol: PROTOCOL,
        event: supertask_core::ipc::event::AI,
        workspace_id: None,
        ts_ms: now_ms(),
        payload: AiStreamPayload::chunk(request_id, delta),
    };
    let _ = app.emit(supertask_core::ipc::event::AI, &envelope);
}

#[derive(Debug, Serialize)]
pub struct AiConfigOut {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_tokens: u32,
    pub provider: String,
    pub api_style: Option<ai::ApiStyle>,
    pub auth_method: ai::AuthMethod,
    pub proxy_enabled: bool,
    pub proxy_url: Option<String>,
    pub context_window: Option<u64>,
    pub max_retries: u32,
}

#[derive(Debug, Serialize)]
pub struct AiConfigSummaryOut {
    pub id: String,
    pub name: String,
    pub is_default: bool,
    pub provider: String,
    pub model: String,
    pub base_url: String,
}

#[derive(Debug, Serialize)]
pub struct AiTemplateOut {
    pub id: String,
    pub name: String,
    pub content: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct AiStatusOut {
    pub configs: Vec<AiConfigSummaryOut>,
    pub default_id: Option<String>,
    pub templates: Vec<AiTemplateOut>,
    pub global_instructions: Option<String>,
    pub key_set: bool,
    pub usage_today: ai::AiUsageOut,
}

impl From<&NamedAiConfig> for AiConfigOut {
    fn from(c: &NamedAiConfig) -> Self {
        Self {
            id: c.id.clone(),
            name: c.name.clone(),
            base_url: c.config.base_url.clone(),
            model: c.config.model.clone(),
            timeout_secs: c.config.timeout_secs,
            max_tokens: c.config.max_tokens,
            provider: c.config.provider.clone(),
            api_style: c.config.api_style,
            auth_method: c.config.auth_method,
            proxy_enabled: c.config.proxy_enabled,
            proxy_url: c.config.proxy_url.clone(),
            context_window: c.config.context_window,
            max_retries: c.config.max_retries,
        }
    }
}

/// app.json 磁盘路径（与 state.rs 同源；这里是 core appdata 目录的直读路径）。
fn appdata_path() -> PathBuf {
    appdata::appdata_dir().join("app.json")
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiConfigSaveIn {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub base_url: Option<String>,
    pub model: String,
    pub timeout_secs: Option<u64>,
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub provider: String,
    pub api_style: Option<ai::ApiStyle>,
    pub auth_method: Option<ai::AuthMethod>,
    #[serde(default)]
    pub proxy_enabled: bool,
    #[serde(default)]
    pub proxy_url: Option<String>,
    #[serde(default)]
    pub context_window: Option<u64>,
    pub max_retries: Option<u32>,
    /// None 不动；Some("") 清除；Some(非空) 覆盖（仅对该配置写入全局单 key 槽位）。
    #[serde(default)]
    pub api_key: Option<String>,
}

pub fn ai_config_save(appdata: &AppDataHandle, input: AiConfigSaveIn) -> Result<AiConfigOut> {
    let mut guard = appdata.lock().expect("appdata lock");
    let previous = guard.clone();
    let saved = ai::config_save(
        &mut guard,
        ai::ConfigSaveInput {
            id: input.id,
            name: input.name,
            base_url: input.base_url,
            model: input.model,
            timeout_secs: input.timeout_secs,
            max_tokens: input.max_tokens,
            provider: input.provider,
            api_style: input.api_style,
            auth_method: input.auth_method.unwrap_or_default(),
            proxy_enabled: input.proxy_enabled,
            proxy_url: input.proxy_url,
            context_window: input.context_window,
            max_retries: input.max_retries,
        },
    )?;
    if let Some(key) = &input.api_key {
        let result = if key.is_empty() {
            ai::clear_key()
        } else {
            ai::write_key(key)
        };
        if let Err(error) = result {
            *guard = previous;
            return Err(error);
        }
    }
    if let Err(error) = appdata::save_at(&appdata_path(), &guard) {
        *guard = previous;
        return Err(error);
    }
    Ok(AiConfigOut::from(&saved))
}

pub fn ai_config_delete(appdata: &AppDataHandle, id: &str) -> Result<()> {
    let mut guard = appdata.lock().expect("appdata lock");
    let previous = guard.clone();
    ai::config_delete(&mut guard, id)?;
    if let Err(error) = appdata::save_at(&appdata_path(), &guard) {
        *guard = previous;
        return Err(error);
    }
    Ok(())
}

pub fn ai_config_default(appdata: &AppDataHandle, id: &str) -> Result<()> {
    let mut guard = appdata.lock().expect("appdata lock");
    let previous = guard.clone();
    ai::config_set_default(&mut guard, id)?;
    if let Err(error) = appdata::save_at(&appdata_path(), &guard) {
        *guard = previous;
        return Err(error);
    }
    Ok(())
}

pub fn ai_instructions_save(appdata: &AppDataHandle, text: &str) -> Result<String> {
    let mut guard = appdata.lock().expect("appdata lock");
    let previous = guard.clone();
    let saved = ai::set_global_instructions(&mut guard, text)?;
    if let Err(error) = appdata::save_at(&appdata_path(), &guard) {
        *guard = previous;
        return Err(error);
    }
    Ok(saved)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTemplateSaveIn {
    pub id: Option<String>,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub enabled: bool,
}

pub fn ai_template_save(appdata: &AppDataHandle, input: AiTemplateSaveIn) -> Result<AiTemplateOut> {
    let mut guard = appdata.lock().expect("appdata lock");
    let previous = guard.clone();
    let saved = ai::template_save(
        &mut guard,
        ai::TemplateSaveInput {
            id: input.id,
            name: input.name,
            content: input.content,
            enabled: input.enabled,
        },
    )?;
    if let Err(error) = appdata::save_at(&appdata_path(), &guard) {
        *guard = previous;
        return Err(error);
    }
    Ok(AiTemplateOut {
        id: saved.id,
        name: saved.name,
        content: saved.content,
        enabled: saved.enabled,
    })
}

pub fn ai_template_delete(appdata: &AppDataHandle, id: &str) -> Result<()> {
    let mut guard = appdata.lock().expect("appdata lock");
    let previous = guard.clone();
    ai::template_delete(&mut guard, id)?;
    if let Err(error) = appdata::save_at(&appdata_path(), &guard) {
        *guard = previous;
        return Err(error);
    }
    Ok(())
}

pub fn ai_status(appdata: &AppDataHandle) -> Result<AiStatusOut> {
    let guard = appdata.lock().expect("appdata lock");
    let list = ai::configs(&guard);
    let default_id = guard
        .ai_default_config
        .clone()
        .or_else(|| list.first().map(|c| c.id.clone()));
    Ok(AiStatusOut {
        configs: list
            .iter()
            .map(|c| AiConfigSummaryOut {
                id: c.id.clone(),
                name: c.name.clone(),
                is_default: Some(c.id.as_str()) == default_id.as_deref(),
                provider: c.config.provider.clone(),
                model: c.config.model.clone(),
                base_url: c.config.base_url.clone(),
            })
            .collect(),
        default_id,
        templates: guard
            .ai_templates
            .iter()
            .map(|t| AiTemplateOut {
                id: t.id.clone(),
                name: t.name.clone(),
                content: t.content.clone(),
                enabled: t.enabled,
            })
            .collect(),
        global_instructions: guard.ai_global_instructions.clone(),
        key_set: ai::read_key()?.is_some(),
        usage_today: guard
            .ai_usage
            .clone()
            .map(|u| ai::AiUsageOut {
                date: u.date.clone(),
                count: u.today_count(),
            })
            .unwrap_or_else(|| ai::AiUsageOut {
                date: ai::today_utc(),
                count: 0,
            }),
    })
}

pub fn ai_models(appdata: &AppDataHandle, config_id: Option<&str>) -> Result<Vec<String>> {
    let snapshot = appdata.lock().expect("appdata lock").clone();
    let key = ai::read_key().unwrap_or(None);
    ai::models(&UreqAiHttp, &snapshot, key.as_deref(), config_id)
}

/// 当前工作区 secret 值（仅用于进 prompt 前的掩码，不进任何返回值/日志）。
fn workspace_secret_values(engine: &Engine) -> Vec<String> {
    let mut values = Vec::new();
    let Ok(spec) = engine.spec() else {
        return values;
    };
    let root = PathBuf::from(&spec.root);
    if let Ok((env, _)) = supertask_core::secrets::load_file_layers(&spec, &root, None) {
        values.extend(env.into_values());
    }
    if let Some(sec) = &spec.secrets {
        for key in &sec.required {
            if let Ok(v) = std::env::var(key) {
                values.push(v);
            }
        }
    }
    values
}

pub fn ai_complete(
    engine: &Engine,
    appdata: &AppDataHandle,
    task: &str,
    payload: &serde_json::Value,
    config_id: Option<&str>,
    stream_emit: Option<Arc<(AppHandle, String)>>,
) -> Result<AiCompleteOut> {
    let task = AiTask::parse(task)?;
    let extra_redact = workspace_secret_values(engine);
    // 不跨网络调用持锁：快照 AppData + key，完成后回写用量
    let (snapshot, key) = {
        let guard = appdata.lock().expect("appdata lock");
        (guard.clone(), ai::read_key().unwrap_or(None))
    };
    let mut working = snapshot;
    let out = if let Some(ctx) = stream_emit {
        let app = ctx.0.clone();
        let request_id = ctx.1.clone();
        ai::complete(
            &UreqAiHttp,
            &mut working,
            key.as_deref(),
            ai::CompleteRequest {
                task,
                payload,
                extra_redact: &extra_redact,
                config_id,
            },
            Some(move |delta: &str| emit_ai_chunk(&app, &request_id, delta)),
        )?
    } else {
        ai::complete(
            &UreqAiHttp,
            &mut working,
            key.as_deref(),
            ai::CompleteRequest {
                task,
                payload,
                extra_redact: &extra_redact,
                config_id,
            },
            None::<fn(&str)>,
        )?
    };
    // 用量回写（best effort；磁盘失败时内存计数仍在）
    {
        let mut guard = appdata.lock().expect("appdata lock");
        guard.ai_usage = working.ai_usage.take();
        let _ = appdata::save_at(&appdata_path(), &guard);
    }
    Ok(out)
}
