//! # 画面（UI）
//!
//! | ファイル | 機能 |
//! |---|---|
//! | `mod.rs`      | 画面全体のレイアウト、共通の部品（アイコンボタン・あいまい検索の表示など） |
//! | `toolbar.rs`  | ツールバー（戻る/進む・パンくず・各種ボタン・⋯ メニュー）、ステータスバー |
//! | `sidebar.rs`  | サイドバー: ファイルツリー・絞り込み、全文検索、タグ |
//! | `outline.rs`  | 目次（現在位置のハイライト）とバックリンク |
//! | `document.rs` | タブバー、ページ内検索バー、本文、ようこそ画面 |
//! | `dialogs.rs`  | ファイル検索（Ctrl+P）、設定、通知（トースト）、画像の拡大表示 |
//! | `preview.rs`  | リンク先プレビュー（リンクにマウスを乗せると内容をポップアップ） |
//! | `slides.rs`   | スライド表示（プレゼンテーションモード） |

mod dialogs;
mod document;
mod outline;
pub mod preview;
mod sidebar;
pub mod slides;
mod toolbar;

use std::path::{Path, PathBuf};

use egui::text::LayoutJob;
use egui::{Button, CentralPanel, FontId, Frame, Margin, RichText, TextFormat, Ui, vec2};

use crate::app::{App, Ranked};
use crate::theme::Palette;
use crate::{textsearch, workspace};

impl App {
    /// 毎フレームの画面全体の描画。
    pub fn draw(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let pal = self.palette.clone();

        // スライド表示中はスライドだけを全画面で描く。
        if self.slides.is_some() {
            self.draw_slides(ui);
            return;
        }

        egui::Panel::top("toolbar")
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(8, 5))
                    .stroke(egui::Stroke::new(1.0, pal.border)),
            )
            .show(ui, |ui| self.toolbar(ui));

        egui::Panel::bottom("status")
            .frame(
                Frame::new()
                    .fill(pal.panel)
                    .inner_margin(Margin::symmetric(10, 3)),
            )
            .show(ui, |ui| self.status_bar(ui));

        if self.settings.show_sidebar {
            egui::Panel::left("sidebar")
                .resizable(true)
                .default_size(270.0)
                .min_size(160.0)
                .frame(Frame::new().fill(pal.panel).inner_margin(Margin::same(8)))
                .show(ui, |ui| self.sidebar(ui));
        }

        if self.settings.show_outline && self.doc().is_some() {
            egui::Panel::right("outline")
                .resizable(true)
                .default_size(240.0)
                .min_size(140.0)
                .frame(Frame::new().fill(pal.panel).inner_margin(Margin::same(8)))
                .show(ui, |ui| self.outline_panel(ui));
        }

        CentralPanel::default()
            .frame(Frame::new().fill(pal.bg))
            .show(ui, |ui| {
                if self.tabs.len() > 1 {
                    self.tab_bar(ui);
                }
                if self.find.open && self.doc().is_some() {
                    self.find_bar(ui);
                }
                let t = std::time::Instant::now();
                self.document(ui);
                crate::app::profile_slow("document", t);
            });

        // 重ねて表示するもの（ダイアログ・ポップアップ）
        self.preview_popup(&ctx);
        self.quick_open(&ctx);
        self.open_dialog_window(&ctx);
        self.settings_window(&ctx);
        self.lightbox(&ctx);
        self.toast_overlay(&ctx);
        self.drop_overlay(&ctx);
    }

    /// 機能: ドラッグ＆ドロップ — ファイルをウィンドウの上に持ってきている間、案内を出す。
    fn drop_overlay(&self, ctx: &egui::Context) {
        if ctx.input(|i| i.raw.hovered_files.is_empty()) {
            return;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::unique("drop_overlay"),
        ));
        painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha(160));
        painter.rect_stroke(
            screen.shrink(16.0),
            12.0,
            egui::Stroke::new(3.0, self.palette.accent),
            egui::StrokeKind::Inside,
        );
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            format!("📂  {}", self.s.drop_here),
            FontId::proportional(22.0),
            egui::Color32::WHITE,
        );
    }
}

