use super::client::ChatMessage;
use super::client::LlmClient;
use super::extraction::{
    self, accepted_due_date, append_window, chat_kind_label, date_label, index, items, number,
    text, ChatPlan, Extractor, Window, DEFAULT_CONFIDENCE, MIN_CONFIDENCE, NOTE_CHARS, OPEN_ITEMS,
    QUOTE_CHARS,
};
pub use super::extraction::{ScanEstimate, ScanOutcome};
use super::indexer::speaker;
use super::Sink;
use crate::commitments::{AiFollowup, NewCommitment, OpenCommitment, AI_STATUSES};
use crate::database::Database;
use crate::error::AppResult;
use crate::memory::clean_title;
use crate::models::DateRange;
use serde_json::Value;
use std::collections::HashSet;

/// Words that suggest a promise, a time, or that an earlier promise was settled.
/// Windows without any of them are not sent to the model.
pub const CUES: &[&str] = &[
    "我来",
    "我去",
    "我会",
    "我帮",
    "我给",
    "我发",
    "我把",
    "我再",
    "帮你",
    "帮我",
    "给你",
    "给我",
    "发你",
    "发我",
    "发给",
    "转你",
    "转给",
    "还你",
    "回头",
    "晚点",
    "晚些",
    "稍后",
    "等下",
    "等会",
    "一会",
    "记得",
    "别忘",
    "忘了",
    "答应",
    "保证",
    "负责",
    "安排",
    "处理",
    "跟进",
    "约",
    "请你",
    "麻烦你",
    "拜托",
    "能不能",
    "可以帮",
    "到时",
    "回你",
    "联系你",
    "告诉你",
    "通知你",
    "提醒",
    "今晚",
    "明天",
    "明早",
    "明晚",
    "后天",
    "下周",
    "下礼拜",
    "下个月",
    "下次",
    "周一",
    "周二",
    "周三",
    "周四",
    "周五",
    "周六",
    "周日",
    "周末",
    "星期",
    "礼拜",
    "月底",
    "月初",
    "年底",
    "之前",
    "以前",
    "号前",
    "点前",
    "前给",
    "截止",
    "deadline",
    "ddl",
    "发了",
    "搞定",
    "完成",
    "收到",
    "弄好",
    "办好",
    "不用了",
    "取消",
    "算了",
    "改到",
    "改成",
    "推迟",
    "延期",
    "提前",
];

const DUE_TEXT_CHARS: usize = 20;

const SYSTEM_PROMPT: &str = r#"你负责从微信聊天记录中找出“约定”：某个人明确答应在之后去做的一件具体的事，例如发送文件、付款、回复、见面、帮忙处理某事。“我”指用户本人。
规则：
1. 只从“本段聊天”中带编号的消息里找，前文只用来理解上下文。
2. 只记录明确答应或主动承诺的事。不要记录：单纯的提问或建议、没有具体安排的客套话（如“改天请你吃饭”）、当场已经做完的事、与“我”无关的其他人之间的约定。
3. direction：“我”答应要做的事写 mine，line 指向我说出承诺的那条消息；对方答应为我做或与我约定的事写 theirs，line 指向对方说出承诺的那条消息。
4. title 用不超过 20 个字概括要做的事，以动作开头，例如“把合同发给小艾”。
5. due_text 抄写原文中的时间说法，没有就写 null；due_date 根据消息日期换算成 YYYY-MM-DD，无法确定就写 null。
6. confidence 是 0 到 1 之间的数字，表示你有多确定这是一个真实的约定。
7. 对“未完成的约定”中的每一项，如果本段聊天表明它已经完成、取消或改了时间，就写进 updates：id 写列表里的编号（如 A1），status 写 done、cancelled 或 changed，line 写证据消息的编号，note 用一句话说明。没有变化就不要写。
只输出一个 JSON 对象，不要输出其他内容，例如：
{"items":[{"line":2,"direction":"mine","title":"把合同发给小艾","due_text":"周五前","due_date":"2026-10-16","confidence":0.9}],"updates":[{"id":"A1","status":"done","line":5,"note":"合同已经发出"}]}
没有约定时输出 {"items":[],"updates":[]}。"#;

pub struct CommitmentExtractor;

impl Extractor for CommitmentExtractor {
    type Open = OpenCommitment;

