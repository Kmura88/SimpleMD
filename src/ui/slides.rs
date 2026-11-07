//! # 機能: スライド表示（プレゼンテーションモード）
//! 開いている文書をスライドに分けて全画面で 1 枚ずつ表示する（F11 で開始、Esc で終了）。
//!
//! 分け方:
//! - 文書に区切り線（`---`）があれば、区切り線ごとに 1 枚
//! - なければ、h1 / h2 の見出しごとに 1 枚
//!
//! 操作: → / ↓ / Space / PageDown / Enter で次へ、← / ↑ / PageUp / Backspace で前へ、
//! Home / End で最初 / 最後、Ctrl + / − で文字サイズ、Esc / F11 で終了。

use std::ops::Range;

use egui::{Align, CentralPanel, Frame, Key, Layout, Modifiers, RichText, ScrollArea, Ui, vec2};

use crate::app::App;
use crate::markdown::{Block, Document};
use crate::render::{self, ViewState};

/// スライド表示の状態。
pub struct SlideState {
    /// 表示中のスライド番号。
    pub index: usize,
    /// 各スライドが元の文書のどのブロック範囲か。
    pub slides: Vec<Range<usize>>,
    /// 表示中のスライドの文書（毎フレーム作り直さないよう覚えておく）。
    current: Option<(usize, Document)>,
    view: ViewState,
    /// 前のフレームで測った内容の高さ（縦方向の中央寄せに使う）。
    content_h: f32,
}

