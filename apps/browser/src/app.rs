//! CEF アプリ定義: ブラウザプロセスの初期化でウィンドウと `Ctx` を作る。

use std::sync::Arc;

use cef::*;
use shell_core::Shell;
use storage::Db;

use crate::ctx::{self, Ctx};
use crate::handlers::{post_ui, post_ui_delayed};
use crate::host::CefHost;
use crate::net_filter;
use crate::profile::profile_dir;
use crate::start_page;
use crate::startup;
use crate::ui::Ui;

/// 定期 Tick の間隔。凍結(5 分)・破棄(30 分)の判定とセッション保存に使う。
const TICK_INTERVAL_MS: i64 = 30_000;

wrap_app! {
    pub struct BrowserApp;

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(BrowserHandler::new())
        }
    }
}

wrap_browser_process_handler! {
    struct BrowserHandler;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            let dir = profile_dir();
            let db = Db::open(&dir.join("profile.db")).unwrap_or_else(|e| {
                eprintln!("browser: cannot open profile db ({e}); using in-memory history");
                Db::open_in_memory().expect("in-memory db")
            });
            // policy.json が無ければ既定。有るのに読めない/壊れている場合は、ブロックや HTTPS 強制が
            // 黙って無効になるのを避けるため、起動せずに終了する(fail-closed)。
            let policy = match load_policy(&dir) {
                Ok(p) => p,
                Err(msg) => {
                    eprintln!("browser: {msg}");
                    quit_message_loop();
                    return;
                }
            };

            let (ui, content) = Ui::build();
            // ブロックリスト(任意): プロファイル直下の blocklist.txt(Adblock Plus 形式または hosts 形式)。
            let blocker = Arc::new(load_blocklist(&dir));
            net_filter::install(policy.clone(), blocker.clone());
            let mut shell = Shell::new(CefHost::new(content), db, policy);
            shell.set_content_blocker(blocker);
            let home_url = match start_page::spawn() {
                Ok(url) => {
                    shell.set_internal_prefix(url.clone());
                    url
                }
                Err(e) => {
                    // 代替ホームは通常の検索サイトなので、履歴から除外しない。
                    eprintln!("browser: start page unavailable ({e}); using fallback home");
                    ctx::FALLBACK_HOME_URL.to_owned()
                }
            };
            ctx::install(Ctx::new(shell, ui, home_url));

            let switch = |name: &str| {
                command_line_get_global()
                    .map(|c| {
                        CefString::from(&c.switch_value(Some(&CefString::from(name)))).to_string()
                    })
                    .unwrap_or_default()
            };
            let startup_urls = startup::collect(&switch("url"), &switch("urls-file"));
            let mut delegate = MainWindowDelegate::new(startup_urls);
            window_create_top_level(Some(&mut delegate));
        }
    }
}

wrap_window_delegate! {
    struct MainWindowDelegate {
        startup_urls: Vec<String>,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 1280, height: 800 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window else { return };
            ctx::with(|c| {
                c.ui.attach(window);
                c.start(&self.startup_urls);
            });
            window.show();
            post_ui(|| {
                ctx::with(|c| c.ui.paint_toolbar());
            });
            schedule_tick();
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            ctx::with(|c| c.save_session());
            1
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            ctx::clear();
            quit_message_loop();
        }
    }
}

/// `policy.json` を読む。ファイルが無ければ既定、有って不正ならエラー。
fn load_policy(dir: &std::path::Path) -> Result<policy::Policy, String> {
    let path = dir.join("policy.json");
    match std::fs::read_to_string(&path) {
        Ok(json) => {
            policy::Policy::from_json(&json).map_err(|e| format!("invalid {}: {e}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(policy::Policy::default()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// `blocklist.txt` を読む。無ければ空(何もブロックしない)。読めない場合も空にして続行し、理由を出す。
fn load_blocklist(dir: &std::path::Path) -> net_policy::ListBlocker {
    let path = dir.join("blocklist.txt");
    match std::fs::read_to_string(&path) {
        Ok(text) => {
            let list = net_policy::ListBlocker::from_list(&text);
            eprintln!("browser: blocklist loaded: {} rules", list.rule_count());
            list
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => net_policy::ListBlocker::default(),
        Err(e) => {
            eprintln!("browser: cannot read {}: {e}", path.display());
            net_policy::ListBlocker::default()
        }
    }
}

fn schedule_tick() {
    post_ui_delayed(
        || {
            // コンテキストが破棄済み(終了中)なら再スケジュールしない。
            if ctx::with(Ctx::tick).is_some() {
                schedule_tick();
            }
        },
        TICK_INTERVAL_MS,
    );
}
