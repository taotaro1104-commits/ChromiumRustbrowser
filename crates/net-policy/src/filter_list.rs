//! フィルタリスト(広告・トラッカーのドメイン一覧)によるコンテンツブロッカー。
//!
//! 対応する書式(ドメイン単位の最小サブセット。メモリ予算 20MB に収めるため、URL パターンの照合はしない):
//! - Adblock Plus 形式: `||example.com^`、`||example.com^$third-party`、例外 `@@||example.com^`
//! - hosts 形式: `0.0.0.0 ads.example.com`(`127.0.0.1` も可)
//! - コメント(`!` / `#` 始まり)、`[Adblock Plus 2.0]` のようなヘッダ、空行は無視
//!
//! 未対応のオプション(`$image`、`$script` など)や、パス・ワイルドカードを含む規則は **無視する**
//! (誤ってサイトを壊すより、取りこぼす側に倒す)。adblock-rust による完全な規則対応は将来の拡張。

use policy::{HostPattern, HostSet};

use crate::cookies::is_third_party;
use crate::decide::{ContentBlocker, Request};

/// 読み込む規則数の上限。巨大なリストでメモリを使い切らないため。
pub const MAX_RULES: usize = 300_000;
/// 1 行の最大長(バイト)。
const MAX_LINE_BYTES: usize = 512;

#[derive(Default)]
pub struct ListBlocker {
    block: HostSet,
    /// `$third-party` 付きの規則。第三者のリクエストだけをブロックする。
    block_third_party: HostSet,
    allow: HostSet,
    rules: usize,
}

enum Rule {
    Block(HostPattern),
    BlockThirdParty(HostPattern),
    Allow(HostPattern),
}

/// 1 行を解析する。対応しない書式は `None`。
fn parse_line(line: &str) -> Option<Rule> {
    let line = line.trim();
    if line.is_empty() || line.len() > MAX_LINE_BYTES {
        return None;
    }
    if line.starts_with(['!', '#', '[']) {
        return None;
    }
    if let Some(rule) = line.strip_prefix("@@||") {
        return domain_rule(rule).map(|(p, _)| Rule::Allow(p));
    }
    if let Some(rule) = line.strip_prefix("||") {
        return domain_rule(rule).map(|(p, third)| {
            if third {
                Rule::BlockThirdParty(p)
            } else {
                Rule::Block(p)
            }
        });
    }
    // hosts 形式: 「0.0.0.0 host」「127.0.0.1 host」。先頭の IP 以外は対象外。
    let mut parts = line.split_whitespace();
    let (ip, host) = (parts.next()?, parts.next()?);
    if !matches!(ip, "0.0.0.0" | "127.0.0.1") || parts.next().is_some_and(|c| !c.starts_with('#')) {
        return None;
    }
    if host == "localhost" || host.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }
    HostPattern::parse(host).ok().map(Rule::Block)
}

/// `example.com^` / `example.com^$third-party` から (ホスト, 第三者のみか) を取り出す。
/// パスやワイルドカード、未対応のオプションを含む規則は `None`。
fn domain_rule(rest: &str) -> Option<(HostPattern, bool)> {
    let (host, options) = rest.split_once('^')?;
    let third = match options {
        "" => false,
        "$third-party" | "$3p" => true,
        _ => return None,
    };
    if host.contains(['/', '*', '?', ':', '|', '$']) {
        return None;
    }
    HostPattern::parse(host).ok().map(|p| (p, third))
}

impl ListBlocker {
    /// リストの本文から作る。解析できない行・上限を超えた分は黙って捨てる(件数は `rule_count`)。
    pub fn from_list(text: &str) -> Self {
        let (mut block, mut third, mut allow) = (Vec::new(), Vec::new(), Vec::new());
        for line in text.lines() {
            if block.len() + third.len() + allow.len() >= MAX_RULES {
                break;
            }
            match parse_line(line) {
                Some(Rule::Block(p)) => block.push(p),
                Some(Rule::BlockThirdParty(p)) => third.push(p),
                Some(Rule::Allow(p)) => allow.push(p),
                None => {}
            }
        }
        Self {
            rules: block.len() + third.len() + allow.len(),
            block: HostSet::new(&block),
            block_third_party: HostSet::new(&third),
            allow: HostSet::new(&allow),
        }
    }

