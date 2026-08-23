use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub os: String,
    pub arch: String,
    pub direct_wechat_read: bool,
    pub bridge_bundled: bool,
    pub setup_state: String,
    pub setup_hint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub id: String,
    pub display_name: String,
    pub source_platform: String,
    pub message_count: i64,
    pub chat_count: i64,
    pub last_sync_at: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DateRange {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryMetrics {
    pub total_messages: i64,
    pub active_days: i64,
    pub chat_count: i64,
    pub sent_ratio: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendPoint {
    pub bucket: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatmapPoint {
    pub weekday: i64,
    pub hour: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedChat {
    pub chat_id: String,
    pub display_name: String,
    pub chat_kind: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeCount {
    pub kind: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardData {
    pub metrics: SummaryMetrics,
    pub daily_trend: Vec<TrendPoint>,
    pub monthly_trend: Vec<TrendPoint>,
    pub heatmap: Vec<HeatmapPoint>,
    pub top_chats: Vec<RankedChat>,
    pub message_types: Vec<TypeCount>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatListItem {
    pub id: String,
    pub display_name: String,
    pub chat_kind: String,
    pub message_count: i64,
    pub last_message_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeywordCount {
    pub word: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageItem {
    pub local_id: i64,
    pub server_id: Option<String>,
    pub occurred_at: String,
    pub sender_name: String,
    pub is_from_me: bool,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatDetailData {
    pub chat: ChatListItem,
    pub total_messages: i64,
    pub sent_messages: i64,
    pub received_messages: i64,
    pub first_message_at: Option<String>,
    pub last_message_at: Option<String>,
    pub daily_trend: Vec<TrendPoint>,
    pub hourly_activity: Vec<HourlyPoint>,
    pub keywords: Vec<KeywordCount>,
    pub messages: Vec<MessageItem>,
    pub next_offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourlyPoint {
    pub hour: i64,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub message: String,
    pub warnings: usize,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub account_id: String,
    pub chats_processed: usize,
    pub messages_imported: usize,
    pub warnings: usize,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapResult {
    pub ready: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub account_id: String,
    pub chats_imported: usize,
    pub messages_imported: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub chats_exported: usize,
    pub messages_exported: usize,
}

#[derive(Debug, Clone)]
pub struct NormalizedMessage {
    pub chat_id: String,
    pub local_id: i64,
    pub server_id: Option<String>,
    pub occurred_at: String,
    pub local_date: String,
    pub local_hour: u32,
    pub local_weekday: u32,
    pub sender_id: String,
    pub sender_name: String,
    pub is_from_me: bool,
    pub kind: String,
    pub text: String,
}
