use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::memory::{clean_text, clean_title, user_date, AiFollowup};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

pub const KINDS: [&str; 2] = ["decision", "question"];
pub const USER_STATUSES: [&str; 4] = ["suggested", "active", "closed", "dismissed"];
pub const AI_STATUSES: [&str; 3] = ["overturned", "changed", "resolved"];
pub const OWNER_CHARS: usize = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub id: i64,
    pub chat_id: String,
    pub chat_name: String,
    pub kind: String,
    pub title: String,
    pub owner_name: Option<String>,
    pub due_date: Option<String>,
    pub quote: String,
    pub speaker_name: String,
    pub status: String,
    pub ai_status: Option<String>,
    pub ai_note: Option<String>,
    pub followup_local_id: Option<i64>,
    pub source_local_id: i64,
    pub source_at: String,
    pub confidence: f64,
    pub user_edited: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NewDecision {
    pub chat_id: String,
    pub kind: String,
    pub title: String,
    pub owner_name: Option<String>,
    pub due_date: Option<String>,
    pub quote: String,
    pub speaker_name: String,
    pub source_local_id: i64,
    pub source_at: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct OpenDecision {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub owner_name: Option<String>,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DecisionFilter {
    pub statuses: Vec<String>,
    pub kinds: Vec<String>,
    pub chat_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionEdit {
    pub title: String,
    pub owner: Option<String>,
    pub due_date: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DecisionPatch {
    pub status: Option<String>,
    pub edit: Option<DecisionEdit>,
}

fn read_decision(row: &Row) -> rusqlite::Result<Decision> {
    Ok(Decision {
        id: row.get(0)?,
        chat_id: row.get(1)?,
        chat_name: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        owner_name: row.get(5)?,
        due_date: row.get(6)?,
        quote: row.get(7)?,
        speaker_name: row.get(8)?,
        status: row.get(9)?,
        ai_status: row.get(10)?,
        ai_note: row.get(11)?,
        followup_local_id: row.get(12)?,
        source_local_id: row.get(13)?,
        source_at: row.get(14)?,
        confidence: row.get(15)?,
        user_edited: row.get::<_, i64>(16)? != 0,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

const SELECT_DECISION: &str = "SELECT d.id, d.chat_id, c.display_name, d.kind, d.title, d.owner_name, d.due_date, d.quote,
        d.speaker_name, d.status, d.ai_status, d.ai_note, d.followup_local_id, d.source_local_id, d.source_at,
        d.confidence, d.user_edited, d.created_at, d.updated_at
 FROM decisions d JOIN chats c ON c.account_id=d.account_id AND c.chat_id=d.chat_id";

impl Database {
    pub fn insert_decision(
        &self,
        account_id: &str,
        item: &NewDecision,
        model: &str,
    ) -> AppResult<Option<i64>> {
        let connection = self.connection()?;
        let now = Utc::now().to_rfc3339();
        let changed = connection.execute(
            "INSERT OR IGNORE INTO decisions(
                account_id, chat_id, kind, title, owner_name, due_date, quote, speaker_name, status,
                source_local_id, source_at, confidence, model, created_at, updated_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'suggested', ?9, ?10, ?11, ?12, ?13, ?13)",
            params![
                account_id,
                item.chat_id,
                item.kind,
                item.title,
                item.owner_name,
                item.due_date,
                item.quote,
                item.speaker_name,
                item.source_local_id,
                item.source_at,
                item.confidence,
                model,
                now
            ],
        )?;
        Ok((changed > 0).then(|| connection.last_insert_rowid()))
    }

    pub fn apply_decision_followup(
        &self,
        account_id: &str,
        followup: &AiFollowup,
    ) -> AppResult<bool> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE decisions SET ai_status=?3, ai_note=?4, followup_local_id=COALESCE(?5, followup_local_id), updated_at=?6
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

    /// Decisions and open questions that started before `before_local_id`, newest first.
    pub fn open_decisions(
        &self,
        account_id: &str,
        chat_id: &str,
        before_local_id: i64,
        limit: i64,
    ) -> AppResult<Vec<OpenDecision>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, kind, title, owner_name, due_date FROM decisions
             WHERE account_id=?1 AND chat_id=?2 AND source_local_id<?3 AND status IN ('suggested', 'active')
               AND COALESCE(ai_status, '') NOT IN ('overturned', 'resolved')
             ORDER BY source_local_id DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(
            params![account_id, chat_id, before_local_id, limit],
            |row| {
                Ok(OpenDecision {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    title: row.get(2)?,
                    owner_name: row.get(3)?,
                    due_date: row.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn list_decisions(
        &self,
        account_id: &str,
        filter: &DecisionFilter,
    ) -> AppResult<Vec<Decision>> {
        let statuses = if filter.statuses.is_empty() {
            USER_STATUSES
                .iter()
                .map(|status| status.to_string())
                .collect()
        } else {
            filter.statuses.clone()
        };
        let kinds = if filter.kinds.is_empty() {
            KINDS.iter().map(|kind| kind.to_string()).collect()
        } else {
            filter.kinds.clone()
        };
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "{SELECT_DECISION}
             WHERE d.account_id=?1 AND d.status IN (SELECT value FROM json_each(?2))
               AND d.kind IN (SELECT value FROM json_each(?3))
               AND (?4 IS NULL OR d.chat_id=?4)
             ORDER BY d.source_at DESC, d.id DESC"
        ))?;
        let rows = statement.query_map(
            params![
                account_id,
                serde_json::to_string(&statuses)?,
                serde_json::to_string(&kinds)?,
                filter.chat_id
            ],
            read_decision,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn decision(&self, account_id: &str, id: i64) -> AppResult<Decision> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!("{SELECT_DECISION} WHERE d.account_id=?1 AND d.id=?2"),
                params![account_id, id],
                read_decision,
            )
            .optional()?
            .ok_or_else(|| AppError::Validation("找不到这条决策，可能已被清空".into()))
    }

    pub fn update_decision(
        &self,
        account_id: &str,
        id: i64,
        patch: &DecisionPatch,
    ) -> AppResult<Decision> {
        let current = self.decision(account_id, id)?;
        let status = match patch.status.as_deref() {
            Some(status) if USER_STATUSES.contains(&status) => status.to_string(),
            Some(_) => return Err(AppError::Validation("未知的决策状态".into())),
            None => current.status.clone(),
        };
        let (title, owner, due_date, edited) = match &patch.edit {
            Some(edit) => {
                let title = clean_title(&edit.title)
                    .ok_or_else(|| AppError::Validation("请填写决策内容".into()))?;
                let due_date = user_date(edit.due_date.as_deref())
                    .map_err(|_| AppError::Validation("截止日期格式应为 YYYY-MM-DD".into()))?;
                (
                    title,
                    clean_text(edit.owner.as_deref().unwrap_or(""), OWNER_CHARS),
                    due_date,
                    true,
                )
            }
            None => (
                current.title.clone(),
                current.owner_name.clone(),
                current.due_date.clone(),
                current.user_edited,
            ),
        };
        let connection = self.connection()?;
        connection.execute(
            "UPDATE decisions SET status=?3, title=?4, owner_name=?5, due_date=?6, user_edited=?7, updated_at=?8
             WHERE account_id=?1 AND id=?2",
            params![
                account_id,
                id,
                status,
                title,
                owner,
                due_date,
                edited as i64,
                Utc::now().to_rfc3339()
            ],
        )?;
        self.decision(account_id, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commitments::tests::{fixture_db, insert_text};

    fn item(local_id: i64, kind: &str) -> NewDecision {
        NewDecision {
            chat_id: "alice".into(),
            kind: kind.into(),
            title: "周五前定稿".into(),
            owner_name: Some("小艾".into()),
            due_date: Some("2026-10-16".into()),
            quote: "那就周五前定稿".into(),
            speaker_name: "我".into(),
            source_local_id: local_id,
            source_at: "2026-10-12T10:00:00+08:00".into(),
            confidence: 0.9,
        }
    }

    #[test]
    fn stores_decisions_once_and_keeps_user_edits() {
        let db = fixture_db();
        insert_text(
            &db,
            "alice",
            1,
            "2026-10-12T10:00:00+08:00",
            true,
            "那就周五前定稿",
        );
        let id = db
            .insert_decision("account", &item(1, "decision"), "m")
            .unwrap()
            .unwrap();
        assert!(db
            .insert_decision("account", &item(1, "decision"), "m")
            .unwrap()
            .is_none());
        assert!(db
            .insert_decision("account", &item(1, "question"), "m")
            .unwrap()
            .is_some());

        let edited = db
            .update_decision(
                "account",
                id,
                &DecisionPatch {
                    status: Some("active".into()),
                    edit: Some(DecisionEdit {
                        title: "周五定稿".into(),
                        owner: Some("  ".into()),
                        due_date: Some("".into()),
                    }),
                },
            )
            .unwrap();
        assert_eq!(edited.status, "active");
        assert_eq!(edited.owner_name, None);
        assert_eq!(edited.due_date, None);
        assert!(edited.user_edited);

        db.apply_decision_followup(
            "account",
            &AiFollowup {
                id,
                status: "overturned".into(),
                note: Some("改期了".into()),
                followup_local_id: Some(4),
            },
        )
        .unwrap();
        let after = db.decision("account", id).unwrap();
        assert_eq!(after.status, "active");
        assert_eq!(after.title, "周五定稿");
        assert_eq!(after.ai_status.as_deref(), Some("overturned"));
        let open = db.open_decisions("account", "alice", 10, 8).unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].kind, "question");
        assert!(db
            .update_decision(
                "account",
                id,
                &DecisionPatch {
                    status: Some("later".into()),
                    edit: None
                }
            )
            .is_err());
    }
}
