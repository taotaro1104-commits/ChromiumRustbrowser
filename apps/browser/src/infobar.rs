//! ツールバーの下に出す確認バー。メッセージと、ボタンを数個並べる(パスワードの保存・自動入力の確認など)。
//! 表示中だけ場所を取り、隠れているときはレイアウトに影響しない。

use cef::*;

use crate::ui::{UiAction, add_styled, box_layout, palette};

pub struct InfoBar {
    panel: Panel,
}

impl InfoBar {
    pub fn new() -> Self {
        let panel = panel_create(None).expect("infobar panel");
        panel.set_to_box_layout(Some(&box_layout(true)));
        panel.set_visible(0);
        Self { panel }
    }

    pub fn panel(&self) -> &Panel {
        &self.panel
    }

    /// メッセージとボタンを表示する。ボタンは (ラベル, 押したときの操作)。
    pub fn show(&self, message: &str, buttons: &[(String, UiAction)]) {
        self.panel.remove_all_child_views();
        self.panel.set_background_color(palette::INFO_BG);
        // メッセージは押せない平らなボタンとして置く(`UiAction::None`)。
        add_styled(
            &self.panel,
            message,
            UiAction::None,
            (palette::INFO_BG, palette::INK),
            "",
        );
        for (label, action) in buttons {
            add_styled(
                &self.panel,
                label,
                *action,
                (palette::WHITE, palette::INK),
                "",
            );
        }
        self.panel.set_visible(1);
        self.panel.invalidate_layout();
    }

    pub fn hide(&self) {
        self.panel.set_visible(0);
        self.panel.remove_all_child_views();
    }
}
