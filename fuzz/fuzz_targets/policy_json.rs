#![no_main]
use libfuzzer_sys::fuzz_target;

// 任意の入力でパニックせず、Ok ならホストが正規化済みであること。
fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    if let Ok(p) = policy::Policy::from_json(s) {
        for h in &p.blocked_hosts {
            assert!(!h.as_str().is_empty());
            assert_eq!(h.as_str(), h.as_str().to_ascii_lowercase());
        }
    }
});
