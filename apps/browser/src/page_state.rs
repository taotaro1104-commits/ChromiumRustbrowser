//! ページの状態(スクロール位置・入力中か・入力欄の内容)の取得と復元。
//!
//! 取得: ブラウザ側から JS を実行し、結果を `console.debug` で返させて `DisplayHandler` で受け取る
//! (レンダラー側のコードを持たずに済む)。ページが偽のメッセージを送れないよう、起動ごとの乱数
//! (nonce)を合言葉にする。nonce は注入する JS の中にしか現れない。
//! 取得するのは平文のテキスト入力だけ。パスワード・カード情報・`autocomplete=off` の欄は含めない。

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::OnceLock;

use serde::Deserialize;
use shell_core::PageState;

/// 受け取るメッセージの上限(バイト)。巨大な偽メッセージで時間を使わないため。
const MAX_MESSAGE_BYTES: usize = 96 * 1024;
/// 1 ページあたり取得する入力欄の数と、1 欄あたりの文字数の上限。
const MAX_FIELDS: usize = 50;
const MAX_FIELD_CHARS: usize = 2000;

/// 起動ごとの合言葉。OS 由来の乱数キーを持つ `RandomState` から作る。
pub fn nonce() -> &'static str {
    static NONCE: OnceLock<String> = OnceLock::new();
    NONCE.get_or_init(|| {
        let part = || RandomState::new().build_hasher().finish();
        format!("{:016x}{:016x}", part(), part())
    })
}

/// ページ状態を取得する JS。結果は `console.debug("__rb:<nonce>:<json>")` で返る。
pub fn capture_script() -> String {
    format!(
        r#"(function(){{try{{
var f=[],els=document.querySelectorAll('input,textarea');
for(var i=0;i<els.length&&f.length<{MAX_FIELDS};i++){{var e=els[i];
 if(e.tagName==='INPUT'&&['text','search','email','url','tel','number'].indexOf((e.type||'text').toLowerCase())<0)continue;
 var ac=(e.getAttribute('autocomplete')||'').toLowerCase();
 if(ac==='off'||ac.indexOf('cc-')===0||ac.indexOf('password')>=0||ac==='one-time-code')continue;
 if(e.value&&e.value!==e.defaultValue)f.push([i,e.value.slice(0,{MAX_FIELD_CHARS})]);}}
var a=document.activeElement;
var ed=f.length>0||(!!a&&a.isContentEditable&&(a.textContent||'').length>0);
console.debug('__rb:{nonce}:'+JSON.stringify({{y:Math.round(window.scrollY||0),e:ed,f:f}}));
}}catch(x){{}}}})();"#,
        nonce = nonce()
    )
}

#[derive(Deserialize)]
struct Report {
    y: f64,
    e: bool,
    f: Vec<(u32, String)>,
}

/// コンソールメッセージがこのアプリの報告なら、ページ状態として解釈する。
pub fn parse_message(message: &str, nonce: &str) -> Option<PageState> {
    if message.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    let json = message
        .strip_prefix("__rb:")?
        .strip_prefix(nonce)?
        .strip_prefix(':')?;
    let report: Report = serde_json::from_str(json).ok()?;
    let fields: Vec<_> = report.f.into_iter().take(MAX_FIELDS).collect();
    let form_state = if fields.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&fields).ok()?)
    };
    Some(PageState {
        // 負数・NaN・巨大値は 0 / 上限に丸める。
        scroll_y: report.y.round().clamp(0.0, f64::from(u32::MAX)) as u32,
        editing: report.e,
        form_state,
    })
}

/// 保存した入力欄の内容を戻す JS。空の欄にだけ入れ、種類が合わない欄は触らない。
/// `form_state` は信頼しない: JSON 文字列リテラルとして埋め込み、JS 側で検査してから使う。
pub fn restore_script(form_state: &str) -> Option<String> {
    let literal = serde_json::to_string(form_state).ok()?;
    Some(format!(
        r#"(function(){{try{{
var d=JSON.parse({literal}),els=document.querySelectorAll('input,textarea');
for(var k=0;k<d.length&&k<{MAX_FIELDS};k++){{var i=d[k][0],v=d[k][1];
 if(typeof i!=='number'||typeof v!=='string'||!els[i])continue;
 var e=els[i];
 if(e.tagName==='INPUT'&&['text','search','email','url','tel','number'].indexOf((e.type||'text').toLowerCase())<0)continue;
 if(e.value)continue;
 e.value=v;e.dispatchEvent(new Event('input',{{bubbles:true}}));}}
}}catch(x){{}}}})();"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: &str = "abc123";

    fn msg(json: &str) -> String {
        format!("__rb:{N}:{json}")
    }

    #[test]
    fn valid_report_is_parsed() {
        let s = parse_message(&msg(r#"{"y":420.6,"e":true,"f":[[2,"draft"]]}"#), N).unwrap();
        assert_eq!(s.scroll_y, 421);
        assert!(s.editing);
        assert_eq!(s.form_state.as_deref(), Some(r#"[[2,"draft"]]"#));
    }

    #[test]
    fn empty_form_means_none_and_odd_scroll_is_clamped() {
        let s = parse_message(&msg(r#"{"y":-5,"e":false,"f":[]}"#), N).unwrap();
        assert_eq!((s.scroll_y, s.form_state), (0, None));
        let s = parse_message(&msg(r#"{"y":1e30,"e":false,"f":[]}"#), N).unwrap();
        assert_eq!(s.scroll_y, u32::MAX);
    }

    #[test]
    fn wrong_nonce_missing_prefix_and_garbage_are_ignored() {
        assert!(parse_message("__rb:other:{\"y\":1,\"e\":false,\"f\":[]}", N).is_none());
        assert!(parse_message("hello", N).is_none());
        assert!(parse_message(&msg("not json"), N).is_none());
        assert!(parse_message(&msg(r#"{"y":1,"e":false}"#), N).is_none());
    }

    #[test]
    fn oversized_message_is_ignored() {
        let big = msg(&format!(
            r#"{{"y":1,"e":false,"f":[[0,"{}"]]}}"#,
            "x".repeat(MAX_MESSAGE_BYTES)
        ));
        assert!(parse_message(&big, N).is_none());
    }

    #[test]
    fn nonce_is_stable_per_run_and_long() {
        assert_eq!(nonce(), nonce());
        assert_eq!(nonce().len(), 32);
    }

    #[test]
    fn capture_script_carries_the_nonce_and_skips_secrets() {
        // 自動フォーカスされた空の入力欄は「入力中」にしない(破棄の対象から外れてしまう)。
        assert!(capture_script().contains("var ed=f.length>0"));
        let js = capture_script();
        assert!(js.contains(nonce()));
        assert!(js.contains("autocomplete") && js.contains("cc-"));
        // 取得対象は種類の許可リスト方式。password / hidden / file 等は含まれない。
        assert!(js.contains("['text','search','email','url','tel','number']"));
    }

    #[test]
    fn restore_script_embeds_state_as_an_escaped_literal() {
        let hostile = r#"[[0,"x"]]"; alert(1); ""#;
        let js = restore_script(hostile).unwrap();
        // 生の引用符がリテラルを抜け出さない(`\"` にエスケープされる)。
        assert!(js.contains(r#"JSON.parse("[[0,\"x\"]]\"; alert(1); \"")"#));
    }
}
