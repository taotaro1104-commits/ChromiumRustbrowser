//! 起動時に開く URL の取り込み。`--url=URL` と `--urls-file=PATH`(1 行 1 URL)。
//! 計測や検証で、決まった状態(タブ数・ページ)から起動するために使う。
//! 取り込んだ URL は通常の入力と同じく、スキーム・ポリシーの検査を通ってから開かれる。

use std::path::Path;

/// 起動時に開くタブ数の上限(コマンドライン経由の暴走を防ぐ)。
pub const MAX_STARTUP_URLS: usize = 50;

/// 1 行 1 URL のテキストを解析する。空行と `#` で始まる行は無視し、上限を超えた分は捨てる。
pub fn parse_urls(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .take(MAX_STARTUP_URLS)
        .map(str::to_owned)
        .collect()
}

/// `--url` と `--urls-file` の内容をまとめる。`--url` が先頭になる。
pub fn collect(single: &str, file: &str) -> Vec<String> {
    let mut urls: Vec<String> = Vec::new();
    if !single.trim().is_empty() {
        urls.push(single.trim().to_owned());
    }
    if !file.trim().is_empty() {
        match std::fs::read_to_string(Path::new(file.trim())) {
            Ok(text) => urls.extend(parse_urls(&text)),
            Err(e) => eprintln!("browser: cannot read --urls-file {file:?}: {e}"),
        }
    }
    urls.truncate(MAX_STARTUP_URLS);
    urls
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_lines_and_comments_are_skipped() {
        let text = "# list\n\nhttps://a.example/\n  https://b.example/  \n#https://c.example/\r\n";
        assert_eq!(
            parse_urls(text),
            ["https://a.example/", "https://b.example/"]
        );
    }

    #[test]
    fn count_is_capped() {
        let text: String = (0..MAX_STARTUP_URLS + 10)
            .map(|i| format!("https://a.example/{i}\n"))
            .collect();
        assert_eq!(parse_urls(&text).len(), MAX_STARTUP_URLS);
    }

    #[test]
    fn single_url_comes_first_and_missing_file_is_ignored() {
        assert_eq!(collect("https://a.example/", ""), ["https://a.example/"]);
        assert_eq!(
            collect("https://a.example/", "/definitely/not/here.txt"),
            ["https://a.example/"]
        );
        assert!(collect("", "").is_empty());
    }

    #[test]
    fn file_urls_follow_the_single_url_and_total_is_capped() {
        let dir = std::env::temp_dir().join(format!("rb-startup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("urls.txt");
        let body: String = (0..MAX_STARTUP_URLS)
            .map(|i| format!("https://f.example/{i}\n"))
            .collect();
        std::fs::write(&path, body).unwrap();
        let urls = collect("https://first.example/", path.to_str().unwrap());
        assert_eq!(urls[0], "https://first.example/");
        assert_eq!(urls.len(), MAX_STARTUP_URLS);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
