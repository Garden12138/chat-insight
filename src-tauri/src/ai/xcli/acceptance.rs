use super::XcliClient;
use crate::ai::client::{AiClient, OpenAiCompatClient};
use crate::ai::events;
use crate::ai::features;
use crate::ai::settings::AiSettings;
use crate::ai::NullSink;
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::models::{DateRange, NormalizedMessage};
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const ACCOUNT: &str = "xcli-fixture";
const CHAT: &str = "fixture-group";
const DATE: &str = "2026-11-18";

fn seed(db: &Database) -> AppResult<()> {
    db.initialize()?;
    db.upsert_account(ACCOUNT, "合成测试账号", "fixture")?;
    db.upsert_chat(ACCOUNT, CHAT, "合成验收群", "group")?;
    for (position, text) in [
        "火星温室演练定在周五上午九点，大家在模拟基地集合。",
        "确认，我负责温室传感器，演练前准备备用电池。",
        "收到，训练结束后请把模拟设备归还到仓库。",
    ]
    .iter()
    .enumerate()
    {
        db.insert_message(
            ACCOUNT,
            &NormalizedMessage {
                chat_id: CHAT.into(),
                local_id: position as i64 + 1,
                server_id: None,
                occurred_at: format!("{DATE}T09:0{position}:00+08:00"),
                local_date: DATE.into(),
                local_hour: 9,
                local_weekday: 2,
                sender_id: format!("fixture-{position}"),
                sender_name: format!("合成成员{}", position + 1),
                is_from_me: position == 0,
                kind: "text".into(),
                text: (*text).into(),
            },
        )?;
    }
    Ok(())
}

fn task_ids(root: &Path) -> BTreeSet<String> {
    fs::read_dir(root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|record| fs::read_dir(record.path().join("xcli/tasks")).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|task| task.file_name().into_string().ok())
        .collect()
}

fn run_case(root: &Path, name: &str, handler: impl FnOnce() -> AppResult<bool>) -> AppResult<()> {
    let before = task_ids(root);
    let started = Instant::now();
    let outcome = handler();
    let after = task_ids(root);
    let ids = after
        .difference(&before)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let passed = matches!(outcome, Ok(true)) && ids.len() == 1;
    println!(
        "{name}: {}, tasks={}, elapsed_ms={}",
        if passed { "passed" } else { "failed" },
        ids.join(","),
        started.elapsed().as_millis()
    );
    if passed {
        Ok(())
    } else {
        outcome?;
        Err(AppError::Ai(format!("真实验收未通过：{name}")))
    }
}

fn local_vectors() -> Option<OpenAiCompatClient> {
    let ollama = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(2))
        .timeout_read(Duration::from_secs(2))
        .build();
    let tags: serde_json::Value = serde_json::from_str(
        &ollama
            .get("http://127.0.0.1:11434/api/tags")
            .call()
            .ok()?
            .into_string()
            .ok()?,
    )
    .ok()?;
    let found = tags["models"]
        .as_array()?
        .iter()
        .any(|model| matches!(model["name"].as_str(), Some("bge-m3" | "bge-m3:latest")));
    found.then(|| OpenAiCompatClient::new(&AiSettings::default(), None))
}

#[test]
#[ignore = "uses the existing Codex login and consumes model quota with synthetic fixture data"]
fn real_xcli_features_on_fixture_data() -> AppResult<()> {
    let home = PathBuf::from(env::var_os("HOME").expect("user home"));
    let records = env::var_os("XCLI_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".xcli"))
        .join("chat-insight");
    let vectors = local_vectors();
    println!(
        "vector_service={}",
        if vectors.is_some() {
            "ollama:bge-m3"
        } else {
            "none"
        }
    );
    let xcli = XcliClient::new(Ok(home.join(".local/bin/xcli")), "auto", vectors);
    let agents = xcli.agents()?;
    let available = agents
        .iter()
        .any(|agent| agent.name == "codex" && agent.available);
    println!("isolated_codex_available={available}");
    if !available {
        return Err(AppError::Ai("隔离环境中的 Codex 登录检查未通过".into()));
    }
    let client = AiClient::Xcli(xcli);
    let fixture = tempfile::tempdir()?;
    let db = Database::new(fixture.path().join("fixture.db"));
    seed(&db)?;
    let date = DateRange {
        from: Some(DATE.into()),
        to: Some(DATE.into()),
    };

    run_case(&records, "summary", || {
        let messages = db.ai_text_messages(ACCOUNT, CHAT, &date, None)?;
        let text = features::summarize_chat(&client, &NullSink, "合成验收群", &messages)?;
        Ok(!text.trim().is_empty())
    })?;
    run_case(&records, "group_event_review", || {
        let review =
            events::review_group_events(&db, &client, &NullSink, ACCOUNT, DATE, Some(CHAT))?;
        Ok(events::should_cache(&review)
            && review.failed_segments == 0
            && !review.events.is_empty())
    })?;
    run_case(&records, "keyword_question", || {
        let answer = features::ask(
            &db,
            &client,
            &NullSink,
            ACCOUNT,
            "火星温室演练约在什么时间？",
            Some(CHAT),
            &date,
            DATE,
        )?;
        Ok(!answer.answer.trim().is_empty() && !answer.sources.is_empty())
    })?;
    run_case(&records, "year_report", || {
        let insights = db.year_insights(ACCOUNT, Some("2026"))?;
        let text = features::year_report(&client, &NullSink, &insights, true)?;
        Ok(!text.trim().is_empty())
    })?;
    Ok(())
}
