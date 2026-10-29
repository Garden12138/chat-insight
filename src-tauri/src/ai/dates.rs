use crate::models::DateRange;
use chrono::{Duration, NaiveDate};

fn ymd(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, day)
}

fn parse_iso_dates(text: &str) -> Vec<(usize, NaiveDate)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut index = 0;
    while index + 10 <= bytes.len() {
        if bytes[index].is_ascii_digit()
            && bytes[index + 1].is_ascii_digit()
            && bytes[index + 2].is_ascii_digit()
            && bytes[index + 3].is_ascii_digit()
            && bytes[index + 4] == b'-'
            && bytes[index + 5].is_ascii_digit()
            && bytes[index + 6].is_ascii_digit()
            && bytes[index + 7] == b'-'
            && bytes[index + 8].is_ascii_digit()
            && bytes[index + 9].is_ascii_digit()
        {
            if let Ok(slice) = std::str::from_utf8(&bytes[index..index + 10]) {
                if let Ok(date) = NaiveDate::parse_from_str(slice, "%Y-%m-%d") {
                    found.push((index, date));
                    index += 10;
                    continue;
                }
            }
        }
        index += 1;
    }
    found
}

fn parse_cn_dates(text: &str) -> Vec<(usize, NaiveDate)> {
    let mut found = Vec::new();
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find('年') {
        let year_end = search_from + offset;
        let year_start = year_end.saturating_sub(4);
        if let Some(year) = text
            .get(year_start..year_end)
            .and_then(|value| value.parse().ok())
        {
            let after_year = year_end + '年'.len_utf8();
            if let Some(month_offset) = text[after_year..].find('月') {
                let month_end = after_year + month_offset;
                if let Some(day_offset) = text[month_end + '月'.len_utf8()..].find('日') {
                    let day_start = month_end + '月'.len_utf8();
                    let day_end = day_start + day_offset;
                    let month = text[after_year..month_end].parse().ok();
                    let day = text[day_start..day_end].parse().ok();
                    if let (Some(month), Some(day)) = (month, day) {
                        if let Some(date) = ymd(year, month, day) {
                            found.push((year_start, date));
                        }
                    }
                }
            }
        }
        search_from = year_end + '年'.len_utf8();
    }
    found
}

fn first_explicit_date(text: &str) -> Option<NaiveDate> {
    let mut matches = parse_iso_dates(text);
    matches.extend(parse_cn_dates(text));
    matches.sort_by_key(|(index, _)| *index);
    matches.into_iter().next().map(|(_, date)| date)
}

fn relative_date(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    if text.contains("前天") {
        Some(today - Duration::days(2))
    } else if text.contains("昨天") {
        Some(today - Duration::days(1))
    } else if text.contains("今天") {
        Some(today)
    } else {
        None
    }
}

/// Parses one exact local date. Yearless dates and fuzzy ranges are ignored.
pub fn parse_exact_date(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    first_explicit_date(text).or_else(|| relative_date(text, today))
}

pub fn format_ymd(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Narrows `range` to a single day. `None` means the day is outside the range.
pub fn intersect_range(range: &DateRange, date: &str) -> Option<DateRange> {
    if range.from.as_deref().is_some_and(|from| date < from) {
        return None;
    }
    if range.to.as_deref().is_some_and(|to| date > to) {
        return None;
    }
    Some(DateRange {
        from: Some(date.to_string()),
        to: Some(date.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn parses_iso_and_chinese_dates() {
        let today = day("2026-10-29");
        assert_eq!(
            parse_exact_date("请看 2026-10-27 的记录", today),
            Some(day("2026-10-27"))
        );
        assert_eq!(
            parse_exact_date("2026年10月27日发生了什么", today),
            Some(day("2026-10-27"))
        );
        assert_eq!(
            parse_exact_date("2026年1月5日", today),
            Some(day("2026-01-05"))
        );
    }

    #[test]
    fn parses_relative_dates_from_today() {
        let today = day("2026-10-29");
        assert_eq!(parse_exact_date("今天发生了什么", today), Some(today));
        assert_eq!(
            parse_exact_date("昨天的群聊", today),
            Some(day("2026-10-28"))
        );
        assert_eq!(
            parse_exact_date("前天有哪些事情", today),
            Some(day("2026-10-27"))
        );
    }

    #[test]
    fn prefers_explicit_date_over_relative_word() {
        let today = day("2026-10-29");
        assert_eq!(
            parse_exact_date("今天先看 2026-10-01 的记录", today),
            Some(day("2026-10-01"))
        );
    }

    #[test]
    fn rejects_yearless_fuzzy_and_impossible_dates() {
        let today = day("2026-10-29");
        assert_eq!(parse_exact_date("10月27日发生了什么", today), None);
        assert_eq!(parse_exact_date("最近发生了什么", today), None);
        assert_eq!(parse_exact_date("上周的群聊", today), None);
        assert_eq!(parse_exact_date("2026-13-01", today), None);
        assert_eq!(parse_exact_date("2025年2月29日", today), None);
        assert_eq!(parse_exact_date("2026年2月30日", today), None);
        assert_eq!(
            parse_exact_date("2024年2月29日", today),
            Some(day("2024-02-29"))
        );
    }

    #[test]
    fn intersects_top_range_or_returns_empty() {
        let covering = DateRange {
            from: Some("2026-01-01".into()),
            to: Some("2026-10-31".into()),
        };
        let winter = DateRange {
            from: Some("2026-01-01".into()),
            to: Some("2026-01-31".into()),
        };
        assert_eq!(
            intersect_range(&covering, "2026-10-27").unwrap().from,
            Some("2026-10-27".into())
        );
        assert!(intersect_range(&winter, "2026-10-27").is_none());
        assert!(intersect_range(&DateRange::default(), "2026-10-27").is_some());
    }
}
