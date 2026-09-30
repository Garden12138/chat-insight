use crate::analysis::top_keywords;
use crate::error::AppResult;
use crate::models::{
    AccountSummary, ChatDetailData, ChatListItem, DashboardData, DateRange, HeatmapPoint,
    HourlyPoint, MessageItem, NormalizedMessage, RankedChat, SummaryMetrics, TrendPoint, TypeCount,
};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub(crate) fn connection(&self) -> AppResult<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(8))?;
        Ok(connection)
    }

    pub fn initialize(&self) -> AppResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = self.connection()?;
        connection.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA user_version = 1;
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                display_name TEXT NOT NULL,
                source_platform TEXT NOT NULL,
                created_at TEXT NOT NULL,
                last_sync_at TEXT
            );
            CREATE TABLE IF NOT EXISTS chats (
                account_id TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                display_name TEXT NOT NULL,
                chat_kind TEXT NOT NULL,
                last_message_at TEXT,
                PRIMARY KEY (account_id, chat_id),
                FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS messages (
                account_id TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                local_id INTEGER NOT NULL,
                server_id TEXT,
                occurred_at TEXT NOT NULL,
                local_date TEXT NOT NULL,
                local_hour INTEGER NOT NULL,
                local_weekday INTEGER NOT NULL,
                sender_id TEXT NOT NULL DEFAULT '',
                sender_name TEXT NOT NULL DEFAULT '',
                is_from_me INTEGER NOT NULL,
                kind TEXT NOT NULL,
                text TEXT NOT NULL DEFAULT '',
                PRIMARY KEY (account_id, chat_id, local_id),
                FOREIGN KEY (account_id, chat_id) REFERENCES chats(account_id, chat_id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_messages_account_date ON messages(account_id, local_date);
            CREATE INDEX IF NOT EXISTS idx_messages_chat_time ON messages(account_id, chat_id, occurred_at DESC);
            CREATE UNIQUE INDEX IF NOT EXISTS uq_messages_server_id ON messages(account_id, chat_id, server_id) WHERE server_id IS NOT NULL AND server_id <> '';
            CREATE TABLE IF NOT EXISTS sync_cursors (
                account_id TEXT NOT NULL,
                chat_id TEXT NOT NULL,
                last_local_id INTEGER NOT NULL DEFAULT 0,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, chat_id),
                FOREIGN KEY (account_id, chat_id) REFERENCES chats(account_id, chat_id) ON DELETE CASCADE
            );
            CREATE TABLE IF NOT EXISTS sync_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                status TEXT NOT NULL,
                chats_processed INTEGER NOT NULL DEFAULT 0,
                messages_imported INTEGER NOT NULL DEFAULT 0,
                warning_count INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )?;
        Ok(())
    }

    pub fn upsert_account(
        &self,
        id: &str,
        display_name: &str,
        source_platform: &str,
    ) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO accounts(id, display_name, source_platform, created_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name, source_platform=excluded.source_platform",
            params![id, display_name, source_platform, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn upsert_chat(
        &self,
        account_id: &str,
        chat_id: &str,
        display_name: &str,
        chat_kind: &str,
    ) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO chats(account_id, chat_id, display_name, chat_kind) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, chat_id) DO UPDATE SET display_name=excluded.display_name, chat_kind=excluded.chat_kind",
            params![account_id, chat_id, display_name, chat_kind],
        )?;
        Ok(())
    }

    pub fn insert_message(&self, account_id: &str, message: &NormalizedMessage) -> AppResult<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "INSERT OR IGNORE INTO messages(
                account_id, chat_id, local_id, server_id, occurred_at, local_date, local_hour,
                local_weekday, sender_id, sender_name, is_from_me, kind, text
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                account_id,
                message.chat_id,
                message.local_id,
                message.server_id,
                message.occurred_at,
                message.local_date,
                message.local_hour,
                message.local_weekday,
                message.sender_id,
                message.sender_name,
                message.is_from_me as i64,
                message.kind,
                message.text,
            ],
        )?;
        if changed > 0 {
            connection.execute(
                "UPDATE chats SET last_message_at = CASE WHEN last_message_at IS NULL OR last_message_at < ?3 THEN ?3 ELSE last_message_at END WHERE account_id=?1 AND chat_id=?2",
                params![account_id, message.chat_id, message.occurred_at],
            )?;
        }
        Ok(changed > 0)
    }

    pub fn max_local_id(&self, account_id: &str, chat_id: &str) -> AppResult<Option<i64>> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            "SELECT MAX(local_id) FROM messages WHERE account_id=?1 AND chat_id=?2",
            params![account_id, chat_id],
            |row| row.get(0),
        )?)
    }

    pub fn save_cursor(
        &self,
        account_id: &str,
        chat_id: &str,
        last_local_id: i64,
    ) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO sync_cursors(account_id, chat_id, last_local_id, updated_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, chat_id) DO UPDATE SET last_local_id=MAX(last_local_id, excluded.last_local_id), updated_at=excluded.updated_at",
            params![account_id, chat_id, last_local_id, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn start_sync_run(&self, account_id: &str) -> AppResult<i64> {
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO sync_runs(account_id, started_at, status) VALUES(?1, ?2, 'running')",
            params![account_id, Utc::now().to_rfc3339()],
        )?;
        Ok(connection.last_insert_rowid())
    }

    pub fn finish_sync_run(
        &self,
        run_id: i64,
        status: &str,
        chats: usize,
        messages: usize,
        warnings: usize,
    ) -> AppResult<()> {
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
        connection.execute(
            "UPDATE sync_runs SET finished_at=?2, status=?3, chats_processed=?4, messages_imported=?5, warning_count=?6 WHERE id=?1",
            params![run_id, now, status, chats as i64, messages as i64, warnings as i64],
        )?;
        connection.execute(
            "UPDATE accounts SET last_sync_at=?2 WHERE id=(SELECT account_id FROM sync_runs WHERE id=?1)",
            params![run_id, now],
        )?;
        Ok(())
    }

    pub fn list_accounts(&self) -> AppResult<Vec<AccountSummary>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT a.id, a.display_name, a.source_platform,
                    (SELECT COUNT(*) FROM messages m WHERE m.account_id=a.id),
                    (SELECT COUNT(*) FROM chats c WHERE c.account_id=a.id),
                    a.last_sync_at
             FROM accounts a ORDER BY COALESCE(a.last_sync_at, a.created_at) DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(AccountSummary {
                id: row.get(0)?,
                display_name: row.get(1)?,
                source_platform: row.get(2)?,
                message_count: row.get(3)?,
                chat_count: row.get(4)?,
                last_sync_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_chats(&self, account_id: &str, search: &str) -> AppResult<Vec<ChatListItem>> {
        let connection = self.connection()?;
        let pattern = format!("%{}%", search.trim());
        let mut statement = connection.prepare(
            "SELECT c.chat_id, c.display_name, c.chat_kind, COUNT(m.local_id), MAX(m.occurred_at)
             FROM chats c LEFT JOIN messages m ON m.account_id=c.account_id AND m.chat_id=c.chat_id
             WHERE c.account_id=?1 AND c.display_name LIKE ?2
             GROUP BY c.chat_id, c.display_name, c.chat_kind
             ORDER BY MAX(m.occurred_at) DESC, c.display_name",
        )?;
        let rows = statement.query_map(params![account_id, pattern], |row| {
            Ok(ChatListItem {
                id: row.get(0)?,
                display_name: row.get(1)?,
                chat_kind: row.get(2)?,
                message_count: row.get(3)?,
                last_message_at: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn dashboard(&self, account_id: &str, range: &DateRange) -> AppResult<DashboardData> {
        let connection = self.connection()?;
        let from = range.from.as_deref();
        let to = range.to.as_deref();
        let filter =
            "account_id=?1 AND (?2 IS NULL OR local_date>=?2) AND (?3 IS NULL OR local_date<=?3)";
        let metrics = connection.query_row(
            &format!("SELECT COUNT(*), COUNT(DISTINCT local_date), COUNT(DISTINCT chat_id), COALESCE(AVG(is_from_me), 0.0) FROM messages WHERE {filter}"),
            params![account_id, from, to],
            |row| Ok(SummaryMetrics { total_messages: row.get(0)?, active_days: row.get(1)?, chat_count: row.get(2)?, sent_ratio: row.get(3)? }),
        )?;

        let daily_trend = query_trend(&connection, &format!("SELECT local_date, COUNT(*) FROM messages WHERE {filter} GROUP BY local_date ORDER BY local_date"), account_id, from, to)?;
        let monthly_trend = query_trend(&connection, &format!("SELECT substr(local_date,1,7), COUNT(*) FROM messages WHERE {filter} GROUP BY substr(local_date,1,7) ORDER BY substr(local_date,1,7)"), account_id, from, to)?;

        let mut heatmap_statement = connection.prepare(&format!("SELECT local_weekday, local_hour, COUNT(*) FROM messages WHERE {filter} GROUP BY local_weekday, local_hour ORDER BY local_weekday, local_hour"))?;
        let heatmap = heatmap_statement
            .query_map(params![account_id, from, to], |row| {
                Ok(HeatmapPoint {
                    weekday: row.get(0)?,
                    hour: row.get(1)?,
                    count: row.get(2)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut top_statement = connection.prepare(
            "SELECT m.chat_id, c.display_name, c.chat_kind, COUNT(*) AS total
             FROM messages m JOIN chats c ON c.account_id=m.account_id AND c.chat_id=m.chat_id
             WHERE m.account_id=?1 AND (?2 IS NULL OR m.local_date>=?2) AND (?3 IS NULL OR m.local_date<=?3)
             GROUP BY m.chat_id, c.display_name, c.chat_kind ORDER BY total DESC LIMIT 10",
        )?;
        let top_chats = top_statement
            .query_map(params![account_id, from, to], |row| {
                Ok(RankedChat {
                    chat_id: row.get(0)?,
                    display_name: row.get(1)?,
                    chat_kind: row.get(2)?,
                    count: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut type_statement = connection.prepare(&format!("SELECT kind, COUNT(*) AS total FROM messages WHERE {filter} GROUP BY kind ORDER BY total DESC"))?;
        let message_types = type_statement
            .query_map(params![account_id, from, to], |row| {
                Ok(TypeCount {
                    kind: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(DashboardData {
            metrics,
            daily_trend,
            monthly_trend,
            heatmap,
            top_chats,
            message_types,
        })
    }

    pub fn chat_detail(
        &self,
        account_id: &str,
        chat_id: &str,
        range: &DateRange,
        offset: i64,
        limit: i64,
    ) -> AppResult<ChatDetailData> {
        let connection = self.connection()?;
        let chat = connection.query_row(
            "SELECT c.chat_id, c.display_name, c.chat_kind,
                    (SELECT COUNT(*) FROM messages m WHERE m.account_id=c.account_id AND m.chat_id=c.chat_id), c.last_message_at
             FROM chats c WHERE c.account_id=?1 AND c.chat_id=?2",
            params![account_id, chat_id],
            |row| Ok(ChatListItem { id: row.get(0)?, display_name: row.get(1)?, chat_kind: row.get(2)?, message_count: row.get(3)?, last_message_at: row.get(4)? }),
        )?;
        let from = range.from.as_deref();
        let to = range.to.as_deref();
        let stats = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(is_from_me),0), MIN(occurred_at), MAX(occurred_at)
             FROM messages WHERE account_id=?1 AND chat_id=?2 AND (?3 IS NULL OR local_date>=?3) AND (?4 IS NULL OR local_date<=?4)",
            params![account_id, chat_id, from, to],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?)),
        )?;

        let mut trend_statement = connection.prepare(
            "SELECT local_date, COUNT(*) FROM messages WHERE account_id=?1 AND chat_id=?2 AND (?3 IS NULL OR local_date>=?3) AND (?4 IS NULL OR local_date<=?4) GROUP BY local_date ORDER BY local_date",
        )?;
        let daily_trend = trend_statement
            .query_map(params![account_id, chat_id, from, to], |row| {
                Ok(TrendPoint {
                    bucket: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut hour_statement = connection.prepare(
            "SELECT local_hour, COUNT(*) FROM messages WHERE account_id=?1 AND chat_id=?2 AND (?3 IS NULL OR local_date>=?3) AND (?4 IS NULL OR local_date<=?4) GROUP BY local_hour ORDER BY local_hour",
        )?;
        let hourly_activity = hour_statement
            .query_map(params![account_id, chat_id, from, to], |row| {
                Ok(HourlyPoint {
                    hour: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut text_statement = connection.prepare(
            "SELECT text FROM messages WHERE account_id=?1 AND chat_id=?2 AND kind='text' AND text<>'' AND (?3 IS NULL OR local_date>=?3) AND (?4 IS NULL OR local_date<=?4)",
        )?;
        let texts = text_statement
            .query_map(params![account_id, chat_id, from, to], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let keywords = top_keywords(texts, 20);

        let safe_limit = limit.clamp(1, 100);
        let safe_offset = offset.max(0);
        let mut message_statement = connection.prepare(
            "SELECT local_id, server_id, occurred_at, sender_name, is_from_me, kind, text
             FROM messages WHERE account_id=?1 AND chat_id=?2 AND (?3 IS NULL OR local_date>=?3) AND (?4 IS NULL OR local_date<=?4)
             ORDER BY occurred_at DESC, local_id DESC LIMIT ?5 OFFSET ?6",
        )?;
        let messages = message_statement
            .query_map(
                params![account_id, chat_id, from, to, safe_limit, safe_offset],
                |row| {
                    Ok(MessageItem {
                        local_id: row.get(0)?,
                        server_id: row.get(1)?,
                        occurred_at: row.get(2)?,
                        sender_name: row.get(3)?,
                        is_from_me: row.get::<_, i64>(4)? != 0,
                        kind: row.get(5)?,
                        text: row.get(6)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let next_offset = (messages.len() as i64 == safe_limit).then_some(safe_offset + safe_limit);

        Ok(ChatDetailData {
            chat,
            total_messages: stats.0,
            sent_messages: stats.1,
            received_messages: stats.0 - stats.1,
            first_message_at: stats.2,
            last_message_at: stats.3,
            daily_trend,
            hourly_activity,
            keywords,
            messages,
            next_offset,
        })
    }

    pub fn clear_account(&self, account_id: &str) -> AppResult<()> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM accounts WHERE id=?1", params![account_id])?;
        Ok(())
    }

    pub fn adopt_legacy_account(
        &self,
        legacy_id: &str,
        account_id: &str,
        display_name: &str,
    ) -> AppResult<bool> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists = |id: &str| -> rusqlite::Result<bool> {
            transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM accounts WHERE id=?1)",
                params![id],
                |row| row.get(0),
            )
        };
        if !exists(legacy_id)? || exists(account_id)? {
            return Ok(false);
        }
        // Child keys are rewritten after the parent, so the checks must wait for commit.
        transaction.pragma_update(None, "defer_foreign_keys", "ON")?;
        transaction.execute(
            "UPDATE accounts SET id=?2, display_name=?3 WHERE id=?1",
            params![legacy_id, account_id, display_name],
        )?;
        for table in ["chats", "messages", "sync_cursors", "sync_runs"] {
            transaction.execute(
                &format!("UPDATE {table} SET account_id=?2 WHERE account_id=?1"),
                params![legacy_id, account_id],
            )?;
        }
        transaction.commit()?;
        Ok(true)
    }
}

fn query_trend(
    connection: &Connection,
    sql: &str,
    account_id: &str,
    from: Option<&str>,
    to: Option<&str>,
) -> AppResult<Vec<TrendPoint>> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(params![account_id, from, to], |row| {
        Ok(TrendPoint {
            bucket: row.get(0)?,
            count: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn fixture_db() -> Database {
        let directory = tempdir().unwrap().keep();
        let db = Database::new(directory.join("test.db"));
        db.initialize().unwrap();
        db.upsert_account("account", "测试账号", "fixture").unwrap();
        db.upsert_chat("account", "alice", "小艾", "direct")
            .unwrap();
        db
    }

    fn message(local_id: i64, date: &str, hour: u32, sent: bool) -> NormalizedMessage {
        NormalizedMessage {
            chat_id: "alice".into(),
            local_id,
            server_id: Some(format!("s{local_id}")),
            occurred_at: format!("{date}T{hour:02}:00:00+08:00"),
            local_date: date.into(),
            local_hour: hour,
            local_weekday: 0,
            sender_id: if sent { "me" } else { "alice" }.into(),
            sender_name: if sent { "我" } else { "小艾" }.into(),
            is_from_me: sent,
            kind: "text".into(),
            text: "周末一起去公园拍照".into(),
        }
    }

    #[test]
    fn inserts_idempotently_and_computes_dashboard() {
        let db = fixture_db();
        assert!(db
            .insert_message("account", &message(1, "2026-08-20", 9, true))
            .unwrap());
        assert!(!db
            .insert_message("account", &message(1, "2026-08-20", 9, true))
            .unwrap());
        assert!(db
            .insert_message("account", &message(2, "2026-08-21", 18, false))
            .unwrap());
        let dashboard = db.dashboard("account", &DateRange::default()).unwrap();
        assert_eq!(dashboard.metrics.total_messages, 2);
        assert_eq!(dashboard.metrics.active_days, 2);
        assert_eq!(dashboard.metrics.sent_ratio, 0.5);
        assert_eq!(dashboard.heatmap.len(), 2);
    }

    #[test]
    fn date_range_filters_detail() {
        let db = fixture_db();
        db.insert_message("account", &message(1, "2025-01-01", 9, true))
            .unwrap();
        db.insert_message("account", &message(2, "2026-08-21", 18, false))
            .unwrap();
        let detail = db
            .chat_detail(
                "account",
                "alice",
                &DateRange {
                    from: Some("2026-01-01".into()),
                    to: None,
                },
                0,
                60,
            )
            .unwrap();
        assert_eq!(detail.total_messages, 1);
        assert_eq!(detail.messages[0].local_id, 2);
    }

    #[test]
    fn adopts_legacy_dataset_once_without_touching_other_accounts() {
        let db = fixture_db();
        db.upsert_account("legacy", "wechat-cli", "macos").unwrap();
        db.upsert_chat("legacy", "alice", "小艾", "direct").unwrap();
        db.insert_message("legacy", &message(7, "2026-08-20", 9, true))
            .unwrap();
        db.save_cursor("legacy", "alice", 7).unwrap();
        let run = db.start_sync_run("legacy").unwrap();
        db.finish_sync_run(run, "complete", 1, 1, 0).unwrap();

        assert!(db
            .adopt_legacy_account("legacy", "wechat:wxid_a", "wxid_a")
            .unwrap());
        assert!(!db
            .adopt_legacy_account("legacy", "wechat:wxid_a", "wxid_a")
            .unwrap());

        let accounts = db.list_accounts().unwrap();
        let adopted = accounts.iter().find(|a| a.id == "wechat:wxid_a").unwrap();
        assert_eq!(adopted.display_name, "wxid_a");
        assert_eq!(adopted.message_count, 1);
        assert!(adopted.last_sync_at.is_some());
        assert!(accounts.iter().all(|a| a.id != "legacy"));
        assert_eq!(db.max_local_id("wechat:wxid_a", "alice").unwrap(), Some(7));
        assert_eq!(db.max_local_id("account", "alice").unwrap(), None);
    }

    #[test]
    fn keeps_legacy_dataset_when_target_account_already_exists() {
        let db = fixture_db();
        db.upsert_account("legacy", "wechat-cli", "macos").unwrap();
        assert!(!db
            .adopt_legacy_account("legacy", "account", "测试账号")
            .unwrap());
        assert_eq!(db.list_accounts().unwrap().len(), 2);
    }
}
