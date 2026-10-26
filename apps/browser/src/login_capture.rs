//! ログインフォームの検出・送信の捕捉・自動入力(パスワード管理の CEF 側)。
//!
//! 捕捉: ページの読込後に JS を注入し、`submit` イベントでユーザー名とパスワードを `console.debug` で返させる。
//! 合言葉(nonce)付きで、ページからの偽メッセージは捨てる。メッセージ内の URL は信用せず、保存先のサイトは
//! ブラウザ側が持つタブの URL から決める。受け取ったパスワードはコンソールに出さず、`Zeroizing` で扱う。
//! 自動入力: 利用者が確認バーで承認したときだけ、最上位フレームの入力欄へ入れる(`fill_script`)。

use serde::Deserialize;
use zeroize::Zeroizing;

use crate::page_state::nonce;

/// 受け取るメッセージの上限(バイト)。
const MAX_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_FIELD_BYTES: usize = 2048;

/// ページから届くログイン関連のメッセージ。
#[derive(Debug, PartialEq, Eq)]
pub enum LoginMessage {
    /// フォームが送信された。
    Submitted {
        username: String,
        password: Zeroizing<String>,
    },
    /// ページにパスワード入力欄があるか(自動入力の候補を出すかどうかの判断材料)。
    Probe { has_password_field: bool },
}

/// 注入する JS。冪等(2 回注入しても 1 回だけ働く)。
pub fn hook_script() -> String {
    format!(
        r#"(function(){{try{{
if(window.__rbLogin)return;window.__rbLogin=true;
var send=function(o){{console.debug('__rbl:{nonce}:'+JSON.stringify(o));}};
var text=['text','email','tel','url',''];
document.addEventListener('submit',function(ev){{try{{
 var f=ev.target;if(!f||!f.querySelectorAll)return;
 var all=f.querySelectorAll('input'),pw=null,pwi=-1;
 for(var i=0;i<all.length;i++){{if((all[i].type||'').toLowerCase()==='password'&&all[i].value){{pw=all[i];pwi=i;break;}}}}
 if(!pw)return;
 var user=null;
 for(var j=pwi-1;j>=0;j--){{var t=(all[j].type||'').toLowerCase();if(text.indexOf(t)>=0&&all[j].value){{user=all[j];break;}}}}
 if(!user)return;
 send({{l:{{u:user.value,p:pw.value}}}});
}}catch(x){{}}}},true);
var probe=function(){{var p=document.querySelector('input[type=password]');send({{pw:!!p&&p.offsetParent!==null}});}};
probe();setTimeout(probe,1500);
}}catch(x){{}}}})();"#,
        nonce = nonce()
    )
}

#[derive(Deserialize)]
struct Raw {
    l: Option<Login>,
    pw: Option<bool>,
}

#[derive(Deserialize)]
struct Login {
    u: String,
    p: String,
}

/// コンソールメッセージがログイン関連の報告なら解釈する。
pub fn parse_message(message: &str, nonce: &str) -> Option<LoginMessage> {
    if message.len() > MAX_MESSAGE_BYTES {
        return None;
    }
    let json = message
        .strip_prefix("__rbl:")?
        .strip_prefix(nonce)?
        .strip_prefix(':')?;
    let raw: Raw = serde_json::from_str(json).ok()?;
    if let Some(l) = raw.l {
        if l.u.len() > MAX_FIELD_BYTES || l.p.len() > MAX_FIELD_BYTES {
            return None;
        }
        return Some(LoginMessage::Submitted {
            username: l.u,
            password: Zeroizing::new(l.p),
        });
    }
    raw.pw
        .map(|has_password_field| LoginMessage::Probe { has_password_field })
}

/// 自動入力の JS。値は JSON 文字列リテラルとして埋め込む(スクリプトとして解釈されない)。
/// 見える最初のパスワード欄と、その直前のテキスト入力欄に入れ、`input` / `change` を発火する。
pub fn fill_script(username: &str, password: &str) -> Option<Zeroizing<String>> {
    let u = serde_json::to_string(username).ok()?;
    let p = Zeroizing::new(serde_json::to_string(password).ok()?);
    Some(Zeroizing::new(format!(
        r#"(function(){{try{{
var U={u},P={p};
var set=function(el,v){{var d=Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value');d.set.call(el,v);
 el.dispatchEvent(new Event('input',{{bubbles:true}}));el.dispatchEvent(new Event('change',{{bubbles:true}}));}};
var all=Array.prototype.slice.call(document.querySelectorAll('input'));
var pw=all.filter(function(e){{return (e.type||'').toLowerCase()==='password'&&e.offsetParent!==null;}})[0];
if(!pw)return;
var i=all.indexOf(pw),user=null;
for(var j=i-1;j>=0;j--){{var t=(all[j].type||'').toLowerCase();
 if(['text','email','tel','url',''].indexOf(t)>=0&&all[j].offsetParent!==null){{user=all[j];break;}}}}
if(user)set(user,U);set(pw,P);
}}catch(x){{}}}})();"#,
        p = *p
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    const N: &str = "abc123";

    fn msg(json: &str) -> String {
        format!("__rbl:{N}:{json}")
    }

    #[test]
    fn submit_and_probe_messages_are_parsed() {
        assert_eq!(
            parse_message(&msg(r#"{"l":{"u":"alice","p":"s3cret"}}"#), N),
            Some(LoginMessage::Submitted {
                username: "alice".into(),
                password: Zeroizing::new("s3cret".into())
            })
        );
        assert_eq!(
            parse_message(&msg(r#"{"pw":true}"#), N),
            Some(LoginMessage::Probe {
                has_password_field: true
            })
        );
    }

    #[test]
    fn forged_oversized_or_malformed_messages_are_ignored() {
        assert!(parse_message("__rbl:other:{\"pw\":true}", N).is_none());
        assert!(parse_message("hello", N).is_none());
        assert!(parse_message(&msg("not json"), N).is_none());
        assert!(parse_message(&msg("{}"), N).is_none());
        let big = "x".repeat(MAX_FIELD_BYTES + 1);
        assert!(parse_message(&msg(&format!(r#"{{"l":{{"u":"a","p":"{big}"}}}}"#)), N).is_none());
        // 状態報告(`__rb:`)とは別の合言葉の接頭辞で、混ざらない。
        assert!(parse_message(&format!("__rb:{N}:{{\"pw\":true}}"), N).is_none());
    }

    #[test]
    fn hook_script_is_idempotent_and_carries_the_nonce() {
        let js = hook_script();
        assert!(js.contains("window.__rbLogin"));
        assert!(js.contains(nonce()));
        // 送信の捕捉はキャプチャ段階の submit のみ(キー入力の記録はしない)。
        assert!(js.contains("addEventListener('submit'"));
        assert!(!js.contains("keydown") && !js.contains("keypress") && !js.contains("keyup"));
    }

    #[test]
    fn fill_script_embeds_values_as_escaped_literals() {
        let hostile_user = "a\"; alert(1); \"";
        let hostile_pass = "p\\\"</script>\u{2028}";
        let js = fill_script(hostile_user, hostile_pass).unwrap();
        assert!(js.contains(r#"var U="a\"; alert(1); \"""#));
        assert!(js.contains(r#"P="p\\\"</script>"#));
        assert!(!js.contains("var U=a"));
    }
}
