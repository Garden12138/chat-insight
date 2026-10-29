use super::indexer::ChunkDraft;
use crate::database::Database;
use crate::error::AppResult;
use crate::models::DateRange;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct TextMessage {
    pub local_id: i64,
    pub occurred_at: String,
    pub local_date: String,
    pub sender_name: String,
    pub is_from_me: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredChunk {
    pub id: i64,
    pub chat_id: String,
    pub chat_name: String,
    pub chat_kind: String,
    pub start_local_id: i64,
    pub end_local_id: i64,
    pub start_at: String,
    pub end_at: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexCounts {
    pub indexed_chunks: i64,
    pub pending_messages: i64,
    pub total_text_messages: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentEstimate {
    pub messages: i64,
    pub chars: i64,
}

#[derive(Debug, Clone)]
pub struct EventTextMessage {
    pub chat_id: String,
    pub chat_name: String,
    pub chat_kind: String,
    pub local_id: i64,
    pub occurred_at: String,
    pub local_date: String,
    pub sender_name: String,
    pub is_from_me: bool,
    pub text: String,
}

const EVENT_FILTER: &str = "m.account_id=?1 AND m.local_date=?2 AND c.chat_kind IN ('group','direct') AND m.kind='text' AND m.text<>'' AND (?3 IS NULL OR m.chat_id=?3)";

pub fn encode_vector(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

pub struct CacheKey<'a> {
    pub account_id: &'a str,
    pub kind: &'a str,
    pub scope: &'a str,
    pub range: &'a DateRange,
    pub model: &'a str,
}

const TEXT_FILTER: &str = "m.account_id=?1 AND (?2 IS NULL OR m.chat_id=?2) AND (?3 IS NULL OR m.local_date>=?3) AND (?4 IS NULL OR m.local_date<=?4) AND m.kind='text' AND m.text<>''";

impl Database {
    pub fn ai_chat_name(&self, account_id: &str, chat_id: &str) -> AppResult<String> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            "SELECT display_name FROM chats WHERE account_id=?1 AND chat_id=?2",
            params![account_id, chat_id],
            |row| row.get(0),
        )?)
    }

    pub fn ai_text_messages(
        &self,
        account_id: &str,
        chat_id: &str,
        range: &DateRange,
        after_local_id: Option<i64>,
    ) -> AppResult<Vec<TextMessage>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT m.local_id, m.occurred_at, m.local_date, m.sender_name, m.is_from_me, m.text FROM messages m
             WHERE {TEXT_FILTER} AND (?5 IS NULL OR m.local_id>?5) ORDER BY m.local_id"
        ))?;
        let rows = statement.query_map(
            params![account_id, chat_id, range.from, range.to, after_local_id],
            |row| {
                Ok(TextMessage {
                    local_id: row.get(0)?,
                    occurred_at: row.get(1)?,
                    local_date: row.get(2)?,
                    sender_name: row.get(3)?,
                    is_from_me: row.get::<_, i64>(4)? != 0,
                    text: row.get(5)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Changes whenever text messages in scope are added or removed; used to invalidate cached results.
    pub fn ai_fingerprint(
        &self,
        account_id: &str,
        chat_id: Option<&str>,
        range: &DateRange,
    ) -> AppResult<String> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            &format!("SELECT COUNT(*) || ':' || COALESCE(MAX(m.rowid), 0) FROM messages m WHERE {TEXT_FILTER}"),
            params![account_id, chat_id, range.from, range.to],
            |row| row.get(0),
        )?)
    }

    pub fn ai_estimate(
        &self,
        account_id: &str,
        chat_id: Option<&str>,
        range: &DateRange,
    ) -> AppResult<ContentEstimate> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(length(m.text)), 0) FROM messages m WHERE {TEXT_FILTER}"),
            params![account_id, chat_id, range.from, range.to],
            |row| Ok(ContentEstimate { messages: row.get(0)?, chars: row.get(1)? }),
        )?)
    }

    pub fn ai_chats_to_index(
        &self,
        account_id: &str,
        model: &str,
    ) -> AppResult<Vec<(String, String, Option<i64>)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT c.chat_id, c.display_name, cur.last_local_id FROM chats c
             LEFT JOIN ai_index_cursors cur ON cur.account_id=c.account_id AND cur.chat_id=c.chat_id AND cur.model=?2
             WHERE c.account_id=?1 AND EXISTS (
                SELECT 1 FROM messages m WHERE m.account_id=c.account_id AND m.chat_id=c.chat_id
                  AND m.kind='text' AND m.text<>'' AND m.local_id > COALESCE(cur.last_local_id, -9223372036854775807)
             )
             ORDER BY c.last_message_at DESC",
        )?;
        let rows = statement.query_map(params![account_id, model], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn ai_cursor(
        &self,
        account_id: &str,
        chat_id: &str,
        model: &str,
    ) -> AppResult<Option<i64>> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT last_local_id FROM ai_index_cursors WHERE account_id=?1 AND chat_id=?2 AND model=?3",
                params![account_id, chat_id, model],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Stores embedded chunks and advances the chat cursor in one transaction.
    pub fn ai_store_chunks(
        &self,
        account_id: &str,
        chat_id: &str,
        model: &str,
        chunks: &[(ChunkDraft, Vec<f32>)],
        cursor: i64,
    ) -> AppResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO ai_chunks(account_id, chat_id, model, start_local_id, end_local_id, start_at, end_at, local_date, message_count, text, embedding)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for (chunk, vector) in chunks {
                insert.execute(params![
                    account_id,
                    chat_id,
                    model,
                    chunk.start_local_id,
                    chunk.end_local_id,
                    chunk.start_at,
                    chunk.end_at,
                    chunk.local_date,
                    chunk.message_count as i64,
                    chunk.text,
                    encode_vector(vector),
                ])?;
            }
        }
        transaction.execute(
            "INSERT INTO ai_index_cursors(account_id, chat_id, model, last_local_id) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, chat_id, model) DO UPDATE SET last_local_id=MAX(last_local_id, excluded.last_local_id)",
            params![account_id, chat_id, model, cursor],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn ai_clear_index(&self, account_id: &str, model: &str) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM ai_chunks WHERE account_id=?1 AND model=?2",
            params![account_id, model],
        )?;
        connection.execute(
            "DELETE FROM ai_index_cursors WHERE account_id=?1 AND model=?2",
            params![account_id, model],
        )?;
        Ok(())
    }

    pub fn ai_index_counts(
        &self,
        account_id: &str,
        chat_id: Option<&str>,
        model: &str,
    ) -> AppResult<IndexCounts> {
        let connection = self.connection()?;
        let indexed_chunks = connection.query_row(
            "SELECT COUNT(*) FROM ai_chunks WHERE account_id=?1 AND model=?2 AND (?3 IS NULL OR chat_id=?3)",
            params![account_id, model, chat_id],
            |row| row.get(0),
        )?;
        let (total_text_messages, pending_messages) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(m.local_id > COALESCE(cur.last_local_id, -9223372036854775807)), 0)
             FROM messages m LEFT JOIN ai_index_cursors cur
               ON cur.account_id=m.account_id AND cur.chat_id=m.chat_id AND cur.model=?2
             WHERE m.account_id=?1 AND (?3 IS NULL OR m.chat_id=?3) AND m.kind='text' AND m.text<>''",
            params![account_id, model, chat_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        Ok(IndexCounts {
            indexed_chunks,
            pending_messages,
            total_text_messages,
        })
    }

    /// Streams `(chunk id, vector)` pairs in scope without loading them all at once.
    pub fn ai_for_each_vector(
        &self,
        account_id: &str,
        model: &str,
        chat_id: Option<&str>,
        range: &DateRange,
        mut visit: impl FnMut(i64, Vec<f32>),
    ) -> AppResult<()> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, embedding FROM ai_chunks WHERE account_id=?1 AND model=?2 AND (?3 IS NULL OR chat_id=?3)
               AND (?4 IS NULL OR local_date>=?4) AND (?5 IS NULL OR local_date<=?5)",
        )?;
        let mut rows =
            statement.query(params![account_id, model, chat_id, range.from, range.to])?;
        while let Some(row) = rows.next()? {
            let bytes: Vec<u8> = row.get(1)?;
            visit(row.get(0)?, decode_vector(&bytes));
        }
        Ok(())
    }

    pub fn ai_chunks_by_ids(&self, ids: &[i64]) -> AppResult<Vec<StoredChunk>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT k.id, k.chat_id, c.display_name, c.chat_kind, k.start_local_id, k.end_local_id, k.start_at, k.end_at, k.text
             FROM ai_chunks k JOIN chats c ON c.account_id=k.account_id AND c.chat_id=k.chat_id WHERE k.id=?1",
        )?;
        let mut chunks = Vec::with_capacity(ids.len());
        for id in ids {
            let chunk = statement
                .query_row(params![id], |row| {
                    Ok(StoredChunk {
                        id: row.get(0)?,
                        chat_id: row.get(1)?,
                        chat_name: row.get(2)?,
                        chat_kind: row.get(3)?,
                        start_local_id: row.get(4)?,
                        end_local_id: row.get(5)?,
                        start_at: row.get(6)?,
                        end_at: row.get(7)?,
                        text: row.get(8)?,
                    })
                })
                .optional()?;
            chunks.extend(chunk);
        }
        Ok(chunks)
    }

    pub fn ai_chunk_containing(
        &self,
        account_id: &str,
        model: &str,
        chat_id: &str,
        local_id: i64,
    ) -> AppResult<Option<i64>> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT id FROM ai_chunks WHERE account_id=?1 AND model=?2 AND chat_id=?3 AND start_local_id<=?4 AND end_local_id>=?4 LIMIT 1",
                params![account_id, model, chat_id, local_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn ai_group_event_messages(
        &self,
        account_id: &str,
        date: &str,
        chat_id: Option<&str>,
    ) -> AppResult<Vec<EventTextMessage>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT m.chat_id, c.display_name, c.chat_kind, m.local_id, m.occurred_at, m.local_date,
                    m.sender_name, m.is_from_me, m.text
             FROM messages m
             JOIN chats c ON c.account_id=m.account_id AND c.chat_id=m.chat_id
             WHERE {EVENT_FILTER}
             ORDER BY m.occurred_at, m.chat_id, m.local_id"
        ))?;
        let rows = statement.query_map(params![account_id, date, chat_id], |row| {
            Ok(EventTextMessage {
                chat_id: row.get(0)?,
                chat_name: row.get(1)?,
                chat_kind: row.get(2)?,
                local_id: row.get(3)?,
                occurred_at: row.get(4)?,
                local_date: row.get(5)?,
                sender_name: row.get(6)?,
                is_from_me: row.get::<_, i64>(7)? != 0,
                text: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn ai_group_event_fingerprint(
        &self,
        account_id: &str,
        date: &str,
        chat_id: Option<&str>,
    ) -> AppResult<String> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            &format!(
                "SELECT COUNT(*) || ':' || COALESCE(MAX(m.rowid), 0)
                 FROM messages m
                 JOIN chats c ON c.account_id=m.account_id AND c.chat_id=m.chat_id
                 WHERE {EVENT_FILTER}"
            ),
            params![account_id, date, chat_id],
            |row| row.get(0),
        )?)
    }

    pub fn ai_cache_get(
        &self,
        key: &CacheKey,
        input_hash: &str,
    ) -> AppResult<Option<(String, String)>> {
        let connection = self.connection()?;
        Ok(connection
            .query_row(
                "SELECT result_json, created_at FROM ai_cache WHERE account_id=?1 AND kind=?2 AND scope=?3
                   AND range_from=?4 AND range_to=?5 AND model=?6 AND input_hash=?7",
                params![
                    key.account_id,
                    key.kind,
                    key.scope,
                    key.range.from.clone().unwrap_or_default(),
                    key.range.to.clone().unwrap_or_default(),
                    key.model,
                    input_hash
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?)
    }

    pub fn ai_cache_put(
        &self,
        key: &CacheKey,
        input_hash: &str,
        result_json: &str,
    ) -> AppResult<String> {
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            "INSERT INTO ai_cache(account_id, kind, scope, range_from, range_to, model, input_hash, result_json, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(account_id, kind, scope, range_from, range_to, model)
             DO UPDATE SET input_hash=excluded.input_hash, result_json=excluded.result_json, created_at=excluded.created_at",
            params![
                key.account_id,
                key.kind,
                key.scope,
                key.range.from.clone().unwrap_or_default(),
                key.range.to.clone().unwrap_or_default(),
                key.model,
                input_hash,
                result_json,
                now
            ],
        )?;
        Ok(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NormalizedMessage;
    use tempfile::tempdir;

    fn fixture_db() -> Database {
        let directory = tempdir().unwrap().keep();
        let db = Database::new(directory.join("test.db"));
        db.initialize().unwrap();
        db.upsert_account("account", "测试", "fixture").unwrap();
        db.upsert_chat("account", "alice", "小艾", "direct")
            .unwrap();
        db.upsert_chat("account", "g1", "产品群", "group").unwrap();
        db.upsert_chat("account", "g2", "运营群", "group").unwrap();
        db
    }

    fn insert(db: &Database, chat_id: &str, local_id: i64, at: &str, kind: &str, text: &str) {
        db.insert_message(
            "account",
            &NormalizedMessage {
                chat_id: chat_id.into(),
                local_id,
                server_id: None,
                occurred_at: at.into(),
                local_date: at[..10].into(),
                local_hour: 9,
                local_weekday: 0,
                sender_id: "other".into(),
                sender_name: "成员".into(),
                is_from_me: false,
                kind: kind.into(),
                text: text.into(),
            },
        )
        .unwrap();
    }

    #[test]
    fn round_trips_vectors() {
        let vector = vec![0.25f32, -1.5, 3.0];
        assert_eq!(decode_vector(&encode_vector(&vector)), vector);
    }

    #[test]
    fn event_query_keeps_same_day_group_and_direct_text() {
        let db = fixture_db();
        insert(
            &db,
            "g1",
            1,
            "2026-10-26T23:50:00+08:00",
            "text",
            "昨天的群消息",
        );
        insert(
            &db,
            "g1",
            2,
            "2026-10-27T09:00:00+08:00",
            "text",
            "当天产品通知",
        );
        insert(
            &db,
            "g1",
            3,
            "2026-10-28T00:10:00+08:00",
            "text",
            "明天的群消息",
        );
        insert(
            &db,
            "alice",
            4,
            "2026-10-27T09:05:00+08:00",
            "text",
            "私聊不要进来",
        );
        insert(
            &db,
            "g2",
            5,
            "2026-10-27T10:00:00+08:00",
            "text",
            "运营群消息",
        );
        insert(&db, "g1", 6, "2026-10-27T11:00:00+08:00", "text", "");
        insert(
            &db,
            "g1",
            7,
            "2026-10-27T12:00:00+08:00",
            "image",
            "图片说明",
        );
        for index in 8..21 {
            insert(
                &db,
                "g1",
                index,
                &format!("2026-10-27T13:{:02}:00+08:00", index),
                "text",
                &format!("超过普通问答上限的第{index}条"),
            );
        }

        let all = db
            .ai_group_event_messages("account", "2026-10-27", None)
            .unwrap();
        assert!(all.iter().all(|message| message.local_date == "2026-10-27"));
        assert!(all
            .iter()
            .all(|message| message.chat_kind == "group" || message.chat_kind == "direct"));
        assert!(all.iter().all(|message| !message.text.is_empty()));
        assert_eq!(
            all.iter()
                .filter(|message| message.chat_id == "alice")
                .count(),
            1
        );
        assert!(all.len() > 12);
        assert_eq!(all.first().unwrap().chat_id, "g1");

        let one = db
            .ai_group_event_messages("account", "2026-10-27", Some("g2"))
            .unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].chat_id, "g2");
        assert_eq!(one[0].text, "运营群消息");

        let direct = db
            .ai_group_event_messages("account", "2026-10-27", Some("alice"))
            .unwrap();
        assert_eq!(direct.len(), 1);
        assert_eq!(direct[0].chat_id, "alice");
        assert_eq!(direct[0].chat_kind, "direct");

        let group_before = db
            .ai_group_event_fingerprint("account", "2026-10-27", Some("g1"))
            .unwrap();
        let all_before = db
            .ai_group_event_fingerprint("account", "2026-10-27", None)
            .unwrap();
        insert(
            &db,
            "alice",
            40,
            "2026-10-27T18:00:00+08:00",
            "text",
            "私聊变化",
        );
        assert_eq!(
            db.ai_group_event_fingerprint("account", "2026-10-27", Some("g1"))
                .unwrap(),
            group_before
        );
        assert_ne!(
            db.ai_group_event_fingerprint("account", "2026-10-27", None)
                .unwrap(),
            all_before
        );
        insert(
            &db,
            "g2",
            41,
            "2026-10-27T18:10:00+08:00",
            "text",
            "群聊新增",
        );
        assert_ne!(
            db.ai_group_event_fingerprint("account", "2026-10-27", None)
                .unwrap(),
            all_before
        );
    }
}
