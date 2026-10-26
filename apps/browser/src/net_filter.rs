//! リソース要求(画像・スクリプト・iframe など)の検査。CEF のリソース要求ハンドラは IO スレッドから呼ばれるため、
//! UI スレッド専用の `Ctx` には触れず、起動時に作った読み取り専用の設定(ポリシーとブロッカー)だけを使う。

use std::sync::{Arc, OnceLock};

use net_policy::{BlockReason, ContentBlocker, Decision, Request, decide};
use policy::Policy;
use url::Url;

struct NetFilter {
    policy: Policy,
    blocker: Arc<dyn ContentBlocker + Send + Sync>,
}

static FILTER: OnceLock<NetFilter> = OnceLock::new();

/// 起動時に一度だけ設定する。2 回目以降は無視される。
pub fn install(policy: Policy, blocker: Arc<dyn ContentBlocker + Send + Sync>) {
    let _ = FILTER.set(NetFilter { policy, blocker });
}

/// このリソース要求をブロックするか。`initiator` は要求元のオリジン(第三者判定に使う)。
/// 設定前・URL が解析できない場合はブロックしない(ナビゲーションの検査は別の経路で行う)。
pub fn should_block(url: &str, initiator: Option<&str>) -> bool {
    let Some(f) = FILTER.get() else { return false };
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    let top = initiator.and_then(|i| Url::parse(i).ok());
    let request = Request {
        url: &url,
        top_frame: top.as_ref(),
    };
    matches!(
        decide(&request, &f.policy, f.blocker.as_ref()),
        Decision::Block(BlockReason::BlockedHost | BlockReason::ContentBlocker)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use net_policy::ListBlocker;

    // `OnceLock` はプロセスで 1 度しか設定できないため、1 つのテストにまとめる。
    #[test]
    fn blocks_listed_hosts_and_policy_hosts_but_not_others() {
        assert!(
            !should_block("https://ads.example/x.js", None),
            "before install"
        );
        let policy = Policy::from_json(r#"{"blocked_hosts":["bad.example"]}"#).unwrap();
        install(
            policy,
            Arc::new(ListBlocker::from_list(
                "||ads.example^\n||t.net^$third-party\n",
            )),
        );
        assert!(should_block("https://ads.example/x.js", None));
        assert!(should_block(
            "https://cdn.bad.example/y.png",
            Some("https://news.example/")
        ));
        assert!(!should_block("https://fine.example/", None));
        assert!(should_block(
            "https://t.net/p",
            Some("https://news.example/")
        ));
        assert!(!should_block("https://t.net/p", Some("https://t.net/")));
        assert!(!should_block("not a url", None));
    }
}
