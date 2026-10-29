use super::client::LlmClient;
use crate::analysis::query_terms;
use crate::database::Database;
use crate::error::AppResult;
use crate::models::{DateRange, SearchHit};
use serde::Serialize;
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};

const RRF_K: f32 = 60.0;
const KEYWORD_HITS_PER_TERM: i64 = 40;
const CANDIDATES: usize = 30;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskSource {
    pub index: usize,
    pub chat_id: String,
    pub chat_name: String,
    pub chat_kind: String,
    pub local_id: i64,
    pub end_local_id: i64,
    pub start_at: String,
    pub end_at: String,
    pub text: String,
}

pub fn dot(left: &[f32], right: &[f32]) -> f32 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Scored(f32, i64);

impl Eq for Scored {}

impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .total_cmp(&other.0)
            .then_with(|| other.1.cmp(&self.1))
    }
}

/// Keeps the `k` highest scores from a stream, best first.
pub struct TopK {
    limit: usize,
    heap: BinaryHeap<Reverse<Scored>>,
}

impl TopK {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            heap: BinaryHeap::new(),
        }
    }

    pub fn push(&mut self, id: i64, score: f32) {
        if self.limit == 0 || !score.is_finite() {
            return;
        }
        let item = Reverse(Scored(score, id));
        if self.heap.len() < self.limit {
            self.heap.push(item);
        } else if self.heap.peek().is_some_and(|lowest| item < *lowest) {
            self.heap.pop();
            self.heap.push(item);
        }
    }

    pub fn into_sorted(self) -> Vec<(i64, f32)> {
        let mut items: Vec<Scored> = self.heap.into_iter().map(|Reverse(item)| item).collect();
        items.sort_by(|a, b| b.cmp(a));
        items
            .into_iter()
            .map(|Scored(score, id)| (id, score))
            .collect()
    }
}

pub fn vector_search(
    db: &Database,
    account_id: &str,
    model: &str,
    chat_id: Option<&str>,
    range: &DateRange,
    query: &[f32],
    limit: usize,
) -> AppResult<Vec<(i64, f32)>> {
    let mut top = TopK::new(limit);
    db.ai_for_each_vector(account_id, model, chat_id, range, |id, vector| {
        if vector.len() == query.len() {
            top.push(id, dot(&vector, query));
        }
    })?;
    Ok(top.into_sorted())
}

/// Reciprocal rank fusion: items ranked well in any list float to the top.
pub fn rrf<T: Clone + Eq + std::hash::Hash>(lists: &[Vec<T>]) -> Vec<T> {
    let mut scores: HashMap<T, (f32, usize)> = HashMap::new();
    let mut order = 0usize;
    for list in lists {
        for (rank, item) in list.iter().enumerate() {
            let entry = scores.entry(item.clone()).or_insert_with(|| {
                order += 1;
                (0.0, order)
            });
            entry.0 += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
    }
    let mut items: Vec<(T, (f32, usize))> = scores.into_iter().collect();
    items.sort_by(|a, b| b.1 .0.total_cmp(&a.1 .0).then(a.1 .1.cmp(&b.1 .1)));
    items.into_iter().map(|(item, _)| item).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum SourceKey {
    Chunk(i64),
    Message(String, i64),
}

fn keyword_candidates(
    db: &Database,
    account_id: &str,
    question: &str,
    chat_id: Option<&str>,
    range: &DateRange,
) -> AppResult<Vec<SearchHit>> {
    let mut matches: HashMap<(String, i64), (usize, SearchHit)> = HashMap::new();
    for term in query_terms(question, 6) {
        for hit in db.search_messages(account_id, &term, chat_id, range, KEYWORD_HITS_PER_TERM)? {
            matches
                .entry((hit.chat_id.clone(), hit.local_id))
                .or_insert((0, hit))
                .0 += 1;
        }
    }
    let mut ranked: Vec<(usize, SearchHit)> = matches.into_values().collect();
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.1.occurred_at.cmp(&a.1.occurred_at))
    });
    Ok(ranked
        .into_iter()
        .take(CANDIDATES)
        .map(|(_, hit)| hit)
        .collect())
}

