//! Checks the commitment pipeline against hand-written conversations in
//! `testdata/commitments/cases.json`. The ignored test measures a real model:
//!
//! ```sh
//! CHAT_INSIGHT_EVAL_MODEL=qwen3:8b cargo test --manifest-path src-tauri/Cargo.toml \
//!   extraction_eval -- --ignored --nocapture
//! ```
//!
//! `CHAT_INSIGHT_EVAL_BASE_URL` and `CHAT_INSIGHT_EVAL_API_KEY` point it at another
//! OpenAI-compatible service; `CHAT_INSIGHT_EVAL_MIN_PRECISION` and
//! `CHAT_INSIGHT_EVAL_MIN_RECALL` turn the report into assertions.

use super::client::fake::FakeClient;
use super::client::OpenAiCompatClient;
use super::commitments::{plan_scan, scan_commitments};
use super::settings::{AiSettings, DEFAULT_BASE_URL};
use super::NullSink;
use crate::commitments::CommitmentFilter;
use crate::database::Database;
use crate::models::{DateRange, NormalizedMessage};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use tempfile::tempdir;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    chat_kind: String,
    chat_name: String,
    messages: Vec<CaseMessage>,
    expected: Vec<Expected>,
}

#[derive(Deserialize)]
struct CaseMessage {
    at: String,
    me: bool,
    #[serde(default)]
    sender: Option<String>,
    text: String,
}

#[derive(Deserialize)]
struct Expected {
    line: i64,
    /// Commitments store a direction here; decision cases reuse the field via `kind`.
    #[serde(alias = "kind")]
    direction: String,
}

type Key = (String, i64, String);

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!("../../testdata/commitments/cases.json")).unwrap()
}

fn decision_cases() -> Vec<Case> {
    serde_json::from_str(include_str!("../../testdata/decisions/cases.json")).unwrap()
}

fn chat_id(index: usize) -> String {
    format!("case{}", index + 1)
}

fn load(cases: &[Case]) -> Database {
    let directory = tempdir().unwrap().keep();
    let db = Database::new(directory.join("eval.db"));
    db.initialize().unwrap();
    db.upsert_account("account", "评估", "fixture").unwrap();
    for (index, case) in cases.iter().enumerate() {
        let chat = chat_id(index);
        db.upsert_chat("account", &chat, &case.chat_name, &case.chat_kind)
            .unwrap();
        for (position, message) in case.messages.iter().enumerate() {
            let sender = if message.me {
                "我".to_string()
            } else {
                message
                    .sender
                    .clone()
                    .unwrap_or_else(|| case.chat_name.clone())
            };
            db.insert_message(
                "account",
                &NormalizedMessage {
                    chat_id: chat.clone(),
                    local_id: position as i64 + 1,
                    server_id: None,
                    occurred_at: message.at.clone(),
                    local_date: message.at[..10].to_string(),
                    local_hour: message.at[11..13].parse().unwrap(),
                    local_weekday: 0,
                    sender_id: sender.clone(),
                    sender_name: sender,
                    is_from_me: message.me,
                    kind: "text".into(),
                    text: message.text.clone(),
                },
            )
            .unwrap();
        }
    }
    db
}

fn expected(cases: &[Case]) -> HashSet<Key> {
    cases
        .iter()
        .enumerate()
        .flat_map(|(index, case)| {
            case.expected
                .iter()
                .map(move |item| (chat_id(index), item.line, item.direction.clone()))
        })
        .collect()
}

fn found(db: &Database) -> HashSet<Key> {
    db.list_commitments("account", &CommitmentFilter::default())
        .unwrap()
        .into_iter()
        .map(|item| (item.chat_id, item.source_local_id, item.direction))
        .collect()
}

#[test]
fn local_filter_keeps_every_expected_commitment() {
    let cases = cases();
    let db = load(&cases);
    let plans = plan_scan(&db, "account", None, &DateRange::default()).unwrap();
    for (chat, local_id, _) in expected(&cases) {
        let window = plans
            .iter()
            .filter(|plan| plan.chat_id == chat)
            .flat_map(|plan| &plan.windows)
            .find(|window| window.lines.iter().any(|line| line.local_id == local_id))
            .unwrap_or_else(|| panic!("{chat}#{local_id} is not in any window"));
        assert!(window.send, "{chat}#{local_id} was filtered out");
    }
}

