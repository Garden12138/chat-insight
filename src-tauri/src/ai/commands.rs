use super::client::LlmClient;
use super::commitments::{self, ScanEstimate, ScanOutcome};
use super::decisions;
use super::events::{self, EventReviewEstimate, EventReviewResult};
use super::features::{self, AskResult, ReportResult, SummaryResult, TopicsResult};
use super::indexer::{self, IndexOutcome, IndexProgress};
use super::settings::{AiSettingsInput, AiSettingsView};
use super::store::{CacheKey, ContentEstimate, IndexCounts};
use super::{AiState, Sink};
use crate::error::{AppError, AppResult};
use crate::insights::YearInsights;
use crate::models::{DateRange, MessageItem, SearchHit};
use crate::{public_error, AppState};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> AppResult<T> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .map_err(|_| "AI 任务意外结束".to_string())?
        .map_err(public_error)
}

/// How a cached AI result should be used.
fn cache_mode(mode: &str) -> AppResult<(bool, bool)> {
    // (read cache, generate when missing or stale)
    match mode {
        "cached" => Ok((true, false)),
        "generate" => Ok((true, true)),
        "refresh" => Ok((false, true)),
        _ => Err(AppError::Validation("未知的生成模式".into())),
    }
}

/// Holds a task lock for the lifetime of the guard.
struct TaskLock(Arc<AtomicBool>);

impl TaskLock {
    fn acquire(flag: &Arc<AtomicBool>, busy: &str) -> AppResult<Self> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| AppError::Ai(busy.into()))?;
        Ok(Self(Arc::clone(flag)))
    }

    fn index(flag: &Arc<AtomicBool>) -> AppResult<Self> {
        Self::acquire(flag, "语义索引正在建立，请稍后再试")
    }
}

