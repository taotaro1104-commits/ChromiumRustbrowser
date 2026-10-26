//! タブ列の折り返し規則。タブが増えたらラベルを縮め、行に分けて全タブを操作可能に保つ。

/// タブ数に応じた (ラベル最大文字数, 1 行あたりのタブ数)。
pub fn tab_layout(count: usize) -> (usize, usize) {
    match count {
        0..=3 => (22, 4),
        4..=6 => (14, 5),
        _ => (8, 6),
    }
}

/// 表示幅(半角=1、全角=2)。タブ名の切り詰めを、日本語でも欧文でも同じ見た目の幅にするため。
fn display_width(c: char) -> usize {
    let wide = matches!(c as u32,
        0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF |
        0xFE30..=0xFE6F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF);
    if wide { 2 } else { 1 }
}

/// 表示幅 `max_width` に収まるよう末尾を `…` で切り詰める。
pub fn clip_by_width(text: &str, max_width: usize) -> String {
    let mut used = 0;
    let mut out = String::new();
    for c in text.chars() {
        used += display_width(c);
        if used > max_width {
            out.push('…');
            return out;
        }
        out.push(c);
    }
    out
}

const DOTS: [&str; 6] = ["🟦", "🟧", "🟩", "🟪", "🟥", "🟨"];

/// サイトごとに決まった色の丸。同じホストは常に同じ色になり、タブを見分けやすくする。
pub fn tab_dot(url: &str) -> &'static str {
    let host = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    let sum = host.bytes().fold(0usize, |a, b| {
        a.wrapping_mul(31).wrapping_add(usize::from(b))
    });
    DOTS[sum % DOTS.len()]
}

/// タブを行ごとに分ける。各行は `per_row` 個まで。
pub fn rows<T>(items: &[T], per_row: usize) -> impl Iterator<Item = &[T]> {
    items.chunks(per_row.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_shrink_as_tabs_grow() {
        assert_eq!(tab_layout(1), (22, 4));
        assert_eq!(tab_layout(5), (14, 5));
        assert_eq!(tab_layout(20), (8, 6));
    }

    #[test]
    fn clipping_counts_full_width_characters_as_two() {
        assert_eq!(clip_by_width("abcdef", 4), "abcd…");
        assert_eq!(clip_by_width("abcd", 4), "abcd");
        assert_eq!(clip_by_width("はじまりのページ", 8), "はじまり…");
        assert_eq!(clip_by_width("はじ", 4), "はじ");
        assert_eq!(clip_by_width("", 4), "");
    }

    #[test]
    fn dot_is_stable_per_host() {
        assert_eq!(
            tab_dot("https://a.example/x"),
            tab_dot("http://a.example/y?z")
        );
        assert!(DOTS.contains(&tab_dot("")));
    }

    #[test]
    fn every_tab_lands_in_exactly_one_row() {
        let items: Vec<usize> = (0..20).collect();
        let (_, per_row) = tab_layout(items.len());
        let flat: Vec<usize> = rows(&items, per_row).flatten().copied().collect();
        assert_eq!(flat, items);
        assert!(rows(&items, per_row).all(|r| r.len() <= per_row));
        assert_eq!(rows(&items, 0).count(), 20);
    }
}
