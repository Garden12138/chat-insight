use super::client::{cancelled, ChatMessage, LlmClient};
use super::dates::{format_ymd, intersect_range, parse_exact_date};
use super::indexer::speaker;
use super::retrieval::{dot, retrieve, AskSource};
use super::store::TextMessage;
use super::Sink;
use crate::analysis::top_keywords;
use crate::database::Database;
use crate::error::{AppError, AppResult};
use crate::insights::YearInsights;
use crate::models::DateRange;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const ASK_SOURCES: usize = 12;
const SEGMENT_CHARS: usize = 6000;
const TOPIC_SAMPLES: usize = 5;
const MIN_TOPIC_CHUNKS: usize = 6;

fn stream_reply(
    client: &dyn LlmClient,
    sink: &dyn Sink,
    messages: &[ChatMessage],
) -> AppResult<String> {
    client.chat(messages, &mut |delta| sink.delta(delta))
}

fn quiet_reply(
    client: &dyn LlmClient,
    sink: &dyn Sink,
    messages: &[ChatMessage],
) -> AppResult<String> {
    client.chat(messages, &mut |_| !sink.cancelled())
}

fn short_time(occurred_at: &str) -> String {
    occurred_at
        .get(..16)
        .unwrap_or(occurred_at)
        .replace('T', " ")
}

// ---------------------------------------------------------------- Ask

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskResult {
    pub answer: String,
    pub sources: Vec<AskSource>,
}

pub const NOTHING_FOUND: &str = "没有在聊天记录中找到和这个问题相关的内容。可以换个说法、调整日期范围，或先在“数据管理 → AI 设置”中建立语义索引。";
pub const DATE_RANGE_EMPTY: &str = "问题中的日期与顶部日期范围没有交集，没有可检索的聊天。";