// ==== 共通の部品 ====

/// アイコンだけのボタン（枠はマウスを乗せたときだけ）。
pub(crate) fn icon_button(ui: &mut Ui, icon: &str, enabled: bool, tip: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        Button::new(RichText::new(icon).size(15.0))
            .frame_when_inactive(false)
            .min_size(vec2(28.0, 26.0)),
    )
    .on_hover_text(tip)
}

/// オン/オフを表すアイコンボタン。
pub(crate) fn icon_toggle(ui: &mut Ui, icon: &str, on: bool, tip: &str) -> egui::Response {
    ui.add(Button::selectable(on, RichText::new(icon).size(15.0)).min_size(vec2(28.0, 26.0)))
        .on_hover_text(tip)
}

/// クリック（Ctrl / 中クリックで新しいタブ）の判定。`Some(new_tab)` ならクリックされた。
pub(crate) fn clicked_with_tab(ui: &Ui, resp: &egui::Response) -> Option<bool> {
    if resp.middle_clicked() {
        Some(true)
    } else if resp.clicked() {
        Some(ui.input(|i| i.modifiers.command))
    } else {
        None
    }
}

/// `files` をあいまい検索して良い順に並べる。
pub(crate) fn rank_files(files: &[PathBuf], query: &str, limit: usize) -> Ranked {
    // 機能: ローマ字検索 — IME が使えなくても `memo` で「メモ.md」を探せるよう、
    // ローマ字をひらがなにした検索語でも照合し、良い方の点数を使う。
    let kana = textsearch::romaji_to_hiragana(query.trim()).filter(|k| k.len() > 1);
    let mut scored: Vec<(i32, usize, Vec<usize>)> = files
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let name = rel_display(f);
            let plain = workspace::fuzzy_match(query, &name);
            let romaji = kana
                .as_deref()
                .and_then(|k| workspace::fuzzy_match(k, &name));
            let best = match (plain, romaji) {
                (Some(a), Some(b)) => Some(if b.0 > a.0 { b } else { a }),
                (a, b) => a.or(b),
            };
            best.map(|(s, pos)| (s, i, pos))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.truncate(limit);
    scored.into_iter().map(|(_, i, p)| (i, p)).collect()
}

/// テスト用: 並べ替えた結果のファイル番号だけを返す。
#[cfg(test)]
pub(crate) fn rank_files_for_test(files: &[PathBuf], query: &str) -> Vec<usize> {
    rank_files(files, query, 10)
        .into_iter()
        .map(|(i, _)| i)
        .collect()
}

/// あいまい検索の結果表示: 一致した文字を強調し、フォルダ部分は薄く小さく。
pub(crate) fn fuzzy_label(text: &str, positions: &[usize], pal: &Palette, size: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let normal = TextFormat::simple(FontId::proportional(size), pal.text);
    let dir_end = text.rfind('/').map_or(0, |i| i + 1);
    let dir_fmt = TextFormat::simple(FontId::proportional(size * 0.9), pal.muted);
    let hit = TextFormat::simple(FontId::new(size, crate::fonts::bold()), pal.accent);
    for (i, (byte, c)) in text.char_indices().enumerate() {
        let fmt = if positions.contains(&i) {
            hit.clone()
        } else if byte < dir_end {
            dir_fmt.clone()
        } else {
            normal.clone()
        };
        job.append(c.encode_utf8(&mut [0; 4]), 0.0, fmt);
    }
    job
}

/// 相対パスを `/` 区切りの文字列にする。
pub fn rel_display(rel: &Path) -> String {
    rel.to_string_lossy().replace('\\', "/")
}

/// パスの最後の要素（フォルダ名・ファイル名）。
pub(crate) fn display_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

/// 3 桁区切り（1234567 → "1,234,567"）。
pub(crate) fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting() {
        assert_eq!(thousands(1234567), "1,234,567");
        assert_eq!(thousands(12), "12");
        assert_eq!(rel_display(Path::new("a/b.md")), "a/b.md");
    }
}