impl Drop for TaskLock {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

fn cached<T: DeserializeOwned>(
    state: &AppState,
    key: &CacheKey,
    fingerprint: &str,
    apply: impl FnOnce(&mut T, String),
) -> AppResult<Option<T>> {
    Ok(state
        .database
        .ai_cache_get(key, fingerprint)?
        .and_then(|(json, created_at)| {
            serde_json::from_str::<T>(&json).ok().map(|mut value| {
                apply(&mut value, created_at);
                value
            })
        }))
}

fn store<T: Serialize>(
    state: &AppState,
    key: &CacheKey,
    fingerprint: &str,
    value: &T,
) -> AppResult<String> {
    state
        .database
        .ai_cache_put(key, fingerprint, &serde_json::to_string(value)?)
}

#[tauri::command]
pub fn ai_get_settings(ai: State<'_, AiState>) -> AiSettingsView {
    ai.settings.view()
}

#[tauri::command]
pub fn ai_save_settings(
    ai: State<'_, AiState>,
    input: AiSettingsInput,
) -> Result<AiSettingsView, String> {
    ai.settings.save(input).map_err(public_error)
}

/// Read-only routing metadata for the consent UI; never sends chat text or invokes a model.
#[tauri::command]
pub async fn ai_xcli_privacy(
    ai: State<'_, AiState>,
) -> Result<Option<super::client::GatewayPrivacy>, String> {
    let (_, client) = ai.client();
    blocking(move || client.gateway_privacy()).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiTestResult {
    pub models: Vec<String>,
    pub chat_model_found: bool,
    pub embedding_dimension: usize,
    pub gateway: Option<super::client::GatewayPrivacy>,
}

fn model_listed(models: &[String], wanted: &str) -> bool {
    models.iter().any(|model| {
        model == wanted
            || model.strip_suffix(":latest") == Some(wanted)
            || wanted.strip_suffix(":latest") == Some(model)
    })
}

#[tauri::command]
pub async fn ai_test_connection(ai: State<'_, AiState>) -> Result<AiTestResult, String> {
    let (settings, client) = ai.client();
    let client = client.with_cloud_consent(settings.allow_cloud);
    if settings.provider != super::settings::AiProvider::Xcli
        && !settings.is_local()
        && !settings.allow_cloud
    {
        return Err(
            "模型服务地址不在本机。如需使用云端服务，请先勾选“允许使用云端模型服务”".into(),
        );
    }
    blocking(move || {
        let gateway = client.gateway_privacy()?;
        let models = client.list_models().unwrap_or_default();
        let embedding_dimension = client.test_embedding_dimension()?;
        Ok(AiTestResult {
            chat_model_found: models.is_empty() || model_listed(&models, &settings.chat_model),
            embedding_dimension,
            models,
            gateway,
        })
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiIndexStatus {
    pub model: String,
    pub running: bool,
    #[serde(flatten)]
    pub counts: IndexCounts,
}

#[tauri::command]
pub fn ai_index_status(
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
) -> Result<AiIndexStatus, String> {
    let (settings, client) = ai.client();
    let model = settings.embed_model;
    let counts = state
        .database
        .ai_index_counts(&account_id, None, client.embed_model())
        .map_err(public_error)?;
    Ok(AiIndexStatus {
        model,
        running: ai.index_running.load(Ordering::SeqCst),
        counts,
    })
}

#[tauri::command]
pub async fn ai_build_index(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    rebuild: bool,
    cloud_consent: bool,
) -> Result<IndexOutcome, String> {
    let (settings, mut client) = ai.client_with_consent(cloud_consent);
    settings
        .ensure_allowed(true, cloud_consent)
        .map_err(public_error)?;
    let lock = TaskLock::index(&ai.index_running).map_err(public_error)?;
    ai.index_cancel.store(false, Ordering::SeqCst);
    let cancel = Arc::clone(&ai.index_cancel);
    let database = state.database.clone();
    let settings_store = ai.settings.clone();
    let emit = move |progress: IndexProgress| {
        let _ = app.emit("ai-index-progress", progress);
    };
    blocking(move || {
        let _lock = lock;
        if let Some(identity) = client.prepare_index(rebuild)? {
            settings_store.accept_embedding_identity(&identity)?;
        }
        if rebuild {
            database.ai_clear_index(&account_id, client.embed_model())?;
        }
        let mut report = |progress: IndexProgress| emit(progress);
        let result = indexer::build_index(&database, &client, &account_id, &cancel, &mut report);
        let (phase, message) = match &result {
            Ok(outcome) if outcome.cancelled => (
                "cancelled",
                "已暂停建立索引，下次会从中断处继续".to_string(),
            ),
            Ok(outcome) => (
                "complete",
                format!("索引已更新：新增 {} 个片段", outcome.chunks),
            ),
            Err(error) => ("error", error.to_string()),
        };
        report(IndexProgress {
            running: false,
            phase: phase.into(),
            current: 0,
            total: 0,
            chunks: result.as_ref().map(|outcome| outcome.chunks).unwrap_or(0),
            message,
        });
        result
    })
    .await
}

#[tauri::command]
pub fn ai_cancel_index(ai: State<'_, AiState>) {
    ai.index_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn ai_cancel(ai: State<'_, AiState>, request_id: String) {
    ai.cancel_stream(&request_id);
}

#[tauri::command]
pub fn search_messages(
    state: State<'_, AppState>,
    account_id: String,
    query: String,
    chat_id: Option<String>,
    range: DateRange,
    limit: i64,
) -> Result<Vec<SearchHit>, String> {
    state
        .database
        .search_messages(&account_id, &query, chat_id.as_deref(), &range, limit)
        .map_err(public_error)
}

#[tauri::command]
pub fn message_context(
    state: State<'_, AppState>,
    account_id: String,
    chat_id: String,
    local_id: i64,
    radius: i64,
) -> Result<Vec<MessageItem>, String> {
    state
        .database
        .message_context(&account_id, &chat_id, local_id, radius)
        .map_err(public_error)
}

#[tauri::command]
pub fn ai_estimate(
    state: State<'_, AppState>,
    account_id: String,
    chat_id: Option<String>,
    range: DateRange,
) -> Result<ContentEstimate, String> {
    state
        .database
        .ai_estimate(&account_id, chat_id.as_deref(), &range)
        .map_err(public_error)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ai_ask(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    question: String,
    chat_id: Option<String>,
    range: DateRange,
    request_id: String,
    cloud_consent: bool,
) -> Result<AskResult, String> {
    let (settings, client) = ai.client_with_consent(cloud_consent);
    settings
        .ensure_allowed(true, cloud_consent)
        .map_err(public_error)?;
    let sink = ai.stream(&app, &request_id);
    let database = state.database.clone();
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    blocking(move || {
        features::ask(
            &database,
            &client,
            &sink,
            &account_id,
            &question,
            chat_id.as_deref(),
            &range,
            &today,
        )
    })
    .await
}

#[tauri::command]
pub fn ai_event_review_estimate(
    state: State<'_, AppState>,
    account_id: String,
    date: String,
    chat_id: Option<String>,
) -> Result<EventReviewEstimate, String> {
    events::estimate_review(&state.database, &account_id, &date, chat_id.as_deref())
        .map_err(public_error)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ai_review_group_events(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    date: String,
    chat_id: Option<String>,
    request_id: String,
    mode: String,
    cloud_consent: bool,
) -> Result<EventReviewResult, String> {
    let (read_cache, generate) = cache_mode(&mode).map_err(public_error)?;
    let (settings, client) = ai.client_with_consent(cloud_consent);
    let scope = events::event_review_scope(chat_id.as_deref());
    let range = crate::models::DateRange {
        from: Some(date.clone()),
        to: Some(date.clone()),
    };
    let key = CacheKey {
        account_id: &account_id,
        kind: "event_review",
        scope: &scope,
        range: &range,
        model: &settings.chat_model,
    };
    let fingerprint = state
        .database
        .ai_group_event_fingerprint(&account_id, &date, chat_id.as_deref())
        .map_err(public_error)?;
    if read_cache {
        let hit = cached(
            &state,
            &key,
            &fingerprint,
            |value: &mut EventReviewResult, created_at| {
                value.cached = true;
                value.generated_at = Some(created_at);
            },
        )
        .map_err(public_error)?;
        if hit.is_some() || !generate {
            return hit.ok_or_else(|| "没有可用的事件回顾缓存".to_string());
        }
    }
    settings
        .ensure_allowed(true, cloud_consent)
        .map_err(public_error)?;
    let sink = ai.stream(&app, &request_id);
    let database = state.database.clone();
    let (task_account, task_date, task_chat) = (account_id.clone(), date.clone(), chat_id.clone());
    let mut result = blocking(move || {
        events::review_group_events(
            &database,
            &client,
            &sink,
            &task_account,
            &task_date,
            task_chat.as_deref(),
        )
    })
    .await?;
    if events::should_cache(&result) {
        result.generated_at =
            Some(store(&state, &key, &fingerprint, &result).map_err(public_error)?);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ai_summarize_chat(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    chat_id: String,
    range: DateRange,
    request_id: String,
    mode: String,
    cloud_consent: bool,
) -> Result<Option<SummaryResult>, String> {
    let (read_cache, generate) = cache_mode(&mode).map_err(public_error)?;
    let (settings, client) = ai.client_with_consent(cloud_consent);
    let key = CacheKey {
        account_id: &account_id,
        kind: "summary",
        scope: &chat_id,
        range: &range,
        model: &settings.chat_model,
    };
    let fingerprint = state
        .database
        .ai_fingerprint(&account_id, Some(&chat_id), &range)
        .map_err(public_error)?;
    if read_cache {
        let hit = cached(
            &state,
            &key,
            &fingerprint,
            |value: &mut SummaryResult, created_at| {
                value.cached = true;
                value.generated_at = Some(created_at);
            },
        )
        .map_err(public_error)?;
        if hit.is_some() || !generate {
            return Ok(hit);
        }
    }
    settings
        .ensure_allowed(true, cloud_consent)
        .map_err(public_error)?;
    let sink = ai.stream(&app, &request_id);
    let database = state.database.clone();
    let (task_account, task_chat, task_range) =
        (account_id.clone(), chat_id.clone(), range.clone());
    let mut result = blocking(move || {
        let chat_name = database.ai_chat_name(&task_account, &task_chat)?;
        let messages = database.ai_text_messages(&task_account, &task_chat, &task_range, None)?;
        let text = features::summarize_chat(&client, &sink, &chat_name, &messages)?;
        Ok(SummaryResult {
            text,
            message_count: messages.len(),
            cached: false,
            generated_at: None,
        })
    })
    .await?;
    result.generated_at = Some(store(&state, &key, &fingerprint, &result).map_err(public_error)?);
    Ok(Some(result))
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ai_chat_topics(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    chat_id: String,
    range: DateRange,
    request_id: String,
    mode: String,
    cloud_consent: bool,
) -> Result<Option<TopicsResult>, String> {
    let (read_cache, generate) = cache_mode(&mode).map_err(public_error)?;
    let (settings, client) = ai.client_with_consent(cloud_consent);
    let model = format!("{}|{}", settings.chat_model, client.embed_model());
    let key = CacheKey {
        account_id: &account_id,
        kind: "topics",
        scope: &chat_id,
        range: &range,
        model: &model,
    };
    let fingerprint = state
        .database
        .ai_fingerprint(&account_id, Some(&chat_id), &range)
        .map_err(public_error)?;
    if read_cache {
        let hit = cached(
            &state,
            &key,
            &fingerprint,
            |value: &mut TopicsResult, created_at| {
                value.cached = true;
                value.generated_at = Some(created_at);
            },
        )
        .map_err(public_error)?;
        if hit.is_some() || !generate {
            return Ok(hit);
        }
    }
    settings
        .ensure_allowed(true, cloud_consent)
        .map_err(public_error)?;
    let lock = TaskLock::index(&ai.index_running).map_err(public_error)?;
    let sink = ai.stream(&app, &request_id);
    let database = state.database.clone();
    let (task_account, task_chat, task_range) =
        (account_id.clone(), chat_id.clone(), range.clone());
    let mut result = blocking(move || {
        let chat_name = database.ai_chat_name(&task_account, &task_chat)?;
        sink.status("正在为这个聊天更新语义索引…");
        let stop = AtomicBool::new(false);
        let mut on_batch = |_: usize| {
            if sink.cancelled() {
                stop.store(true, Ordering::SeqCst);
            }
        };
        indexer::index_chat(
            &database,
            &client,
            &task_account,
            &task_chat,
            &chat_name,
            &stop,
            &mut on_batch,
        )?;
        drop(lock);
        if sink.cancelled() {
            return Err(super::client::cancelled());
        }
        features::chat_topics(
            &database,
            &client,
            &sink,
            &task_account,
            &task_chat,
            &task_range,
        )
    })
    .await?;
    result.generated_at = Some(store(&state, &key, &fingerprint, &result).map_err(public_error)?);
    Ok(Some(result))
}

#[tauri::command]
pub fn query_year_insights(
    state: State<'_, AppState>,
    account_id: String,
    year: Option<String>,
) -> Result<YearInsights, String> {
    state
        .database
        .year_insights(&account_id, year.as_deref())
        .map_err(public_error)
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ai_year_report(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    year: String,
    request_id: String,
    mode: String,
    anonymize: bool,
) -> Result<Option<ReportResult>, String> {
    let (read_cache, generate) = cache_mode(&mode).map_err(public_error)?;
    let (settings, client) = ai.client();
    let client = client.with_cloud_consent(settings.allow_cloud);
    let range = DateRange {
        from: Some(format!("{year}-01-01")),
        to: Some(format!("{year}-12-31")),
    };
    let key = CacheKey {
        account_id: &account_id,
        kind: "report",
        scope: &year,
        range: &range,
        model: &settings.chat_model,
    };
    let fingerprint = state
        .database
        .ai_fingerprint(&account_id, None, &range)
        .map_err(public_error)?;
    if read_cache {
        let hit = cached(
            &state,
            &key,
            &fingerprint,
            |value: &mut ReportResult, created_at| {
                value.cached = true;
                value.generated_at = Some(created_at);
            },
        )
        .map_err(public_error)?;
        if hit.is_some() || !generate {
            return Ok(hit);
        }
    }
    settings
        .ensure_allowed(false, false)
        .map_err(public_error)?;
    let sink = ai.stream(&app, &request_id);
    let database = state.database.clone();
    let task_account = account_id.clone();
    let task_year = year.clone();
    let mut result = blocking(move || {
        let insights = database.year_insights(&task_account, Some(&task_year))?;
        let text = features::year_report(&client, &sink, &insights, anonymize)?;
        Ok(ReportResult {
            text,
            anonymized: anonymize,
            cached: false,
            generated_at: None,
        })
    })
    .await?;
    result.generated_at = Some(store(&state, &key, &fingerprint, &result).map_err(public_error)?);
    Ok(Some(result))
}

#[tauri::command]
pub async fn ai_commitment_estimate(
    state: State<'_, AppState>,
    account_id: String,
    range: DateRange,
    chat_id: Option<String>,
) -> Result<ScanEstimate, String> {
    let database = state.database.clone();
    blocking(move || {
        let plans = commitments::plan_scan(&database, &account_id, chat_id.as_deref(), &range)?;
        Ok(commitments::estimate(&plans))
    })
    .await
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ai_scan_commitments(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    range: DateRange,
    chat_id: Option<String>,
    request_id: String,
    cloud_consent: bool,
) -> Result<ScanOutcome, String> {
    let (settings, client) = ai.client_with_consent(cloud_consent);
    settings
        .ensure_allowed(true, cloud_consent)
        .map_err(public_error)?;
    let lock =
        TaskLock::acquire(&ai.scan_running, "正在扫描，请等这次扫描结束").map_err(public_error)?;
    let sink = ai.stream(&app, &request_id);
    let database = state.database.clone();
    blocking(move || {
        let _lock = lock;
        commitments::scan_commitments(
            &database,
            &client,
            &sink,
            &account_id,
            chat_id.as_deref(),
            &range,
            &settings.chat_model,
        )
    })
    .await
}

#[tauri::command]
pub async fn ai_decision_estimate(
    state: State<'_, AppState>,
    account_id: String,
    chat_id: String,
    range: DateRange,
) -> Result<ScanEstimate, String> {
    let database = state.database.clone();
    blocking(move || {
        let plans = decisions::plan_scan(&database, &account_id, &chat_id, &range)?;
        Ok(decisions::estimate(&plans))
    })
    .await
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ai_scan_decisions(
    app: AppHandle,
    state: State<'_, AppState>,
    ai: State<'_, AiState>,
    account_id: String,
    chat_id: String,
    range: DateRange,
    request_id: String,
    cloud_consent: bool,
) -> Result<ScanOutcome, String> {
    let (settings, client) = ai.client_with_consent(cloud_consent);
    settings
        .ensure_allowed(true, cloud_consent)
        .map_err(public_error)?;
    let lock =
        TaskLock::acquire(&ai.scan_running, "正在扫描，请等这次扫描结束").map_err(public_error)?;
    let sink = ai.stream(&app, &request_id);
    let database = state.database.clone();
    blocking(move || {
        let _lock = lock;
        decisions::scan_decisions(
            &database,
            &client,
            &sink,
            &account_id,
            &chat_id,
            &range,
            &settings.chat_model,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_ollama_latest_tags() {
        let models = vec!["qwen3:8b".to_string(), "bge-m3:latest".to_string()];
        assert!(model_listed(&models, "bge-m3"));
        assert!(model_listed(&models, "qwen3:8b"));
        assert!(!model_listed(&models, "qwen3:14b"));
    }

    #[test]
    fn parses_cache_modes() {
        assert_eq!(cache_mode("cached").unwrap(), (true, false));
        assert_eq!(cache_mode("refresh").unwrap(), (false, true));
        assert!(cache_mode("other").is_err());
    }
}
