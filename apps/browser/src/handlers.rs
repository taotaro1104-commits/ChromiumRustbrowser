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

        fn audio_handler(&self) -> Option<AudioHandler> {
            Some(TabAudioHandler::new(self.tab))
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(TabDownloadHandler::new(0))
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

        /// 自分が注入した JS の報告(`__rb:<nonce>:...`)だけを受け取り、コンソールには出さない。
        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: ::std::os::raw::c_int,
        ) -> ::std::os::raw::c_int {
            let message = message.map(CefString::to_string).unwrap_or_default();
            match crate::page_state::parse_message(&message, crate::page_state::nonce()) {
                Some(state) => {
                    let tab = self.tab;
                    post_ui(move || {
                        ctx::with(|c| c.on_page_state(tab, state));
                    });
                    1
                }
                None => 0,
            }
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
            if let Some(js) = ctx::with(|c| c.take_pending_form(tab))
                .flatten()
                .and_then(|f| crate::page_state::restore_script(&f))
            {
                frame.execute_java_script(
                    Some(&CefString::from(js.as_str())),
                    Some(&CefString::from(url.as_str())),
                    0,
                );
            }
            post_ui(move || {
                ctx::with(|c| c.on_load_end(tab, &url));
            });
        }
    }
}

wrap_audio_handler! {
    struct TabAudioHandler {
        tab: TabHandle,
    }

    impl AudioHandler {
        /// 既定のパラメータのまま、ストリームの開始・停止だけを受け取る(音声データは使わない)。
        fn audio_parameters(
            &self,
            _browser: Option<&mut Browser>,
            _params: Option<&mut AudioParameters>,
        ) -> ::std::os::raw::c_int {
            1
        }

        fn on_audio_stream_started(
            &self,
            _browser: Option<&mut Browser>,
            _params: Option<&AudioParameters>,
            _channels: ::std::os::raw::c_int,
        ) {
            let tab = self.tab;
            post_ui(move || {
                ctx::with(|c| c.on_audible(tab, true));
            });
        }

        fn on_audio_stream_stopped(&self, _browser: Option<&mut Browser>) {
            let tab = self.tab;
            post_ui(move || {
                ctx::with(|c| c.on_audible(tab, false));
            });
        }
    }
}

// ダウンロード: 安全なファイル名にして「ダウンロード」フォルダへ保存する。実行形式などは保存しない。
wrap_download_handler! {
    struct TabDownloadHandler {
        _unused: u8,
    }

    impl DownloadHandler {
        // 既定は「拒否」なので、許可を明示する。保存の可否は `on_before_download` が決める。
        fn can_download(
            &self,
            _browser: Option<&mut Browser>,
            _url: Option<&CefString>,
            _request_method: Option<&CefString>,
        ) -> ::std::os::raw::c_int {
            1
        }

        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            _download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> ::std::os::raw::c_int {
            let Some(callback) = callback else { return 0 };
            let suggested = suggested_name.map(CefString::to_string).unwrap_or_default();
            let name = crate::downloads::sanitize_file_name(&suggested);
            let target = (!crate::downloads::is_blocked(&name))
                .then(crate::downloads::downloads_dir)
                .flatten()
                .and_then(|dir| {
                    std::fs::create_dir_all(&dir).ok()?;
                    crate::downloads::unique_path(&dir, &name, |p| p.exists())
                });
            match target {
                Some(path) => {
                    callback.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), 0);
                }
                None => {
                    // 空のパスで続行すると既定の一時フォルダへ保存されてしまうため、続行せず、
                    // 既定の処理(Alloy スタイルでは取り消し)に任せる。
                    eprintln!("browser: download refused: {name}");
                    return 0;
                }
            }
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            callback: Option<&mut DownloadItemCallback>,
        ) {
            let Some(item) = download_item else { return };
            // 念のため: 保存先の名前が拒否対象なら、始まっていても取り消す。
            let path = CefString::from(&item.full_path()).to_string();
            let file = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !file.is_empty() && crate::downloads::is_blocked(&file) {
                if let Some(cb) = callback {
                    cb.cancel();
                }
                return;
            }
            if item.is_complete() != 0 {
                post_ui(move || {
                    ctx::with(|c| c.on_download_complete(&file));
                });
            }
        }
    }
}

// サブリソース(画像・スクリプト・iframe 等)の要求を、ポリシーとブロックリストで検査する。
// IO スレッドで呼ばれるため `Ctx` には触れず、読み取り専用の `net_filter` だけを使う。
wrap_resource_request_handler! {
    struct TabResourceRequestHandler {
        initiator: String,
    }

    impl ResourceRequestHandler {
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            let Some(request) = request else {
                return ReturnValue::CONTINUE;
            };
            let url = CefString::from(&request.url()).to_string();
            let initiator = (!self.initiator.is_empty()).then_some(self.initiator.as_str());
            if crate::net_filter::should_block(&url, initiator) {
                ReturnValue::CANCEL
            } else {
                ReturnValue::CONTINUE
            }
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
        /// ナビゲーション以外のリソース要求だけを検査する(ナビゲーションは `on_before_browse` が受け持つ)。
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            is_navigation: ::std::os::raw::c_int,
            _is_download: ::std::os::raw::c_int,
            request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut ::std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            if is_navigation != 0 {
                return None;
            }
            let initiator = request_initiator.map(CefString::to_string).unwrap_or_default();
            Some(TabResourceRequestHandler::new(initiator))
        }

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
        /// こちらから閉じているタブは、ビュー階層から外して閉じを完了させる(`true` = 自前で処理)。
        /// それ以外(ユーザーがウィンドウを閉じた場合)は既定の動作に任せる。
        fn do_close(&self, _browser: Option<&mut Browser>) -> ::std::os::raw::c_int {
            let tab = self.tab;
            if !crate::host::is_closing(tab) {
                return 0;
            }
            post_ui(move || crate::host::tab_closed(tab));
            1
        }

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
