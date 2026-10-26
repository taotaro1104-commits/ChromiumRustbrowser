//! ダウンロードの保存先決定。ページ側が示すファイル名は信頼せず、安全なファイル名と未使用のパスにする。
//! 実行形式など危険な種類は保存しない(ユーザーの確認 UI を作るまでの保守的な方針)。

use std::path::{Path, PathBuf};

/// ファイル名の最大長(バイトではなく文字数)。拡張子は残す。
const MAX_NAME_CHARS: usize = 120;
/// 同名ファイルがあるときに試す連番の上限。
const MAX_SUFFIX: u32 = 999;

/// 保存しない拡張子(小文字、ドットなし)。起動・スクリプト実行・ショートカット等を伴うもの。
const BLOCKED_EXTENSIONS: &[&str] = &[
    "exe",
    "msi",
    "bat",
    "cmd",
    "com",
    "scr",
    "pif",
    "ps1",
    "psm1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "hta",
    "jar",
    "lnk",
    "url",
    "reg",
    "cpl",
    "msc",
    "dll",
    "sys",
    "appx",
    "msix",
    "application",
    "gadget",
];

/// Windows が予約しているデバイス名。
const RESERVED_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// ページが示したファイル名から、保存に使える名前を作る。空になる場合は `download`。
pub fn sanitize_file_name(suggested: &str) -> String {
    // パス区切り以降だけを使う(`..\..\evil.txt` → `evil.txt`)。
    let base = suggested.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        .collect();
    // 末尾のドットと空白は Windows で無視されるため、拡張子の偽装に使われる(`a.exe.`)。
    let cleaned = cleaned
        .trim()
        .trim_end_matches(['.', ' '])
        .trim_start_matches('.');
    let (stem, ext) = match cleaned.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s, Some(e)),
        _ => (cleaned, None),
    };
    let mut stem: String = stem.trim().chars().take(MAX_NAME_CHARS).collect();
    if stem.is_empty() {
        stem = "download".into();
    }
    if RESERVED_NAMES.contains(&stem.to_ascii_lowercase().as_str()) {
        stem.insert(0, '_');
    }
    match ext {
        Some(e) if !e.is_empty() => format!("{stem}.{}", e.chars().take(16).collect::<String>()),
        _ => stem,
    }
}

/// 保存しない種類か。名前のどの位置の拡張子でも見る(`a.exe.txt` は安全だが `a.txt.exe` は拒否)。
pub fn is_blocked(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower
        .rsplit_once('.')
        .is_some_and(|(_, ext)| BLOCKED_EXTENSIONS.contains(&ext))
}

/// `dir` 内で未使用のパス。同名があれば `name (1).ext` のように連番を付ける。
/// 上限まで埋まっていれば `None`。`exists` は存在確認(テストで差し替える)。
pub fn unique_path(dir: &Path, file_name: &str, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let first = dir.join(file_name);
    if !exists(&first) {
        return Some(first);
    }
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((s, e)) => (s, format!(".{e}")),
        None => (file_name, String::new()),
    };
    (1..=MAX_SUFFIX)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !exists(p))
}

/// 保存先のフォルダ(ユーザーの「ダウンロード」)。
pub fn downloads_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join("Downloads"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_parts_and_forbidden_characters_are_removed() {
        assert_eq!(sanitize_file_name("..\\..\\evil.txt"), "evil.txt");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_file_name("a<b>:c\"d|e?f*.txt"), "abcdef.txt");
        assert_eq!(sanitize_file_name("re\u{0}port\n.pdf"), "report.pdf");
    }

    #[test]
    fn empty_hidden_and_reserved_names_are_fixed() {
        assert_eq!(sanitize_file_name(""), "download");
        assert_eq!(sanitize_file_name("///"), "download");
        assert_eq!(sanitize_file_name(".htaccess"), "htaccess");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("nul"), "_nul");
    }

    #[test]
    fn trailing_dots_cannot_hide_an_extension() {
        assert_eq!(sanitize_file_name("setup.exe. "), "setup.exe");
        assert!(is_blocked(&sanitize_file_name("setup.exe...")));
    }

    #[test]
    fn long_names_are_shortened_but_keep_the_extension() {
        let name = sanitize_file_name(&format!("{}.pdf", "あ".repeat(500)));
        assert_eq!(name.chars().count(), MAX_NAME_CHARS + ".pdf".len());
        assert!(name.ends_with(".pdf"));
    }

    #[test]
    fn executables_are_blocked_by_the_last_extension_only() {
        for bad in [
            "a.exe",
            "A.EXE",
            "x.tar.msi",
            "run.PS1",
            "photo.txt.exe",
            "a.lnk",
        ] {
            assert!(is_blocked(bad), "{bad}");
        }
        for ok in [
            "a.pdf",
            "a.exe.txt",
            "noext",
            "archive.zip",
            "page.html",
            "script.json",
        ] {
            assert!(!is_blocked(ok), "{ok}");
        }
    }

    #[test]
    fn unique_path_adds_a_counter_before_the_extension() {
        let dir = Path::new("D:/dl");
        let taken = [dir.join("a.pdf"), dir.join("a (1).pdf"), dir.join("b")];
        let exists = |p: &Path| taken.iter().any(|t| t == p);
        assert_eq!(unique_path(dir, "c.pdf", exists), Some(dir.join("c.pdf")));
        assert_eq!(
            unique_path(dir, "a.pdf", exists),
            Some(dir.join("a (2).pdf"))
        );
        assert_eq!(unique_path(dir, "b", exists), Some(dir.join("b (1)")));
    }

    #[test]
    fn unique_path_gives_up_when_every_slot_is_taken() {
        assert_eq!(unique_path(Path::new("D:/dl"), "a.pdf", |_| true), None);
    }
}
