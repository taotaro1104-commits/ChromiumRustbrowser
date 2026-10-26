#![no_main]
use libfuzzer_sys::fuzz_target;

// 任意の URL 文字列 × ブロックホストで decide がパニックせず、ブロック対象は Allow されない。
fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    let Some((url, host)) = s.split_once('\n') else { return };
    let Ok(url) = url::Url::parse(url) else { return };
    let json = serde_host(host);
    let Ok(p) = policy::Policy::from_json(&json) else { return };
    let req = net_policy::Request { url: &url, top_frame: None };
    let d = net_policy::decide(&req, &p, &net_policy::NoopBlocker);
    if let Some(h) = url.host_str() {
        let h = h.trim_end_matches('.').to_ascii_lowercase();
        if matches!(url.scheme(), "http" | "https" | "ws" | "wss" | "ftp") && p.is_host_blocked(&h) {
            assert!(matches!(d, net_policy::Decision::Block(_)));
        }
    }
});

fn serde_host(h: &str) -> String {
    let esc: String = h.chars().filter(|c| !c.is_control() && !matches!(c, '"' | '\\')).collect();
    format!("{{\"blocked_hosts\": [\"{esc}\"]}}")
}
