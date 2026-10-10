//! Storage shared by the memory extractors (commitments, decisions): which messages each
//! extractor has already read, plus small helpers for cleaning model output.

use crate::ai::store::TextMessage;
use crate::database::Database;
use crate::error::AppResult;
use crate::models::DateRange;
use chrono::{NaiveDate, Utc};
use rusqlite::params;

pub const TITLE_CHARS: usize = 60;

/// What the model concluded about an existing item from later messages.
#[derive(Debug, Clone, PartialEq)]
pub struct AiFollowup {
    pub id: i64,
    pub status: String,
    pub note: Option<String>,
    pub followup_local_id: Option<i64>,
}

/// Merges overlapping or touching `local_id` intervals; `touching` decides whether two
/// disjoint neighbours have nothing between them.
pub fn merge_intervals(
    mut intervals: Vec<(i64, i64)>,
    touching: &mut dyn FnMut(i64, i64) -> AppResult<bool>,
) -> AppResult<Vec<(i64, i64)>> {
    intervals.sort();
    let mut merged: Vec<(i64, i64)> = Vec::with_capacity(intervals.len());
    for (from, to) in intervals {
        if let Some(last) = merged.last_mut() {
            if from <= last.1 + 1 || touching(last.1, from)? {
                last.1 = last.1.max(to);
                continue;
            }
        }
        merged.push((from, to));
    }
    Ok(merged)
}

pub fn is_covered(intervals: &[(i64, i64)], local_id: i64) -> bool {
    intervals
        .iter()
        .any(|(from, to)| *from <= local_id && local_id <= *to)
}

pub fn clean_title(title: &str) -> Option<String> {
    clean_text(title, TITLE_CHARS)
}

/// Collapses whitespace and truncates; `None` when nothing is left.
pub fn clean_text(text: &str, limit: usize) -> Option<String> {
    let text: String = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect();
    (!text.is_empty()).then_some(text)
}

