#![no_main]
use libfuzzer_sys::fuzz_target;

// Url と判定された入力のスキームは明示許可のものだけ(javascript: / data: 等を通さない)。
fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    if let omnibox::Input::Url(u) = omnibox::classify(s) {
        assert!(
            matches!(u.scheme(), "http" | "https" | "file" | "about"),
            "{s:?} -> {u}"
        );
    }
});