impl App {
    /// スライド表示を始める（全画面にする）。
    pub fn start_slides(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.doc() else { return };
        let slides = split_slides(&doc.doc.blocks);
        if slides.is_empty() {
            return;
        }
        self.slides = Some(SlideState {
            index: 0,
            slides,
            current: None,
            view: ViewState::default(),
            content_h: 0.0,
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }

    pub fn stop_slides(&mut self, ctx: &egui::Context) {
        self.slides = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    }

    /// スライド表示中のキー操作。
    pub fn slide_input(&mut self, ctx: &egui::Context) {
        let Some(st) = &mut self.slides else { return };
        let n = st.slides.len();
        let mut stop = false;
        let mut scale = 0.0f32;
        ctx.input_mut(|i| {
            let none = Modifiers::NONE;
            let next = [
                Key::ArrowRight,
                Key::ArrowDown,
                Key::Space,
                Key::PageDown,
                Key::Enter,
            ];
            let prev = [Key::ArrowLeft, Key::ArrowUp, Key::PageUp, Key::Backspace];
            if next.iter().any(|k| i.consume_key(none, *k)) {
                st.index = (st.index + 1).min(n - 1);
            }
            if prev.iter().any(|k| i.consume_key(none, *k)) {
                st.index = st.index.saturating_sub(1);
            }
            if i.consume_key(none, Key::Home) {
                st.index = 0;
            }
            if i.consume_key(none, Key::End) {
                st.index = n - 1;
            }
            if i.consume_key(none, Key::Escape) || i.consume_key(none, Key::F11) {
                stop = true;
            }
            if i.consume_key(Modifiers::COMMAND, Key::Plus)
                || i.consume_key(Modifiers::COMMAND, Key::Equals)
            {
                scale += 0.1;
            }
            if i.consume_key(Modifiers::COMMAND, Key::Minus) {
                scale -= 0.1;
            }
        });
        if scale != 0.0 {
            self.settings.slide_scale = (self.settings.slide_scale + scale).clamp(1.0, 3.5);
        }
        if stop {
            self.stop_slides(ctx);
        }
    }

    /// スライドを描く（文字を大きくし、縦横とも中央寄せ）。
    pub(super) fn draw_slides(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let pal = self.palette.clone();
        let s = self.s;
        let font_size = self.settings.font_size * self.settings.slide_scale;
        let Some(doc) = self.tabs[self.active].doc.as_ref() else {
            self.stop_slides(&ctx);
            return;
        };
        let base_dir = doc.base_dir.clone();
        let st = self.slides.as_mut().unwrap();
        // 表示するスライドの文書を用意する（番号が変わったときだけ）。
        if st.current.as_ref().map(|(i, _)| *i) != Some(st.index) {
            let range = st.slides[st.index].clone();
            st.current = Some((
                st.index,
                Document {
                    blocks: doc.doc.blocks[range].to_vec(),
                    footnotes: Vec::new(),
                    headings: doc.doc.headings.clone(),
                    front_matter: None,
                },
            ));
            st.view = ViewState::default();
            st.content_h = 0.0;
        }
        let (index, total) = (st.index, st.slides.len());
        let mut nav = 0isize;
        CentralPanel::default()
            .frame(Frame::new().fill(pal.bg))
            .show(ui, |ui| {
                let full = ui.available_rect_before_wrap();
                // 下部: ページ番号と操作の説明、前後ボタン
                egui::Panel::bottom("slide_footer")
                    .frame(Frame::new().inner_margin(egui::Margin::symmetric(16, 8)))
                    .show_separator_line(false)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("◀").clicked() {
                                nav = -1;
                            }
                            ui.label(
                                RichText::new(s.slide_counter(index + 1, total)).color(pal.muted),
                            );
                            if ui.button("▶").clicked() {
                                nav = 1;
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(s.slide_help).size(12.0).color(pal.muted));
                            });
                        });
                    });
                let width = (full.width() * 0.86).min(1300.0);
                let avail_h = ui.available_height();
                let (_, slide_doc) = st.current.as_ref().unwrap();
                let view = &mut st.view;
                let content_h = st.content_h;
                let out = ScrollArea::vertical()
                    .id_salt(("slide", index))
                    .auto_shrink(false)
                    .show_viewport(ui, |ui, viewport| {
                        let origin_y = ui.min_rect().top();
                        // 内容が画面より低ければ縦に中央寄せ。
                        ui.add_space(((avail_h - content_h) / 2.0).max(32.0));
                        let top = ui.cursor().top();
                        ui.horizontal_top(|ui| {
                            ui.add_space(((ui.available_width() - width) / 2.0).max(24.0));
                            ui.vertical(|ui| {
                                ui.set_width(width);
                                render::show(
                                    ui,
                                    slide_doc,
                                    view,
                                    &render::Options {
                                        palette: &pal,
                                        strings: s,
                                        font_size,
                                        base_dir: &base_dir,
                                        find: None,
                                        find_current: 0,
                                        visible: viewport.translate(vec2(0.0, origin_y)),
                                        origin_y,
                                        heading_links: false,
                                    },
                                );
                            });
                        });
                        // render::show は末尾に余白を足すので、その分を引いて実際の高さにする。
                        ui.cursor().top() - top - font_size * 6.0
                    });
                st.content_h = out.inner;
            });
        if nav != 0
            && let Some(st) = &mut self.slides
        {
            st.index = (st.index as isize + nav).clamp(0, total as isize - 1) as usize;
        }
    }
}

/// 文書のブロックをスライドに分ける（区切り線があればそれで、なければ h1/h2 で）。
pub fn split_slides(blocks: &[Block]) -> Vec<Range<usize>> {
    let mut slides = Vec::new();
    let mut start = 0;
    if blocks.iter().any(|b| matches!(b, Block::Rule)) {
        for (i, b) in blocks.iter().enumerate() {
            if matches!(b, Block::Rule) {
                slides.push(start..i);
                start = i + 1;
            }
        }
        slides.push(start..blocks.len());
    } else {
        for (i, b) in blocks.iter().enumerate() {
            if i > start && matches!(b, Block::Heading { level, .. } if *level <= 2) {
                slides.push(start..i);
                start = i;
            }
        }
        slides.push(start..blocks.len());
    }
    slides.retain(|r| !r.is_empty());
    slides
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;

    #[test]
    fn splits_on_rules_or_headings() {
        let d = parse("# A\n\none\n\n---\n\n# B\n\n---\n\n---\n\nlast");
        assert_eq!(split_slides(&d.blocks).len(), 3); // 空のスライドは除く
        let d = parse("intro\n\n# A\n\na\n\n## B\n\nb\n\n### C\n\nc");
        let s = split_slides(&d.blocks);
        assert_eq!(s.len(), 3); // intro / A / B（C は B の中）
        assert!(split_slides(&[]).is_empty());
    }
}