    pub fn rule_count(&self) -> usize {
        self.rules
    }
}

impl ContentBlocker for ListBlocker {
    fn should_block(&self, request: &Request<'_>) -> bool {
        let Some(host) = request.url.host_str() else {
            return false;
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        // 例外が最優先。
        if self.allow.matches(&host) {
            return false;
        }
        if self.block.matches(&host) {
            return true;
        }
        // 第三者のみの規則。トップフレームが不明(ナビゲーション自体)なら第一者として扱い、ブロックしない。
        self.block_third_party.matches(&host)
            && request
                .top_frame
                .is_some_and(|top| is_third_party(request.url, top))
    }
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::*;

    const LIST: &str = "\
[Adblock Plus 2.0]
! Title: test list
||ads.example.com^
||tracker.net^$third-party
@@||ok.ads.example.com^
||path.example.org/banner^
||wild*.example.org^
||img.example.org^$image
0.0.0.0 hosts-style.example
127.0.0.1 localhost
0.0.0.0 0.0.0.0
# comment
";

    fn blocked(list: &ListBlocker, url: &str, top: Option<&str>) -> bool {
        let url = Url::parse(url).unwrap();
        let top = top.map(|t| Url::parse(t).unwrap());
        list.should_block(&Request {
            url: &url,
            top_frame: top.as_ref(),
        })
    }

    #[test]
    fn domain_rules_block_the_host_and_its_subdomains() {
        let l = ListBlocker::from_list(LIST);
        assert!(blocked(&l, "https://ads.example.com/x.js", None));
        assert!(blocked(&l, "https://cdn.ads.example.com/x.js", None));
        assert!(!blocked(&l, "https://example.com/", None));
        assert!(!blocked(&l, "https://notads.example.com/", None));
    }

    #[test]
    fn exceptions_win_over_blocks() {
        let l = ListBlocker::from_list(LIST);
        assert!(!blocked(&l, "https://ok.ads.example.com/x.js", None));
        assert!(!blocked(&l, "https://deep.ok.ads.example.com/x.js", None));
    }

    #[test]
    fn third_party_only_rules_need_a_different_top_frame() {
        let l = ListBlocker::from_list(LIST);
        assert!(blocked(
            &l,
            "https://tracker.net/p.gif",
            Some("https://news.example/")
        ));
        assert!(!blocked(
            &l,
            "https://tracker.net/p.gif",
            Some("https://tracker.net/")
        ));
        // ナビゲーション自体(トップフレーム不明)は第一者として扱う。
        assert!(!blocked(&l, "https://tracker.net/", None));
    }

    #[test]
    fn hosts_format_works_and_localhost_is_ignored() {
        let l = ListBlocker::from_list(LIST);
        assert!(blocked(&l, "http://hosts-style.example/", None));
        assert!(!blocked(&l, "http://localhost/", None));
    }

    #[test]
    fn unsupported_rules_are_ignored_rather_than_overblocking() {
        let l = ListBlocker::from_list(LIST);
        // パス付き・ワイルドカード・未対応オプションは無視される。
        assert!(!blocked(&l, "https://path.example.org/banner", None));
        assert!(!blocked(&l, "https://wild1.example.org/", None));
        assert!(!blocked(&l, "https://img.example.org/a.png", None));
        // 有効なのは ads / tracker / ok(例外) / hosts-style の 4 件。
        assert_eq!(l.rule_count(), 4);
    }

    #[test]
    fn rule_count_is_capped_and_hostile_lines_are_survivable() {
        let big: String = (0..MAX_RULES + 100)
            .map(|i| format!("||h{i}.example^\n"))
            .collect();
        assert_eq!(ListBlocker::from_list(&big).rule_count(), MAX_RULES);
        let hostile = format!("||{}^\n@@||^\n||^\n\u{0}\n||a b^\n", "a".repeat(10_000));
        assert_eq!(ListBlocker::from_list(&hostile).rule_count(), 0);
        assert!(!blocked(
            &ListBlocker::default(),
            "https://a.example/",
            None
        ));
    }
}