pub fn ask_prompt(question: &str, sources: &[AskSource], today: &str) -> Vec<ChatMessage> {
    let context = sources
        .iter()
        .map(|source| {
            format!(
                "[{}] 聊天：{}（{}） 时间：{} 至 {}\n{}",
                source.index,
                source.chat_name,
                if source.chat_kind == "group" {
                    "群聊"
                } else {
                    "好友"
                },
                short_time(&source.start_at),
                short_time(&source.end_at),
                source.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    vec![
        ChatMessage::system(format!(
            "你是 Chat Insight 的聊天记录助手。今天是 {today}。只根据用户提供的聊天片段回答问题，使用简体中文。\
             片段里的“我”指用户本人。在用到某个片段的句子末尾标注它的编号，例如 [2]，可以同时标注多个。\
             如果片段里没有答案，就直接说没有找到，不要猜测或编造。回答简洁，必要时用短列表。"
        )),
        ChatMessage::user(format!("聊天片段：\n\n{context}\n\n问题：{question}")),
    ]
}

#[allow(clippy::too_many_arguments)]
pub fn ask(
    db: &Database,
    client: &dyn LlmClient,
    sink: &dyn Sink,
    account_id: &str,
    question: &str,
    chat_id: Option<&str>,
    range: &DateRange,
    today: &str,
) -> AppResult<AskResult> {
    let question = question.trim();
    if question.is_empty() {
        return Err(AppError::Validation("请输入问题".into()));
    }
    sink.status("正在检索相关聊天…");
    let search_range = match NaiveDate::parse_from_str(today, "%Y-%m-%d")
        .ok()
        .and_then(|today| parse_exact_date(question, today))
    {
        Some(date) => match intersect_range(range, &format_ymd(date)) {
            Some(narrowed) => narrowed,
            None => {
                sink.delta(DATE_RANGE_EMPTY);
                return Ok(AskResult {
                    answer: DATE_RANGE_EMPTY.into(),
                    sources: Vec::new(),
                });
            }
        },
        None => range.clone(),
    };
    let sources = retrieve(
        db,
        client,
        account_id,
        question,
        chat_id,
        &search_range,
        ASK_SOURCES,
    )?;
    if sources.is_empty() {
        sink.delta(NOTHING_FOUND);
        return Ok(AskResult {
            answer: NOTHING_FOUND.into(),
            sources,
        });
    }
    sink.status("正在生成回答…");
    let answer = stream_reply(client, sink, &ask_prompt(question, &sources, today))?;
    Ok(AskResult { answer, sources })
}

// ---------------------------------------------------------------- Summary

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryResult {
    pub text: String,
    pub message_count: usize,
    pub cached: bool,
    pub generated_at: Option<String>,
}

pub fn segment_lines(messages: &[TextMessage], limit: usize) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current = String::new();
    for message in messages {
        let body: String = message.text.trim().chars().take(800).collect();
        let line = format!(
            "{} {}: {}\n",
            short_time(&message.occurred_at)
                .get(5..)
                .unwrap_or_default(),
            speaker(message),
            body.replace('\n', " ")
        );
        if !current.is_empty() && current.chars().count() + line.chars().count() > limit {
            segments.push(std::mem::take(&mut current));
        }
        current.push_str(&line);
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
}

fn group_notes(notes: &[String], limit: usize) -> Vec<String> {
    let mut groups = Vec::new();
    let mut current = String::new();
    for note in notes {
        if !current.is_empty() && current.chars().count() + note.chars().count() > limit {
            groups.push(std::mem::take(&mut current));
        }
        current.push_str(note);
        current.push_str("\n\n");
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

const SUMMARY_FORMAT: &str = "输出格式：先用一两句话总体概括这段时间的聊天，然后用“### 主要话题”和“### 约定与待办”两个小标题分别列出要点（每条以“- ”开头；某部分没有内容就省略）。使用简体中文，不要编造聊天里没有的内容。";

/// Summarizes a chat. Long histories are summarized segment by segment, then the
/// segment notes are merged (repeatedly, if needed) into the final answer.
pub fn summarize_chat(
    client: &dyn LlmClient,
    sink: &dyn Sink,
    chat_name: &str,
    messages: &[TextMessage],
) -> AppResult<String> {
    if messages.is_empty() {
        return Err(AppError::Ai("当前日期范围内没有可以总结的文本消息".into()));
    }
    let period = format!(
        "{} 至 {}",
        short_time(&messages[0].occurred_at)
            .get(..10)
            .unwrap_or_default(),
        short_time(&messages[messages.len() - 1].occurred_at)
            .get(..10)
            .unwrap_or_default()
    );
    let segments = segment_lines(messages, SEGMENT_CHARS);
    let intro = format!("下面是微信聊天“{chat_name}”在 {period} 的记录，“我”指用户本人。");
    if segments.len() == 1 {
        sink.status("正在生成摘要…");
        return stream_reply(
            client,
            sink,
            &[
                ChatMessage::system(format!("你负责总结聊天记录。{SUMMARY_FORMAT}")),
                ChatMessage::user(format!("{intro}\n\n{}", segments[0])),
            ],
        );
    }

    let mut notes = Vec::with_capacity(segments.len());
    for (position, segment) in segments.iter().enumerate() {
        if sink.cancelled() {
            return Err(cancelled());
        }
        sink.status(&format!(
            "正在阅读第 {}/{} 段聊天…",
            position + 1,
            segments.len()
        ));
        notes.push(quiet_reply(
            client,
            sink,
            &[
                ChatMessage::system(
                    "你负责整理聊天记录。用 3 到 6 条要点概括这段聊天：主要话题、做出的决定或约定、提到的待办。每条以“- ”开头，只输出要点。",
                ),
                ChatMessage::user(format!("{intro}这是其中第 {} 段：\n\n{segment}", position + 1)),
            ],
        )?);
    }
    while notes.iter().map(|note| note.chars().count()).sum::<usize>() > SEGMENT_CHARS {
        let groups = group_notes(&notes, SEGMENT_CHARS);
        if groups.len() >= notes.len() {
            break;
        }
        sink.status("正在合并要点…");
        notes = groups
            .iter()
            .map(|group| {
                quiet_reply(
                    client,
                    sink,
                    &[
                        ChatMessage::system("把下面按时间顺序排列的聊天要点合并精简成不超过 8 条要点，每条以“- ”开头，只输出要点。"),
                        ChatMessage::user(group.clone()),
                    ],
                )
            })
            .collect::<AppResult<Vec<_>>>()?;
    }
    sink.status("正在生成摘要…");
    stream_reply(
        client,
        sink,
        &[
            ChatMessage::system(format!(
                "你负责把分段整理的聊天要点合并成一份完整摘要。{SUMMARY_FORMAT}"
            )),
            ChatMessage::user(format!(
                "{intro}以下是按时间顺序分段整理的要点：\n\n{}",
                notes.join("\n\n")
            )),
        ],
    )
}

// ---------------------------------------------------------------- Topics

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    pub name: String,
    pub description: String,
    pub share: f64,
    pub chunk_count: usize,
    pub first_at: String,
    pub last_at: String,
    pub sample: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicsResult {
    pub topics: Vec<Topic>,
    pub chunk_count: usize,
    pub cached: bool,
    pub generated_at: Option<String>,
}

struct Lcg(u64);

impl Lcg {
    fn next_f32(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32) / ((1u64 << 24) as f32)
    }
}

fn normalized_mean(vectors: &[&Vec<f32>], dimension: usize) -> Option<Vec<f32>> {
    if vectors.is_empty() {
        return None;
    }
    let mut sum = vec![0.0f32; dimension];
    for vector in vectors {
        for (total, value) in sum.iter_mut().zip(vector.iter()) {
            *total += value;
        }
    }
    Some(super::client::normalize(sum))
}

/// Spherical k-means with k-means++ seeding; deterministic for a given seed.
pub fn kmeans(
    vectors: &[Vec<f32>],
    k: usize,
    iterations: usize,
    seed: u64,
) -> (Vec<usize>, Vec<Vec<f32>>) {
    let k = k.min(vectors.len()).max(1);
    let dimension = vectors.first().map(Vec::len).unwrap_or(0);
    let mut random = Lcg(seed);
    let mut centroids: Vec<Vec<f32>> = vec![vectors[0].clone()];
    while centroids.len() < k {
        let distances: Vec<f32> = vectors
            .iter()
            .map(|vector| {
                centroids
                    .iter()
                    .map(|centroid| (1.0 - dot(vector, centroid)).max(0.0))
                    .fold(f32::INFINITY, f32::min)
                    .powi(2)
            })
            .collect();
        let total: f32 = distances.iter().sum();
        if total <= f32::EPSILON {
            break;
        }
        let mut target = random.next_f32() * total;
        let mut chosen = vectors.len() - 1;
        for (index, distance) in distances.iter().enumerate() {
            target -= distance;
            if target <= 0.0 {
                chosen = index;
                break;
            }
        }
        centroids.push(vectors[chosen].clone());
    }

    let nearest = |vector: &Vec<f32>, centroids: &[Vec<f32>]| {
        centroids
            .iter()
            .enumerate()
            .map(|(index, centroid)| (index, dot(vector, centroid)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| index)
            .unwrap_or(0)
    };
    let mut assignments: Vec<usize> = vectors
        .iter()
        .map(|vector| nearest(vector, &centroids))
        .collect();
    for _ in 0..iterations {
        for (cluster, centroid) in centroids.iter_mut().enumerate() {
            let members: Vec<&Vec<f32>> = vectors
                .iter()
                .zip(&assignments)
                .filter(|(_, assigned)| **assigned == cluster)
                .map(|(vector, _)| vector)
                .collect();
            if let Some(mean) = normalized_mean(&members, dimension) {
                *centroid = mean;
            }
        }
        let next: Vec<usize> = vectors
            .iter()
            .map(|vector| nearest(vector, &centroids))
            .collect();
        if next == assignments {
            break;
        }
        assignments = next;
    }
    (assignments, centroids)
}

pub fn topic_count(chunks: usize) -> usize {
    (((chunks as f64) / 3.0).sqrt().round() as usize)
        .clamp(2, 8)
        .min(chunks / 3)
        .max(1)
}

#[derive(Deserialize)]
struct TopicLabel {
    id: usize,
    name: String,
    #[serde(default)]
    description: String,
}

fn parse_labels(reply: &str) -> Vec<TopicLabel> {
    let (Some(start), Some(end)) = (reply.find('['), reply.rfind(']')) else {
        return Vec::new();
    };
    if end <= start {
        return Vec::new();
    }
    serde_json::from_str(&reply[start..=end]).unwrap_or_default()
}

pub fn chat_topics(
    db: &Database,
    client: &dyn LlmClient,
    sink: &dyn Sink,
    account_id: &str,
    chat_id: &str,
    range: &DateRange,
) -> AppResult<TopicsResult> {
    let mut ids = Vec::new();
    let mut vectors = Vec::new();
    db.ai_for_each_vector(
        account_id,
        client.embed_model(),
        Some(chat_id),
        range,
        |id, vector| {
            ids.push(id);
            vectors.push(vector);
        },
    )?;
    if vectors.len() < MIN_TOPIC_CHUNKS {
        return Err(AppError::Ai(
            "当前日期范围内的聊天太少，暂时无法归纳话题".into(),
        ));
    }
    sink.status("正在聚类聊天片段…");
    let (assignments, centroids) = kmeans(&vectors, topic_count(vectors.len()), 30, 42);
    let chunks = db.ai_chunks_by_ids(&ids)?;

    struct Cluster {
        members: Vec<usize>,
        samples: Vec<usize>,
    }
    let mut clusters: Vec<Cluster> = centroids
        .iter()
        .enumerate()
        .map(|(cluster, centroid)| {
            let members: Vec<usize> = (0..ids.len())
                .filter(|index| assignments[*index] == cluster)
                .collect();
            let mut ranked = members.clone();
            ranked.sort_by(|a, b| {
                dot(&vectors[*b], centroid).total_cmp(&dot(&vectors[*a], centroid))
            });
            ranked.truncate(TOPIC_SAMPLES);
            Cluster {
                members,
                samples: ranked,
            }
        })
        .filter(|cluster| !cluster.members.is_empty())
        .collect();
    clusters.sort_by_key(|cluster| std::cmp::Reverse(cluster.members.len()));

    let sample_text = |index: usize| -> String {
        chunks
            .get(index)
            .map(|chunk| chunk.text.chars().take(300).collect())
            .unwrap_or_default()
    };
    let groups = clusters
        .iter()
        .enumerate()
        .map(|(position, cluster)| {
            let samples = cluster
                .samples
                .iter()
                .map(|index| sample_text(*index))
                .collect::<Vec<_>>()
                .join("\n---\n");
            format!(
                "## 第 {} 组（{} 段）\n{samples}",
                position + 1,
                cluster.members.len()
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    sink.status("正在为话题命名…");
    let reply = quiet_reply(
        client,
        sink,
        &[
            ChatMessage::system(
                "下面是同一个微信聊天中按语义聚类得到的几组聊天片段。为每一组起一个 2 到 8 个字的话题名，并写一句不超过 30 字的说明。\
                 只输出 JSON 数组，例如 [{\"id\":1,\"name\":\"周末出游\",\"description\":\"约时间地点、分享照片\"}]，不要输出其他内容。",
            ),
            ChatMessage::user(groups),
        ],
    )?;
    let labels = parse_labels(&reply);

    let total = ids.len();
    let topics = clusters
        .iter()
        .enumerate()
        .map(|(position, cluster)| {
            let label = labels.iter().find(|label| label.id == position + 1);
            let name = label
                .map(|label| label.name.trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| {
                    let texts = cluster.members.iter().map(|index| sample_text(*index));
                    top_keywords(texts, 3)
                        .into_iter()
                        .map(|keyword| keyword.word)
                        .collect::<Vec<_>>()
                        .join("、")
                });
            let times: Vec<&str> = cluster
                .members
                .iter()
                .filter_map(|index| chunks.get(*index))
                .flat_map(|chunk| [chunk.start_at.as_str(), chunk.end_at.as_str()])
                .collect();
            Topic {
                name: if name.is_empty() {
                    format!("话题 {}", position + 1)
                } else {
                    name
                },
                description: label
                    .map(|label| label.description.trim().to_string())
                    .unwrap_or_default(),
                share: cluster.members.len() as f64 / total as f64,
                chunk_count: cluster.members.len(),
                first_at: times
                    .iter()
                    .min()
                    .map(|time| time.to_string())
                    .unwrap_or_default(),
                last_at: times
                    .iter()
                    .max()
                    .map(|time| time.to_string())
                    .unwrap_or_default(),
                sample: cluster
                    .samples
                    .first()
                    .map(|index| sample_text(*index).chars().take(120).collect())
                    .unwrap_or_default(),
            }
        })
        .collect();
    Ok(TopicsResult {
        topics,
        chunk_count: total,
        cached: false,
        generated_at: None,
    })
}

// ---------------------------------------------------------------- Year report

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportResult {
    pub text: String,
    pub anonymized: bool,
    pub cached: bool,
    pub generated_at: Option<String>,
}

/// Replaces contact names with stable aliases; returns the alias table for restoring them.
pub fn alias_names(insights: &YearInsights) -> Vec<(String, String)> {
    let mut aliases: Vec<(String, String)> = Vec::new();
    let mut direct = 0;
    let mut groups = 0;
    let chats = insights
        .top_chats
        .iter()
        .map(|chat| (&chat.display_name, chat.chat_kind == "group"))
        .chain(
            insights
                .relationships
                .iter()
                .map(|chat| (&chat.display_name, chat.chat_kind == "group")),
        )
        .chain(
            insights
                .cooling
                .iter()
                .map(|chat| (&chat.display_name, chat.chat_kind == "group")),
        )
        .chain(
            insights
                .latest_night
                .iter()
                .map(|late| (&late.chat_name, false)),
        );
    for (name, is_group) in chats {
        if name.is_empty() || aliases.iter().any(|(real, _)| real == name) {
            continue;
        }
        let alias = if is_group {
            groups += 1;
            format!("群聊{groups}号")
        } else {
            direct += 1;
            format!("联系人{direct}号")
        };
        aliases.push((name.clone(), alias));
    }
    aliases
}

fn restore_names(text: &str, aliases: &[(String, String)]) -> String {
    aliases
        .iter()
        .rev()
        .fold(text.to_string(), |text, (real, alias)| {
            text.replace(alias, real)
        })
}

pub fn report_payload(insights: &YearInsights, rename: &dyn Fn(&str) -> String) -> Value {
    let percent = |value: Option<f64>| value.map(|value| format!("{:.0}%", value * 100.0));
    let minutes = |value: Option<f64>| value.map(|value| format!("{value:.0} 分钟"));
    json!({
        "年份": insights.year,
        "消息总数": insights.total_messages,
        "我发出的消息": insights.sent_messages,
        "去年消息总数": insights.previous_total,
        "有聊天的天数": insights.active_days,
        "聊过天的对象数": insights.chat_count,
        "最长连续聊天天数": insights.longest_streak.as_ref().map(|streak| json!({"天数": streak.days, "开始": streak.start, "结束": streak.end})),
        "消息最多的一天": insights.busiest_day.as_ref().map(|day| json!({"日期": day.bucket, "消息数": day.count})),
        "最活跃的月份": insights.busiest_month.as_ref().map(|month| json!({"月份": month.bucket, "消息数": month.count})),
        "最活跃的时段": insights.busiest_hour.as_ref().map(|hour| format!("{}:00-{}:59", hour.hour, hour.hour)),
        "最晚的一次发消息": insights.latest_night.as_ref().map(|late| json!({"时间": short_time(&late.occurred_at), "聊天": rename(&late.chat_name)})),
        "每月消息数": insights.monthly.iter().map(|month| json!([month.bucket, month.count])).collect::<Vec<_>>(),
        "聊得最多的对象": insights.top_chats.iter().map(|chat| json!({"名称": rename(&chat.display_name), "类型": if chat.chat_kind == "group" { "群聊" } else { "好友" }, "消息数": chat.count})).collect::<Vec<_>>(),
        "重要关系": insights.relationships.iter().map(|chat| json!({
            "名称": rename(&chat.display_name),
            "今年消息数": chat.count,
            "去年消息数": chat.previous_count,
            "变化": match chat.trend.as_str() { "warming" => "升温", "cooling" => "降温", "new" => "今年新认识或新开始聊", _ => "平稳" },
            "由我主动开启对话的比例": percent(chat.initiated_by_me),
            "我的回复时间中位数": minutes(chat.my_reply_minutes),
            "对方的回复时间中位数": minutes(chat.their_reply_minutes),
        })).collect::<Vec<_>>(),
        "明显变少的聊天": insights.cooling.iter().map(|chat| json!({"名称": rename(&chat.display_name), "今年": chat.count, "去年": chat.previous_count})).collect::<Vec<_>>(),
    })
}

/// Writes the yearly review from aggregate numbers only; no message text is sent.
pub fn year_report(
    client: &dyn LlmClient,
    sink: &dyn Sink,
    insights: &YearInsights,
    anonymize: bool,
) -> AppResult<String> {
    if insights.total_messages == 0 {
        return Err(AppError::Ai("这一年还没有聊天记录".into()));
    }
    let aliases = if anonymize {
        alias_names(insights)
    } else {
        Vec::new()
    };
    let rename = |name: &str| {
        aliases
            .iter()
            .find(|(real, _)| real == name)
            .map(|(_, alias)| alias.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let payload = serde_json::to_string_pretty(&report_payload(insights, &rename))?;
    sink.status("正在撰写年度报告…");
    let text = stream_reply(
        client,
        sink,
        &[
            ChatMessage::system(
                "你是一位温暖、细腻的写作者，要根据用户一年的微信聊天统计数据写一份年度聊天回顾。用第二人称“你”，使用简体中文和 Markdown。\
                 结构：开头一段年度概览；然后用“### ”小标题依次写“你的聊天节奏”“重要的人”“关系的变化”，最后用一句话作为结尾寄语。\
                 总长度 400 到 700 字。只能使用提供的数据，可以做合理的解读，但不要编造具体事件或聊天内容。名称保持原样。",
            ),
            ChatMessage::user(format!("统计数据：\n{payload}")),
        ],
    )?;
    Ok(restore_names(&text, &aliases))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::client::fake::{fake_vector, FakeClient};
    use crate::ai::indexer::build_index;
    use crate::ai::NullSink;
    use crate::models::NormalizedMessage;
    use std::sync::atomic::AtomicBool;
    use tempfile::tempdir;

    fn seeded_db(texts: &[&str]) -> Database {
        let directory = tempdir().unwrap().keep();
        let db = Database::new(directory.join("test.db"));
        db.initialize().unwrap();
        db.upsert_account("account", "测试", "fixture").unwrap();
        db.upsert_chat("account", "alice", "小艾", "direct")
            .unwrap();
        for (index, text) in texts.iter().enumerate() {
            let day = 1 + index % 28;
            db.insert_message(
                "account",
                &NormalizedMessage {
                    chat_id: "alice".into(),
                    local_id: index as i64 + 1,
                    server_id: None,
                    occurred_at: format!("2026-08-{day:02}T{:02}:00:00+08:00", 8 + index / 28),
                    local_date: format!("2026-08-{day:02}"),
                    local_hour: 9,
                    local_weekday: 0,
                    sender_id: String::new(),
                    sender_name: "小艾".into(),
                    is_from_me: index % 2 == 0,
                    kind: "text".into(),
                    text: text.to_string(),
                },
            )
            .unwrap();
        }
        db
    }

    #[test]
    fn answers_with_cited_sources() {
        let db = seeded_db(&[
            "周末一起去公园拍照",
            "下周去日本旅游，机票订好了",
            "晚上吃火锅",
        ]);
        let client = FakeClient::new(&["你们计划下周去日本 [1]"]);
        let result = ask(
            &db,
            &client,
            &NullSink,
            "account",
            "我们什么时候去日本？",
            None,
            &DateRange::default(),
            "2026-10-09",
        )
        .unwrap();
        assert_eq!(result.answer, "你们计划下周去日本 [1]");
        assert_eq!(result.sources[0].index, 1);
        assert!(result.sources[0].text.contains("日本"));
        let prompt = client.prompts.lock().unwrap().join("\n");
        assert!(prompt.contains("[1] 聊天：小艾（好友）"));
        assert!(prompt.contains("问题：我们什么时候去日本？"));
    }

    #[test]
    fn says_so_when_nothing_matches() {
        let db = seeded_db(&["周末一起去公园拍照"]);
        let client = FakeClient::new(&[]);
        let result = ask(
            &db,
            &client,
            &NullSink,
            "account",
            "量子计算",
            None,
            &DateRange::default(),
            "2026-10-09",
        )
        .unwrap();
        assert_eq!(result.answer, NOTHING_FOUND);
        assert!(client.prompts.lock().unwrap().is_empty());
    }

    #[test]
    fn exact_date_is_a_hard_filter_for_ordinary_ask() {
        let db = seeded_db(&["周末一起去公园拍照", "下周去日本旅游，机票订好了"]);
        let client = FakeClient::new(&["当天只提到了公园 [1]"]);
        let result = ask(
            &db,
            &client,
            &NullSink,
            "account",
            "2026年8月1日公园是谁提的",
            None,
            &DateRange {
                from: Some("2026-08-01".into()),
                to: Some("2026-08-31".into()),
            },
            "2026-10-29",
        )
        .unwrap();
        assert_eq!(result.sources.len(), 1);
        assert!(result.sources[0].text.contains("公园"));
        assert!(!result.sources[0].text.contains("日本"));
    }

    #[test]
    fn empty_date_intersection_does_not_fall_back() {
        let db = seeded_db(&["周末一起去公园拍照"]);
        let client = FakeClient::new(&["不应该被调用"]);
        let result = ask(
            &db,
            &client,
            &NullSink,
            "account",
            "2026-10-27谁去了公园",
            None,
            &DateRange {
                from: Some("2026-01-01".into()),
                to: Some("2026-01-31".into()),
            },
            "2026-10-29",
        )
        .unwrap();
        assert_eq!(result.answer, DATE_RANGE_EMPTY);
        assert!(result.sources.is_empty());
        assert!(client.prompts.lock().unwrap().is_empty());
    }

    #[test]
    fn yearless_date_does_not_constrain_ask() {
        let db = seeded_db(&["周末一起去公园拍照", "下周去日本旅游，机票订好了"]);
        let client = FakeClient::new(&["提到了日本 [1]"]);
        let result = ask(
            &db,
            &client,
            &NullSink,
            "account",
            "8月2日我们什么时候去日本？",
            None,
            &DateRange::default(),
            "2026-10-29",
        )
        .unwrap();
        assert!(result
            .sources
            .iter()
            .any(|source| source.text.contains("日本")));
    }

    #[test]
    fn summarizes_long_chats_in_segments() {
        let long = "这是一段比较长的聊天内容，用来测试分段摘要。".repeat(20);
        let texts: Vec<&str> = (0..40).map(|_| long.as_str()).collect();
        let db = seeded_db(&texts);
        let messages = db
            .ai_text_messages("account", "alice", &DateRange::default(), None)
            .unwrap();
        let segments = segment_lines(&messages, SEGMENT_CHARS);
        assert!(segments.len() > 1);
        let client = FakeClient::new(&[]);
        let summary = summarize_chat(&client, &NullSink, "小艾", &messages).unwrap();
        assert_eq!(summary, "好的");
        assert_eq!(client.prompts.lock().unwrap().len(), segments.len() + 1);
    }

    #[test]
    fn kmeans_separates_clear_groups() {
        let mut vectors = Vec::new();
        for index in 0..6 {
            vectors.push(fake_vector(&format!("aaaa{}", index % 2)));
            vectors.push(fake_vector(&format!("zzzz{}", index % 2)));
        }
        let (first, _) = kmeans(&vectors, 2, 20, 7);
        let (second, _) = kmeans(&vectors, 2, 20, 7);
        assert_eq!(first, second);
        assert_ne!(first[0], first[1]);
        assert!(first.iter().step_by(2).all(|cluster| *cluster == first[0]));
        assert_eq!(topic_count(6), 2);
        assert_eq!(topic_count(300), 8);
    }

    #[test]
    fn names_topics_from_model_reply_or_keywords() {
        let texts: Vec<String> = (0..24)
            .map(|index| {
                if index % 2 == 0 {
                    format!("周末去公园拍照{index}")
                } else {
                    format!("项目方案明天评审{index}")
                }
            })
            .collect();
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let db = seeded_db(&refs);
        let client = FakeClient::new(&[
            "```json\n[{\"id\":1,\"name\":\"周末出游\",\"description\":\"约拍照\"}]\n```",
        ]);
        build_index(
            &db,
            &client,
            "account",
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let result = chat_topics(
            &db,
            &client,
            &NullSink,
            "account",
            "alice",
            &DateRange::default(),
        )
        .unwrap();
        assert!(result.topics.len() >= 2);
        assert_eq!(result.topics[0].name, "周末出游");
        assert!(!result.topics[1].name.is_empty());
        let total: f64 = result.topics.iter().map(|topic| topic.share).sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn report_hides_names_from_model_and_restores_them() {
        let db = seeded_db(&["你好", "早上好"]);
        let insights = db.year_insights("account", Some("2026")).unwrap();
        let client = FakeClient::new(&["今年你和联系人1号聊得最多。"]);
        let text = year_report(&client, &NullSink, &insights, true).unwrap();
        assert_eq!(text, "今年你和小艾聊得最多。");
        let prompt = client.prompts.lock().unwrap().join("\n");
        assert!(!prompt.contains("小艾"));
        assert!(prompt.contains("联系人1号"));
    }
}
