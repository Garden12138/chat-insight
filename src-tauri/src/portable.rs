use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::models::{ExportResult, ImportResult};
use chrono::{DateTime, Datelike, Timelike, Utc};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

const FORMAT: &str = "chat-insight-jsonl";
const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum PortableRecord {
    Manifest {
        format: String,
        schema_version: u32,
        app_version: String,
        exported_at: String,
        dataset_id: String,
        dataset_name: String,
    },
    Chat {
        chat_id: String,
        display_name: String,
        chat_kind: String,
    },
    Message {
        chat_id: String,
        local_id: i64,
        server_id: Option<String>,
        occurred_at: String,
        sender_id: String,
        sender_name: String,
        is_from_me: bool,
        kind: String,
        text: String,
    },
}

pub fn export_jsonl(database: &Database, account_id: &str, path: &Path) -> AppResult<ExportResult> {
    let connection = database.connection()?;
    let display_name: String = connection
        .query_row(
            "SELECT display_name FROM accounts WHERE id=?1",
            params![account_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::Validation("找不到要导出的数据集".into()))?;
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let dataset_id = account_id
        .strip_prefix("import:")
        .unwrap_or(account_id)
        .to_string();
    write_record(
        &mut writer,
        &PortableRecord::Manifest {
            format: FORMAT.into(),
            schema_version: SCHEMA_VERSION,
            app_version: env!("CARGO_PKG_VERSION").into(),
            exported_at: Utc::now().to_rfc3339(),
            dataset_id,
            dataset_name: display_name,
        },
    )?;

    let mut chat_count = 0usize;
    let mut chat_statement = connection.prepare(
        "SELECT chat_id, display_name, chat_kind FROM chats WHERE account_id=?1 ORDER BY chat_id",
    )?;
    let chats = chat_statement.query_map(params![account_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for chat in chats {
        let (chat_id, display_name, chat_kind) = chat?;
        write_record(
            &mut writer,
            &PortableRecord::Chat {
                chat_id,
                display_name,
                chat_kind,
            },
        )?;
        chat_count += 1;
    }

    let mut message_count = 0usize;
    let mut message_statement = connection.prepare(
        "SELECT chat_id, local_id, server_id, occurred_at, sender_id, sender_name, is_from_me, kind, text
         FROM messages WHERE account_id=?1 ORDER BY chat_id, occurred_at, local_id",
    )?;
    let messages = message_statement.query_map(params![account_id], |row| {
        Ok(PortableRecord::Message {
            chat_id: row.get(0)?,
            local_id: row.get(1)?,
            server_id: row.get(2)?,
            occurred_at: row.get(3)?,
            sender_id: row.get(4)?,
            sender_name: row.get(5)?,
            is_from_me: row.get::<_, i64>(6)? != 0,
            kind: row.get(7)?,
            text: row.get(8)?,
        })
    })?;
    for message in messages {
        write_record(&mut writer, &message?)?;
        message_count += 1;
    }
    writer.flush()?;
    Ok(ExportResult {
        chats_exported: chat_count,
        messages_exported: message_count,
    })
}

pub fn import_jsonl(database: &Database, path: &Path) -> AppResult<ImportResult> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let first_line = lines
        .next()
        .ok_or_else(|| AppError::Validation("导入文件为空".into()))??;
    let manifest: PortableRecord = serde_json::from_str(&first_line)?;
    let (dataset_id, dataset_name) = match manifest {
        PortableRecord::Manifest {
            format,
            schema_version,
            dataset_id,
            dataset_name,
            ..
        } if format == FORMAT && schema_version == SCHEMA_VERSION => (dataset_id, dataset_name),
        PortableRecord::Manifest { .. } => {
            return Err(AppError::Validation(
                "暂不支持这个 Chat Insight 数据版本".into(),
            ))
        }
        _ => {
            return Err(AppError::Validation(
                "首行必须是 Chat Insight manifest".into(),
            ))
        }
    };
    if dataset_id.trim().is_empty() || dataset_id.len() > 256 {
        return Err(AppError::Validation("数据集标识无效".into()));
    }

    let account_id = format!("import:{dataset_id}");
    let mut connection = database.connection()?;
    let transaction = connection.transaction()?;
    transaction.execute(
        "INSERT INTO accounts(id, display_name, source_platform, created_at) VALUES(?1, ?2, 'jsonl-import', ?3)
         ON CONFLICT(id) DO UPDATE SET display_name=excluded.display_name",
        params![account_id, dataset_name, Utc::now().to_rfc3339()],
    )?;

    let mut chats_imported = 0usize;
    let mut messages_imported = 0usize;
    for (index, line) in lines.enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let record: PortableRecord = serde_json::from_str(&line).map_err(|_| {
            AppError::Validation(format!("第 {} 行不是有效的 Chat Insight 记录", index + 2))
        })?;
        match record {
            PortableRecord::Chat {
                chat_id,
                display_name,
                chat_kind,
            } => {
                if chat_id.is_empty() {
                    return Err(AppError::Validation(format!(
                        "第 {} 行缺少 chat_id",
                        index + 2
                    )));
                }
                chats_imported += transaction.execute(
                    "INSERT INTO chats(account_id, chat_id, display_name, chat_kind) VALUES(?1, ?2, ?3, ?4)
                     ON CONFLICT(account_id, chat_id) DO UPDATE SET display_name=excluded.display_name, chat_kind=excluded.chat_kind",
                    params![account_id, chat_id, display_name, normalize_chat_kind(&chat_kind)],
                )?;
            }
            PortableRecord::Message {
                chat_id,
                local_id,
                server_id,
                occurred_at,
                sender_id,
                sender_name,
                is_from_me,
                kind,
                text,
            } => {
                let parsed = DateTime::parse_from_rfc3339(&occurred_at).map_err(|_| {
                    AppError::Validation(format!("第 {} 行的时间不是 RFC3339", index + 2))
                })?;
                let exists: bool = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM chats WHERE account_id=?1 AND chat_id=?2)",
                    params![account_id, chat_id],
                    |row| row.get(0),
                )?;
                if !exists {
                    return Err(AppError::Validation(format!(
                        "第 {} 行引用了尚未声明的聊天",
                        index + 2
                    )));
                }
                messages_imported += transaction.execute(
                    "INSERT OR IGNORE INTO messages(account_id, chat_id, local_id, server_id, occurred_at, local_date, local_hour, local_weekday, sender_id, sender_name, is_from_me, kind, text)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        account_id, chat_id, local_id, server_id, occurred_at,
                        parsed.format("%Y-%m-%d").to_string(), parsed.hour(),
                        parsed.weekday().num_days_from_monday(), sender_id, sender_name,
                        is_from_me as i64, normalize_message_kind(&kind), text,
                    ],
                )?;
            }
            PortableRecord::Manifest { .. } => {
                return Err(AppError::Validation("一个文件只能包含一个 manifest".into()))
            }
        }
    }
    transaction.execute(
        "UPDATE chats SET last_message_at=(SELECT MAX(m.occurred_at) FROM messages m WHERE m.account_id=chats.account_id AND m.chat_id=chats.chat_id) WHERE account_id=?1",
        params![account_id],
    )?;
    transaction.execute(
        "UPDATE accounts SET last_sync_at=?2 WHERE id=?1",
        params![account_id, Utc::now().to_rfc3339()],
    )?;
    transaction.commit()?;
    Ok(ImportResult {
        account_id,
        chats_imported,
        messages_imported,
    })
}

