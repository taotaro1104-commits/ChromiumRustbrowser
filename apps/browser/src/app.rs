//! CEF アプリ定義: ブラウザプロセスの初期化でウィンドウと `Ctx` を作る。

use cef::*;
use shell_core::Shell;
use storage::Db;

use crate::ctx::{self, Ctx};
use crate::handlers::{post_ui, post_ui_delayed};
use crate::host::CefHost;
use crate::profile::profile_dir;
use crate::start_page;
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
            let policy = std::fs::read_to_string(dir.join("policy.json"))
                .ok()
                .and_then(|json| {
                    policy::Policy::from_json(&json)
                        .map_err(|e| eprintln!("browser: invalid policy.json: {e}"))
                        .ok()
                })
                .unwrap_or_default();

            let (ui, content) = Ui::build();
            let mut shell = Shell::new(CefHost::new(content), db, policy);
            let home_url = start_page::spawn().unwrap_or_else(|e| {
                eprintln!("browser: start page unavailable ({e}); using fallback home");
                ctx::FALLBACK_HOME_URL.to_owned()
            });
            shell.set_internal_prefix(home_url.clone());
            ctx::install(Ctx::new(shell, ui, home_url));

            let initial_url = command_line_get_global()
                .map(|c| CefString::from(&c.switch_value(Some(&CefString::from("url")))).to_string())
                .unwrap_or_default();
            let mut delegate = MainWindowDelegate::new(initial_url);
            window_create_top_level(Some(&mut delegate));
        }
    }
}

wrap_window_delegate! {
    struct MainWindowDelegate {
        initial_url: String,
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
                c.start(Some(self.initial_url.as_str()));
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