    fn kind(&self) -> &'static str {
        "commitment"
    }

    fn cues(&self) -> &'static [&'static str] {
        CUES
    }

    fn group_needs_me(&self) -> bool {
        true
    }

    fn open_items(
        &self,
        db: &Database,
        account_id: &str,
        chat_id: &str,
        before_local_id: i64,
    ) -> AppResult<Vec<OpenCommitment>> {
        db.open_commitments(account_id, chat_id, before_local_id, OPEN_ITEMS)
    }

    fn prompt(
        &self,
        plan: &ChatPlan,
        window: &Window,
        open: &[OpenCommitment],
    ) -> Vec<ChatMessage> {
        extraction_prompt(&plan.chat_name, &plan.chat_kind, window, open)
    }

    fn apply(
        &self,
        db: &Database,
        account_id: &str,
        plan: &ChatPlan,
        window: &Window,
        open: &[OpenCommitment],
        reply: &Value,
        model: &str,
    ) -> AppResult<(usize, usize)> {
        let (suggestions, followups) = validate(
            &plan.chat_id,
            plan.chat_kind == "group",
            window,
            open,
            reply,
        );
        let mut created = 0;
        let mut updated = 0;
        for suggestion in &suggestions {
            if db
                .insert_commitment(account_id, suggestion, model)?
                .is_some()
            {
                created += 1;
            }
        }
        for followup in &followups {
            if db.apply_ai_followup(account_id, followup)? {
                updated += 1;
            }
        }
        Ok((created, updated))
    }
}

