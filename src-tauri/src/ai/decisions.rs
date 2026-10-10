use super::client::ChatMessage;
use super::client::LlmClient;
use super::extraction::{
    self, accepted_due_date, append_window, chat_kind_label, date_label, index, items, number,
    text, ChatPlan, Extractor, ScanEstimate, ScanOutcome, Window, DEFAULT_CONFIDENCE,
    MIN_CONFIDENCE, NOTE_CHARS, OPEN_ITEMS, QUOTE_CHARS,
};
use super::indexer::speaker;
use super::Sink;
use crate::database::Database;
use crate::decisions::{NewDecision, OpenDecision, AI_STATUSES, KINDS, OWNER_CHARS};
use crate::error::AppResult;
use crate::memory::{clean_text, clean_title, AiFollowup};
use crate::models::DateRange;
use serde_json::Value;
use std::collections::HashSet;

pub const CUES: &[&str] = &[
    "定了",
    "就这么定",
    "决定",
    "确定",
    "那就",
    "同意",
    "没问题",
    "通过",
    "改成",
    "改到",
    "取消",
    "推迟",
    "先不",
    "暂定",
    "待定",
    "再议",
    "还没定",
    "谁来",
    "谁负责",
    "负责",
    "方案",
    "结论",
    "投票",
    "截止",
    "今晚",
    "明天",
    "后天",
    "下周",
    "周一",
    "周二",
    "周三",
    "周四",
    "周五",
    "周六",
    "周日",
    "周末",
    "月底",
    "之前",
    "deadline",
];

const SYSTEM_PROMPT: &str = r#"你负责从微信群聊中找出两类内容。“我”指用户本人。
1. 决定：大家已经达成一致，或有人拍板的具体结论。例如确定了时间、方案、负责人。
2. 待定：有人明确提出、但这段聊天里还没有定下来的事项。
不要记录闲聊、个人看法、单纯的提问，以及还在发散、没有形成事项的讨论。
规则：
1. 只从“本段聊天”中带编号的消息里找，前文只用来理解上下文。
2. kind 写 decision 或 question。line 指向说出结论或提出事项的那条消息。
3. title 用不超过 20 个字概括，例如“周五前完成设计稿”。
4. owner 写负责人的名字，没有就写 null。due_date 根据消息日期换算成 YYYY-MM-DD，无法确定就写 null。
5. confidence 是 0 到 1 之间的数字。
6. 对“仍然有效的记录”中的每一项，如果本段聊天表明决定被推翻或改了内容，status 写 overturned 或 changed；待定事项定下来了就写 resolved。id 写列表里的编号（如 D1），line 写证据消息，note 用一句话说明。没有变化就不要写。
只输出一个 JSON 对象，例如：
{"items":[{"line":3,"kind":"decision","title":"周五前完成设计稿","owner":"小艾","due_date":"2026-10-16","confidence":0.9}],"updates":[{"id":"D1","status":"overturned","line":4,"note":"改到下周一"}]}
没有内容时输出 {"items":[],"updates":[]}。"#;

pub struct DecisionExtractor;

impl Extractor for DecisionExtractor {
    type Open = OpenDecision;

    fn kind(&self) -> &'static str {
        "decision"
    }

    fn cues(&self) -> &'static [&'static str] {
        CUES
    }

    fn group_needs_me(&self) -> bool {
        false
    }

    fn open_items(
        &self,
        db: &Database,
        account_id: &str,
        chat_id: &str,
        before_local_id: i64,
    ) -> AppResult<Vec<OpenDecision>> {
        db.open_decisions(account_id, chat_id, before_local_id, OPEN_ITEMS)
    }

    fn prompt(&self, plan: &ChatPlan, window: &Window, open: &[OpenDecision]) -> Vec<ChatMessage> {
        extraction_prompt(&plan.chat_name, &plan.chat_kind, window, open)
    }

    fn apply(
        &self,
        db: &Database,
        account_id: &str,
        plan: &ChatPlan,
        window: &Window,
        open: &[OpenDecision],
        reply: &Value,
        model: &str,
    ) -> AppResult<(usize, usize)> {
        let (suggestions, followups) = validate(&plan.chat_id, window, open, reply);
        let mut created = 0;
        let mut updated = 0;
        for suggestion in &suggestions {
            if db.insert_decision(account_id, suggestion, model)?.is_some() {
                created += 1;
            }
        }
        for followup in &followups {
            if db.apply_decision_followup(account_id, followup)? {
                updated += 1;
            }
        }
        Ok((created, updated))
    }
}

