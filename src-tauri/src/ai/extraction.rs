//! Shared conversation-windowing and scan loop used by every memory extractor.

use super::client::{ChatMessage, LlmClient};
use super::indexer::speaker;
use super::store::TextMessage;
use super::Sink;
use crate::database::Database;
use crate::error::AppResult;
use crate::memory::is_covered;
use crate::models::DateRange;
use chrono::{DateTime, Datelike, Duration};
use serde::Serialize;
use serde_json::Value;

const GAP_MINUTES: i64 = 30;
const WINDOW_CHARS: usize = 3000;
const LINE_CHARS: usize = 300;
const CONTEXT_MESSAGES: usize = 6;
pub const OPEN_ITEMS: i64 = 8;
pub const MIN_CONFIDENCE: f64 = 0.3;
pub const DEFAULT_CONFIDENCE: f64 = 0.6;
pub const QUOTE_CHARS: usize = 200;
pub const NOTE_CHARS: usize = 80;
pub const MAX_DUE_DAYS: i64 = 400;
/// Room for a reasoning model to think plus a short JSON answer. A reply cut off here
/// cannot be parsed, so its window is retried on the next scan.
pub const MAX_REPLY_TOKENS: u32 = 4096;

const WEEKDAYS: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

pub fn has_cue(text: &str, cues: &[&str]) -> bool {
    let lower = text.to_lowercase();
    cues.iter().any(|cue| lower.contains(cue))
}

#[derive(Debug, Clone)]
pub struct Window {
    /// Messages right before the window, shown to the model for context only.
    pub context: Vec<TextMessage>,
    pub lines: Vec<TextMessage>,
    /// Whether the window passed the local filter and goes to the model.
    pub send: bool,
}

impl Window {
    pub fn bounds(&self) -> (i64, i64) {
        (
            self.lines.first().map_or(0, |line| line.local_id),
            self.lines.last().map_or(0, |line| line.local_id),
        )
    }

    pub fn chars(&self) -> usize {
        self.context
            .iter()
            .chain(&self.lines)
            .map(|line| line_body(line).chars().count())
            .sum()
    }
}

