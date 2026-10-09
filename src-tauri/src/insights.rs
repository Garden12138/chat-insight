use crate::database::Database;
use crate::error::AppResult;
use crate::models::{HourlyPoint, RankedChat, TrendPoint, TypeCount};
use chrono::{DateTime, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::HashMap;

const CONVERSATION_GAP_MINUTES: i64 = 6 * 60;
const REPLY_WINDOW_MINUTES: i64 = 12 * 60;
const RELATIONSHIP_LIMIT: usize = 8;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Streak {
    pub days: i64,
    pub start: String,
    pub end: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LateMessage {
    pub occurred_at: String,
    pub chat_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipInsight {
    pub chat_id: String,
    pub display_name: String,
    pub chat_kind: String,
    pub count: i64,
    pub previous_count: i64,
    pub trend: String,
    pub initiated_by_me: Option<f64>,
    pub my_reply_minutes: Option<f64>,
    pub their_reply_minutes: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelationshipChange {
    pub chat_id: String,
    pub display_name: String,
    pub chat_kind: String,
    pub count: i64,
    pub previous_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct YearInsights {
    pub year: String,
    pub available_years: Vec<String>,
    pub total_messages: i64,
    pub sent_messages: i64,
    pub active_days: i64,
    pub chat_count: i64,
    pub previous_total: i64,
    pub longest_streak: Option<Streak>,
    pub busiest_day: Option<TrendPoint>,
    pub busiest_month: Option<TrendPoint>,
    pub busiest_hour: Option<HourlyPoint>,
    pub latest_night: Option<LateMessage>,
    pub monthly: Vec<TrendPoint>,
    pub top_chats: Vec<RankedChat>,
    pub message_types: Vec<TypeCount>,
    pub relationships: Vec<RelationshipInsight>,
    pub cooling: Vec<RelationshipChange>,
}

fn year_bounds(year: &str) -> (String, String) {
    (format!("{year}-01-01"), format!("{year}-12-31"))
}

pub fn longest_streak(dates: &[String]) -> Option<Streak> {
    let parsed: Vec<NaiveDate> = dates
        .iter()
        .filter_map(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        .collect();
    let mut best: Option<(NaiveDate, NaiveDate)> = None;
    let mut start = *parsed.first()?;
    let mut previous = start;
    for date in parsed
        .iter()
        .skip(1)
        .copied()
        .chain(std::iter::once(NaiveDate::MAX))
    {
        if date.signed_duration_since(previous).num_days() != 1 {
            if best.map_or(true, |(s, e)| (previous - start) > (e - s)) {
                best = Some((start, previous));
            }
            start = date;
        }
        previous = date;
    }
    best.map(|(start, end)| Streak {
        days: (end - start).num_days() + 1,
        start: start.format("%Y-%m-%d").to_string(),
        end: end.format("%Y-%m-%d").to_string(),
    })
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

pub struct Rhythm {
    pub initiated_by_me: Option<f64>,
    pub my_reply_minutes: Option<f64>,
    pub their_reply_minutes: Option<f64>,
}

/// Who opens conversations after a quiet period, and how fast each side answers.
pub fn conversation_rhythm(messages: &[(String, bool)]) -> Rhythm {
    let mut starts = 0usize;
    let mut my_starts = 0usize;
    let mut my_replies = Vec::new();
    let mut their_replies = Vec::new();
    let mut previous: Option<(DateTime<chrono::FixedOffset>, bool)> = None;
    for (occurred_at, from_me) in messages {
        let Ok(time) = DateTime::parse_from_rfc3339(occurred_at) else {
            continue;
        };
        let gap = previous.map(|(last, _)| (time - last).num_seconds() as f64 / 60.0);
        if gap.map_or(true, |minutes| minutes > CONVERSATION_GAP_MINUTES as f64) {
            starts += 1;
            my_starts += usize::from(*from_me);
        } else if let (Some(minutes), Some((_, last_from_me))) = (gap, previous) {
            if last_from_me != *from_me && minutes <= REPLY_WINDOW_MINUTES as f64 {
                if *from_me {
                    my_replies.push(minutes);
                } else {
                    their_replies.push(minutes);
                }
            }
        }
        previous = Some((time, *from_me));
    }
    Rhythm {
        initiated_by_me: (starts > 0).then(|| my_starts as f64 / starts as f64),
        my_reply_minutes: median(&mut my_replies),
        their_reply_minutes: median(&mut their_replies),
    }
}

pub fn trend_label(count: i64, previous: i64) -> &'static str {
    if previous == 0 {
        "new"
    } else if count as f64 >= previous as f64 * 1.5 {
        "warming"
    } else if count as f64 <= previous as f64 * 0.67 {
        "cooling"
    } else {
        "steady"
    }
}

fn chat_counts(
    connection: &Connection,
    account_id: &str,
    from: &str,
    to: &str,
) -> rusqlite::Result<Vec<RankedChat>> {
    let mut statement = connection.prepare(
        "SELECT m.chat_id, c.display_name, c.chat_kind, COUNT(*) AS total
         FROM messages m JOIN chats c ON c.account_id=m.account_id AND c.chat_id=m.chat_id
         WHERE m.account_id=?1 AND m.local_date>=?2 AND m.local_date<=?3
         GROUP BY m.chat_id ORDER BY total DESC",
    )?;
    let rows = statement.query_map(params![account_id, from, to], |row| {
        Ok(RankedChat {
            chat_id: row.get(0)?,
            display_name: row.get(1)?,
            chat_kind: row.get(2)?,
            count: row.get(3)?,
        })
    })?;
    rows.collect()
}

impl Database {
    pub fn year_insights(&self, account_id: &str, year: Option<&str>) -> AppResult<YearInsights> {
        let connection = self.connection()?;
        let mut years_statement = connection.prepare(
            "SELECT DISTINCT substr(local_date, 1, 4) AS year FROM messages WHERE account_id=?1 ORDER BY year DESC",
        )?;
        let available_years = years_statement
            .query_map(params![account_id], |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let year = year
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .or_else(|| available_years.first().cloned())
            .unwrap_or_else(|| chrono::Local::now().format("%Y").to_string());
        let (from, to) = year_bounds(&year);
        let previous_year = year
            .parse::<i32>()
            .map(|value| value - 1)
            .unwrap_or(0)
            .to_string();
        let (previous_from, previous_to) = year_bounds(&previous_year);
        let scope = "account_id=?1 AND local_date>=?2 AND local_date<=?3";

        let (total_messages, sent_messages, active_days, chat_count) = connection.query_row(
            &format!("SELECT COUNT(*), COALESCE(SUM(is_from_me), 0), COUNT(DISTINCT local_date), COUNT(DISTINCT chat_id) FROM messages WHERE {scope}"),
            params![account_id, from, to],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let previous_total = connection.query_row(
            &format!("SELECT COUNT(*) FROM messages WHERE {scope}"),
            params![account_id, previous_from, previous_to],
            |row| row.get(0),
        )?;

        let mut day_statement = connection.prepare(&format!(
            "SELECT local_date, COUNT(*) FROM messages WHERE {scope} GROUP BY local_date ORDER BY local_date"
        ))?;
        let days = day_statement
            .query_map(params![account_id, from, to], |row| {
                Ok(TrendPoint {
                    bucket: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let longest_streak = longest_streak(
            &days
                .iter()
                .map(|day| day.bucket.clone())
                .collect::<Vec<_>>(),
        );
        let busiest_day = days.iter().max_by_key(|day| day.count).cloned();

        let mut month_counts: HashMap<String, i64> = HashMap::new();
        for day in &days {
            *month_counts.entry(day.bucket[..7].to_string()).or_default() += day.count;
        }
        let monthly: Vec<TrendPoint> = (1..=12)
            .map(|month| {
                let bucket = format!("{year}-{month:02}");
                TrendPoint {
                    count: month_counts.get(&bucket).copied().unwrap_or(0),
                    bucket,
                }
            })
            .collect();
        let busiest_month = monthly
            .iter()
            .filter(|m| m.count > 0)
            .max_by_key(|m| m.count)
            .cloned();

        let busiest_hour = connection
            .query_row(
                &format!("SELECT local_hour, COUNT(*) AS total FROM messages WHERE {scope} GROUP BY local_hour ORDER BY total DESC LIMIT 1"),
                params![account_id, from, to],
                |row| Ok(HourlyPoint { hour: row.get(0)?, count: row.get(1)? }),
            )
            .optional()?;

        // Hours shifted so 05:00 sorts first and 04:59 last: the "latest" message of a night.
        let latest_night = connection
            .query_row(
                "SELECT m.occurred_at, c.display_name FROM messages m
                 JOIN chats c ON c.account_id=m.account_id AND c.chat_id=m.chat_id
                 WHERE m.account_id=?1 AND m.local_date>=?2 AND m.local_date<=?3 AND m.is_from_me=1
                   AND (m.local_hour>=22 OR m.local_hour<5)
                 ORDER BY (m.local_hour + 19) % 24 DESC, substr(m.occurred_at, 15, 5) DESC LIMIT 1",
                params![account_id, from, to],
                |row| {
                    Ok(LateMessage {
                        occurred_at: row.get(0)?,
                        chat_name: row.get(1)?,
                    })
                },
            )
            .optional()?;

        let mut type_statement = connection.prepare(&format!(
            "SELECT kind, COUNT(*) AS total FROM messages WHERE {scope} GROUP BY kind ORDER BY total DESC"
        ))?;
        let message_types = type_statement
            .query_map(params![account_id, from, to], |row| {
                Ok(TypeCount {
                    kind: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let current = chat_counts(&connection, account_id, &from, &to)?;
        let previous: HashMap<String, RankedChat> =
            chat_counts(&connection, account_id, &previous_from, &previous_to)?
                .into_iter()
                .map(|chat| (chat.chat_id.clone(), chat))
                .collect();
        let current_counts: HashMap<&str, i64> = current
            .iter()
            .map(|chat| (chat.chat_id.as_str(), chat.count))
            .collect();

        let mut rhythm_statement = connection.prepare(
            "SELECT occurred_at, is_from_me FROM messages
             WHERE account_id=?1 AND chat_id=?2 AND local_date>=?3 AND local_date<=?4 ORDER BY occurred_at, local_id",
        )?;
        let mut relationships = Vec::new();
        for chat in current
            .iter()
            .filter(|chat| chat.chat_kind != "group")
            .take(RELATIONSHIP_LIMIT)
        {
            let messages = rhythm_statement
                .query_map(params![account_id, chat.chat_id, from, to], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let rhythm = conversation_rhythm(&messages);
            let previous_count = previous
                .get(&chat.chat_id)
                .map(|item| item.count)
                .unwrap_or(0);
            relationships.push(RelationshipInsight {
                chat_id: chat.chat_id.clone(),
                display_name: chat.display_name.clone(),
                chat_kind: chat.chat_kind.clone(),
                count: chat.count,
                previous_count,
                trend: trend_label(chat.count, previous_count).into(),
                initiated_by_me: rhythm.initiated_by_me,
                my_reply_minutes: rhythm.my_reply_minutes,
                their_reply_minutes: rhythm.their_reply_minutes,
            });
        }

        let mut cooling: Vec<RelationshipChange> = previous
            .values()
            .filter_map(|chat| {
                let count = current_counts
                    .get(chat.chat_id.as_str())
                    .copied()
                    .unwrap_or(0);
                (chat.count >= 50 && (count as f64) <= chat.count as f64 * 0.5).then(|| {
                    RelationshipChange {
                        chat_id: chat.chat_id.clone(),
                        display_name: chat.display_name.clone(),
                        chat_kind: chat.chat_kind.clone(),
                        count,
                        previous_count: chat.count,
                    }
                })
            })
            .collect();
        cooling.sort_by_key(|chat| std::cmp::Reverse(chat.previous_count - chat.count));
        cooling.truncate(5);

        Ok(YearInsights {
            year,
            available_years,
            total_messages,
            sent_messages,
            active_days,
            chat_count,
            previous_total,
            longest_streak,
            busiest_day,
            busiest_month,
            busiest_hour,
            latest_night,
            monthly,
            top_chats: current.into_iter().take(10).collect(),
            message_types,
            relationships,
            cooling,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NormalizedMessage;
    use tempfile::tempdir;

    #[test]
    fn finds_longest_streak() {
        let dates: Vec<String> = [
            "2026-01-01",
            "2026-01-02",
            "2026-01-05",
            "2026-01-06",
            "2026-01-07",
            "2026-01-09",
        ]
        .iter()
        .map(|date| date.to_string())
        .collect();
        let streak = longest_streak(&dates).unwrap();
        assert_eq!(
            (streak.days, streak.start.as_str(), streak.end.as_str()),
            (3, "2026-01-05", "2026-01-07")
        );
        assert_eq!(longest_streak(&["2026-03-01".to_string()]).unwrap().days, 1);
        assert!(longest_streak(&[]).is_none());
    }

    #[test]
    fn measures_initiative_and_reply_speed() {
        let at = |time: &str| format!("2026-08-20T{time}:00+08:00");
        let messages = vec![
            (at("09:00"), true),
            (at("09:10"), false),
            (at("09:12"), true),
            (at("20:00"), false),
            (at("20:30"), true),
        ];
        let rhythm = conversation_rhythm(&messages);
        assert_eq!(rhythm.initiated_by_me, Some(0.5));
        assert_eq!(rhythm.their_reply_minutes, Some(10.0));
        assert_eq!(rhythm.my_reply_minutes, Some(16.0));
    }

    #[test]
    fn labels_trends() {
        assert_eq!(trend_label(10, 0), "new");
        assert_eq!(trend_label(300, 100), "warming");
        assert_eq!(trend_label(50, 100), "cooling");
        assert_eq!(trend_label(100, 100), "steady");
    }

    #[test]
    fn summarizes_a_year() {
        let directory = tempdir().unwrap().keep();
        let db = Database::new(directory.join("test.db"));
        db.initialize().unwrap();
        db.upsert_account("account", "测试", "fixture").unwrap();
        db.upsert_chat("account", "alice", "小艾", "direct")
            .unwrap();
        db.upsert_chat("account", "bob", "老王", "direct").unwrap();
        let mut id = 0;
        let mut insert = |chat: &str, date: &str, hour: u32, from_me: bool| {
            id += 1;
            db.insert_message(
                "account",
                &NormalizedMessage {
                    chat_id: chat.into(),
                    local_id: id,
                    server_id: None,
                    occurred_at: format!("{date}T{hour:02}:30:00+08:00"),
                    local_date: date.into(),
                    local_hour: hour,
                    local_weekday: 0,
                    sender_id: String::new(),
                    sender_name: String::new(),
                    is_from_me: from_me,
                    kind: "text".into(),
                    text: "你好".into(),
                },
            )
            .unwrap();
        };
        for day in 1..=60 {
            insert(
                "bob",
                &format!("2025-{:02}-{:02}", 1 + (day - 1) / 28, 1 + (day - 1) % 28),
                10,
                day % 2 == 0,
            );
        }
        insert("alice", "2026-03-01", 23, true);
        insert("alice", "2026-03-02", 1, true);
        insert("alice", "2026-03-02", 9, false);
        insert("bob", "2026-03-03", 10, false);

        let insights = db.year_insights("account", Some("2026")).unwrap();
        assert_eq!(insights.available_years, vec!["2026", "2025"]);
        assert_eq!((insights.total_messages, insights.previous_total), (4, 60));
        assert_eq!(insights.longest_streak.as_ref().unwrap().days, 3);
        assert_eq!(insights.monthly.len(), 12);
        assert_eq!(insights.busiest_month.as_ref().unwrap().bucket, "2026-03");
        assert!(insights
            .latest_night
            .as_ref()
            .unwrap()
            .occurred_at
            .starts_with("2026-03-02T01"));
        assert_eq!(insights.relationships[0].display_name, "小艾");
        assert_eq!(insights.relationships[0].trend, "new");
        assert_eq!(insights.cooling.len(), 1);
        assert_eq!(insights.cooling[0].display_name, "老王");

        let latest = db.year_insights("account", None).unwrap();
        assert_eq!(latest.year, "2026");
    }
}