fn write_record(writer: &mut BufWriter<File>, record: &PortableRecord) -> AppResult<()> {
    serde_json::to_writer(&mut *writer, record)?;
    writer.write_all(b"\n")?;
    Ok(())
}

fn normalize_chat_kind(value: &str) -> &'static str {
    if value == "group" {
        "group"
    } else {
        "direct"
    }
}

pub(crate) fn normalize_message_kind(value: &str) -> String {
    match value.to_lowercase().as_str() {
        "text" | "image" | "video" | "voice" | "file" | "link" | "sticker" | "location"
        | "call" | "system" => value.to_lowercase(),
        _ => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NormalizedMessage;
    use tempfile::tempdir;

    #[test]
    fn round_trip_is_idempotent() {
        let directory = tempdir().unwrap();
        let source = Database::new(directory.path().join("source.db"));
        source.initialize().unwrap();
        source
            .upsert_account("source", "虚构账号", "fixture")
            .unwrap();
        source
            .upsert_chat("source", "team", "周末小队", "group")
            .unwrap();
        source
            .insert_message(
                "source",
                &NormalizedMessage {
                    chat_id: "team".into(),
                    local_id: 7,
                    server_id: Some("server-7".into()),
                    occurred_at: "2026-08-22T10:30:00+08:00".into(),
                    local_date: "2026-08-22".into(),
                    local_hour: 10,
                    local_weekday: 5,
                    sender_id: "alice".into(),
                    sender_name: "小艾".into(),
                    is_from_me: false,
                    kind: "text".into(),
                    text: "周末公园见".into(),
                },
            )
            .unwrap();
        let export_path = directory.path().join("fixture.jsonl");
        export_jsonl(&source, "source", &export_path).unwrap();

        let target = Database::new(directory.path().join("target.db"));
        target.initialize().unwrap();
        let first = import_jsonl(&target, &export_path).unwrap();
        let second = import_jsonl(&target, &export_path).unwrap();
        assert_eq!(first.messages_imported, 1);
        assert_eq!(second.messages_imported, 0);
        assert_eq!(
            target
                .dashboard(&first.account_id, &Default::default())
                .unwrap()
                .metrics
                .total_messages,
            1
        );
    }

    #[test]
    fn rejects_unknown_schema_without_writing() {
        let directory = tempdir().unwrap();
        let db = Database::new(directory.path().join("target.db"));
        db.initialize().unwrap();
        let path = directory.path().join("bad.jsonl");
        std::fs::write(&path, r#"{"type":"manifest","format":"chat-insight-jsonl","schema_version":99,"app_version":"9","exported_at":"now","dataset_id":"bad","dataset_name":"bad"}"#).unwrap();
        assert!(import_jsonl(&db, &path).is_err());
        assert!(db.list_accounts().unwrap().is_empty());
    }
}
