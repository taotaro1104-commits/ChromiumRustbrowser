//! 保存済みパスワードの管理画面(別ウィンドウ)。サイトとユーザー名の一覧と、削除だけを持つ。
//! **パスワードは表示しない**(画面に出さなければ、肩越しの覗き見や画面共有・スクリーンショットで漏れない)。

use std::cell::RefCell;

use cef::*;

use crate::ctx;
use crate::ui::{box_layout, palette};

/// 一度に並べる件数の上限。
const MAX_ROWS: usize = 100;
const FONT: &str = "Yu Gothic UI, Bold 12px";

thread_local! {
    static WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// 開く。既に開いていれば前面に出す。
pub fn open() {
    let existing = WINDOW.with(|w| w.borrow().clone());
    if let Some(w) = existing {
        w.show();
        crate::handlers::post_ui(refresh);
        return;
    }
    let mut delegate = PasswordsDelegate::new(0);
    window_create_top_level(Some(&mut delegate));
}

wrap_button_delegate! {
    struct DeleteButtonDelegate {
        origin: String,
        username: String,
    }

    impl ViewDelegate {}

    impl ButtonDelegate {
        fn on_button_pressed(&self, _button: Option<&mut Button>) {
            let (origin, username) = (self.origin.clone(), self.username.clone());
            // ボタンの押下中にビューを作り直さない。
            crate::handlers::post_ui(move || {
                ctx::with(|c| c.delete_login(&origin, &username));
                refresh();
            });
        }
    }
}

wrap_button_delegate! {
    struct NoopButtonDelegate {
        _unused: u8,
    }

    impl ViewDelegate {}

    impl ButtonDelegate {
        fn on_button_pressed(&self, _button: Option<&mut Button>) {}
    }
}

wrap_window_delegate! {
    struct PasswordsDelegate {
        _unused: u8,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 760, height: 520 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window else { return };
            window.set_title(Some(&CefString::from("保存済みのパスワード")));
            WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
            // 開く操作の最中は `Ctx` が借用されているため、一覧の読み込みは後で行う。
            crate::handlers::post_ui(refresh);
            window.show();
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            WINDOW.with(|w| *w.borrow_mut() = None);
        }
    }
}

fn label(text: &str) -> LabelButton {
    let mut d = NoopButtonDelegate::new(0);
    let b = label_button_create(Some(&mut d), Some(&CefString::from(text))).expect("label");
    b.set_font_list(Some(&CefString::from(FONT)));
    b
}

fn paint(b: &LabelButton, bg: u32, fg: u32) {
    b.set_background_color(bg);
    for state in [
        ButtonState::NORMAL,
        ButtonState::HOVERED,
        ButtonState::PRESSED,
    ] {
        b.set_text_color(state, fg);
    }
}

/// 開いている管理画面の一覧を作り直す。
fn refresh() {
    if let Some(w) = WINDOW.with(|w| w.borrow().clone()) {
        rebuild(&w);
    }
}

/// 一覧を作り直す。
fn rebuild(window: &Window) {
    let entries = ctx::with(|c| c.shell.all_logins(MAX_ROWS)).and_then(Result::ok);
    window.remove_all_child_views();
    window.set_to_box_layout(Some(&box_layout(false)));
    window.set_background_color(palette::BAR);

    let header = label("パスワードは画面に表示しません。削除すると、このブラウザから消えます。");
    window.add_child_view(Some(&mut View::from(&header)));
    paint(&header, palette::BAR, palette::INK);

    match entries {
        Some(list) if !list.is_empty() => {
            for e in list {
                let row = panel_create(None).expect("row");
                row.set_to_box_layout(Some(&box_layout(true)));
                window.add_child_view(Some(&mut View::from(&row)));
                row.set_background_color(palette::STRIP);

                let text = format!("{}   {}", e.origin, e.username);
                let name = label(&text);
                row.add_child_view(Some(&mut View::from(&name)));
                paint(&name, palette::WHITE, palette::INK);

                let mut del = DeleteButtonDelegate::new(e.origin.clone(), e.username.clone());
                let btn = label_button_create(Some(&mut del), Some(&CefString::from("削除")))
                    .expect("delete");
                btn.set_font_list(Some(&CefString::from(FONT)));
                row.add_child_view(Some(&mut View::from(&btn)));
                paint(&btn, palette::CLOSE_BG, palette::CLOSE_FG);
            }
        }
        _ => {
            let empty = label(
                "保存されたパスワードはありません。ログインして「保存」を選ぶと、ここに並びます。",
            );
            window.add_child_view(Some(&mut View::from(&empty)));
            paint(&empty, palette::WHITE, palette::INK);
        }
    }
    window.invalidate_layout();
}
