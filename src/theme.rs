//! # 機能: 配色（ライト / ダーク）
//!
//! - テーマの切り替えモード（システム設定に従う / ライト / ダーク）
//! - GitHub の Markdown 表示を手本にした配色パレット
//! - 注記（NOTE / TIP など）ごとの色
//! - パレットに合わせた egui の `Visuals`

use egui::{Color32, Visuals};
use serde::{Deserialize, Serialize};

use crate::markdown::Alert;

// ==== 機能: テーマモード（システム / ライト / ダーク） ====

/// テーマの選択。設定ファイルに保存される。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    /// ボタンを押したときの次のモード（システム → ライト → ダーク → システム）。
    pub fn next(self) -> Self {
        match self {
            ThemeMode::System => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::System,
        }
    }

    /// 切り替えボタンに表示するアイコン文字。
    pub fn icon(self) -> &'static str {
        match self {
            ThemeMode::System => "🌓",
            ThemeMode::Light => "☀",
            ThemeMode::Dark => "🌙",
        }
    }
}

// ==== 機能: 配色パレット ====

/// 描画で使う色の一式。
#[derive(Debug, Clone)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub panel: Color32,
    pub text: Color32,
    pub heading: Color32,
    pub muted: Color32,
    pub link: Color32,
    pub border: Color32,
    pub code_bg: Color32,
    pub inline_code_bg: Color32,
    pub table_stripe: Color32,
    pub mark_bg: Color32,
    pub find_bg: Color32,
    pub find_current_bg: Color32,
    pub accent: Color32,
    /// 行内数式の文字色
    pub math: Color32,
    pub selection: Color32,
}

/// `0xRRGGBB` 形式の数値から色を作る。
const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Palette {
    /// ダークモードかどうかに応じたパレットを作る。
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                bg: hex(0x0d1117),
                panel: hex(0x151b23),
                text: hex(0xe6edf3),
                heading: hex(0xf0f6fc),
                muted: hex(0x9198a1),
                link: hex(0x4493f8),
                border: hex(0x3d444d),
                code_bg: hex(0x151b23),
                inline_code_bg: hex(0x2a313c),
                table_stripe: hex(0x151b23),
                mark_bg: hex(0x5c4a12),
                find_bg: hex(0x6e5a1e),
                find_current_bg: hex(0xc27b10),
                accent: hex(0x2f81f7),
                math: hex(0xd2a8ff),
                selection: Color32::from_rgba_unmultiplied(56, 139, 253, 90),
            }
        } else {
            Self {
                dark,
                bg: hex(0xffffff),
                panel: hex(0xf6f8fa),
                text: hex(0x1f2328),
                heading: hex(0x1f2328),
                muted: hex(0x59636e),
                link: hex(0x0969da),
                border: hex(0xd1d9e0),
                code_bg: hex(0xf6f8fa),
                inline_code_bg: hex(0xeaedf0),
                table_stripe: hex(0xf6f8fa),
                mark_bg: hex(0xfff8c5),
                find_bg: hex(0xfff1a8),
                find_current_bg: hex(0xffb84d),
                accent: hex(0x0969da),
                math: hex(0x6639ba),
                selection: Color32::from_rgba_unmultiplied(84, 174, 255, 90),
            }
        }
    }

    // ==== 機能: 注記の色 ====

    /// 注記（NOTE / TIP / IMPORTANT / WARNING / CAUTION）の種類ごとの色。
    pub fn alert(&self, alert: Alert) -> Color32 {
        match (alert, self.dark) {
            (Alert::Note, false) => hex(0x0969da),
            (Alert::Tip, false) => hex(0x1a7f37),
            (Alert::Important, false) => hex(0x8250df),
            (Alert::Warning, false) => hex(0x9a6700),
            (Alert::Caution, false) => hex(0xd1242f),
            (Alert::Note, true) => hex(0x4493f8),
            (Alert::Tip, true) => hex(0x3fb950),
            (Alert::Important, true) => hex(0xab7df8),
            (Alert::Warning, true) => hex(0xd29922),
            (Alert::Caution, true) => hex(0xf85149),
        }
    }

    // ==== 機能: egui の見た目（Visuals）への反映 ====

    /// パレットに合わせた egui の `Visuals` を作る。
    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        v.panel_fill = self.panel;
        v.window_fill = self.panel;
        v.extreme_bg_color = self.bg;
        v.faint_bg_color = self.table_stripe;
        v.hyperlink_color = self.link;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.selection;
        v.selection.stroke.color = self.text;
        v.widgets.noninteractive.bg_stroke.color = self.border;
        v.window_stroke.color = self.border;
        v
    }
}
