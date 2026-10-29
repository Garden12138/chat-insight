use super::client::{cancelled, ChatMessage, LlmClient};
use super::dates::format_ymd;
use super::extraction;
use super::store::EventTextMessage;
use super::Sink;
use crate::database::Database;
use crate::error::{AppError, AppResult};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const GAP_MINUTES: i64 = 45;
const MAX_SEGMENT_CHARS: usize = 8000;
const CATEGORIES: &[&str] = &[
    "announcement",
    "decision",
    "task_progress",
    "issue",
    "plan",
    "information",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventReviewEstimate {
    pub date: String,
    pub chat_count: usize,
    pub message_count: usize,
    pub chars: usize,
    pub segment_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSourceRef {
    pub chat_id: String,
    pub chat_name: String,
    pub chat_kind: String,
    pub local_id: i64,
    pub occurred_at: String,
    pub sender_name: String,
    pub is_from_me: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventItem {
    pub id: String,
    pub title: String,
    pub category: String,
    pub summary: String,
    pub start_at: String,
    pub end_at: String,
    pub source_refs: Vec<EventSourceRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduled_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventReviewResult {
    pub date: String,
    pub scope: String,
    pub events: Vec<EventItem>,
    pub chat_count: usize,
    pub message_count: usize,
    pub processed_messages: usize,
    pub total_segments: usize,
    pub completed_segments: usize,
    pub failed_segments: usize,
    pub complete: bool,
    pub cancelled: bool,
    pub cached: bool,
    pub generated_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EventSegment {
    pub chat_id: String,
    pub chat_name: String,
    pub messages: Vec<EventTextMessage>,
}

pub fn should_cache(result: &EventReviewResult) -> bool {
    result.complete && !result.cancelled && result.failed_segments == 0
}

pub fn event_review_scope(chat_id: Option<&str>) -> String {
    chat_id
        .filter(|value| !value.is_empty())
        .unwrap_or("all-chats")
        .to_string()
}

fn minutes_between(earlier: &str, later: &str) -> i64 {
    match (
        DateTime::parse_from_rfc3339(earlier),
        DateTime::parse_from_rfc3339(later),
    ) {
        (Ok(start), Ok(end)) => (end - start).num_minutes(),
        _ => 0,
    }
}

fn message_chars(message: &EventTextMessage) -> usize {
    message.text.chars().count()
}

pub fn segment_messages(messages: &[EventTextMessage]) -> Vec<EventSegment> {
    let mut by_chat: Vec<(String, String, Vec<EventTextMessage>)> = Vec::new();
    let mut index_by_chat = HashMap::<String, usize>::new();
    for message in messages {
        if let Some(&index) = index_by_chat.get(&message.chat_id) {
            by_chat[index].2.push(message.clone());
        } else {
            index_by_chat.insert(message.chat_id.clone(), by_chat.len());
            by_chat.push((
                message.chat_id.clone(),
                message.chat_name.clone(),
                vec![message.clone()],
            ));
        }
    }

    let mut segments = Vec::new();
    for (chat_id, chat_name, chat_messages) in by_chat {
        let mut current: Vec<EventTextMessage> = Vec::new();
        let mut chars = 0usize;
        for message in chat_messages {
            let size = message_chars(&message);
            let should_split = current.last().is_some_and(|previous| {
                minutes_between(&previous.occurred_at, &message.occurred_at) > GAP_MINUTES
                    || (chars + size > MAX_SEGMENT_CHARS && !current.is_empty())
            });
            if should_split {
                segments.push(EventSegment {
                    chat_id: chat_id.clone(),
                    chat_name: chat_name.clone(),
                    messages: std::mem::take(&mut current),
                });
                chars = 0;
            }
            chars += size;
            current.push(message);
        }
        if !current.is_empty() {
            segments.push(EventSegment {
                chat_id,
                chat_name,
                messages: current,
            });
        }
    }
    segments
}

pub fn estimate_from_messages(date: &str, messages: &[EventTextMessage]) -> EventReviewEstimate {
    let chats: HashSet<&str> = messages
        .iter()
        .map(|message| message.chat_id.as_str())
        .collect();
    EventReviewEstimate {
        date: date.to_string(),
        chat_count: chats.len(),
        message_count: messages.len(),
        chars: messages.iter().map(message_chars).sum(),
        segment_count: segment_messages(messages).len(),
    }
}

pub fn estimate_review(
    db: &Database,
    account_id: &str,
    date: &str,
    chat_id: Option<&str>,
) -> AppResult<EventReviewEstimate> {
    let messages = db.ai_group_event_messages(account_id, date, chat_id)?;
    Ok(estimate_from_messages(date, &messages))
}

fn short_time(occurred_at: &str) -> String {
    occurred_at
        .get(11..16)
        .or_else(|| occurred_at.get(..16))
        .unwrap_or(occurred_at)
        .replace('T', " ")
}

fn event_speaker(message: &EventTextMessage) -> &str {
    if message.is_from_me {
        "我"
    } else if message.sender_name.trim().is_empty() {
        "对方"
    } else {
        message.sender_name.trim()
    }
}

fn numbered_prompt(segment: &EventSegment) -> String {
    segment
        .messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            format!(
                "[{}] {} {} {}: {}",
                index + 1,
                message.local_date,
                short_time(&message.occurred_at),
                event_speaker(message),
                message.text.trim()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn event_prompt(date: &str, segment: &EventSegment) -> Vec<ChatMessage> {
    vec![
        ChatMessage::system(format!(
            "你是 Chat Insight 的聊天事件整理助手。只根据本段编号消息提取 {date} 当天值得记录的事件。\
             使用简体中文。不要编造。闲聊、表情和没有事实的寒暄不要输出。\
             只返回 JSON，格式为 {{\"events\":[{{\"title\":\"\",\"category\":\"announcement|decision|task_progress|issue|plan|information\",\"summary\":\"\",\"source_indices\":[1],\"scheduled_date\":\"YYYY-MM-DD\"}}]}}。\
             category 只能是上述六个值。source_indices 必须是本段已有编号，至少有一个。\
             若消息明确提到未来日期，把 scheduled_date 填成该日期，并把 category 设为 plan。\
             没有事件时返回 {{\"events\":[]}}。"
        )),
        ChatMessage::user(format!(
            "聊天：{}（{}，{}）\n日期：{date}\n\n本段消息：\n{}",
            segment.chat_name,
            if segment.messages.first().is_some_and(|message| message.chat_kind == "group") {
                "群聊"
            } else {
                "私聊"
            },
            segment.chat_id,
            numbered_prompt(segment)
        )),
    ]
}

fn source_ref(message: &EventTextMessage) -> EventSourceRef {
    EventSourceRef {
        chat_id: message.chat_id.clone(),
        chat_name: message.chat_name.clone(),
        chat_kind: message.chat_kind.clone(),
        local_id: message.local_id,
        occurred_at: message.occurred_at.clone(),
        sender_name: message.sender_name.clone(),
        is_from_me: message.is_from_me,
        text: message.text.clone(),
    }
}

fn normalize_title(title: &str) -> String {
    title
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_lowercase()
}

fn valid_category(value: &str) -> Option<String> {
    CATEGORIES
        .iter()
        .find(|category| **category == value)
        .map(|category| (*category).to_string())
}

fn parse_scheduled_date(raw: Option<&Value>, review_date: &str) -> Option<String> {
    let text = extraction::text(raw, 16)?;
    let date = NaiveDate::parse_from_str(&text, "%Y-%m-%d").ok()?;
    let formatted = format_ymd(date);
    (formatted.as_str() >= review_date).then_some(formatted)
}

fn parse_events_from_reply(
    reply: &str,
    segment: &EventSegment,
    review_date: &str,
) -> Result<Vec<EventItem>, AppError> {
    let value = extraction::parse_reply(reply)
        .ok_or_else(|| AppError::Ai("模型返回的事件 JSON 无法解析".into()))?;
    let items = value
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| AppError::Ai("模型返回缺少 events".into()))?;
    let mut events = Vec::new();
    let mut had_invalid = false;
    for item in items {
        let title = match extraction::text(item.get("title"), 80) {
            Some(title) => title,
            None => {
                had_invalid = true;
                continue;
            }
        };
        let mut category = match item
            .get("category")
            .and_then(Value::as_str)
            .and_then(valid_category)
        {
            Some(category) => category,
            None => {
                had_invalid = true;
                continue;
            }
        };
        let summary = extraction::text(item.get("summary"), 400).unwrap_or_default();
        let indices = item
            .get("source_indices")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut seen = HashSet::new();
        let mut refs = Vec::new();
        for value in indices {
            match extraction::index(Some(&value)).and_then(|index| index.checked_sub(1)) {
                Some(index) if index < segment.messages.len() && seen.insert(index) => {
                    refs.push(source_ref(&segment.messages[index]));
                }
                Some(_) | None => had_invalid = true,
            }
        }
        if refs.is_empty() {
            had_invalid = true;
            continue;
        }
        refs.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then(left.chat_id.cmp(&right.chat_id))
                .then(left.local_id.cmp(&right.local_id))
        });
        if let Some(scheduled) = parse_scheduled_date(item.get("scheduled_date"), review_date) {
            if scheduled.as_str() > review_date {
                category = "plan".into();
            }
            let start_at = refs[0].occurred_at.clone();
            let end_at = refs.last().unwrap().occurred_at.clone();
            events.push(EventItem {
                id: format!(
                    "{}:{}:{}",
                    refs[0].chat_id,
                    refs[0].local_id,
                    normalize_title(&title)
                ),
                title,
                category,
                summary,
                start_at,
                end_at,
                source_refs: refs,
                scheduled_date: Some(scheduled),
            });
        } else {
            let start_at = refs[0].occurred_at.clone();
            let end_at = refs.last().unwrap().occurred_at.clone();
            events.push(EventItem {
                id: format!(
                    "{}:{}:{}",
                    refs[0].chat_id,
                    refs[0].local_id,
                    normalize_title(&title)
                ),
                title,
                category,
                summary,
                start_at,
                end_at,
                source_refs: refs,
                scheduled_date: None,
            });
        }
    }
    if had_invalid && events.is_empty() {
        return Err(AppError::Ai("事件来源无法验证".into()));
    }
    Ok(events)
}

fn sources_overlap(left: &[EventSourceRef], right: &[EventSourceRef]) -> bool {
    left.iter().any(|first| {
        right
            .iter()
            .any(|second| first.chat_id == second.chat_id && first.local_id == second.local_id)
    })
}

fn merge_refs(left: &[EventSourceRef], right: &[EventSourceRef]) -> Vec<EventSourceRef> {
    let mut merged = left.to_vec();
    for item in right {
        if !merged
            .iter()
            .any(|existing| existing.chat_id == item.chat_id && existing.local_id == item.local_id)
        {
            merged.push(item.clone());
        }
    }
    merged.sort_by(|a, b| {
        a.occurred_at
            .cmp(&b.occurred_at)
            .then(a.chat_id.cmp(&b.chat_id))
            .then(a.local_id.cmp(&b.local_id))
    });
    merged
}

pub fn dedupe_events(events: Vec<EventItem>) -> Vec<EventItem> {
    let mut kept: Vec<EventItem> = Vec::new();
    for event in events {
        if let Some(existing) = kept.iter_mut().find(|candidate| {
            candidate.category == event.category
                && (normalize_title(&candidate.title) == normalize_title(&event.title)
                    || sources_overlap(&candidate.source_refs, &event.source_refs))
        }) {
            existing.source_refs = merge_refs(&existing.source_refs, &event.source_refs);
            if existing.summary.len() < event.summary.len() {
                existing.summary = event.summary;
            }
            if existing.start_at > event.start_at {
                existing.start_at = event.start_at;
                existing.title = event.title;
                existing.id = event.id;
            }
            if existing.end_at < event.end_at {
                existing.end_at = event.end_at;
            }
            if existing.scheduled_date.is_none() {
                existing.scheduled_date = event.scheduled_date;
            }
        } else {
            kept.push(event);
        }
    }
    kept.sort_by(|left, right| {
        left.start_at
            .cmp(&right.start_at)
            .then(
                left.source_refs[0]
                    .chat_id
                    .cmp(&right.source_refs[0].chat_id),
            )
            .then(
                left.source_refs[0]
                    .local_id
                    .cmp(&right.source_refs[0].local_id),
            )
    });
    kept
}

fn is_cancelled(error: &AppError) -> bool {
    matches!(error, AppError::Ai(message) if message == "已停止生成")
}

fn empty_result(
    date: &str,
    chat_id: Option<&str>,
    estimate: EventReviewEstimate,
) -> EventReviewResult {
    EventReviewResult {
        date: date.to_string(),
        scope: event_review_scope(chat_id),
        events: Vec::new(),
        chat_count: estimate.chat_count,
        message_count: estimate.message_count,
        processed_messages: 0,
        total_segments: estimate.segment_count,
        completed_segments: 0,
        failed_segments: 0,
        complete: true,
        cancelled: false,
        cached: false,
        generated_at: None,
    }
}

pub fn review_group_events(
    db: &Database,
    client: &dyn LlmClient,
    sink: &dyn Sink,
    account_id: &str,
    date: &str,
    chat_id: Option<&str>,
) -> AppResult<EventReviewResult> {
    if NaiveDate::parse_from_str(date, "%Y-%m-%d").is_err() {
        return Err(AppError::Validation("请选择有效的回顾日期".into()));
    }
    let messages = db.ai_group_event_messages(account_id, date, chat_id)?;
    let estimate = estimate_from_messages(date, &messages);
    if messages.is_empty() {
        return Ok(empty_result(date, chat_id, estimate));
    }
    let segments = segment_messages(&messages);
    let mut events = Vec::new();
    let mut completed = 0usize;
    let mut failed = 0usize;
    let mut processed = 0usize;
    let mut cancelled_run = false;
    for (index, segment) in segments.iter().enumerate() {
        if sink.cancelled() {
            cancelled_run = true;
            break;
        }
        sink.status(&format!(
            "正在分析第 {}/{} 个片段",
            index + 1,
            segments.len()
        ));
        match client.chat(&event_prompt(date, segment), &mut |_| !sink.cancelled()) {
            Ok(reply) => match parse_events_from_reply(&reply, segment, date) {
                Ok(parsed) => {
                    completed += 1;
                    processed += segment.messages.len();
                    events.extend(parsed);
                }
                Err(_) => {
                    failed += 1;
                }
            },
            Err(error) if is_cancelled(&error) || error.to_string() == cancelled().to_string() => {
                cancelled_run = true;
                break;
            }
            Err(_) => {
                failed += 1;
            }
        }
    }
    if sink.cancelled() {
        cancelled_run = true;
    }
    let complete = !cancelled_run && failed == 0 && completed == segments.len();
    Ok(EventReviewResult {
        date: date.to_string(),
        scope: event_review_scope(chat_id),
        events: dedupe_events(events),
        chat_count: estimate.chat_count,
        message_count: estimate.message_count,
        processed_messages: processed,
        total_segments: segments.len(),
        completed_segments: completed,
        failed_segments: failed,
        complete,
        cancelled: cancelled_run,
        cached: false,
        generated_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::client::fake::FakeClient;
    use crate::ai::NullSink;
    use crate::models::NormalizedMessage;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tempfile::tempdir;

    struct CancelSink {
        after: usize,
        seen: AtomicUsize,
        flag: AtomicBool,
    }

    impl CancelSink {
        fn new(after: usize) -> Self {
            Self {
                after,
                seen: AtomicUsize::new(0),
                flag: AtomicBool::new(false),
            }
        }
    }

    impl Sink for CancelSink {
        fn delta(&self, _text: &str) -> bool {
            !self.cancelled()
        }
        fn status(&self, _text: &str) {
            let seen = self.seen.fetch_add(1, Ordering::SeqCst) + 1;
            if seen >= self.after {
                self.flag.store(true, Ordering::SeqCst);
            }
        }
        fn cancelled(&self) -> bool {
            self.flag.load(Ordering::SeqCst)
        }
    }

    fn fixture_db() -> Database {
        let directory = tempdir().unwrap().keep();
        let db = Database::new(directory.join("test.db"));
        db.initialize().unwrap();
        db.upsert_account("account", "测试", "fixture").unwrap();
        db
    }

    fn add_chat(db: &Database, id: &str, name: &str) {
        db.upsert_chat("account", id, name, "group").unwrap();
    }

    fn add_direct(db: &Database, id: &str, name: &str) {
        db.upsert_chat("account", id, name, "direct").unwrap();
    }

    fn add_msg(db: &Database, chat: &str, local_id: i64, at: &str, text: &str) {
        db.insert_message(
            "account",
            &NormalizedMessage {
                chat_id: chat.into(),
                local_id,
                server_id: None,
                occurred_at: at.into(),
                local_date: at[..10].into(),
                local_hour: 9,
                local_weekday: 2,
                sender_id: "other".into(),
                sender_name: "成员".into(),
                is_from_me: false,
                kind: "text".into(),
                text: text.into(),
            },
        )
        .unwrap();
    }

    fn event_json(title: &str, category: &str, indices: &[usize]) -> String {
        let source = indices
            .iter()
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!(
            r#"{{"events":[{{"title":"{title}","category":"{category}","summary":"{title}","source_indices":[{source}]}}]}}"#
        )
    }

    #[test]
    fn every_message_belongs_to_exactly_one_segment() {
        let mut messages = Vec::new();
        for chat in 0..8 {
            add_chat_message(
                &mut messages,
                &format!("g{chat}"),
                chat + 1,
                "2026-10-27T09:00:00+08:00",
                "通知",
            );
            add_chat_message(
                &mut messages,
                &format!("g{chat}"),
                chat + 20,
                "2026-10-27T12:00:00+08:00",
                "后续",
            );
        }
        let long = "很长的正文".repeat(2000);
        add_chat_message(&mut messages, "g0", 99, "2026-10-27T18:00:00+08:00", &long);
        let segments = segment_messages(&messages);
        let mut seen = HashSet::new();
        for segment in &segments {
            for message in &segment.messages {
                assert_eq!(message.local_date, "2026-10-27");
                assert!(
                    seen.insert((message.chat_id.clone(), message.local_id)),
                    "duplicate assignment"
                );
            }
        }
        assert_eq!(seen.len(), messages.len());
        assert!(segments.len() > 12);
        assert!(segments.iter().any(|segment| {
            segment
                .messages
                .iter()
                .any(|message| message.text.chars().count() > MAX_SEGMENT_CHARS)
        }));
        assert!(numbered_prompt(&segments[0]).contains("[1]"));
        let long_prompt = numbered_prompt(
            segments
                .iter()
                .find(|segment| {
                    segment
                        .messages
                        .iter()
                        .any(|message| message.local_id == 99)
                })
                .unwrap(),
        );
        assert!(long_prompt.contains(&long));
    }

    fn add_chat_message(
        messages: &mut Vec<EventTextMessage>,
        chat_id: &str,
        local_id: i64,
        at: &str,
        text: &str,
    ) {
        messages.push(EventTextMessage {
            chat_id: chat_id.into(),
            chat_name: chat_id.into(),
            chat_kind: "group".into(),
            local_id,
            occurred_at: at.into(),
            local_date: at[..10].into(),
            sender_name: "成员".into(),
            is_from_me: false,
            text: text.into(),
        });
    }

    #[test]
    fn validates_json_sources_and_keeps_chat_local_id_pairs() {
        let db = fixture_db();
        add_chat(&db, "g1", "产品群");
        add_chat(&db, "g2", "运营群");
        add_msg(
            &db,
            "g1",
            1,
            "2026-10-27T09:00:00+08:00",
            "发布窗口改为今晚",
        );
        add_msg(
            &db,
            "g2",
            1,
            "2026-10-27T09:05:00+08:00",
            "同样的 local_id 在另一个群",
        );
        let client = FakeClient::new(&[
            &event_json("发布窗口调整", "decision", &[1]),
            &event_json("运营同步", "information", &[1]),
        ]);
        let result =
            review_group_events(&db, &client, &NullSink, "account", "2026-10-27", None).unwrap();
        assert_eq!(result.events.len(), 2);
        assert!(result.complete);
        assert_eq!(result.failed_segments, 0);
        assert!(result
            .events
            .iter()
            .all(|event| !event.source_refs.is_empty()));
        let chats: HashSet<_> = result
            .events
            .iter()
            .flat_map(|event| {
                event
                    .source_refs
                    .iter()
                    .map(|source| (source.chat_id.as_str(), source.local_id))
            })
            .collect();
        assert!(chats.contains(&("g1", 1)));
        assert!(chats.contains(&("g2", 1)));
        assert!(should_cache(&result));
    }

    #[test]
    fn drops_illegal_json_and_out_of_range_sources() {
        let db = fixture_db();
        add_chat(&db, "g1", "产品群");
        add_msg(&db, "g1", 1, "2026-10-27T09:00:00+08:00", "接口告警恢复");
        add_msg(&db, "g1", 2, "2026-10-27T10:00:00+08:00", "继续观察");
        let client = FakeClient::new(&[
            "这不是 JSON",
            r#"{"events":[{"title":"坏引用","category":"issue","summary":"x","source_indices":[9]}]}"#,
        ]);
        let result =
            review_group_events(&db, &client, &NullSink, "account", "2026-10-27", None).unwrap();
        assert!(!result.complete);
        assert_eq!(result.failed_segments, result.total_segments);
        assert!(result.events.is_empty());
        assert!(!should_cache(&result));
    }

    #[test]
    fn dedupes_same_event_and_keeps_unrelated_ones() {
        let first = EventItem {
            id: "g1:1:发布".into(),
            title: "发布窗口调整".into(),
            category: "decision".into(),
            summary: "短".into(),
            start_at: "2026-10-27T09:00:00+08:00".into(),
            end_at: "2026-10-27T09:00:00+08:00".into(),
            source_refs: vec![EventSourceRef {
                chat_id: "g1".into(),
                chat_name: "产品群".into(),
                chat_kind: "group".into(),
                local_id: 1,
                occurred_at: "2026-10-27T09:00:00+08:00".into(),
                sender_name: "成员".into(),
                is_from_me: false,
                text: "发布窗口改为今晚".into(),
            }],
            scheduled_date: None,
        };
        let duplicate = EventItem {
            id: "g1:2:发布重复".into(),
            title: "发布窗口调整".into(),
            category: "decision".into(),
            summary: "更完整的摘要".into(),
            start_at: "2026-10-27T09:10:00+08:00".into(),
            end_at: "2026-10-27T09:10:00+08:00".into(),
            source_refs: vec![EventSourceRef {
                chat_id: "g1".into(),
                chat_name: "产品群".into(),
                chat_kind: "group".into(),
                local_id: 2,
                occurred_at: "2026-10-27T09:10:00+08:00".into(),
                sender_name: "成员".into(),
                is_from_me: false,
                text: "对，今晚发布".into(),
            }],
            scheduled_date: None,
        };
        let other = EventItem {
            id: "g2:1:故障".into(),
            title: "接口超时".into(),
            category: "issue".into(),
            summary: "另一件事".into(),
            start_at: "2026-10-27T08:00:00+08:00".into(),
            end_at: "2026-10-27T08:00:00+08:00".into(),
            source_refs: vec![EventSourceRef {
                chat_id: "g2".into(),
                chat_name: "运营群".into(),
                chat_kind: "group".into(),
                local_id: 1,
                occurred_at: "2026-10-27T08:00:00+08:00".into(),
                sender_name: "成员".into(),
                is_from_me: false,
                text: "接口超时".into(),
            }],
            scheduled_date: None,
        };
        let merged = dedupe_events(vec![first, duplicate, other]);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].title, "接口超时");
        assert_eq!(merged[1].source_refs.len(), 2);
        assert_eq!(merged[1].summary, "更完整的摘要");
    }

    #[test]
    fn future_schedule_is_marked_plan() {
        let db = fixture_db();
        add_chat(&db, "g1", "产品群");
        add_msg(&db, "g1", 1, "2026-10-27T09:00:00+08:00", "下周一全员评审");
        let client = FakeClient::new(&[
            r#"{"events":[{"title":"全员评审","category":"announcement","summary":"下周一评审","source_indices":[1],"scheduled_date":"2026-11-02"}]}"#,
        ]);
        let result =
            review_group_events(&db, &client, &NullSink, "account", "2026-10-27", None).unwrap();
        assert_eq!(result.events[0].category, "plan");
        assert_eq!(
            result.events[0].scheduled_date.as_deref(),
            Some("2026-11-02")
        );
    }

    #[test]
    fn cancel_returns_partial_uncached_result() {
        let db = fixture_db();
        add_chat(&db, "g1", "产品群");
        add_chat(&db, "g2", "运营群");
        add_msg(&db, "g1", 1, "2026-10-27T09:00:00+08:00", "第一条");
        add_msg(&db, "g2", 1, "2026-10-27T12:00:00+08:00", "第二条");
        let client = FakeClient::new(&[
            &event_json("第一条事件", "information", &[1]),
            &event_json("第二条事件", "information", &[1]),
        ]);
        let sink = CancelSink::new(1);
        let result =
            review_group_events(&db, &client, &sink, "account", "2026-10-27", None).unwrap();
        assert!(result.cancelled);
        assert!(!result.complete);
        assert!(!should_cache(&result));
        assert!(result.completed_segments < result.total_segments);
    }

    #[test]
    fn estimate_does_not_need_a_model() {
        let db = fixture_db();
        add_chat(&db, "g1", "产品群");
        add_direct(&db, "alice", "小艾");
        add_msg(&db, "g1", 1, "2026-10-27T09:00:00+08:00", "群消息");
        add_msg(&db, "alice", 1, "2026-10-27T09:00:00+08:00", "私聊");
        let estimate = estimate_review(&db, "account", "2026-10-27", None).unwrap();
        assert_eq!(estimate.chat_count, 2);
        assert_eq!(estimate.message_count, 2);
        assert_eq!(estimate.segment_count, 2);
        let group_only = estimate_review(&db, "account", "2026-10-27", Some("g1")).unwrap();
        assert_eq!(group_only.chat_count, 1);
        assert_eq!(group_only.message_count, 1);
        let direct_only = estimate_review(&db, "account", "2026-10-27", Some("alice")).unwrap();
        assert_eq!(direct_only.chat_count, 1);
        assert_eq!(direct_only.message_count, 1);
    }
}
