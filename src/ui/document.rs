//! # 機能: 本文エリア
//! - タブバー（タブが 2 つ以上のとき。クリックで切替、× / 中クリックで閉じる、+ で新規）
//! - ページ内検索バー
//! - 本文（読みやすい幅で中央寄せ、描画結果の操作をアプリへ渡す）
//! - ようこそ画面（ファイルを開いていないタブ）

use std::path::{Path, PathBuf};

use egui::{
    Align, Button, Frame, Key, Layout, Margin, RichText, ScrollArea, Sense, TextEdit, Ui, vec2,
};

use super::{icon_button, rel_display, thousands};
use crate::app::{App, ScrollTarget};
use crate::render::{self, Output};

impl App {
    // ==== 機能: タブバー ====

    pub(super) fn tab_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let pal = self.palette.clone();
        let s = self.s;
        let mut activate = None;
        let mut close = None;
        let mut new = false;
        Frame::new()
            .fill(pal.panel)
            .inner_margin(Margin::symmetric(6, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::horizontal()
                    .id_salt("tabbar")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (i, tab) in self.tabs.iter().enumerate() {
                                let active = i == self.active;
                                let title = tab.title(s);
                                let frame = Frame::new()
                                    .fill(if active { pal.bg } else { pal.panel })
                                    .stroke(egui::Stroke::new(1.0, pal.border))
                                    .corner_radius(6)
                                    .inner_margin(Margin::symmetric(8, 3));
                                let r = frame.show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let color = if active { pal.text } else { pal.muted };
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(&title).size(13.0).color(color),
                                            )
                                            .selectable(false)
                                            .truncate(),
                                        );
                                        if ui
                                            .add(
                                                Button::new(RichText::new("×").size(13.0))
                                                    .frame_when_inactive(false)
                                                    .small(),
                                            )
                                            .on_hover_text(s.tip_close_tab)
                                            .clicked()
                                        {
                                            close = Some(i);
                                        }
                                    });
                                });
                                let resp = ui.interact(
                                    r.response.rect,
                                    ui.scope_id().with(("tab", i)),
                                    Sense::click(),
                                );
                                if resp.clicked() {
                                    activate = Some(i);
                                }
                                if resp.middle_clicked() {
                                    close = Some(i);
                                }
                                if let Some(p) = tab.path() {
                                    resp.on_hover_text(p.display().to_string());
                                }
                            }
                            if icon_button(ui, "+", true, s.tip_new_tab).clicked() {
                                new = true;
                            }
                        });
                    });
            });
        if let Some(i) = close {
            self.close_tab(&ctx, i);
        } else if let Some(i) = activate {
            self.activate_tab(&ctx, i);
        } else if new {
            self.new_tab(&ctx);
        }
    }

    // ==== 機能: ページ内検索バー ====

    pub(super) fn find_bar(&mut self, ui: &mut Ui) {
        let pal = self.palette.clone();
        let s = self.s;
        let n = self.tab().view.match_count;
        Frame::new()
            .fill(pal.panel)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label("🔍");
                    let resp = ui.add(
                        TextEdit::singleline(&mut self.find.query)
                            .hint_text(s.find_hint)
                            .desired_width(260.0),
                    );
                    if std::mem::take(&mut self.find.focus) {
                        resp.request_focus();
                    }
                    if resp.changed() {
                        // 一致の位置は次の描画で分かるので、1 フレーム待ってから移動する。
                        self.find.current = 0;
                        self.tab_mut().scroll_to(ScrollTarget::Match(0), 1);
                    }
                    if resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        let back = ui.input(|i| i.modifiers.shift);
                        self.find_step(if back { -1 } else { 1 });
                        resp.request_focus();
                    }
                    let status = if self.find.query.is_empty() {
                        String::new()
                    } else if n == 0 {
                        s.no_results.into()
                    } else {
                        format!("{} / {}", (self.find.current % n) + 1, n)
                    };
                    ui.label(RichText::new(status).size(12.0).color(pal.muted));
                    if icon_button(ui, "⬆", n > 0, s.tip_prev).clicked() {
                        self.find_step(-1);
                    }
                    if icon_button(ui, "⬇", n > 0, s.tip_next).clicked() {
                        self.find_step(1);
                    }
                    if ui
                        .selectable_label(self.find.case_sensitive, "Aa")
                        .on_hover_text(s.match_case)
                        .clicked()
                    {
                        self.find.case_sensitive ^= true;
                        self.find.current = 0;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icon_button(ui, "✕", true, s.tip_close).clicked() {
                            self.close_find();
                        }
                    });
                });
            });
    }

    // ==== 機能: 本文 ====

    pub(super) fn document(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.doc().is_none() {
            self.welcome(ui);
            return;
        }
        if let Some(err) = self.doc().and_then(|d| d.error.clone()) {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(format!("⚠  {}:\n{err}", self.s.could_not_open))
                        .color(self.palette.muted),
                );
            });
            return;
        }
        let scroll = self.take_scroll();
        let find = self.find.needle().cloned();
        let find_current = self.find.current;
        // タブの文書と描画状態を同時に借りる（別フィールドなので可能）。
        let tab = &mut self.tabs[self.active];
        let doc = tab.doc.as_ref().unwrap();
        let view = &mut tab.view;
        let pal = &self.palette;
        let settings = &self.settings;
        let strings = self.s;

        let mut area = ScrollArea::vertical()
            .id_salt(("doc", self.active, &doc.path))
            .auto_shrink(false);
        if let Some(y) = scroll {
            area = area.vertical_scroll_offset(y);
        }
        let out = area.show_viewport(ui, |ui, viewport| {
            let origin_y = ui.min_rect().top();
            let visible = viewport.translate(vec2(0.0, origin_y));
            let width = ui.available_width();
            let max_w = if settings.readable_width {
                settings.content_width
            } else {
                f32::INFINITY
            };
            let side = ((width - max_w) / 2.0).max(36.0);
            let column = (width - side * 2.0).max(120.0);
            ui.add_space(28.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(column);
                    render::show(
                        ui,
                        &doc.doc,
                        view,
                        &render::Options {
                            palette: pal,
                            strings,
                            font_size: settings.font_size,
                            base_dir: &doc.base_dir,
                            find: find.as_ref(),
                            find_current,
                            visible,
                            origin_y,
                            heading_links: true,
                        },
                    )
                })
                .inner
            })
            .inner
        });
        tab.scroll_offset = out.state.clamped_offset().y;
        tab.content_h = out.content_size.y;
        self.viewport_h = out.inner_rect.height();
        self.handle_render_output(&ctx, out.inner);
    }

    /// 描画中に起きた操作（リンク・画像・見出しリンクのコピー・ホバー）を処理する。
    fn handle_render_output(&mut self, ctx: &egui::Context, out: Output) {
        self.update_preview(ctx, out.hovered_link.clone());
        if let Some(click) = out.clicked_link {
            self.preview.clear();
            self.follow_link(ctx, &click);
        }
        if let Some(uri) = out.clicked_image {
            self.lightbox = Some(uri);
        }
        // 機能: 見出しリンクのコピー — `[見出し](フォルダからの相対パス#アンカー)` 形式
        if let Some((text, anchor)) = out.copy_heading
            && let Some(path) = self.tab().path()
        {
            let rel = path.strip_prefix(&self.root).unwrap_or(path);
            let link = format!(
                "[{}]({}#{})",
                text.trim(),
                rel_display(rel).replace(' ', "%20"),
                anchor
            );
            ctx.copy_text(link);
            self.toast(ctx, self.s.link_copied);
        }
    }

    // ==== 機能: ようこそ画面 ====

    fn welcome(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let pal = self.palette.clone();
        let s = self.s;
        let mut open: Option<(PathBuf, bool)> = None;
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(520.0);
                ui.label(
                    RichText::new("mdview")
                        .size(40.0)
                        .strong()
                        .color(pal.heading),
                );
                // 機能: バージョン表示（Cargo.toml の version）
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .size(12.0)
                        .color(pal.muted),
                );
                ui.label(RichText::new(self.root.display().to_string()).color(pal.muted));
                ui.add_space(20.0);
                if let Some(tree) = &self.tree {
                    if tree.files.is_empty() {
                        ui.label(s.no_files_found);
                        ui.label(RichText::new(s.open_another_folder).color(pal.muted));
                    } else {
                        ui.label(s.md_file_count(&thousands(tree.files.len())));
                    }
                }
                ui.add_space(12.0);
                if ui
                    .button(format!("🔎  {}  (Ctrl+P)", s.go_to_file))
                    .clicked()
                {
                    self.open_quick();
                }
                let recent: Vec<PathBuf> = self
                    .settings
                    .recent_files
                    .iter()
                    .filter(|p| p.starts_with(&self.root) && p.is_file())
                    .take(8)
                    .cloned()
                    .collect();
                if !recent.is_empty() {
                    ui.add_space(24.0);
                    ui.label(RichText::new(s.recent).size(11.0).strong().color(pal.muted));
                    for p in recent {
                        let rel = p.strip_prefix(&self.root).unwrap_or(&p).to_path_buf();
                        let resp = ui.link(rel_display(Path::new(&rel)));
                        if let Some(new_tab) = super::clicked_with_tab(ui, &resp) {
                            open = Some((p, new_tab));
                        }
                    }
                }
                ui.add_space(24.0);
                ui.label(
                    RichText::new(s.shortcuts)
                        .size(11.0)
                        .strong()
                        .color(pal.muted),
                );
                egui::Grid::new("shortcuts")
                    .num_columns(2)
                    .spacing([24.0, 4.0])
                    .show(ui, |ui| {
                        for (k, v) in s.shortcut_list {
                            ui.label(RichText::new(*k).monospace().color(pal.accent));
                            ui.label(*v);
                            ui.end_row();
                        }
                    });
            });
        });
        if let Some((p, new_tab)) = open {
            self.open_with(&ctx, &p, None, new_tab);
        }
    }
}
