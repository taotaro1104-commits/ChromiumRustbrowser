//! Views ベースの最小 UI: ツールバー(戻る/進む/更新/アドレス/★/+)、タブ列、コンテンツ領域。

use std::cell::RefCell;

use cef::*;
use ffi_traits::TabHandle;

use crate::ctx;

const MAX_LABEL_CHARS: usize = 22;

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
    window: RefCell<Option<Window>>,
    last_address: RefCell<String>,
}

fn button(text: &str, action: UiAction) -> LabelButton {
    let mut delegate = ActionButtonDelegate::new(action);
    label_button_create(Some(&mut delegate), Some(&CefString::from(text)))
        .expect("label_button_create")
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

fn ellipsize(text: &str) -> String {
    let mut chars = text.chars();
    let head: String = chars.by_ref().take(MAX_LABEL_CHARS).collect();
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
        address.set_placeholder_text(Some(&CefString::from("URL または検索語")));
        let bookmark = button("☆", UiAction::Bookmark);

        let layout = toolbar
            .set_to_box_layout(Some(&box_layout(true)))
            .expect("toolbar layout");
        for (text, action) in [
            ("◀", UiAction::Back),
            ("▶", UiAction::Forward),
            ("⟳", UiAction::Reload),
        ] {
            add_child(&toolbar, &button(text, action));
        }
        add_child(&toolbar, &address);
        layout.set_flex_for_view(Some(&mut View::from(&address)), 1);
        add_child(&toolbar, &bookmark);
        add_child(&toolbar, &button("＋", UiAction::NewTab));

        tab_strip.set_to_box_layout(Some(&box_layout(true)));

        let ui = Self {
            toolbar,
            tab_strip,
            content: content.clone(),
            address,
            bookmark,
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

    pub fn set_tabs(&self, tabs: &[TabLabel]) {
        self.tab_strip.remove_all_child_views();
        for t in tabs {
            let mark = if t.active { "● " } else { "" };
            let label = format!("{mark}{}", ellipsize(&t.text));
            add_child(
                &self.tab_strip,
                &button(&label, UiAction::ActivateTab(t.tab)),
            );
            add_child(&self.tab_strip, &button("×", UiAction::CloseTab(t.tab)));
        }
        self.tab_strip.invalidate_layout();
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
