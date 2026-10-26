//! Views ベースの最小 UI: ツールバー(戻る/進む/更新/アドレス/★/+)、タブ列、コンテンツ領域。

use std::cell::RefCell;

use cef::*;
use ffi_traits::TabHandle;

use crate::ctx;
use crate::tab_layout::{rows, tab_dot, tab_layout};

/// 配色(ARGB)。ツールバーは淡いラベンダー、アクティブなタブは青、＋は桃色、★は黄色。
mod palette {
    pub const BAR: u32 = 0xFFE9_E4FF;
    pub const STRIP: u32 = 0xFFDD_E2FF;
    pub const INK: u32 = 0xFF23_233B;
    pub const WHITE: u32 = 0xFFFF_FFFF;
    pub const LAGOON: u32 = 0xFF3D_6CF5;
    pub const BUBBLEGUM: u32 = 0xFFFF_6FB1;
    pub const SUNSHINE: u32 = 0xFFFF_C933;
    pub const CLOSE_BG: u32 = 0xFFFF_E3E3;
    pub const CLOSE_FG: u32 = 0xFFD6_3B3B;
}

const FONT_UI: &str = "Yu Gothic UI, Bold 12px";

#[derive(Debug, Clone, Copy)]
pub enum UiAction {
    Back,
    Forward,
    Reload,
    Bookmark,
    NewTab,
    ActivateTab(TabHandle),
    CloseTab(TabHandle),
}

pub struct TabLabel {
    pub tab: TabHandle,
    pub text: String,
    pub url: String,
    pub active: bool,
}

wrap_button_delegate! {
    struct ActionButtonDelegate {
        action: UiAction,
    }

    impl ViewDelegate {}

    impl ButtonDelegate {
        fn on_button_pressed(&self, _button: Option<&mut Button>) {
            let action = self.action;
            ctx::with(|c| c.act(action));
        }
    }
}

wrap_textfield_delegate! {
    struct AddressDelegate {
        _unused: u8,
    }

    impl ViewDelegate {}

    impl TextfieldDelegate {
        fn on_key_event(
            &self,
            textfield: Option<&mut Textfield>,
            event: Option<&KeyEvent>,
        ) -> ::std::os::raw::c_int {
            const VK_RETURN: i32 = 13;
            let (Some(textfield), Some(event)) = (textfield, event) else {
                return 0;
            };
            if event.windows_key_code != VK_RETURN
                || event.type_ != KeyEventType::from(sys::cef_key_event_type_t::KEYEVENT_RAWKEYDOWN)
            {
                return 0;
            }
            let text = CefString::from(&textfield.text()).to_string();
            ctx::with(|c| c.submit_address(&text));
            1
        }
    }
}

pub struct Ui {
    toolbar: Panel,
    tab_strip: Panel,
    content: Panel,
    address: Textfield,
    bookmark: LabelButton,
    /// ウィンドウに載せた後でないと色が反映されないため、`attach` で塗る。
    toolbar_buttons: Vec<(LabelButton, (u32, u32))>,
    window: RefCell<Option<Window>>,
    last_address: RefCell<String>,
}

fn button(text: &str, action: UiAction) -> LabelButton {
    let mut delegate = ActionButtonDelegate::new(action);
    label_button_create(Some(&mut delegate), Some(&CefString::from(text)))
        .expect("label_button_create")
}

/// 親に追加してから、背景色・文字色・ツールチップを付ける。
/// 親に追加する前に背景色を設定しても反映されないため、この順序を守る。
fn add_styled<P: ImplPanel>(
    parent: &P,
    text: &str,
    action: UiAction,
    colors: (u32, u32),
    tip: &str,
) -> LabelButton {
    let b = button(text, action);
    add_child(parent, &b);
    b.set_font_list(Some(&CefString::from(FONT_UI)));
    b.set_tooltip_text(Some(&CefString::from(tip)));
    paint(&b, colors);
    b
}

fn paint(b: &LabelButton, (bg, fg): (u32, u32)) {
    b.set_background_color(bg);
    for state in [
        ButtonState::NORMAL,
        ButtonState::HOVERED,
        ButtonState::PRESSED,
    ] {
        b.set_text_color(state, fg);
    }
}

fn box_layout(horizontal: bool) -> BoxLayoutSettings {
    BoxLayoutSettings {
        size: std::mem::size_of::<BoxLayoutSettings>(),
        horizontal: i32::from(horizontal),
        between_child_spacing: 4,
        inside_border_insets: Insets {
            top: 4,
            left: 4,
            bottom: 4,
            right: 4,
        },
        main_axis_alignment: AxisAlignment::START,
        cross_axis_alignment: AxisAlignment::STRETCH,
        ..Default::default()
    }
}