/// Finds the conversation snippets most relevant to a question by fusing keyword
/// matches with vector similarity (when an index exists).
pub fn retrieve(
    db: &Database,
    client: &dyn LlmClient,
    account_id: &str,
    question: &str,
    chat_id: Option<&str>,
    range: &DateRange,
    limit: usize,
) -> AppResult<Vec<AskSource>> {
    client.validate_embed_model()?;
    let model = client.embed_model();
    let mut keyword_keys = Vec::new();
    for hit in keyword_candidates(db, account_id, question, chat_id, range)? {
        let key = match db.ai_chunk_containing(account_id, model, &hit.chat_id, hit.local_id)? {
            Some(id) => SourceKey::Chunk(id),
            None => SourceKey::Message(hit.chat_id, hit.local_id),
        };
        if !keyword_keys.contains(&key) {
            keyword_keys.push(key);
        }
    }

    let mut vector_keys = Vec::new();
    if db
        .ai_index_counts(account_id, chat_id, model)?
        .indexed_chunks
        > 0
    {
        let query = client
            .embed(&[question.to_string()])?
            .pop()
            .unwrap_or_default();
        vector_keys = vector_search(db, account_id, model, chat_id, range, &query, CANDIDATES)?
            .into_iter()
            .map(|(id, _)| SourceKey::Chunk(id))
            .collect();
    }

    let keys: Vec<SourceKey> = rrf(&[vector_keys, keyword_keys])
        .into_iter()
        .take(limit)
        .collect();
    let chunk_ids: Vec<i64> = keys
        .iter()
        .filter_map(|key| match key {
            SourceKey::Chunk(id) => Some(*id),
            SourceKey::Message(..) => None,
        })
        .collect();
    let chunks: HashMap<i64, _> = db
        .ai_chunks_by_ids(&chunk_ids)?
        .into_iter()
        .map(|chunk| (chunk.id, chunk))
        .collect();
    let chat_names: HashMap<String, (String, String)> = db
        .list_chats(account_id, "")?
        .into_iter()
        .map(|chat| (chat.id, (chat.display_name, chat.chat_kind)))
        .collect();

    let mut sources = Vec::new();
    for key in keys {
        let index = sources.len() + 1;
        match key {
            SourceKey::Chunk(id) => {
                if let Some(chunk) = chunks.get(&id) {
                    if !occurs_in_range(&chunk.start_at, range)
                        || !occurs_in_range(&chunk.end_at, range)
                    {
                        continue;
                    }
                    sources.push(AskSource {
                        index,
                        chat_id: chunk.chat_id.clone(),
                        chat_name: chunk.chat_name.clone(),
                        chat_kind: chunk.chat_kind.clone(),
                        local_id: chunk.start_local_id,
                        end_local_id: chunk.end_local_id,
                        start_at: chunk.start_at.clone(),
                        end_at: chunk.end_at.clone(),
                        text: chunk.text.clone(),
                    });
                }
            }
            SourceKey::Message(chat, local_id) => {
                let context: Vec<_> = db
                    .message_context(account_id, &chat, local_id, 3)?
                    .into_iter()
                    .filter(|item| occurs_in_range(&item.occurred_at, range))
                    .collect();
                let (Some(first), Some(last)) = (context.first(), context.last()) else {
                    continue;
                };
                let (chat_name, chat_kind) = chat_names.get(&chat).cloned().unwrap_or_default();
                let text = context
                    .iter()
                    .filter(|item| !item.text.trim().is_empty())
                    .map(|item| {
                        let speaker = if item.is_from_me {
                            "我"
                        } else if item.sender_name.is_empty() {
                            "对方"
                        } else {
                            item.sender_name.as_str()
                        };
                        let body: String = item.text.chars().take(300).collect();
                        format!("{speaker}: {}", body.replace('\n', " "))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                sources.push(AskSource {
                    index,
                    chat_id: chat,
                    chat_name,
                    chat_kind,
                    local_id,
                    end_local_id: local_id,
                    start_at: first.occurred_at.clone(),
                    end_at: last.occurred_at.clone(),
                    text,
                });
            }
        }
    }
    Ok(sources)
}

fn occurs_in_range(occurred_at: &str, range: &DateRange) -> bool {
    let date = occurred_at.get(..10).unwrap_or(occurred_at);
    if range.from.as_deref().is_some_and(|from| date < from) {
        return false;
    }
    if range.to.as_deref().is_some_and(|to| date > to) {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_highest_scores_in_order() {
        let mut top = TopK::new(3);
        for (id, score) in [
            (1, 0.1),
            (2, 0.9),
            (3, 0.5),
            (4, 0.7),
            (5, f32::NAN),
            (6, 0.2),
        ] {
            top.push(id, score);
        }
        assert_eq!(
            top.into_sorted()
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            vec![2, 4, 3]
        );
    }

    #[test]
    fn fuses_rankings() {
        let merged = rrf(&[vec!["a", "b", "c"], vec!["c", "d", "a"]]);
        assert_eq!(merged[..2], ["a", "c"]);
        assert_eq!(merged.len(), 4);
    }

    #[test]
    fn dot_product_of_unit_vectors_is_cosine() {
        assert!((dot(&[1.0, 0.0], &[0.6, 0.8]) - 0.6).abs() < 1e-6);
    }
}
