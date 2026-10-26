//! タブ列の折り返し規則。タブが増えたらラベルを縮め、行に分けて全タブを操作可能に保つ。

/// タブ数に応じた (ラベル最大文字数, 1 行あたりのタブ数)。
pub fn tab_layout(count: usize) -> (usize, usize) {
    match count {
        0..=3 => (22, 4),
        4..=6 => (14, 5),
        _ => (8, 6),
    }
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