pub fn line_body(message: &TextMessage) -> String {
    let text: String = message.text.trim().chars().take(LINE_CHARS).collect();
    text.replace('\n', " ")
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

/// Groups not-yet-scanned messages into conversation windows. A scanned message, a quiet
/// gap, or the size limit ends a window.
pub fn build_windows(
    all: &[TextMessage],
    pending: &[bool],
    cues: &[&str],
    group_needs_me: bool,
) -> Vec<Window> {
    let mut windows = Vec::new();
    let mut start: Option<usize> = None;
    let mut chars = 0usize;
    let flush = |windows: &mut Vec<Window>, start: usize, end: usize| {
        let lines = all[start..end].to_vec();
        let send = lines.iter().any(|line| has_cue(&line.text, cues))
            && (!group_needs_me || lines.iter().any(|line| line.is_from_me));
        windows.push(Window {
            context: all[start.saturating_sub(CONTEXT_MESSAGES)..start].to_vec(),
            lines,
            send,
        });
    };
    for (index, message) in all.iter().enumerate() {
        if !pending.get(index).copied().unwrap_or(false) {
            if let Some(first) = start.take() {
                flush(&mut windows, first, index);
            }
            continue;
        }
        let size = line_body(message).chars().count();
        if let Some(first) = start {
            if minutes_between(&all[index - 1].occurred_at, &message.occurred_at) > GAP_MINUTES
                || chars + size > WINDOW_CHARS
            {
                flush(&mut windows, first, index);
                start = None;
            }
        }
        if start.is_none() {
            start = Some(index);
            chars = 0;
        }
        chars += size;
    }
    if let Some(first) = start {
        flush(&mut windows, first, all.len());
    }
    windows
}

#[derive(Debug, Clone)]
pub struct ChatPlan {
    pub chat_id: String,
    pub chat_name: String,
    pub chat_kind: String,
    pub windows: Vec<Window>,
}

pub fn plan_chat(
    db: &Database,
    account_id: &str,
    (chat_id, chat_name, chat_kind): (String, String, String),
    range: &DateRange,
    kind: &str,
    cues: &[&str],
    group_needs_me: bool,
) -> AppResult<ChatPlan> {
    let messages = db.ai_text_messages(account_id, &chat_id, range, None)?;
    let mut windows = Vec::new();
    if let Some(first) = messages.first() {
        let mut all = db.text_messages_before(
            account_id,
            &chat_id,
            first.local_id,
            CONTEXT_MESSAGES as i64,
        )?;
        let intervals = db.scanned_intervals(account_id, &chat_id, kind)?;
        let mut pending = vec![false; all.len()];
        pending.extend(
            messages
                .iter()
                .map(|message| !is_covered(&intervals, message.local_id)),
        );
        all.extend(messages);
        let needs_me = group_needs_me && chat_kind == "group";
        windows = build_windows(&all, &pending, cues, needs_me);
    }
    Ok(ChatPlan {
        chat_id,
        chat_name,
        chat_kind,
        windows,
    })
}

pub fn plan_scan(
    db: &Database,
    account_id: &str,
    chat_id: Option<&str>,
    range: &DateRange,
    kind: &str,
    cues: &[&str],
    group_needs_me: bool,
) -> AppResult<Vec<ChatPlan>> {
    db.chats_with_text(account_id, chat_id, range)?
        .into_iter()
        .map(|chat| plan_chat(db, account_id, chat, range, kind, cues, group_needs_me))
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanEstimate {
    /// Chats with at least one window for the model.
    pub chats: usize,
    pub windows: usize,
    pub skipped: usize,
    pub chars: usize,
}

pub fn estimate(plans: &[ChatPlan]) -> ScanEstimate {
    let mut result = ScanEstimate::default();
    for plan in plans {
        let mut sends = false;
        for window in &plan.windows {
            if window.send {
                sends = true;
                result.windows += 1;
                result.chars += window.chars();
            } else {
                result.skipped += 1;
            }
        }
        result.chats += usize::from(sends);
    }
    result
}

pub fn date_label(message: &TextMessage) -> String {
    match chrono::NaiveDate::parse_from_str(&message.local_date, "%Y-%m-%d") {
        Ok(date) => format!(
            "{}（{}）",
            message.local_date,
            WEEKDAYS[date.weekday().num_days_from_monday() as usize]
        ),
        Err(_) => message.local_date.clone(),
    }
}

pub fn stamp(message: &TextMessage) -> String {
    message
        .occurred_at
        .get(5..16)
        .unwrap_or_default()
        .replace('T', " ")
}

pub fn chat_kind_label(chat_kind: &str) -> &'static str {
    if chat_kind == "group" {
        "群聊"
    } else {
        "好友"
    }
}

/// Appends the read-only context and the numbered window the model may cite.
pub fn append_window(body: &mut String, window: &Window) {
    if !window.context.is_empty() {
        body.push_str("\n前文（仅供参考）：\n");
        for message in &window.context {
            body.push_str(&format!(
                "{} {}: {}\n",
                stamp(message),
                speaker(message),
                line_body(message)
            ));
        }
    }
    body.push_str("\n本段聊天：\n");
    for (index, message) in window.lines.iter().enumerate() {
        body.push_str(&format!(
            "[{}] {} {}: {}\n",
            index + 1,
            stamp(message),
            speaker(message),
            line_body(message)
        ));
    }
}

/// Keeps a model date only when it falls on or after the message and within a year.
pub fn accepted_due_date(raw: Option<&str>, sent_on: &str) -> Option<String> {
    let date = crate::memory::parse_date(raw?)?;
    crate::memory::parse_date(sent_on).and_then(|sent| {
        (date >= sent && date <= sent + Duration::days(MAX_DUE_DAYS))
            .then(|| date.format("%Y-%m-%d").to_string())
    })
}

/// Reads the model's JSON reply, tolerating code fences, prose around it, or a bare
/// array of items. `None` means nothing usable came back.
pub fn parse_reply(reply: &str) -> Option<Value> {
    let slice = |open: char, close: char| -> Option<Value> {
        let (start, end) = (reply.find(open)?, reply.rfind(close)?);
        (start < end)
            .then(|| serde_json::from_str::<Value>(&reply[start..=end]).ok())
            .flatten()
    };
    let array_first = match (reply.find('['), reply.find('{')) {
        (Some(bracket), Some(brace)) => bracket < brace,
        (Some(_), None) => true,
        _ => false,
    };
    let object = || slice('{', '}').filter(Value::is_object);
    let array = || match slice('[', ']') {
        Some(Value::Array(items)) => Some(serde_json::json!({ "items": items })),
        _ => None,
    };
    if array_first {
        array().or_else(object)
    } else {
        object().or_else(array)
    }
}

pub fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

pub fn index(value: Option<&Value>) -> Option<usize> {
    match value? {
        Value::Number(number) => number.as_u64().map(|index| index as usize),
        Value::String(text) => text
            .trim()
            .trim_matches(|c| c == '[' || c == ']')
            .trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .parse()
            .ok(),
        _ => None,
    }
}

pub fn text(value: Option<&Value>, limit: usize) -> Option<String> {
    let text: String = value?.as_str()?.trim().chars().take(limit).collect();
    (!text.is_empty() && text != "null").then_some(text)
}

pub fn items(value: &Value, key: &str) -> Vec<Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// One kind of memory the scan loop can look for.
pub trait Extractor {
    type Open;

    fn kind(&self) -> &'static str;
    fn cues(&self) -> &'static [&'static str];
    /// Group windows without a message from the user are skipped when this is true.
    fn group_needs_me(&self) -> bool;
    fn open_items(
        &self,
        db: &Database,
        account_id: &str,
        chat_id: &str,
        before_local_id: i64,
    ) -> AppResult<Vec<Self::Open>>;
    fn prompt(&self, plan: &ChatPlan, window: &Window, open: &[Self::Open]) -> Vec<ChatMessage>;
    /// Validates the reply and writes it. Returns how many items were created and updated.
    #[allow(clippy::too_many_arguments)]
    fn apply(
        &self,
        db: &Database,
        account_id: &str,
        plan: &ChatPlan,
        window: &Window,
        open: &[Self::Open],
        reply: &Value,
        model: &str,
    ) -> AppResult<(usize, usize)>;
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScanOutcome {
    pub chats: usize,
    /// Windows the model read.
    pub windows: usize,
    /// Windows the local filter passed over.
    pub skipped: usize,
    /// Windows whose reply could not be read; they are retried next time.
    pub failed: usize,
    pub created: usize,
    pub updated: usize,
    pub cancelled: bool,
}

/// Reads chats in scope window by window, oldest first within each chat, so that items
/// found early can be followed up by later windows. Every window is recorded as scanned
/// once handled, so an interrupted scan resumes where it stopped.
#[allow(clippy::too_many_arguments)]
pub fn scan<E: Extractor>(
    extractor: &E,
    db: &Database,
    client: &dyn LlmClient,
    sink: &dyn Sink,
    account_id: &str,
    chat_id: Option<&str>,
    range: &DateRange,
    model: &str,
) -> AppResult<ScanOutcome> {
    sink.status("正在整理要扫描的聊天…");
    let plans = plan_scan(
        db,
        account_id,
        chat_id,
        range,
        extractor.kind(),
        extractor.cues(),
        extractor.group_needs_me(),
    )?;
    let total = estimate(&plans).windows;
    let mut outcome = ScanOutcome::default();
    for plan in &plans {
        let mut read_chat = false;
        for window in &plan.windows {
            if sink.cancelled() {
                outcome.cancelled = true;
                return Ok(outcome);
            }
            let (from, to) = window.bounds();
            if !window.send {
                db.mark_scanned(account_id, &plan.chat_id, extractor.kind(), from, to, model)?;
                outcome.skipped += 1;
                continue;
            }
            sink.status(&format!(
                "正在阅读“{}”（第 {}/{total} 段）…",
                plan.chat_name,
                outcome.windows + outcome.failed + 1
            ));
            let open = extractor.open_items(db, account_id, &plan.chat_id, from)?;
            let prompt = extractor.prompt(plan, window, &open);
            let reply =
                match client.chat_limited(&prompt, MAX_REPLY_TOKENS, &mut |_| !sink.cancelled()) {
                    Ok(reply) => reply,
                    Err(_) if sink.cancelled() => {
                        outcome.cancelled = true;
                        return Ok(outcome);
                    }
                    Err(error) => return Err(error),
                };
            let Some(value) = parse_reply(&reply) else {
                outcome.failed += 1;
                continue;
            };
            let (created, updated) =
                extractor.apply(db, account_id, plan, window, &open, &value, model)?;
            outcome.created += created;
            outcome.updated += updated;
            db.mark_scanned(account_id, &plan.chat_id, extractor.kind(), from, to, model)?;
            outcome.windows += 1;
            read_chat = true;
        }
        outcome.chats += usize::from(read_chat);
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    const CUES: &[&str] = &["周五", "发你", "明天", "我去"];

    #[test]
    fn splits_windows_on_gaps_size_and_scanned_messages() {
        let all = vec![
            message(1, &at(12, 9, 0), false, "合同什么时候给我"),
            message(2, &at(12, 9, 1), true, "周五前我发你"),
            message(3, &at(12, 11, 0), false, "中午吃什么"),
            message(4, &at(12, 11, 1), true, "随便"),
            message(5, &at(12, 11, 2), false, "那就面吧"),
        ];
        let windows = build_windows(&all, &[true; 5], CUES, false);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].bounds(), (1, 2));
        assert!(windows[0].send);
        assert!(!windows[1].send);
        assert_eq!(windows[1].context.len(), 2);

        let windows = build_windows(&all, &[true, true, true, false, true], CUES, false);
        assert_eq!(
            windows.iter().map(Window::bounds).collect::<Vec<_>>(),
            vec![(1, 2), (3, 3), (5, 5)]
        );
        assert_eq!(windows[2].context.len(), 4);

        let long = "很长的一段话".repeat(60);
        let all: Vec<_> = (1..=20)
            .map(|id| message(id, &at(12, 9, id as u32), id % 2 == 0, &long))
            .collect();
        assert!(build_windows(&all, &[true; 20], CUES, false).len() > 1);
    }

    #[test]
    fn group_windows_can_require_my_messages() {
        let all = vec![
            message(1, &at(12, 9, 0), false, "明天谁去拿快递"),
            message(2, &at(12, 9, 1), false, "我去"),
        ];
        assert!(!build_windows(&all, &[true; 2], CUES, true)[0].send);
        assert!(build_windows(&all, &[true; 2], CUES, false)[0].send);
    }

    #[test]
    fn parses_loose_replies() {
        let fenced = "```json\n{\"items\":[{\"line\":1}],\"updates\":[]}\n```";
        assert_eq!(items(&parse_reply(fenced).unwrap(), "items").len(), 1);
        let bare = "结果：[{\"line\":\"2\"}]";
        assert_eq!(items(&parse_reply(bare).unwrap(), "items").len(), 1);
        assert!(parse_reply("没有找到").is_none());
        assert!(parse_reply("{坏的}").is_none());
        assert_eq!(index(Some(&serde_json::json!("D3"))), Some(3));
    }
}
