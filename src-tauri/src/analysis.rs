use crate::models::KeywordCount;
use jieba_rs::Jieba;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

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

/// Words that frame a question rather than describe what to look for.
const QUESTION_WORDS: &[&str] = &[
    "上次",
    "之前",
    "以前",
    "最近",
    "那次",
    "哪个",
    "哪些",
    "哪里",
    "哪天",
    "几号",
    "几月",
    "什么时候",
    "是不是",
    "有没有",
    "有人",
    "谁",
    "聊天",
    "聊过",
    "说过",
    "提到",
    "提过",
    "讨论",
    "记得",
    "告诉",
    "一下",
    "关于",
    "问题",
    "帮我",
    "找到",
    "找找",
    "我们",
    "我和",
    "和我",
];

fn jieba() -> &'static Jieba {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    JIEBA.get_or_init(Jieba::new)
}

/// Content words of a natural-language question, used for keyword recall.
pub fn query_terms(question: &str, limit: usize) -> Vec<String> {
    let stop_words: HashSet<&str> = STOP_WORDS.iter().chain(QUESTION_WORDS).copied().collect();
    let mut terms: Vec<String> = Vec::new();
    for token in jieba().cut(question, false) {
        let word = token.trim().to_lowercase();
        if word.chars().count() < 2
            || stop_words.contains(word.as_str())
            || !word.chars().any(char::is_alphanumeric)
            || terms.contains(&word)
        {
            continue;
        }
        terms.push(word);
        if terms.len() >= limit {
            break;
        }
    }
    terms
}

/// Removes WeChat emoji codes such as `[捂脸]` so they are not counted as words.
pub fn strip_emoji_codes(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('[') {
        output.push_str(&rest[..start]);
        let tail = &rest[start + 1..];
        match tail.find([']', '[']) {
            Some(end)
                if tail.as_bytes()[end] == b']'
                    && (1..=6).contains(&tail[..end].chars().count())
                    && !tail[..end].chars().any(char::is_whitespace) =>
            {
                rest = &tail[end + 1..];
            }
            _ => {
                output.push('[');
                rest = tail;
            }
        }
    }
    output.push_str(rest);
    output
}

pub fn top_keywords<I>(texts: I, limit: usize) -> Vec<KeywordCount>
where
    I: IntoIterator<Item = String>,
{
    let jieba = jieba();
    let stop_words: HashSet<&str> = STOP_WORDS.iter().copied().collect();
    let mut counts: HashMap<String, usize> = HashMap::new();

    for text in texts {
        if text.starts_with('[') && text.ends_with(']') {
            continue;
        }
        let text = strip_emoji_codes(&text);
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

    #[test]
    fn ignores_wechat_emoji_codes() {
        assert_eq!(strip_emoji_codes("哈哈[捂脸]好的[旺柴]"), "哈哈好的");
        assert_eq!(
            strip_emoji_codes("数组 a[0] 和 [很长很长的说明文字] [a b]"),
            "数组 a 和 [很长很长的说明文字] [a b]"
        );
        assert_eq!(strip_emoji_codes("[[微笑]"), "[");
        let keywords = top_keywords(
            vec![
                "今天吃火锅[捂脸]".to_string(),
                "火锅真好吃[捂脸]".to_string(),
            ],
            10,
        );
        assert!(keywords.iter().any(|item| item.word == "火锅"));
        assert!(!keywords.iter().any(|item| item.word == "捂脸"));
    }

    #[test]
    fn extracts_question_terms() {
        let terms = query_terms("我和老王上次说去日本是什么时候？", 6);
        assert!(terms.contains(&"日本".to_string()));
        assert!(!terms.iter().any(|term| term == "上次" || term == "什么"));
    }
}