#[test]
fn correct_answers_survive_validation() {
    let cases = cases();
    let db = load(&cases);
    let wanted = expected(&cases);
    let plans = plan_scan(&db, "account", None, &DateRange::default()).unwrap();
    let mut replies = Vec::new();
    for plan in &plans {
        for window in plan.windows.iter().filter(|window| window.send) {
            let items: Vec<_> = window
                .lines
                .iter()
                .enumerate()
                .flat_map(|(position, line)| {
                    wanted
                        .iter()
                        .filter(|(chat, local_id, _)| *chat == plan.chat_id && *local_id == line.local_id)
                        .map(move |(_, _, direction)| {
                            json!({"line": position + 1, "direction": direction, "title": format!("约定{}", line.local_id), "confidence": 0.9})
                        })
                })
                .collect();
            replies.push(json!({ "items": items, "updates": [] }).to_string());
        }
    }
    let refs: Vec<&str> = replies.iter().map(String::as_str).collect();
    let client = FakeClient::new(&refs);
    let outcome = scan_commitments(
        &db,
        &client,
        &NullSink,
        "account",
        None,
        &DateRange::default(),
        "fake",
    )
    .unwrap();
    assert_eq!(outcome.failed, 0);
    assert_eq!(found(&db), wanted);
}

#[test]
#[ignore = "needs a running model service; see the module docs"]
fn measures_precision_and_recall_with_a_real_model() {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let settings = AiSettings {
        enabled: true,
        base_url: env("CHAT_INSIGHT_EVAL_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into()),
        chat_model: env("CHAT_INSIGHT_EVAL_MODEL").unwrap_or_else(|| "qwen3:8b".into()),
        ..AiSettings::default()
    };
    let client = OpenAiCompatClient::new(&settings, env("CHAT_INSIGHT_EVAL_API_KEY"));
    let cases = cases();
    let db = load(&cases);
    let outcome = scan_commitments(
        &db,
        &client,
        &NullSink,
        "account",
        None,
        &DateRange::default(),
        &settings.chat_model,
    )
    .unwrap();
    let wanted = expected(&cases);
    let got = found(&db);
    let hits = got.intersection(&wanted).count();
    for (index, case) in cases.iter().enumerate() {
        let chat = chat_id(index);
        let missed: Vec<_> = wanted
            .difference(&got)
            .filter(|key| key.0 == chat)
            .collect();
        let extra: Vec<_> = got
            .difference(&wanted)
            .filter(|key| key.0 == chat)
            .collect();
        if !missed.is_empty() || !extra.is_empty() {
            println!("{}：漏掉 {:?}，多出 {:?}", case.name, missed, extra);
        }
    }
    let precision = if got.is_empty() {
        1.0
    } else {
        hits as f64 / got.len() as f64
    };
    let recall = if wanted.is_empty() {
        1.0
    } else {
        hits as f64 / wanted.len() as f64
    };
    println!(
        "模型 {}：精确率 {:.0}%（{hits}/{}），召回率 {:.0}%（{hits}/{}），读取 {} 段，跳过 {} 段，无法识别 {} 段",
        settings.chat_model,
        precision * 100.0,
        got.len(),
        recall * 100.0,
        wanted.len(),
        outcome.windows,
        outcome.skipped,
        outcome.failed
    );
    if let Some(minimum) =
        env("CHAT_INSIGHT_EVAL_MIN_PRECISION").and_then(|value| value.parse::<f64>().ok())
    {
        assert!(precision >= minimum, "precision {precision:.2} < {minimum}");
    }
    if let Some(minimum) =
        env("CHAT_INSIGHT_EVAL_MIN_RECALL").and_then(|value| value.parse::<f64>().ok())
    {
        assert!(recall >= minimum, "recall {recall:.2} < {minimum}");
    }
}

fn decision_found(db: &Database) -> HashSet<Key> {
    db.list_decisions("account", &crate::decisions::DecisionFilter::default())
        .unwrap()
        .into_iter()
        .map(|item| (item.chat_id, item.source_local_id, item.kind))
        .collect()
}

fn assert_filter_keeps_expected(cases: &[Case], cues: &[&str], group_needs_me: bool) {
    let db = load(cases);
    let plans = super::extraction::plan_scan(
        &db,
        "account",
        None,
        &DateRange::default(),
        "decision",
        cues,
        group_needs_me,
    )
    .unwrap();
    for (chat, local_id, _) in expected(cases) {
        let window = plans
            .iter()
            .filter(|plan| plan.chat_id == chat)
            .flat_map(|plan| &plan.windows)
            .find(|window| window.lines.iter().any(|line| line.local_id == local_id))
            .unwrap_or_else(|| panic!("{chat}#{local_id} is not in any window"));
        assert!(window.send, "{chat}#{local_id} was filtered out");
    }
}

#[test]
fn local_filter_keeps_every_expected_decision() {
    assert_filter_keeps_expected(&decision_cases(), super::decisions::CUES, false);
}

#[test]
fn decision_answers_survive_validation() {
    let cases = decision_cases();
    let db = load(&cases);
    let wanted = expected(&cases);
    let plans = super::extraction::plan_scan(
        &db,
        "account",
        None,
        &DateRange::default(),
        "decision",
        super::decisions::CUES,
        false,
    )
    .unwrap();
    for plan in &plans {
        let mut replies = Vec::new();
        for window in plan.windows.iter().filter(|window| window.send) {
            let items: Vec<_> = window
                .lines
                .iter()
                .enumerate()
                .flat_map(|(position, line)| {
                    wanted
                        .iter()
                        .filter(|(chat, local_id, _)| *chat == plan.chat_id && *local_id == line.local_id)
                        .map(move |(_, _, kind)| {
                            json!({"line": position + 1, "kind": kind, "title": format!("决策{}", line.local_id), "confidence": 0.9})
                        })
                })
                .collect();
            replies.push(json!({ "items": items, "updates": [] }).to_string());
        }
        let refs: Vec<&str> = replies.iter().map(String::as_str).collect();
        let client = FakeClient::new(&refs);
        super::decisions::scan_decisions(
            &db,
            &client,
            &NullSink,
            "account",
            &plan.chat_id,
            &DateRange::default(),
            "fake",
        )
        .unwrap();
    }
    assert_eq!(decision_found(&db), wanted);
}

#[test]
#[ignore = "needs a running model service; see the module docs"]
fn measures_decision_precision_and_recall_with_a_real_model() {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let settings = AiSettings {
        enabled: true,
        base_url: env("CHAT_INSIGHT_EVAL_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.into()),
        chat_model: env("CHAT_INSIGHT_EVAL_MODEL").unwrap_or_else(|| "qwen3:8b".into()),
        ..AiSettings::default()
    };
    let client = OpenAiCompatClient::new(&settings, env("CHAT_INSIGHT_EVAL_API_KEY"));
    let cases = decision_cases();
    let db = load(&cases);
    let mut windows = 0;
    let mut skipped = 0;
    let mut failed = 0;
    for (index, _) in cases.iter().enumerate() {
        let outcome = super::decisions::scan_decisions(
            &db,
            &client,
            &NullSink,
            "account",
            &chat_id(index),
            &DateRange::default(),
            &settings.chat_model,
        )
        .unwrap();
        windows += outcome.windows;
        skipped += outcome.skipped;
        failed += outcome.failed;
    }
    let wanted = expected(&cases);
    let got = decision_found(&db);
    let hits = got.intersection(&wanted).count();
    for (index, case) in cases.iter().enumerate() {
        let chat = chat_id(index);
        let missed: Vec<_> = wanted
            .difference(&got)
            .filter(|key| key.0 == chat)
            .collect();
        let extra: Vec<_> = got
            .difference(&wanted)
            .filter(|key| key.0 == chat)
            .collect();
        if !missed.is_empty() || !extra.is_empty() {
            println!("{}：漏掉 {:?}，多出 {:?}", case.name, missed, extra);
        }
    }
    let precision = if got.is_empty() {
        1.0
    } else {
        hits as f64 / got.len() as f64
    };
    let recall = if wanted.is_empty() {
        1.0
    } else {
        hits as f64 / wanted.len() as f64
    };
    println!(
        "决策 · 模型 {}：精确率 {:.0}%（{hits}/{}），召回率 {:.0}%（{hits}/{}），读取 {windows} 段，跳过 {skipped} 段，无法识别 {failed} 段",
        settings.chat_model,
        precision * 100.0,
        got.len(),
        recall * 100.0,
        wanted.len()
    );
}