fn ellipsize(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

impl Ui {
    /// ビューを作る(ウィンドウへの配置は `attach`)。
    pub fn build() -> (Self, Panel) {
        let toolbar = panel_create(None).expect("toolbar panel");
        let tab_strip = panel_create(None).expect("tab strip panel");
        let content = panel_create(None).expect("content panel");
        content.set_to_fill_layout();

        let mut address_delegate = AddressDelegate::new(0);
        let address = textfield_create(Some(&mut address_delegate)).expect("address textfield");
        address.set_placeholder_text(Some(&CefString::from("どこへ行く? URL か検索ワードを入力")));
        address.set_font_list(Some(&CefString::from("Yu Gothic UI, 13px")));

        let mut toolbar_buttons = Vec::new();
        let layout = toolbar
            .set_to_box_layout(Some(&box_layout(true)))
            .expect("toolbar layout");
        for (text, action, tip) in [
            ("◀", UiAction::Back, "戻る"),
            ("▶", UiAction::Forward, "進む"),
            ("⟳", UiAction::Reload, "再読み込み"),
        ] {
            let colors = (palette::WHITE, palette::INK);
            toolbar_buttons.push((add_styled(&toolbar, text, action, colors, tip), colors));
        }
        add_child(&toolbar, &address);
        address.set_background_color(palette::WHITE);
        layout.set_flex_for_view(Some(&mut View::from(&address)), 1);
        let bookmark = add_styled(
            &toolbar,
            "☆",
            UiAction::Bookmark,
            (palette::WHITE, palette::INK),
            "ブックマーク",
        );
        let plus_colors = (palette::BUBBLEGUM, palette::WHITE);
        let plus = add_styled(&toolbar, "＋", UiAction::NewTab, plus_colors, "新しいタブ");
        toolbar_buttons.push((plus, plus_colors));

        tab_strip.set_to_box_layout(Some(&box_layout(false)));

        let ui = Self {
            toolbar,
            tab_strip,
            content: content.clone(),
            address,
            bookmark,
            toolbar_buttons,
            window: RefCell::new(None),
            last_address: RefCell::new(String::new()),
        };
        (ui, content)
    }

    /// ウィンドウ生成後に呼ぶ。縦に ツールバー / タブ列 / コンテンツ を並べる。
    pub fn attach(&self, window: &Window) {
        let layout = window
            .set_to_box_layout(Some(&box_layout(false)))
            .expect("window layout");
        add_child(window, &self.toolbar);
        add_child(window, &self.tab_strip);
        add_child(window, &self.content);
        layout.set_flex_for_view(Some(&mut View::from(&self.content)), 1);
        *self.window.borrow_mut() = Some(window.clone());
    }

    /// タブが多いときはラベルを縮め、行を分けて全タブに手が届くようにする。
    pub fn set_tabs(&self, tabs: &[TabLabel]) {
        self.tab_strip.remove_all_child_views();
        let (max_chars, per_row) = tab_layout(tabs.len());
        for row_tabs in rows(tabs, per_row) {
            let row = panel_create(None).expect("tab row panel");
            row.set_to_box_layout(Some(&box_layout(true)));
            add_child(&self.tab_strip, &row);
            row.set_background_color(palette::STRIP);
            for t in row_tabs {
                let label = format!("{} {}", tab_dot(&t.url), ellipsize(&t.text, max_chars));
                let colors = if t.active {
                    (palette::LAGOON, palette::WHITE)
                } else {
                    (palette::WHITE, palette::INK)
                };
                add_styled(&row, &label, UiAction::ActivateTab(t.tab), colors, &t.text);
                add_styled(
                    &row,
                    "×",
                    UiAction::CloseTab(t.tab),
                    (palette::CLOSE_BG, palette::CLOSE_FG),
                    "タブを閉じる",
                );
            }
        }
        self.tab_strip.invalidate_layout();
    }

    /// ウィンドウを表示した後に呼ぶ。表示前だとツールバーの色が反映されない。
    pub fn paint_toolbar(&self) {
        self.toolbar.set_background_color(palette::BAR);
        self.tab_strip.set_background_color(palette::STRIP);
        for (b, colors) in &self.toolbar_buttons {
            paint(b, *colors);
        }
    }

    /// 入力中の文字列を捨てて、指定の URL を表示し直す。
    pub fn reset_address(&self, url: &str) {
        self.address.set_text(Some(&CefString::from(url)));
        *self.last_address.borrow_mut() = url.to_owned();
    }

    pub fn set_address(&self, url: &str) {
        let mut last = self.last_address.borrow_mut();
        if *last != url {
            self.address.set_text(Some(&CefString::from(url)));
            *last = url.to_owned();
        }
    }

    pub fn set_bookmarked(&self, on: bool) {
        self.bookmark
            .set_text(Some(&CefString::from(if on { "★" } else { "☆" })));
        let (bg, fg) = if on {
            (palette::SUNSHINE, palette::INK)
        } else {
            (palette::WHITE, palette::INK)
        };
        self.bookmark.set_background_color(bg);
        for state in [
            ButtonState::NORMAL,
            ButtonState::HOVERED,
            ButtonState::PRESSED,
        ] {
            self.bookmark.set_text_color(state, fg);
        }
    }

    pub fn set_window_title(&self, title: &str) {
        if let Some(w) = self.window.borrow().as_ref() {
            let t = if title.is_empty() {
                "Rust Browser".to_owned()
            } else {
                format!("{title} - Rust Browser")
            };
            w.set_title(Some(&CefString::from(t.as_str())));
        }
    }
}

fn add_child<P: ImplPanel, V>(parent: &P, child: &V)
where
    for<'a> View: From<&'a V>,
{
    parent.add_child_view(Some(&mut View::from(child)));
}