pub fn plan_scan(
    db: &Database,
    account_id: &str,
    chat_id: &str,
    range: &DateRange,
) -> AppResult<Vec<ChatPlan>> {
    let extractor = DecisionExtractor;
    extraction::plan_scan(
        db,
        account_id,
        Some(chat_id),
        range,
        extractor.kind(),
        extractor.cues(),
        extractor.group_needs_me(),
    )
}

pub fn estimate(plans: &[ChatPlan]) -> ScanEstimate {
    extraction::estimate(plans)
}

pub fn extraction_prompt(
    chat_name: &str,
    chat_kind: &str,
    window: &Window,
    open: &[OpenDecision],
) -> Vec<ChatMessage> {
    let mut body = format!(
        "聊天：{chat_name}（{}）\n本段聊天日期：{}\n",
        chat_kind_label(chat_kind),
        window.lines.first().map(date_label).unwrap_or_default()
    );
    if !open.is_empty() {
        body.push_str("\n仍然有效的记录：\n");
        for (position, item) in open.iter().enumerate() {
            let label = if item.kind == "question" {
                "待定"
            } else {
                "决定"
            };
            let owner = item
                .owner_name
                .as_deref()
                .map(|name| format!("，{name}负责"))
                .unwrap_or_default();
            let due = item
                .due_date
                .as_deref()
                .map(|date| format!("（截止 {date}）"))
                .unwrap_or_default();
            body.push_str(&format!(
                "[D{}] {label}：{}{owner}{due}\n",
                position + 1,
                item.title
            ));
        }
    }
    append_window(&mut body, window);
    vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(body)]
}

fn allowed_update(kind: &str, status: &str) -> bool {
    match status {
        "resolved" => kind == "question",
        "overturned" | "changed" => kind == "decision",
        _ => false,
    }
}

pub fn validate(
    chat_id: &str,
    window: &Window,
    open: &[OpenDecision],
    reply: &Value,
) -> (Vec<NewDecision>, Vec<AiFollowup>) {
    let mut seen = HashSet::new();
    let mut suggestions = Vec::new();
    for item in items(reply, "items") {
        let Some(message) = index(item.get("line"))
            .and_then(|line| line.checked_sub(1))
            .and_then(|line| window.lines.get(line))
        else {
            continue;
        };
        let Some(kind) = item
            .get("kind")
            .and_then(Value::as_str)
            .filter(|kind| KINDS.contains(kind))
            .map(str::to_string)
        else {
            continue;
        };
        let Some(title) = item
            .get("title")
            .and_then(Value::as_str)
            .and_then(clean_title)
        else {
            continue;
        };
        let confidence = number(item.get("confidence"))
            .unwrap_or(DEFAULT_CONFIDENCE)
            .clamp(0.0, 1.0);
        if confidence < MIN_CONFIDENCE || !seen.insert((message.local_id, kind.clone())) {
            continue;
        }
        suggestions.push(NewDecision {
            chat_id: chat_id.to_string(),
            kind,
            title,
            owner_name: text(item.get("owner"), OWNER_CHARS)
                .and_then(|name| clean_text(&name, OWNER_CHARS)),
            due_date: accepted_due_date(
                text(item.get("due_date"), 10).as_deref(),
                &message.local_date,
            ),
            quote: message.text.trim().chars().take(QUOTE_CHARS).collect(),
            speaker_name: speaker(message).to_string(),
            source_local_id: message.local_id,
            source_at: message.occurred_at.clone(),
            confidence,
        });
    }

    let mut updated = HashSet::new();
    let mut followups = Vec::new();
    for update in items(reply, "updates") {
        let Some(target) = index(update.get("id"))
            .and_then(|id| id.checked_sub(1))
            .and_then(|id| open.get(id))
        else {
            continue;
        };
        let Some(status) = update
            .get("status")
            .and_then(Value::as_str)
            .filter(|status| AI_STATUSES.contains(status) && allowed_update(&target.kind, status))
        else {
            continue;
        };
        if !updated.insert(target.id) {
            continue;
        }
        followups.push(AiFollowup {
            id: target.id,
            status: status.to_string(),
            note: text(update.get("note"), NOTE_CHARS),
            followup_local_id: index(update.get("line"))
                .and_then(|line| line.checked_sub(1))
                .and_then(|line| window.lines.get(line))
                .map(|message| message.local_id),
        });
    }
    (suggestions, followups)
}

