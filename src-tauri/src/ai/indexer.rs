use super::client::LlmClient;
use super::store::TextMessage;
use crate::database::Database;
use crate::error::AppResult;
use crate::models::DateRange;
use chrono::DateTime;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};

const GAP_MINUTES: i64 = 30;
const MAX_MESSAGES: usize = 12;
const MAX_CHARS: usize = 600;
const MAX_LINE_CHARS: usize = 500;
const MIN_CHUNK_CHARS: usize = 4;
const EMBED_BATCH: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct ChunkDraft {
    pub start_local_id: i64,
    pub end_local_id: i64,
    pub start_at: String,
    pub end_at: String,
    pub local_date: String,
    pub message_count: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress {
    pub running: bool,
    pub phase: String,
    pub current: usize,
    pub total: usize,
    pub chunks: usize,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexOutcome {
    pub chats: usize,
    pub chunks: usize,
    pub cancelled: bool,
}

pub fn speaker(message: &TextMessage) -> &str {
    if message.is_from_me {
        "我"
    } else if message.sender_name.trim().is_empty() {
        "对方"
    } else {
        message.sender_name.trim()
    }
}

pub fn message_line(message: &TextMessage) -> String {
    let text: String = message.text.trim().chars().take(MAX_LINE_CHARS).collect();
    format!("{}: {}", speaker(message), text.replace('\n', " "))
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

/// Groups consecutive messages into conversation-sized chunks. A new chunk starts after
/// a quiet gap, or when the current one has enough messages or text.
pub fn chunk_messages(messages: &[TextMessage]) -> Vec<ChunkDraft> {
    let mut chunks = Vec::new();
    let mut group: Vec<&TextMessage> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let mut chars = 0usize;
    let mut content_chars = 0usize;

    let mut flush =
        |group: &mut Vec<&TextMessage>, lines: &mut Vec<String>, content_chars: usize| {
            if let (Some(first), Some(last)) = (group.first(), group.last()) {
                if content_chars >= MIN_CHUNK_CHARS {
                    chunks.push(ChunkDraft {
                        start_local_id: first.local_id,
                        end_local_id: last.local_id,
                        start_at: first.occurred_at.clone(),
                        end_at: last.occurred_at.clone(),
                        local_date: first.local_date.clone(),
                        message_count: group.len(),
                        text: lines.join("\n"),
                    });
                }
            }
            group.clear();
            lines.clear();
        };

    for message in messages {
        let text = message.text.trim();
        if text.is_empty() {
            continue;
        }
        let line = message_line(message);
        let line_chars = line.chars().count();
        if let Some(previous) = group.last() {
            if minutes_between(&previous.occurred_at, &message.occurred_at) > GAP_MINUTES
                || group.len() >= MAX_MESSAGES
                || chars + line_chars > MAX_CHARS
            {
                flush(&mut group, &mut lines, content_chars);
                chars = 0;
                content_chars = 0;
            }
        }
        chars += line_chars;
        content_chars += text.chars().count();
        group.push(message);
        lines.push(line);
    }
    flush(&mut group, &mut lines, content_chars);
    chunks
}

fn embedding_input(chat_name: &str, chunk: &ChunkDraft) -> String {
    format!(
        "聊天：{chat_name}\n日期：{}\n{}",
        chunk.local_date, chunk.text
    )
}

/// Embeds new messages of one chat. Progress is committed per batch so a cancelled or
/// failed run resumes where it stopped.
pub fn index_chat(
    db: &Database,
    client: &dyn LlmClient,
    account_id: &str,
    chat_id: &str,
    chat_name: &str,
    cancel: &AtomicBool,
    on_batch: &mut dyn FnMut(usize),
) -> AppResult<usize> {
    let model = client.embed_model();
    let cursor = db.ai_cursor(account_id, chat_id, model)?;
    let messages = db.ai_text_messages(account_id, chat_id, &DateRange::default(), cursor)?;
    let Some(last_local_id) = messages.last().map(|message| message.local_id) else {
        return Ok(0);
    };
    let chunks = chunk_messages(&messages);
    let mut stored = 0;
    for batch in chunks.chunks(EMBED_BATCH) {
        if cancel.load(Ordering::SeqCst) {
            return Ok(stored);
        }
        let inputs: Vec<String> = batch
            .iter()
            .map(|chunk| embedding_input(chat_name, chunk))
            .collect();
        let vectors = client.embed(&inputs)?;
        let rows: Vec<(ChunkDraft, Vec<f32>)> = batch.iter().cloned().zip(vectors).collect();
        let batch_cursor = batch
            .last()
            .map(|chunk| chunk.end_local_id)
            .unwrap_or(last_local_id);
        db.ai_store_chunks(account_id, chat_id, model, &rows, batch_cursor)?;
        stored += batch.len();
        on_batch(batch.len());
    }
    db.ai_store_chunks(account_id, chat_id, model, &[], last_local_id)?;
    Ok(stored)
}

pub fn build_index(
    db: &Database,
    client: &dyn LlmClient,
    account_id: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(IndexProgress),
) -> AppResult<IndexOutcome> {
    let chats = db.ai_chats_to_index(account_id, client.embed_model())?;
    let total = chats.len();
    let mut chunks = 0usize;
    for (position, (chat_id, chat_name, _)) in chats.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            return Ok(IndexOutcome {
                chats: position,
                chunks,
                cancelled: true,
            });
        }
        progress(IndexProgress {
            running: true,
            phase: "indexing".into(),
            current: position,
            total,
            chunks,
            message: format!("正在建立索引：{chat_name}（{}/{total}）", position + 1),
        });
        let mut on_batch = |count: usize| {
            chunks += count;
            progress(IndexProgress {
                running: true,
                phase: "indexing".into(),
                current: position,
                total,
                chunks,
                message: format!("正在建立索引：{chat_name}（{}/{total}）", position + 1),
            });
        };
        index_chat(
            db,
            client,
            account_id,
            chat_id,
            chat_name,
            cancel,
            &mut on_batch,
        )?;
    }
    Ok(IndexOutcome {
        chats: total,
        chunks,
        cancelled: cancel.load(Ordering::SeqCst),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::client::fake::FakeClient;
    use crate::models::NormalizedMessage;
    use tempfile::tempdir;

    pub fn text(local_id: i64, minute: i64, from_me: bool, body: &str) -> TextMessage {
        let hour = 9 + minute / 60;
        TextMessage {
            local_id,
            occurred_at: format!("2026-08-20T{hour:02}:{:02}:00+08:00", minute % 60),
            local_date: "2026-08-20".into(),
            sender_name: if from_me {
                "我".into()
            } else {
                "小艾".into()
            },
            is_from_me: from_me,
            text: body.into(),
        }
    }

    #[test]
    fn splits_on_gaps_counts_and_length() {
        let mut messages = vec![
            text(1, 0, false, "周末去公园吗"),
            text(2, 1, true, "好呀，几点"),
            text(3, 50, false, "改成下午三点"),
        ];
        let chunks = chunk_messages(&messages);
        assert_eq!(chunks.len(), 2);
        assert_eq!((chunks[0].start_local_id, chunks[0].end_local_id), (1, 2));
        assert_eq!(chunks[0].text, "小艾: 周末去公园吗\n我: 好呀，几点");

        messages = (0..30)
            .map(|id| text(id, 0, id % 2 == 0, "收到收到"))
            .collect();
        assert_eq!(chunk_messages(&messages).len(), 3);

        let long = "很长的消息".repeat(60);
        messages = vec![text(1, 0, false, &long), text(2, 0, false, &long)];
        assert_eq!(chunk_messages(&messages).len(), 2);
    }

    #[test]
    fn drops_chunks_without_real_content() {
        let messages = vec![
            text(1, 0, false, "嗯"),
            text(2, 90, false, "明天见面聊方案"),
        ];
        let chunks = chunk_messages(&messages);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_local_id, 2);
    }

    #[test]
    fn indexes_incrementally() {
        let directory = tempdir().unwrap().keep();
        let db = Database::new(directory.join("test.db"));
        db.initialize().unwrap();
        db.upsert_account("account", "测试", "fixture").unwrap();
        db.upsert_chat("account", "alice", "小艾", "direct")
            .unwrap();
        let insert = |local_id: i64, hour: u32, body: &str| {
            db.insert_message(
                "account",
                &NormalizedMessage {
                    chat_id: "alice".into(),
                    local_id,
                    server_id: None,
                    occurred_at: format!("2026-08-20T{hour:02}:00:00+08:00"),
                    local_date: "2026-08-20".into(),
                    local_hour: hour,
                    local_weekday: 3,
                    sender_id: "alice".into(),
                    sender_name: "小艾".into(),
                    is_from_me: false,
                    kind: "text".into(),
                    text: body.into(),
                },
            )
            .unwrap();
        };
        insert(1, 9, "周末一起去公园拍照");
        insert(2, 12, "下周去北京出差");
        let client = FakeClient::new(&[]);
        let cancel = AtomicBool::new(false);
        let outcome = build_index(&db, &client, "account", &cancel, &mut |_| {}).unwrap();
        assert_eq!((outcome.chats, outcome.chunks), (1, 2));
        let counts = db.ai_index_counts("account", None, "fake-embed").unwrap();
        assert_eq!((counts.indexed_chunks, counts.pending_messages), (2, 0));

        insert(3, 18, "晚上吃火锅");
        assert_eq!(
            db.ai_index_counts("account", None, "fake-embed")
                .unwrap()
                .pending_messages,
            1
        );
        let outcome = build_index(&db, &client, "account", &cancel, &mut |_| {}).unwrap();
        assert_eq!(outcome.chunks, 1);
        assert_eq!(
            db.ai_index_counts("account", None, "fake-embed")
                .unwrap()
                .indexed_chunks,
            3
        );

        let outcome = build_index(&db, &client, "account", &cancel, &mut |_| {}).unwrap();
        assert_eq!((outcome.chats, outcome.chunks), (0, 0));
    }
}