pub fn plan_scan(
    db: &Database,
    account_id: &str,
    chat_id: Option<&str>,
    range: &DateRange,
) -> AppResult<Vec<ChatPlan>> {
    let extractor = CommitmentExtractor;
    extraction::plan_scan(
        db,
        account_id,
        chat_id,
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
    open: &[OpenCommitment],
) -> Vec<ChatMessage> {
    let mut body = format!(
        "聊天：{chat_name}（{}）\n本段聊天日期：{}\n",
        chat_kind_label(chat_kind),
        window.lines.first().map(date_label).unwrap_or_default()
    );
    if chat_kind == "group" {
        body.push_str("这是群聊：只记录我答应的事，以及别人明确答应为我做的事；别人在群里认领的公共任务、别人之间的安排都不要记录。\n");
    }
    if !open.is_empty() {
        body.push_str("\n未完成的约定：\n");
        for (position, item) in open.iter().enumerate() {
            let who = if item.direction == "mine" {
                "我答应".to_string()
            } else {
                format!("{}答应", item.owner_name)
            };
            let due = item
                .due_date
                .as_deref()
                .map(|date| format!("（截止 {date}）"))
                .unwrap_or_default();
            body.push_str(&format!("[A{}] {who}：{}{due}\n", position + 1, item.title));
        }
    }
    append_window(&mut body, window);
    vec![ChatMessage::system(SYSTEM_PROMPT), ChatMessage::user(body)]
}

/// In a group, someone else's promise only counts when it is said to somebody; tasks
/// claimed for the whole group ("我负责订会议室") are not promises to the user.
fn addressed(text: &str) -> bool {
    text.contains('你') || text.contains('您') || text.contains('@')
}

/// Keeps only suggestions and follow-ups whose evidence checks out against the window.
pub fn validate(
    chat_id: &str,
    is_group: bool,
    window: &Window,
    open: &[OpenCommitment],
    reply: &Value,
) -> (Vec<NewCommitment>, Vec<AiFollowup>) {
    let mut seen = HashSet::new();
    let mut suggestions = Vec::new();
    for item in items(reply, "items") {
        let Some(message) = index(item.get("line"))
            .and_then(|line| line.checked_sub(1))
            .and_then(|line| window.lines.get(line))
        else {
            continue;
        };
        let direction = match item.get("direction").and_then(Value::as_str) {
            Some("mine") => "mine",
            Some("theirs") => "theirs",
            _ => continue,
        };
        if (direction == "mine") != message.is_from_me
            || (is_group && direction == "theirs" && !addressed(&message.text))
        {
            continue;
        }
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
        if confidence < MIN_CONFIDENCE || !seen.insert((message.local_id, direction)) {
            continue;
        }
        suggestions.push(NewCommitment {
            chat_id: chat_id.to_string(),
            direction: direction.to_string(),
            owner_name: speaker(message).to_string(),
            title,
            quote: message.text.trim().chars().take(QUOTE_CHARS).collect(),
            due_text: text(item.get("due_text"), DUE_TEXT_CHARS),
            due_date: accepted_due_date(
                text(item.get("due_date"), 10).as_deref(),
                &message.local_date,
            ),
            source_local_id: message.local_id,
            source_end_local_id: message.local_id,
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
            .filter(|status| *status != "open" && AI_STATUSES.contains(status))
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

pub fn scan_commitments(
    db: &Database,
    client: &dyn LlmClient,
    sink: &dyn Sink,
    account_id: &str,
    chat_id: Option<&str>,
    range: &DateRange,
    model: &str,
) -> AppResult<ScanOutcome> {
    extraction::scan(
        &CommitmentExtractor,
        db,
        client,
        sink,
        account_id,
        chat_id,
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
    use crate::commitments::CommitmentFilter;
    use std::cell::Cell;

    fn message(local_id: i64, at: &str, from_me: bool, text: &str) -> TextMessage {
        TextMessage {
            local_id,
            occurred_at: at.into(),
            local_date: at[..10].into(),
            sender_name: if from_me { "我" } else { "小艾" }.into(),
            is_from_me: from_me,
            text: text.into(),
        }
    }

    fn at(day: u32, hour: u32, minute: u32) -> String {
        format!("2026-10-{day:02}T{hour:02}:{minute:02}:00+08:00")
    }

    #[test]
    fn prompt_numbers_lines_and_lists_open_items() {
        let window = Window {
            context: vec![message(1, &at(12, 8, 59), false, "在吗")],
            lines: vec![
                message(2, &at(12, 9, 0), false, "合同什么时候给我"),
                message(3, &at(12, 9, 1), true, "周五前我发你"),
            ],
            send: true,
        };
        let open = vec![OpenCommitment {
            id: 9,
            direction: "theirs".into(),
            owner_name: "小艾".into(),
            title: "把发票寄给我".into(),
            due_date: Some("2026-10-14".into()),
        }];
        let prompt = extraction_prompt("小艾", "direct", &window, &open)[1]
            .content
            .clone();
        assert!(prompt.contains("本段聊天日期：2026-10-12（周一）"));
        assert!(prompt.contains("[A1] 小艾答应：把发票寄给我（截止 2026-10-14）"));
        assert!(prompt.contains("10-12 08:59 小艾: 在吗"));
        assert!(prompt.contains("[2] 10-12 09:01 我: 周五前我发你"));
    }

    #[test]
    fn validation_drops_unsupported_claims() {
        let window = Window {
            context: Vec::new(),
            lines: vec![
                message(10, &at(12, 9, 0), false, "我明天把钱转你"),
                message(11, &at(12, 9, 1), true, "好，周五前我把合同发你"),
            ],
            send: true,
        };
        let open = vec![OpenCommitment {
            id: 42,
            direction: "mine".into(),
            owner_name: "我".into(),
            title: "还书".into(),
            due_date: None,
        }];
        let reply = serde_json::json!({
            "items": [
                {"line": 2, "direction": "mine", "title": "把合同发给小艾", "due_text": "周五前", "due_date": "2026-10-16", "confidence": 0.9},
                {"line": "[1]", "direction": "theirs", "title": "转钱给我", "due_date": "2025-01-01", "confidence": "0.8"},
                {"line": 1, "direction": "mine", "title": "说话人不对"},
                {"line": 5, "direction": "mine", "title": "行号不存在"},
                {"line": 2, "direction": "mine", "title": "重复"},
                {"line": 2, "direction": "theirs", "title": "方向不对"},
                {"line": 2, "direction": "mine", "title": " "},
                {"line": 1, "direction": "theirs", "title": "太不确定", "confidence": 0.1}
            ],
            "updates": [
                {"id": "A1", "status": "done", "line": 2, "note": "已经还了"},
                {"id": "A2", "status": "done"},
                {"id": "A1", "status": "cancelled"},
                {"id": 1, "status": "maybe"}
            ]
        });
        let (suggestions, followups) = validate("alice", false, &window, &open, &reply);
        let (in_group, _) = validate("team", true, &window, &open, &reply);
        assert_eq!(in_group.len(), 2);
        let claimed = Window {
            lines: vec![message(20, &at(12, 10, 0), false, "那我负责订会议室")],
            ..window.clone()
        };
        let claim =
            serde_json::json!({"items": [{"line": 1, "direction": "theirs", "title": "订会议室"}]});
        assert!(validate("team", true, &claimed, &[], &claim).0.is_empty());
        assert_eq!(validate("bob", false, &claimed, &[], &claim).0.len(), 1);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].source_local_id, 11);
        assert_eq!(suggestions[0].owner_name, "我");
        assert_eq!(suggestions[0].due_date.as_deref(), Some("2026-10-16"));
        assert_eq!(suggestions[1].direction, "theirs");
        assert_eq!(suggestions[1].owner_name, "小艾");
        assert_eq!(suggestions[1].due_date, None);
        assert_eq!(suggestions[1].due_text, None);
        assert!((suggestions[1].confidence - 0.8).abs() < 1e-9);
        assert_eq!(
            followups,
            vec![AiFollowup {
                id: 42,
                status: "done".into(),
                note: Some("已经还了".into()),
                followup_local_id: Some(11),
            }]
        );
    }

    fn seed(db: &Database) {
        insert_text(db, "alice", 1, &at(12, 9, 0), false, "合同什么时候给我");
        insert_text(db, "alice", 2, &at(12, 9, 1), true, "周五前我把合同发你");
        insert_text(db, "alice", 3, &at(12, 13, 0), false, "中午吃了吗");
        insert_text(db, "alice", 4, &at(12, 13, 1), true, "吃了");
        insert_text(db, "alice", 5, &at(15, 18, 0), true, "合同发了，你看下");
        insert_text(db, "alice", 6, &at(15, 18, 1), false, "收到");
    }

    #[test]
    fn scans_incrementally_and_follows_up() {
        let db = fixture_db();
        seed(&db);
        let client = FakeClient::new(&[
            r#"{"items":[{"line":2,"direction":"mine","title":"把合同发给小艾","due_text":"周五前","due_date":"2026-10-16","confidence":0.9}],"updates":[]}"#,
            r#"{"items":[],"updates":[{"id":"A1","status":"done","line":1,"note":"合同已发"}]}"#,
        ]);
        let range = DateRange::default();
        let planned = estimate(&plan_scan(&db, "account", None, &range).unwrap());
        assert_eq!((planned.chats, planned.windows, planned.skipped), (1, 2, 1));
        assert!(planned.chars > 0);
        let outcome =
            scan_commitments(&db, &client, &NullSink, "account", None, &range, "m").unwrap();
        assert_eq!(
            outcome,
            ScanOutcome {
                chats: 1,
                windows: 2,
                skipped: 1,
                failed: 0,
                created: 1,
                updated: 1,
                cancelled: false,
            }
        );
        let prompts = client.prompts.lock().unwrap().clone();
        assert!(prompts[1].contains("[A1] 我答应：把合同发给小艾（截止 2026-10-16）"));
        let items = db
            .list_commitments("account", &CommitmentFilter::default())
            .unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].status, "suggested");
        assert_eq!(items[0].ai_status.as_deref(), Some("done"));
        assert_eq!(items[0].followup_local_id, Some(5));

        let again =
            scan_commitments(&db, &client, &NullSink, "account", None, &range, "m").unwrap();
        assert_eq!((again.windows, again.skipped), (0, 0));
        assert_eq!(client.prompts.lock().unwrap().len(), 2);

        insert_text(
            &db,
            "alice",
            7,
            &at(16, 9, 0),
            false,
            "下周一我把发票寄给你",
        );
        let fresh = plan_scan(&db, "account", None, &range).unwrap();
        assert_eq!(fresh[0].windows.len(), 1);
        assert_eq!(fresh[0].windows[0].bounds(), (7, 7));
        assert_eq!(fresh[0].windows[0].context.len(), 6);
    }

    struct CancelAfter {
        statuses: Cell<usize>,
        limit: usize,
    }

    impl Sink for CancelAfter {
        fn delta(&self, _text: &str) -> bool {
            true
        }
        fn status(&self, _text: &str) {
            self.statuses.set(self.statuses.get() + 1);
        }
        fn cancelled(&self) -> bool {
            self.statuses.get() >= self.limit
        }
    }

    #[test]
    fn resumes_after_cancel_and_retries_unreadable_replies() {
        let db = fixture_db();
        seed(&db);
        let client = FakeClient::new(&["这里没有 JSON"]);
        let sink = CancelAfter {
            statuses: Cell::new(0),
            limit: 2,
        };
        let range = DateRange::default();
        let outcome = scan_commitments(&db, &client, &sink, "account", None, &range, "m").unwrap();
        assert!(outcome.cancelled);
        assert_eq!((outcome.windows, outcome.failed), (0, 0));
        assert_eq!(client.prompts.lock().unwrap().len(), 1);

        let outcome =
            scan_commitments(&db, &client, &NullSink, "account", None, &range, "m").unwrap();
        assert!(!outcome.cancelled);
        assert_eq!(
            (outcome.windows, outcome.failed, outcome.skipped),
            (0, 2, 1)
        );

        let outcome =
            scan_commitments(&db, &client, &NullSink, "account", None, &range, "m").unwrap();
        assert_eq!((outcome.failed, outcome.skipped), (2, 0));
        assert_eq!(client.prompts.lock().unwrap().len(), 5);
    }
}
