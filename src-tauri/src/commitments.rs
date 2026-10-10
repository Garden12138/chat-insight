use crate::database::Database;
use crate::error::{AppError, AppResult};
pub use crate::memory::AiFollowup;
use crate::memory::{clean_title, user_date};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

pub const USER_STATUSES: [&str; 4] = ["suggested", "open", "done", "dismissed"];
pub const AI_STATUSES: [&str; 5] = ["open", "done", "cancelled", "changed", "unclear"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Commitment {
    pub id: i64,
    pub chat_id: String,
    pub chat_name: String,
    pub chat_kind: String,
    pub direction: String,
    pub owner_name: String,
    pub title: String,
    pub quote: String,
    pub due_text: Option<String>,
    pub due_date: Option<String>,
    pub status: String,
    pub ai_status: Option<String>,
    pub ai_note: Option<String>,
    pub followup_local_id: Option<i64>,
    pub source_local_id: i64,
    pub source_end_local_id: i64,
    pub source_at: String,
    pub confidence: f64,
    pub user_edited: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// A validated suggestion ready to be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCommitment {
    pub chat_id: String,
    pub direction: String,
    pub owner_name: String,
    pub title: String,
    pub quote: String,
    pub due_text: Option<String>,
    pub due_date: Option<String>,
    pub source_local_id: i64,
    pub source_end_local_id: i64,
    pub source_at: String,
    pub confidence: f64,
}

/// An unfinished commitment shown to the model so it can notice follow-ups.
#[derive(Debug, Clone)]
pub struct OpenCommitment {
    pub id: i64,
    pub direction: String,
    pub owner_name: String,
    pub title: String,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitmentFilter {
    pub statuses: Vec<String>,
    pub direction: Option<String>,
    pub chat_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentEdit {
    pub title: String,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitmentPatch {
    pub status: Option<String>,
    pub edit: Option<CommitmentEdit>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommitmentCounts {
    pub suggested: i64,
    pub open: i64,
    pub overdue: i64,
    pub done: i64,
    pub dismissed: i64,
    /// Every suggestion the AI has made, whatever the user did with it.
    pub suggested_total: i64,
    /// Suggestions the user kept (confirmed or completed).
    pub accepted_total: i64,
}

fn read_commitment(row: &Row) -> rusqlite::Result<Commitment> {
    Ok(Commitment {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        chat_name: row.get(2)?,
        chat_kind: row.get(3)?,
        direction: row.get(4)?,
        owner_name: row.get(5)?,
        title: row.get(6)?,
        quote: row.get(7)?,
        due_text: row.get(8)?,
        due_date: row.get(9)?,
        status: row.get(10)?,
        ai_status: row.get(11)?,
        ai_note: row.get(12)?,
        followup_local_id: row.get(13)?,
        source_local_id: row.get(14)?,
        source_end_local_id: row.get(15)?,
        source_at: row.get(16)?,
        confidence: row.get(17)?,
        user_edited: row.get::<_, i64>(18)? != 0,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
    })
}

const SELECT_COMMITMENT: &str = "SELECT k.id, k.chat_id, c.display_name, c.chat_kind, k.direction, k.owner_name, k.title, k.quote,
        k.due_text, k.due_date, k.status, k.ai_status, k.ai_note, k.followup_local_id, k.source_local_id,
        k.source_end_local_id, k.source_at, k.confidence, k.user_edited, k.created_at, k.updated_at
 FROM commitments k JOIN chats c ON c.account_id=k.account_id AND c.chat_id=k.chat_id";

impl Database {
    /// Stores a suggestion unless one already exists for the same source message and
    /// direction. Returns the new id.
    pub fn insert_commitment(
        &self,
        account_id: &str,
        item: &NewCommitment,
        model: &str,
    ) -> AppResult<Option<i64>> {
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
        let changed = connection.execute(
            "INSERT OR IGNORE INTO commitments(
                account_id, chat_id, direction, owner_name, title, quote, due_text, due_date, status,
                source_local_id, source_end_local_id, source_at, confidence, model, created_at, updated_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'suggested', ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
            params![
                account_id,
                item.chat_id,
                item.direction,
                item.owner_name,
                item.title,
                item.quote,
                item.due_text,
                item.due_date,
                item.source_local_id,
                item.source_end_local_id,
                item.source_at,
                item.confidence,
                model,
                now
            ],
        )?;
        Ok((changed > 0).then(|| connection.last_insert_rowid()))
    }

    /// Writes the model's follow-up judgement. Never touches the user's status or edits.
    pub fn apply_ai_followup(&self, account_id: &str, followup: &AiFollowup) -> AppResult<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE commitments SET ai_status=?3, ai_note=?4, followup_local_id=COALESCE(?5, followup_local_id), updated_at=?6
             WHERE account_id=?1 AND id=?2",
            params![
                account_id,
                followup.id,
                followup.status,
                followup.note,
                followup.followup_local_id,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(changed > 0)
    }

    /// Unfinished commitments of a chat that started before `before_local_id`, newest first.
    pub fn open_commitments(
        &self,
        account_id: &str,
        chat_id: &str,
        before_local_id: i64,
        limit: i64,
    ) -> AppResult<Vec<OpenCommitment>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, direction, owner_name, title, due_date FROM commitments
             WHERE account_id=?1 AND chat_id=?2 AND source_local_id<?3 AND status IN ('suggested', 'open')
               AND COALESCE(ai_status, 'open') NOT IN ('done', 'cancelled')
             ORDER BY source_local_id DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![account_id, chat_id, before_local_id, limit],
            |row| {
                Ok(OpenCommitment {
                    id: row.get(0)?,
                    direction: row.get(1)?,
                    owner_name: row.get(2)?,
                    title: row.get(3)?,
                    due_date: row.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_commitments(
        &self,
        account_id: &str,
        filter: &CommitmentFilter,
    ) -> AppResult<Vec<Commitment>> {
        let statuses = if filter.statuses.is_empty() {
            USER_STATUSES
                .iter()
                .map(|status| status.to_string())
                .collect()
        } else {
            filter.statuses.clone()
        };
        let statuses_json = serde_json::to_string(&statuses)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "{SELECT_COMMITMENT}
             WHERE k.account_id=?1 AND k.status IN (SELECT value FROM json_each(?2))
               AND (?3 IS NULL OR k.direction=?3) AND (?4 IS NULL OR k.chat_id=?4)
             ORDER BY k.source_at DESC, k.id DESC"
        ))?;
        let rows = statement.query_map(
            params![account_id, statuses_json, filter.direction, filter.chat_id],
            read_commitment,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn commitment(&self, account_id: &str, id: i64) -> AppResult<Commitment> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!("{SELECT_COMMITMENT} WHERE k.account_id=?1 AND k.id=?2"),
                params![account_id, id],
                read_commitment,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("找不到这条约定，可能已被清空".into()))
    }

    pub fn update_commitment(
        &self,
        account_id: &str,
        id: i64,
        patch: &CommitmentPatch,
    ) -> AppResult<Commitment> {
        let current = self.commitment(account_id, id)?;
        let status = match patch.status.as_deref() {
            Some(status) if USER_STATUSES.contains(&status) => status.to_string(),
            Some(_) => return Err(AppError::Validation("未知的约定状态".into())),
            None => current.status.clone(),
        };
        let (title, due_date, edited) = match &patch.edit {
            Some(edit) => {
                let title = clean_title(&edit.title)
                    .ok_or_else(|| AppError::Validation("请填写约定内容".into()))?;
                let due_date = user_date(edit.due_date.as_deref())
                    .map_err(|_| AppError::Validation("截止日期格式应为 YYYY-MM-DD".into()))?;
                (title, due_date, true)
            }
            None => (
                current.title.clone(),
                current.due_date.clone(),
                current.user_edited,
            ),
        };
        let connection = self.connection()?;
        connection.execute(
            "UPDATE commitments SET status=?3, title=?4, due_date=?5, user_edited=?6, updated_at=?7
             WHERE account_id=?1 AND id=?2",
            params![
                account_id,
                id,
                status,
                title,
                due_date,
                edited as i64,
                Utc::now().to_rfc3339()
            ],
        )?;
        self.commitment(account_id, id)
    }

    pub fn commitment_counts(&self, account_id: &str, today: &str) -> AppResult<CommitmentCounts> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            "SELECT
                COALESCE(SUM(status='suggested'), 0),
                COALESCE(SUM(status='open'), 0),
                COALESCE(SUM(status='open' AND due_date IS NOT NULL AND due_date<?2), 0),
                COALESCE(SUM(status='done'), 0),
                COALESCE(SUM(status='dismissed'), 0),
                COUNT(*),
                COALESCE(SUM(status IN ('open', 'done')), 0)
             FROM commitments WHERE account_id=?1",
            params![account_id, today],
            |row| {
                Ok(CommitmentCounts {
                    suggested: row.get(0)?,
                    open: row.get(1)?,
                    overdue: row.get(2)?,
                    done: row.get(3)?,
                    dismissed: row.get(4)?,
                    suggested_total: row.get(5)?,
                    accepted_total: row.get(6)?,
                })
            },
        )?)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::models::NormalizedMessage;
    use tempfile::tempdir;

    pub fn fixture_db() -> Database {
        let directory = tempdir().unwrap().keep();
        let db = Database::new(directory.join("test.db"));
        db.initialize().unwrap();
        db.upsert_account("account", "测试", "fixture").unwrap();
        db.upsert_chat("account", "alice", "小艾", "direct")
            .unwrap();
        db
    }

    pub fn insert_text(
        db: &Database,
        chat_id: &str,
        local_id: i64,
        at: &str,
        from_me: bool,
        text: &str,
    ) {
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
                sender_id: if from_me { "me" } else { "other" }.into(),
                sender_name: if from_me { "我" } else { "小艾" }.into(),
                is_from_me: from_me,
                kind: "text".into(),
                text: text.into(),
            },
        )
        .unwrap();
    }

    pub fn suggestion(local_id: i64, direction: &str, title: &str) -> NewCommitment {
        NewCommitment {
            chat_id: "alice".into(),
            direction: direction.into(),
            owner_name: "我".into(),
            title: title.into(),
            quote: "周五前我把合同发你".into(),
            due_text: Some("周五前".into()),
            due_date: Some("2026-10-16".into()),
            source_local_id: local_id,
            source_end_local_id: local_id,
            source_at: "2026-10-12T10:00:00+08:00".into(),
            confidence: 0.9,
        }
    }

    #[test]
    fn stores_suggestions_once_and_counts_them() {
        let db = fixture_db();
        insert_text(
            &db,
            "alice",
            1,
            "2026-10-12T10:00:00+08:00",
            true,
            "周五前我把合同发你",
        );
        let id = db
            .insert_commitment("account", &suggestion(1, "mine", "把合同发给小艾"), "m")
            .unwrap()
            .unwrap();
        assert!(db
            .insert_commitment("account", &suggestion(1, "mine", "另一种说法"), "m")
            .unwrap()
            .is_none());
        assert!(db
            .insert_commitment("account", &suggestion(1, "theirs", "小艾等合同"), "m")
            .unwrap()
            .is_some());
        let listed = db
            .list_commitments(
                "account",
                &CommitmentFilter {
                    statuses: vec!["suggested".into()],
                    direction: Some("mine".into()),
                    chat_id: None,
                },
            )
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].chat_name, "小艾");

        db.update_commitment(
            "account",
            id,
            &CommitmentPatch {
                status: Some("open".into()),
                edit: None,
            },
        )
        .unwrap();
        let counts = db.commitment_counts("account", "2026-10-20").unwrap();
        assert_eq!(
            counts,
            CommitmentCounts {
                suggested: 1,
                open: 1,
                overdue: 1,
                done: 0,
                dismissed: 0,
                suggested_total: 2,
                accepted_total: 1,
            }
        );
    }

    #[test]
    fn ai_followups_never_override_the_user() {
        let db = fixture_db();
        insert_text(
            &db,
            "alice",
            1,
            "2026-10-12T10:00:00+08:00",
            true,
            "周五前我把合同发你",
        );
        let id = db
            .insert_commitment("account", &suggestion(1, "mine", "把合同发给小艾"), "m")
            .unwrap()
            .unwrap();
        let edited = db
            .update_commitment(
                "account",
                id,
                &CommitmentPatch {
                    status: Some("open".into()),
                    edit: Some(CommitmentEdit {
                        title: "  发合同  ".into(),
                        due_date: Some("2026-10-17".into()),
                    }),
                },
            )
            .unwrap();
        assert_eq!(edited.title, "发合同");
        assert!(edited.user_edited);
        assert_eq!(
            db.open_commitments("account", "alice", 10, 8)
                .unwrap()
                .len(),
            1
        );

        db.apply_ai_followup(
            "account",
            &AiFollowup {
                id,
                status: "done".into(),
                note: Some("已发送".into()),
                followup_local_id: Some(3),
            },
        )
        .unwrap();
        let after = db.commitment("account", id).unwrap();
        assert_eq!(after.status, "open");
        assert_eq!(after.title, "发合同");
        assert_eq!(after.due_date.as_deref(), Some("2026-10-17"));
        assert_eq!(after.ai_status.as_deref(), Some("done"));
        assert_eq!(after.followup_local_id, Some(3));
        assert!(db
            .open_commitments("account", "alice", 10, 8)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn rejects_invalid_edits() {
        let db = fixture_db();
        insert_text(
            &db,
            "alice",
            1,
            "2026-10-12T10:00:00+08:00",
            true,
            "周五前我把合同发你",
        );
        let id = db
            .insert_commitment("account", &suggestion(1, "mine", "把合同发给小艾"), "m")
            .unwrap()
            .unwrap();
        let patch = |status: Option<&str>, title: &str, due: Option<&str>| CommitmentPatch {
            status: status.map(str::to_string),
            edit: Some(CommitmentEdit {
                title: title.into(),
                due_date: due.map(str::to_string),
            }),
        };
        assert!(db
            .update_commitment("account", id, &patch(Some("later"), "x", None))
            .is_err());
        assert!(db
            .update_commitment("account", id, &patch(None, "  ", None))
            .is_err());
        assert!(db
            .update_commitment("account", id, &patch(None, "x", Some("下周")))
            .is_err());
        let cleared = db
            .update_commitment("account", id, &patch(None, "x", Some("")))
            .unwrap();
        assert_eq!(cleared.due_date, None);
        assert!(db
            .update_commitment("other", id, &patch(None, "x", None))
            .is_err());
    }

    #[test]
    fn clearing_and_adopting_datasets_carry_commitments() {
        let db = fixture_db();
        db.upsert_account("legacy", "wechat-cli", "macos").unwrap();
        db.upsert_chat("legacy", "alice", "小艾", "direct").unwrap();
        db.insert_message(
            "legacy",
            &NormalizedMessage {
                chat_id: "alice".into(),
                local_id: 1,
                server_id: None,
                occurred_at: "2026-10-12T10:00:00+08:00".into(),
                local_date: "2026-10-12".into(),
                local_hour: 10,
                local_weekday: 0,
                sender_id: "me".into(),
                sender_name: "我".into(),
                is_from_me: true,
                kind: "text".into(),
                text: "周五前我把合同发你".into(),
            },
        )
        .unwrap();
        db.insert_commitment("legacy", &suggestion(1, "mine", "发合同"), "m")
            .unwrap();
        db.mark_scanned("legacy", "alice", "commitment", 1, 1, "m")
            .unwrap();
        assert!(db
            .adopt_legacy_account("legacy", "wechat:wxid_a", "wxid_a")
            .unwrap());
        assert_eq!(
            db.commitment_counts("wechat:wxid_a", "2026-10-12")
                .unwrap()
                .suggested,
            1
        );
        assert_eq!(
            db.scanned_intervals("wechat:wxid_a", "alice", "commitment")
                .unwrap(),
            vec![(1, 1)]
        );
        db.clear_account("wechat:wxid_a").unwrap();
        assert_eq!(
            db.commitment_counts("wechat:wxid_a", "2026-10-12")
                .unwrap()
                .suggested_total,
            0
        );
        assert!(db
            .scanned_intervals("wechat:wxid_a", "alice", "commitment")
            .unwrap()
            .is_empty());
    }
}
