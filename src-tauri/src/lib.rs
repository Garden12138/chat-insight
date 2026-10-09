mod ai;
mod analysis;
mod bridge;
mod database;
mod error;
mod insights;
mod media;
mod models;
mod portable;
mod wechat_account;

use database::Database;
use media::{MediaKind, MediaResolver};
use models::{
    AccountSummary, BootstrapResult, ChatDetailData, ChatListItem, DashboardData, DateRange,
    ExportResult, ImportResult, MediaInfo, PlatformCapabilities, SyncResult,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use wechat_account::AccountStore;

struct AppState {
    database: Database,
    account_store: AccountStore,
    sync_running: Arc<AtomicBool>,
    sync_cancel: Arc<AtomicBool>,
}

impl AppState {
    fn new(database: Database, account_store: AccountStore) -> Self {
        Self {
            database,
            account_store,
            sync_running: Arc::new(AtomicBool::new(false)),
            sync_cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn public_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[tauri::command]
fn get_capabilities(app: AppHandle) -> PlatformCapabilities {
    bridge::capabilities(&app)
}

#[tauri::command]
fn list_accounts(state: State<'_, AppState>) -> Result<Vec<AccountSummary>, String> {
    state.database.list_accounts().map_err(public_error)
}

#[tauri::command]
async fn bootstrap_bridge(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BootstrapResult, String> {
    connect_bridge(app, &state, false).await
}

#[tauri::command]
async fn switch_wechat_account(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<BootstrapResult, String> {
    connect_bridge(app, &state, true).await
}

async fn connect_bridge(
    app: AppHandle,
    state: &AppState,
    require_switch: bool,
) -> Result<BootstrapResult, String> {
    if state
        .sync_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有同步任务正在运行，请等待完成后再连接微信".into());
    }
    let running = Arc::clone(&state.sync_running);
    let store = state.account_store.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = bridge::connect(&app, &store, require_switch);
        running.store(false, Ordering::SeqCst);
        result
    })
    .await
    .map_err(|_| "本机授权任务意外结束".to_string())?;
    result.map_err(public_error)
}

#[tauri::command]
async fn start_sync(app: AppHandle, state: State<'_, AppState>) -> Result<SyncResult, String> {
    if state
        .sync_running
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("已有同步任务正在运行".into());
    }
    state.sync_cancel.store(false, Ordering::SeqCst);
    let running = Arc::clone(&state.sync_running);
    let cancel = Arc::clone(&state.sync_cancel);
    let database = state.database.clone();
    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let result = bridge::sync_wechat(&worker_app, &database, &cancel);
        running.store(false, Ordering::SeqCst);
        result
    })
    .await
    .map_err(|_| "同步任务意外结束".to_string())?;
    result.map_err(public_error)
}

#[tauri::command]
fn cancel_sync(state: State<'_, AppState>) {
    state.sync_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
fn query_dashboard(
    state: State<'_, AppState>,
    account_id: String,
    range: DateRange,
) -> Result<DashboardData, String> {
    state
        .database
        .dashboard(&account_id, &range)
        .map_err(public_error)
}

#[tauri::command]
fn list_chats(
    state: State<'_, AppState>,
    account_id: String,
    search: String,
) -> Result<Vec<ChatListItem>, String> {
    state
        .database
        .list_chats(&account_id, &search)
        .map_err(public_error)
}

#[tauri::command]
fn query_chat_detail(
    state: State<'_, AppState>,
    account_id: String,
    chat_id: String,
    range: DateRange,
    offset: i64,
    limit: i64,
) -> Result<ChatDetailData, String> {
    state
        .database
        .chat_detail(&account_id, &chat_id, &range, offset, limit)
        .map_err(public_error)
}

#[tauri::command]
async fn media_info(
    app: AppHandle,
    resolver: State<'_, MediaResolver>,
    account_id: String,
    chat_id: String,
    local_id: i64,
    kind: String,
) -> Result<MediaInfo, String> {
    let resolver = resolver.inner().clone();
    let kind = MediaKind::from_message_kind(&kind);
    tauri::async_runtime::spawn_blocking(move || {
        resolver.info(&app, &account_id, &chat_id, local_id, kind)
    })
    .await
    .map_err(|_| "媒体读取任务意外结束".to_string())
}

#[tauri::command]
async fn import_jsonl(state: State<'_, AppState>, path: PathBuf) -> Result<ImportResult, String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || portable::import_jsonl(&database, &path))
        .await
        .map_err(|_| "导入任务意外结束".to_string())?
        .map_err(public_error)
}

#[tauri::command]
async fn export_jsonl(
    state: State<'_, AppState>,
    account_id: String,
    path: PathBuf,
) -> Result<ExportResult, String> {
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        portable::export_jsonl(&database, &account_id, &path)
    })
    .await
    .map_err(|_| "导出任务意外结束".to_string())?
    .map_err(public_error)
}

#[tauri::command]
fn clear_account(state: State<'_, AppState>, account_id: String) -> Result<(), String> {
    state
        .database
        .clear_account(&account_id)
        .map_err(public_error)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .register_asynchronous_uri_scheme_protocol("chatmedia", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(media::respond(&app, &request));
            });
        })
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            let database = Database::new(app_data.join("chat-insight.db"));
            database
                .initialize()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            // Best effort: an unreadable bridge config leaves the old dataset as is.
            let _ = wechat_account::adopt_legacy_dataset(&database);
            let account_store = AccountStore::new(app_data.join("wechat-accounts"));
            app.manage(AppState::new(database, account_store));
            app.manage(MediaResolver::new(app_data.join("sticker-cache")));
            app.manage(ai::AiState::new(ai::settings::SettingsStore::new(
                app_data.join("ai"),
            )));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_capabilities,
            list_accounts,
            bootstrap_bridge,
            switch_wechat_account,
            start_sync,
            cancel_sync,
            query_dashboard,
            list_chats,
            query_chat_detail,
            media_info,
            import_jsonl,
            export_jsonl,
            clear_account,
            ai::commands::ai_get_settings,
            ai::commands::ai_save_settings,
            ai::commands::ai_test_connection,
            ai::commands::ai_index_status,
            ai::commands::ai_build_index,
            ai::commands::ai_cancel_index,
            ai::commands::ai_cancel,
            ai::commands::ai_estimate,
            ai::commands::search_messages,
            ai::commands::message_context,
            ai::commands::ai_ask,
            ai::commands::ai_summarize_chat,
            ai::commands::ai_chat_topics,
            ai::commands::query_year_insights,
            ai::commands::ai_year_report,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Chat Insight");
}
