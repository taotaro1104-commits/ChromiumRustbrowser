//! タブごとの CEF クライアント(表示・読込・ライフスパン)と UI スレッドへのタスク投入。

use std::cell::RefCell;
use std::rc::Rc;

use cef::*;
use ffi_traits::TabHandle;

use crate::ctx;

type PendingTask = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

wrap_task! {
    struct ClosureTask {
        f: PendingTask,
    }

    impl Task {
        fn execute(&self) {
            if let Some(f) = self.f.borrow_mut().take() {
                f();
            }
        }
    }
}

/// UI スレッドで後から実行する(ボタン押下中のビュー破棄など、再入を避ける)。
pub fn post_ui(f: impl FnOnce() + 'static) {
    let mut task = ClosureTask::new(Rc::new(RefCell::new(Some(Box::new(f)))));
    post_task(ThreadId::UI, Some(&mut task));
}

pub fn post_ui_delayed(f: impl FnOnce() + 'static, delay_ms: i64) {
    let mut task = ClosureTask::new(Rc::new(RefCell::new(Some(Box::new(f)))));
    post_delayed_task(ThreadId::UI, Some(&mut task), delay_ms);
}

wrap_client! {
    pub struct TabClient {
        tab: TabHandle,
    }

    impl Client {
        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(TabDisplayHandler::new(self.tab))
        }

        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(TabLifeSpanHandler::new(self.tab))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(TabLoadHandler::new(self.tab))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(TabRequestHandler::new(self.tab))
        }
    }
}

wrap_display_handler! {
    struct TabDisplayHandler {
        tab: TabHandle,
    }

    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            let title = title.map(CefString::to_string).unwrap_or_default();
            let tab = self.tab;
            post_ui(move || {
                ctx::with(|c| c.on_title(tab, &title));
            });
        }

        fn on_address_change(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            // サブフレームの遷移は無視する。
            if !frame.is_some_and(|f| f.is_main() != 0) {
                return;
            }
            let url = url.map(CefString::to_string).unwrap_or_default();
            let tab = self.tab;
            post_ui(move || {
                ctx::with(|c| c.on_address(tab, &url));
            });
        }
    }
}

wrap_load_handler! {
    struct TabLoadHandler {
        tab: TabHandle,
    }

    impl LoadHandler {
        fn on_load_end(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _http_status_code: ::std::os::raw::c_int,
        ) {
            let Some(frame) = frame else { return };
            if frame.is_main() == 0 {
                return;
            }
            let url = CefString::from(&frame.url()).to_string();
            let tab = self.tab;
            // スクロール復元はフレームを使うので、ここで実行する。
            if let Some(y) = ctx::with(|c| c.take_pending_scroll(tab)).flatten() {
                let js = CefString::from(format!("window.scrollTo(0,{y})").as_str());
                frame.execute_java_script(Some(&js), Some(&CefString::from(url.as_str())), 0);
            }
            post_ui(move || {
                ctx::with(|c| c.on_load_end(tab, &url));
            });
        }
    }
}

/// ページ起点のメインフレーム遷移にも、アドレスバーと同じスキーム・ポリシー検査をかける。
fn navigation_allowed(url: &str) -> bool {
    // 再入で Ctx を借りられないときも、スキームの検査だけは必ず行う。
    ctx::with(|c| c.shell.allows_navigation(url))
        .unwrap_or_else(|| browser_core::is_allowed_url(url))
}

wrap_request_handler! {
    struct TabRequestHandler {
        tab: TabHandle,
    }

    impl RequestHandler {
        fn on_before_browse(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _user_gesture: ::std::os::raw::c_int,
            _is_redirect: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let (Some(frame), Some(request)) = (frame, request) else {
                return 0;
            };
            if frame.is_main() == 0 {
                return 0;
            }
            let url = CefString::from(&request.url()).to_string();
            if navigation_allowed(&url) {
                0
            } else {
                eprintln!("browser: blocked navigation (tab {:?})", self.tab);
                1
            }
        }
    }
}

wrap_life_span_handler! {
    struct TabLifeSpanHandler {
        tab: TabHandle,
    }

    impl LifeSpanHandler {
        /// ポップアップは新しいウィンドウにせず、新規タブとして開く。
        fn on_before_popup(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _popup_id: ::std::os::raw::c_int,
            target_url: Option<&CefString>,
            _target_frame_name: Option<&CefString>,
            _target_disposition: WindowOpenDisposition,
            user_gesture: ::std::os::raw::c_int,
            _popup_features: Option<&PopupFeatures>,
            _window_info: Option<&mut WindowInfo>,
            _client: Option<&mut Option<Client>>,
            _settings: Option<&mut BrowserSettings>,
            _extra_info: Option<&mut Option<DictionaryValue>>,
            _no_javascript_access: Option<&mut ::std::os::raw::c_int>,
        ) -> ::std::os::raw::c_int {
            let url = target_url.map(CefString::to_string).unwrap_or_default();
            // ユーザー操作を伴わないポップアップ(`window.open` の自動連打)は開かない。
            if user_gesture != 0 && !url.is_empty() {
                post_ui(move || {
                    ctx::with(|c| c.open_url_in_new_tab(&url));
                });
            }
            // 1 = ポップアップの作成を取り消す。
            1
        }
    }
}