pub fn scan_decisions(
    db: &Database,
    client: &dyn LlmClient,
    sink: &dyn Sink,
    account_id: &str,
    chat_id: &str,
    range: &DateRange,
    model: &str,
) -> AppResult<ScanOutcome> {
    extraction::scan(
        &DecisionExtractor,
        db,
        client,
        sink,
        account_id,
        Some(chat_id),
        range,
        model,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::client::fake::FakeClient;
    use crate::ai::store::TextMessage;
    use crate::ai::NullSink;
    use crate::commitments::tests::{fixture_db, insert_text};
    use crate::decisions::DecisionFilter;

    fn message(local_id: i64, text: &str) -> TextMessage {
        TextMessage {
            local_id,
            occurred_at: "2026-10-12T10:00:00+08:00".into(),
            local_date: "2026-10-12".into(),
            sender_name: "小艾".into(),
            is_from_me: false,
            text: text.into(),
        }
    }

    #[test]
    fn validation_keeps_supported_updates_only() {
        let window = Window {
            context: Vec::new(),
            lines: vec![message(3, "那就周五前定稿，小艾来")],
            send: true,
        };
        let open = vec![
            OpenDecision {
                id: 1,
                kind: "decision".into(),
                title: "周一评审".into(),
                owner_name: None,
                due_date: None,
            },
            OpenDecision {
                id: 2,
                kind: "question".into(),
                title: "场地还没定".into(),
                owner_name: None,
                due_date: None,
            },
        ];
        let reply = serde_json::json!({
            "items": [
                {"line": 1, "kind": "decision", "title": "周五前定稿", "owner": "小艾", "due_date": "2026-10-16", "confidence": 0.8},
                {"line": 1, "kind": "nope", "title": "不算"},
                {"line": 9, "kind": "question", "title": "行号不存在"}
            ],
            "updates": [
                {"id": "D1", "status": "resolved", "line": 1},
                {"id": "D1", "status": "overturned", "line": 1, "note": "改了"},
                {"id": "D2", "status": "resolved", "line": 1, "note": "定了"}
            ]
        });
        let (suggestions, followups) = validate("team", &window, &open, &reply);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].owner_name.as_deref(), Some("小艾"));
        assert_eq!(suggestions[0].speaker_name, "小艾");
        assert_eq!(
            followups
                .iter()
                .map(|item| (item.id, item.status.clone()))
                .collect::<Vec<_>>(),
            vec![(1, "overturned".to_string()), (2, "resolved".to_string())]
        );
    }

    #[test]
    fn scans_a_group_and_follows_up() {
        let db = fixture_db();
        db.upsert_chat("account", "team", "项目群", "group")
            .unwrap();
        insert_text(
            &db,
            "team",
            1,
            "2026-10-12T10:00:00+08:00",
            false,
            "设计稿什么时候定？",
        );
        insert_text(
            &db,
            "team",
            2,
            "2026-10-12T10:01:00+08:00",
            true,
            "那就周五前定稿",
        );
        insert_text(
            &db,
            "team",
            3,
            "2026-10-13T09:00:00+08:00",
            false,
            "改成下周一吧，周五来不及",
        );
        let client = FakeClient::new(&[
            r#"{"items":[{"line":2,"kind":"decision","title":"周五前定稿","owner":null,"due_date":"2026-10-16","confidence":0.9}],"updates":[]}"#,
            r#"{"items":[],"updates":[{"id":"D1","status":"overturned","line":1,"note":"改到下周一"}]}"#,
        ]);
        let range = DateRange::default();
        let outcome =
            scan_decisions(&db, &client, &NullSink, "account", "team", &range, "m").unwrap();
        assert_eq!(
            (outcome.created, outcome.updated, outcome.failed),
            (1, 1, 0)
        );
        let items = db
            .list_decisions(
                "account",
                &DecisionFilter {
                    chat_id: Some("team".into()),
                    ..DecisionFilter::default()
                },
            )
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, "suggested");
        assert_eq!(items[0].ai_status.as_deref(), Some("overturned"));
        assert!(client.prompts.lock().unwrap()[1].contains("[D1] 决定：周五前定稿"));
    }
}