pub fn parse_date(value: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

/// Normalizes an optional `YYYY-MM-DD` from the user; an empty value clears the date.
pub fn user_date(value: Option<&str>) -> Result<Option<String>, ()> {
    match value.map(str::trim) {
        None | Some("") => Ok(None),
        Some(value) => parse_date(value)
            .map(|date| Some(date.format("%Y-%m-%d").to_string()))
            .ok_or(()),
    }
}

impl Database {
    /// Chats with text messages in scope, most recently active first.
    pub fn chats_with_text(
        &self,
        account_id: &str,
        chat_id: Option<&str>,
        range: &DateRange,
    ) -> AppResult<Vec<(String, String, String)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT c.chat_id, c.display_name, c.chat_kind FROM chats c
             WHERE c.account_id=?1 AND (?2 IS NULL OR c.chat_id=?2) AND EXISTS (
                SELECT 1 FROM messages m WHERE m.account_id=c.account_id AND m.chat_id=c.chat_id
                  AND m.kind='text' AND m.text<>'' AND (?3 IS NULL OR m.local_date>=?3) AND (?4 IS NULL OR m.local_date<=?4)
             )
             ORDER BY c.last_message_at DESC, c.chat_id",
        )?;
        let rows = statement
            .query_map(params![account_id, chat_id, range.from, range.to], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// The `limit` text messages right before `local_id`, oldest first.
    pub fn text_messages_before(
        &self,
        account_id: &str,
        chat_id: &str,
        local_id: i64,
        limit: i64,
    ) -> AppResult<Vec<TextMessage>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT local_id, occurred_at, local_date, sender_name, is_from_me, text FROM messages
             WHERE account_id=?1 AND chat_id=?2 AND local_id<?3 AND kind='text' AND text<>''
             ORDER BY local_id DESC LIMIT ?4",
        )?;
        let mut rows = statement
            .query_map(params![account_id, chat_id, local_id, limit], |row| {
                Ok(TextMessage {
                    local_id: row.get(0)?,
                    occurred_at: row.get(1)?,
                    local_date: row.get(2)?,
                    sender_name: row.get(3)?,
                    is_from_me: row.get::<_, i64>(4)? != 0,
                    text: row.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn scanned_intervals(
        &self,
        account_id: &str,
        chat_id: &str,
        kind: &str,
    ) -> AppResult<Vec<(i64, i64)>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT from_local_id, to_local_id FROM memory_scans
             WHERE account_id=?1 AND chat_id=?2 AND kind=?3 ORDER BY from_local_id",
        )?;
        let rows = statement.query_map(params![account_id, chat_id, kind], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Records `[from, to]` as read by the `kind` extractor and merges it with
    /// neighbouring intervals that have no text message between them.
    pub fn mark_scanned(
        &self,
        account_id: &str,
        chat_id: &str,
        kind: &str,
        from: i64,
        to: i64,
        model: &str,
    ) -> AppResult<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut intervals: Vec<(i64, i64)> = {
            let mut statement = transaction.prepare(
                "SELECT from_local_id, to_local_id FROM memory_scans WHERE account_id=?1 AND chat_id=?2 AND kind=?3",
            )?;
            let rows = statement.query_map(params![account_id, chat_id, kind], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        intervals.push((from.min(to), from.max(to)));
        let merged = {
            let mut between = transaction.prepare(
                "SELECT NOT EXISTS (SELECT 1 FROM messages WHERE account_id=?1 AND chat_id=?2
                   AND local_id>?3 AND local_id<?4 AND kind='text' AND text<>'')",
            )?;
            merge_intervals(intervals, &mut |end, start| {
                Ok(
                    between.query_row(params![account_id, chat_id, end, start], |row| {
                        row.get::<_, bool>(0)
                    })?,
                )
            })?
        };
        transaction.execute(
            "DELETE FROM memory_scans WHERE account_id=?1 AND chat_id=?2 AND kind=?3",
            params![account_id, chat_id, kind],
        )?;
        let now = Utc::now().to_rfc3339();
        for (from, to) in merged {
            transaction.execute(
                "INSERT INTO memory_scans(account_id, chat_id, kind, from_local_id, to_local_id, model, scanned_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![account_id, chat_id, kind, from, to, model, now],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitments::tests::{fixture_db, insert_text};

    #[test]
    fn merges_overlapping_and_touching_intervals() {
        let merged = merge_intervals(vec![(10, 12), (1, 3), (4, 6), (8, 9)], &mut |end, start| {
            Ok(end == 6 && start == 8)
        })
        .unwrap();
        assert_eq!(merged, vec![(1, 12)]);
        let apart = merge_intervals(vec![(1, 2), (5, 6)], &mut |_, _| Ok(false)).unwrap();
        assert_eq!(apart, vec![(1, 2), (5, 6)]);
        assert!(is_covered(&apart, 2) && !is_covered(&apart, 3));
    }

    #[test]
    fn scanned_intervals_merge_across_gaps_and_stay_per_kind() {
        let db = fixture_db();
        for (id, text) in [(1, "早"), (5, "中午吃什么"), (9, "晚安")] {
            insert_text(&db, "alice", id, "2026-10-12T10:00:00+08:00", false, text);
        }
        db.mark_scanned("account", "alice", "commitment", 1, 1, "m")
            .unwrap();
        db.mark_scanned("account", "alice", "commitment", 9, 9, "m")
            .unwrap();
        db.mark_scanned("account", "alice", "decision", 5, 5, "m")
            .unwrap();
        assert_eq!(
            db.scanned_intervals("account", "alice", "commitment")
                .unwrap(),
            vec![(1, 1), (9, 9)]
        );
        db.mark_scanned("account", "alice", "commitment", 5, 5, "m")
            .unwrap();
        assert_eq!(
            db.scanned_intervals("account", "alice", "commitment")
                .unwrap(),
            vec![(1, 9)]
        );
        assert_eq!(
            db.scanned_intervals("account", "alice", "decision")
                .unwrap(),
            vec![(5, 5)]
        );
    }

    #[test]
    fn cleans_text_and_user_dates() {
        assert_eq!(clean_text("  张  三 ", 20).as_deref(), Some("张 三"));
        assert_eq!(clean_text(" ", 20), None);
        assert_eq!(user_date(Some("")), Ok(None));
        assert_eq!(user_date(Some("2026-10-20")), Ok(Some("2026-10-20".into())));
        assert!(user_date(Some("下周")).is_err());
    }
}
