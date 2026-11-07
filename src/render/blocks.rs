//! # 機能: 基本ブロックの描画
//! 見出し・段落・画像・引用/注記・リスト・`<details>`・HTML ブロック・区切り線・
//! フロントマター・脚注。

use egui::{
    Color32, CornerRadius, FontId, Frame, Label, Margin, Rect, RichText, Sense, Shape, Stroke, Ui,
    pos2, vec2,
};

use super::{Renderer, TextBase};
use crate::markdown::*;
use crate::{fonts, util};

impl Renderer<'_> {
    // ==== 機能: 見出し（＋見出しリンクのコピー） ====

    /// 見出しを描く。位置を記録し、h1/h2 には下線を引く。
    /// マウスを乗せると左の余白に 🔗 ボタンが出て、`[見出し](ファイル#アンカー)` をコピーできる。
    pub(super) fn heading(
        &mut self,
        ui: &mut Ui,
        level: u8,
        content: &Inline,
        anchor: &str,
        ordinal: usize,
    ) {
        let top = ui.cursor().top();
        let y = top - self.origin_y;
        self.anchors.insert(anchor.to_string(), y);
        if let Some(h) = self.heading_ys.get_mut(ordinal) {
            *h = y;
        }
        let scale = [2.0, 1.5, 1.25, 1.0, 0.875, 0.85][(level as usize).clamp(1, 6) - 1];
        let base = TextBase {
            size: self.size * scale,
            bold: true,
            italic: false,
            color: if level == 6 {
                self.pal.muted
            } else {
                self.pal.heading
            },
            line_height: 1.3,
        };
        self.inline_flow(ui, &content.runs, &base);
        let text_bottom = ui.cursor().top();
        if level <= 2 {
            ui.add_space(self.size * 0.3);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, self.pal.border);
        }

        if self.heading_links {
            // ボタンは本文の列の左の余白に重ねて置く（`overlay_button` 参照）。
            let left = ui.max_rect().left();
            let row = Rect::from_min_max(
                pos2(left - 30.0, top),
                pos2(ui.max_rect().right(), text_bottom),
            );
            if ui.rect_contains_pointer(row) {
                let btn = Rect::from_center_size(
                    pos2(left - 16.0, top + base.size * 0.65),
                    vec2(22.0, 22.0),
                );
                let id = ui.scope_id().with(("heading_link", anchor));
                let resp = self.overlay_button(ui, btn, id, "🔗", false);
                if resp.on_hover_text(self.s.copy_heading_link).clicked() {
                    self.out.copy_heading = Some((content.plain_text(), anchor.to_string()));
                }
            }
        }
    }

    // ==== 機能: 段落 ====

    /// 段落を描く。画像だけの段落（バッジの列など）は横に並べる。
    pub(super) fn paragraph(&mut self, ui: &mut Ui, inline: &Inline) {
        if inline.is_image_only() {
            let images: Vec<&ImageRef> = inline
                .runs
                .iter()
                .filter_map(|r| match r {
                    Run::Image(img) => Some(img),
                    _ => None,
                })
                .collect();
            self.image_row(ui, &images);
        } else {
            let base = self.body();
            self.inline_flow(ui, &inline.runs, &base);
        }
    }

    /// 文字と画像が混ざった行内要素を描く（画像の前後で文字の流れを区切る）。
    pub(super) fn inline_flow(&mut self, ui: &mut Ui, runs: &[Run], base: &TextBase) {
        let mut start = 0;
        for (i, run) in runs.iter().enumerate() {
            if let Run::Image(img) = run {
                if runs[start..i].iter().any(has_text) {
                    self.text(ui, &runs[start..i], base);
                }
                self.image(ui, img);
                start = i + 1;
            }
        }
        if runs[start..].iter().any(has_text) {
            self.text(ui, &runs[start..], base);
        }
    }

    // ==== 機能: 画像（クリックで拡大表示） ====

    /// 画像を描く。幅が足りないときは縮小する。
    /// リンク付きの画像はクリックでリンクを開き、そうでなければ拡大表示（ライトボックス）を開く。
    pub(super) fn image(&mut self, ui: &mut Ui, img: &ImageRef) {
        let uri = util::image_uri(&img.url, self.base_dir);
        let mut image = egui::Image::new(uri.clone())
            .fit_to_original_size(1.0)
            .max_width(ui.available_width())
            .corner_radius(3)
            .sense(Sense::click());
        if !img.alt.is_empty() {
            image = image.alt_text(img.alt.clone());
        }
        let resp = ui.add(image);
        let tip = if img.title.is_empty() {
            &img.alt
        } else {
            &img.title
        };
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() || resp.middle_clicked() {
            match &img.link {
                Some(link) => {
                    let new_tab = resp.middle_clicked() || ui.input(|i| i.modifiers.command);
                    self.out.clicked_link = Some(super::LinkClick {
                        href: link.clone(),
                        new_tab,
                    });
                }
                None => self.out.clicked_image = Some(uri),
            }
        }
        if !tip.is_empty() {
            resp.on_hover_text(tip.as_str());
        }
    }

    /// 画像を横に並べて描く（折り返しあり）。
    pub(super) fn image_row(&mut self, ui: &mut Ui, images: &[&ImageRef]) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
            for img in images {
                self.image(ui, img);
            }
        });
    }

    // ==== 機能: 引用・注記（> [!NOTE] など） ====

    /// 引用を描く。注記（alert）なら色付きの帯とタイトルを付ける。
    pub(super) fn quote(&mut self, ui: &mut Ui, alert: Option<Alert>, blocks: &[Block]) {
        let bar = alert.map_or(self.pal.border, |a| self.pal.alert(a));
        let saved = self.text_color;
        if alert.is_none() {
            self.text_color = self.pal.muted;
        }
        let resp = Frame::new()
            .inner_margin(Margin {
                left: 16,
                right: 0,
                top: 2,
                bottom: 2,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(a) = alert {
                    ui.label(
                        RichText::new(format!("{}  {}", a.icon(), self.alert_title(a)))
                            .font(FontId::new(self.size, fonts::bold()))
                            .color(bar),
                    );
                    ui.add_space(self.size * 0.4);
                }
                self.blocks(ui, blocks);
            });
        let r = resp.response.rect;
        ui.painter().rect_filled(
            Rect::from_min_max(r.left_top(), pos2(r.left() + 4.0, r.bottom())),
            CornerRadius::same(2),
            bar,
        );
        self.text_color = saved;
    }

    /// 注記のタイトル（表示言語に合わせる）。
    fn alert_title(&self, a: Alert) -> &'static str {
        match a {
            Alert::Note => self.s.alert_note,
            Alert::Tip => self.s.alert_tip,
            Alert::Important => self.s.alert_important,
            Alert::Warning => self.s.alert_warning,
            Alert::Caution => self.s.alert_caution,
        }
    }

    // ==== 機能: リスト（番号付き・箇条書き・タスク） ====

    /// リストを描く。左に記号の列を確保し、右に項目の中身を描く。
    pub(super) fn list(&mut self, ui: &mut Ui, start: Option<u64>, items: &[ListItem]) {
        self.list_depth += 1;
        let depth = self.list_depth;
        let size = self.size;
        // 番号付きリストは最大の番号が収まる幅を確保する。
        let marker_w = match start {
            Some(s) => {
                let widest = format!("{}.", s + items.len().saturating_sub(1) as u64);
                ui.ctx().fonts_mut(|f| {
                    f.layout_no_wrap(widest, FontId::proportional(size), Color32::WHITE)
                        .size()
                        .x
                }) + size * 0.6
            }
            None => size * 1.5,
        };
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                ui.add_space(size * 0.3);
            }
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let line_h = size * 1.6;
                let w = if item.task.is_some() {
                    size * 1.6
                } else {
                    marker_w
                };
                let (rect, _) = ui.allocate_exact_size(vec2(w, line_h), Sense::hover());
                self.list_marker(ui, rect, start.map(|s| s + i as u64), item.task, depth);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.blocks(ui, &item.blocks);
                });
            });
        }
        self.list_depth -= 1;
    }

    /// リストの記号を描く: タスクのチェックボックス / 番号 / 深さ別の黒丸・白丸・四角。
    fn list_marker(
        &self,
        ui: &Ui,
        rect: Rect,
        number: Option<u64>,
        task: Option<bool>,
        depth: usize,
    ) {
        let p = ui.painter();
        let size = self.size;
        let color = self.text_color;
        if let Some(checked) = task {
            let s = size * 0.85;
            let b = Rect::from_center_size(
                pos2(rect.left() + s / 2.0 + 1.0, rect.center().y),
                vec2(s, s),
            );
            if checked {
                p.rect_filled(b, 3.0, self.pal.accent);
                let pts = vec![
                    pos2(b.left() + s * 0.22, b.center().y),
                    pos2(b.left() + s * 0.42, b.bottom() - s * 0.25),
                    pos2(b.right() - s * 0.2, b.top() + s * 0.25),
                ];
                p.add(Shape::line(pts, Stroke::new(2.0, Color32::WHITE)));
            } else {
                p.rect_stroke(
                    b,
                    3.0,
                    Stroke::new(1.5, self.pal.muted),
                    egui::StrokeKind::Inside,
                );
            }
        } else if let Some(n) = number {
            p.text(
                pos2(rect.right() - size * 0.4, rect.center().y),
                egui::Align2::RIGHT_CENTER,
                format!("{n}."),
                FontId::proportional(size),
                color,
            );
        } else {
            let c = pos2(rect.left() + size * 0.55, rect.center().y);
            let r = size * 0.17;
            match depth {
                1 => {
                    p.circle_filled(c, r, color);
                }
                2 => {
                    p.circle_stroke(c, r, Stroke::new(1.2, color));
                }
                _ => {
                    p.rect_filled(
                        Rect::from_center_size(c, vec2(r * 1.8, r * 1.8)),
                        0.0,
                        color,
                    );
                }
            }
        }
    }

    // ==== 機能: <details> 折りたたみ ====

    /// `<details>` を描く。見出し行のクリックで開閉し、状態はウィジェット ID ごとに覚える。
    pub(super) fn details(
        &mut self,
        ui: &mut Ui,
        summary: &Inline,
        default_open: bool,
        blocks: &[Block],
    ) {
        let id = self.id(ui, "details");
        let open = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(id))
            .unwrap_or(default_open);
        let resp = Frame::new()
            .stroke(Stroke::new(1.0, self.pal.border))
            .corner_radius(6)
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // 見出し行: ▸/▾ と summary。行全体をクリック可能にする。
                let header = ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if open { "▾" } else { "▸" })
                            .size(self.size)
                            .color(self.pal.muted),
                    );
                    ui.add_space(6.0);
                    let mut base = self.body();
                    base.bold = true;
                    self.text(ui, &summary.runs, &base);
                });
                let click = ui.interact(
                    header.response.rect,
                    ui.scope_id().with("hdr"),
                    Sense::click(),
                );
                if click.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if click.clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(id, !open));
                }
                if open {
                    ui.add_space(self.size * 0.6);
                    self.blocks(ui, blocks);
                }
            });
        let _ = resp;
    }

    // ==== 機能: HTML ブロック（画像と文字だけ表示。スクリプトは実行しない） ====

    pub(super) fn html(&mut self, ui: &mut Ui, html: &HtmlBlock) {
        if !html.images.is_empty() {
            let images: Vec<&ImageRef> = html.images.iter().collect();
            self.image_row(ui, &images);
        }
        if !html.text.is_empty() {
            if !html.images.is_empty() {
                ui.add_space(self.size * 0.4);
            }
            let mut base = self.body();
            base.color = self.pal.muted;
            let runs = [Run::Text {
                text: html.text.clone(),
                style: Style::default(),
            }];
            self.text(ui, &runs, &base);
        }
    }

    // ==== 機能: 区切り線 ====

    pub(super) fn rule(&self, ui: &mut Ui) {
        ui.add_space(self.size * 0.4);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
        ui.painter().rect_filled(rect, 1.5, self.pal.border);
        ui.add_space(self.size * 0.4);
    }

    // ==== 機能: フロントマター（折りたたみ表示） ====

    pub(super) fn front_matter(&mut self, ui: &mut Ui, src: &str) {
        let id = self.id(ui, "front_matter");
        let open = ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        let arrow = if open { "▾" } else { "▸" };
        let resp = ui.add(
            Label::new(
                RichText::new(format!("{arrow}  {}", self.s.front_matter))
                    .color(self.pal.muted)
                    .size(self.size * 0.8),
            )
            .selectable(false)
            .sense(Sense::click()),
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            ui.ctx().data_mut(|d| d.insert_temp(id, !open));
        }
        if open {
            ui.add_space(self.size * 0.4);
            self.code_block(ui, "yaml", src);
        }
        ui.add_space(self.size);
    }

    // ==== 機能: 脚注（文書の末尾に一覧） ====

    pub(super) fn footnotes(&mut self, ui: &mut Ui, notes: &[Footnote]) {
        ui.add_space(self.size * 2.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, self.pal.border);
        ui.add_space(self.size * 0.8);
        let saved = (self.size, self.text_color);
        self.size *= 0.875;
        self.text_color = self.pal.muted;
        for note in notes {
            let y = ui.cursor().top() - self.origin_y;
            self.anchors.insert(note.anchor.clone(), y);
            ui.horizontal_top(|ui| {
                ui.add_sized(
                    vec2(self.size * 2.2, self.size * 1.6),
                    Label::new(
                        RichText::new(format!("{}.", note.number))
                            .size(self.size)
                            .color(self.pal.muted),
                    ),
                );
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    self.blocks(ui, &note.blocks);
                });
            });
            ui.add_space(self.size * 0.4);
        }
        (self.size, self.text_color) = saved;
    }
}

/// 空白以外の文字を含むテキスト要素か。
fn has_text(run: &Run) -> bool {
    matches!(run, Run::Text { text, .. } if !text.trim().is_empty())
}
