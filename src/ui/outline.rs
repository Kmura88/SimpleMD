//! # 機能: 目次とバックリンク（右側のパネル）
//! - 目次: 見出しの一覧。読んでいる位置の見出しをハイライトし、クリックでその見出しへ移動する。
//! - バックリンク: このノートへリンクしている他のノートの一覧。

use std::path::PathBuf;

use egui::{Align, Button, Layout, RichText, ScrollArea, TextWrapMode, Ui};

use super::sidebar::note_button;
use crate::app::{App, ScrollTarget};

impl App {
    pub(super) fn outline_panel(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.s;
        let backlinks = self.backlinks();
        let has_headings = self.doc().is_some_and(|d| !d.doc.headings.is_empty());

        // 目次が長くてもバックリンクが見えるよう、目次の高さを制限する。
        let outline_h = if backlinks.is_empty() {
            ui.available_height() - 80.0
        } else {
            (ui.available_height() * 0.62).max(120.0)
        };
        if has_headings {
            ui.label(
                RichText::new(s.outline)
                    .size(11.0)
                    .color(self.palette.muted)
                    .strong(),
            );
            ui.add_space(4.0);
            self.outline(ui, outline_h);
            ui.add_space(10.0);
        }

        ui.label(
            RichText::new(s.backlinks_count(backlinks.len()))
                .size(11.0)
                .color(self.palette.muted)
                .strong(),
        );
        ui.add_space(4.0);
        if backlinks.is_empty() {
            ui.label(
                RichText::new(s.no_backlinks)
                    .size(12.0)
                    .color(self.palette.muted),
            );
            return;
        }
        let mut open: Option<(PathBuf, bool)> = None;
        if let Some(tree) = &self.tree {
            ScrollArea::vertical()
                .id_salt("backlinks")
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                        ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                        for &i in &backlinks {
                            if let Some(new_tab) = note_button(ui, tree, i, &self.palette) {
                                open = Some((tree.root.join(&tree.files[i]), new_tab));
                            }
                        }
                    });
                });
        }
        if let Some((p, new_tab)) = open {
            self.open_with(&ctx, &p, None, new_tab);
        }
    }

    /// 機能: バックリンク — 開いているノートへリンクしているノートの番号。
    fn backlinks(&self) -> Vec<usize> {
        let (Some(tree), Some(doc)) = (&self.tree, self.doc()) else {
            return Vec::new();
        };
        tree.index_of(&doc.path)
            .and_then(|i| tree.index.backlinks.get(i))
            .cloned()
            .unwrap_or_default()
    }

    /// 機能: 目次 — 見出しの一覧（字下げ・現在位置のハイライト・クリックで移動）。
    fn outline(&mut self, ui: &mut Ui, max_h: f32) {
        let Some(doc) = self.doc() else { return };
        let headings = &doc.doc.headings;
        let min_level = headings.iter().map(|h| h.level).min().unwrap_or(1);
        // 読んでいる見出し: 画面の上端から少し下までにある最後の見出し。
        // ただし末尾までスクロールしたら、画面内にある最後の見出しにする（短い最後の節も選べるように）。
        let tab = self.tab();
        let at_bottom = tab.scroll_offset + self.viewport_h >= tab.content_h - 2.0;
        let probe = if at_bottom {
            tab.scroll_offset + self.viewport_h
        } else {
            tab.scroll_offset + 60.0
        };
        let current = tab
            .view
            .heading_ys
            .iter()
            .enumerate()
            .filter(|(_, y)| y.is_finite() && **y <= probe)
            .map(|(i, _)| i)
            .next_back();
        let mut jump = None;
        ScrollArea::vertical()
            .id_salt("outline")
            .max_height(max_h)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    for (i, h) in headings.iter().enumerate() {
                        let indent = (h.level - min_level) as f32 * 12.0;
                        let selected = current == Some(i);
                        let color = if selected {
                            self.palette.accent
                        } else if h.level <= min_level + 1 {
                            self.palette.text
                        } else {
                            self.palette.muted
                        };
                        let text = RichText::new(&h.text).size(13.0).color(color);
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            if ui.add(Button::selectable(selected, text)).clicked() {
                                jump = Some(i);
                            }
                        });
                    }
                });
            });
        if let Some(i) = jump {
            self.scroll_to(ScrollTarget::Heading(i));
        }
    }
}
