use serde::Deserialize;

/// ブロック対象などのホスト指定。`example.com` はそのホストと全サブドメインに一致する。
/// `*.example.com` 表記も受け付けて同じ意味に正規化する。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub struct HostPattern(String);

impl HostPattern {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let s = raw.trim().to_ascii_lowercase();
        let s = s.strip_prefix("*.").unwrap_or(&s);
        let s = s.trim_end_matches('.');
        if s.is_empty() {
            return Err("empty host pattern".into());
        }
        if s.len() > 253 {
            return Err(format!("host pattern too long: {} bytes", s.len()));
        }
        let ok = s.split('.').all(|l| {
            !l.is_empty()
                && l.chars()
                    .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        });
        if !ok {
            return Err(format!("invalid host pattern: {raw:?}"));
        }
        // URL 側と同じ IDNA 正規化(punycode)にして、Unicode 表記のパターンも一致させる。
        match url::Host::parse(s) {
            Ok(url::Host::Domain(d)) => Ok(Self(d)),
            Ok(other) => Ok(Self(other.to_string())),
            Err(e) => Err(format!("invalid host pattern {raw:?}: {e}")),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `host`(小文字の正規化済みホスト名)がこのパターンに一致するか。
    pub fn matches(&self, host: &str) -> bool {
        host == self.0
            || host
                .strip_suffix(self.0.as_str())
                .is_some_and(|rest| rest.ends_with('.'))
    }
}

/// `HostPattern` 群のハッシュ索引。照合はホストのラベル数に比例(パターン数に依存しない)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HostSet(std::collections::HashSet<String>);

impl HostSet {
    pub fn new(patterns: &[HostPattern]) -> Self {
        Self(patterns.iter().map(|p| p.0.clone()).collect())
    }

    /// `host`(小文字の正規化済み)自身、またはその親ドメインが含まれるか。
    pub fn matches(&self, host: &str) -> bool {
        if self.0.is_empty() {
            return false;
        }
        let mut rest = host;
        loop {
            if self.0.contains(rest) {
                return true;
            }
            match rest.split_once('.') {
                Some((_, parent)) => rest = parent,
                None => return false,
            }
        }
    }
}

impl TryFrom<String> for HostPattern {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_case_wildcard_and_trailing_dot() {
        for raw in ["Example.COM", "*.example.com", " example.com. "] {
            assert_eq!(HostPattern::parse(raw).unwrap().as_str(), "example.com");
        }
    }

    #[test]
    fn rejects_malformed_patterns() {
        for raw in [
            "",
            "  ",
            "*.",
            "http://a.com",
            "a.com/x",
            "a b.com",
            "a..com",
            "a.com:80",
            &"a".repeat(300),
        ] {
            assert!(HostPattern::parse(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn host_set_agrees_with_pattern_matching() {
        let pats: Vec<_> = ["example.com", "ads.net"]
            .iter()
            .map(|s| HostPattern::parse(s).unwrap())
            .collect();
        let set = HostSet::new(&pats);
        for host in [
            "example.com",
            "a.b.example.com",
            "badexample.com",
            "example.com.evil.net",
            "com",
            "x.ads.net",
            "",
        ] {
            assert_eq!(
                set.matches(host),
                pats.iter().any(|p| p.matches(host)),
                "{host:?}"
            );
        }
        assert!(!HostSet::default().matches("example.com"));
    }

    #[test]
    fn matches_host_and_subdomains_only() {
        let p = HostPattern::parse("example.com").unwrap();
        assert!(p.matches("example.com"));
        assert!(p.matches("a.b.example.com"));
        assert!(!p.matches("badexample.com"));
        assert!(!p.matches("example.com.evil.net"));
        assert!(!p.matches("com"));
    }
}
