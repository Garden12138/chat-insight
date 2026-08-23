use crate::models::KeywordCount;
use jieba_rs::Jieba;
use std::collections::{HashMap, HashSet};

const STOP_WORDS: &[&str] = &[
    "一个",
    "一些",
    "这个",
    "那个",
    "这样",
    "那样",
    "我们",
    "你们",
    "他们",
    "自己",
    "什么",
    "怎么",
    "可以",
    "不是",
    "就是",
    "没有",
    "已经",
    "还是",
    "然后",
    "但是",
    "因为",
    "所以",
    "如果",
    "的话",
    "一下",
    "现在",
    "今天",
    "明天",
    "昨天",
    "时候",
    "知道",
    "觉得",
    "应该",
    "可能",
    "需要",
    "真的",
    "好的",
    "嗯嗯",
    "哈哈",
    "哈哈哈",
    "哈哈哈哈",
    "收到",
    "谢谢",
    "不客气",
    "没事",
    "这里",
    "那里",
    "and",
    "the",
    "that",
    "this",
    "with",
    "from",
    "have",
    "your",
    "you",
    "are",
    "for",
    "not",
];

pub fn top_keywords<I>(texts: I, limit: usize) -> Vec<KeywordCount>
where
    I: IntoIterator<Item = String>,
{
    let jieba = Jieba::new();
    let stop_words: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for text in texts {
        if text.starts_with('[') && text.ends_with(']') {
            continue;
        }
        for token in jieba.cut(&text, false) {
            let word = token.trim().to_lowercase();
            if word.chars().count() < 2
                || stop_words.contains(word.as_str())
                || !word.chars().any(char::is_alphanumeric)
                || word.chars().all(char::is_numeric)
            {
                continue;
            }
            *counts.entry(word).or_default() += 1;
        }
    }

    let mut result: Vec<_> = counts
        .into_iter()
        .map(|(word, count)| KeywordCount { word, count })
        .collect();
    result.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.word.cmp(&b.word)));
    result.truncate(limit);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_chinese_keywords_and_filters_noise() {
        let keywords = top_keywords(
            vec![
                "周末一起去公园拍照，公园天气很好".to_string(),
                "好的，周末公园见".to_string(),
                "[图片]".to_string(),
            ],
            5,
        );
        assert!(keywords
            .iter()
            .any(|item| item.word == "公园" && item.count >= 2));
        assert!(!keywords.iter().any(|item| item.word == "好的"));
    }
}
