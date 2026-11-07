//! # 機能: コードブロック・数式ブロック・Mermaid 図

use egui::text::LayoutJob;
use egui::{
    Align, Color32, FontId, Frame, Layout, Margin, Rect, Stroke, TextFormat, TextWrapMode, Ui, vec2,
};

use super::Renderer;
use crate::highlight;

impl Renderer<'_> {
    // ==== 機能: コードブロック（構文ハイライト・横スクロール・コピー） ====

    /// コードの LayoutJob を作る。言語が分かれば syntect で色を付ける
    /// （色付けはバックグラウンドで行い、終わるまでは単色）。
    fn code_job(&self, ctx: &egui::Context, code: &str, lang: &str, size: f32) -> LayoutJob {
        let mut job = LayoutJob::default();
        job.wrap.max_width = f32::INFINITY;
        let fmt = |color: Color32, italics: bool| TextFormat {
            font_id: FontId::monospace(size),
            color,
            italics,
            line_height: Some(size * 1.5),
            ..Default::default()
        };
        match highlight::highlight_async(ctx, code, lang, self.pal.dark) {
            Some(spans) => {
                for s in spans.iter() {
                    job.append(&code[s.range.clone()], 0.0, fmt(s.color, s.italic));
                }
            }
            None => job.append(code, 0.0, fmt(self.pal.text, false)),
        }
        job
    }

    /// コードブロックを描く。`mermaid` は図として描画し、失敗したらコードとして表示する。
    /// 長い行は折り返さず横スクロール。マウスを乗せると言語名とコピーボタンが出る。
    pub(super) fn code_block(&mut self, ui: &mut Ui, lang: &str, code: &str) {
        if lang.eq_ignore_ascii_case("mermaid") && self.mermaid(ui, code) {
            return;
        }
        let id = self.id(ui, "code");
        let size = self.size * 0.875;
        let mut job = self.code_job(ui.ctx(), code, lang, size);
        let frame = Frame::new()
            .fill(self.pal.code_bg)
            .stroke(Stroke::new(1.0, self.pal.border))
            .corner_radius(6)
            .inner_margin(Margin::symmetric(14, 12));
        let resp = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::horizontal()
                .id_salt(id)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
                    self.show_job(ui, &mut job, &[]);
                });
        });
        let rect = resp.response.rect;
        self.copy_overlay(ui, rect, id, lang, code);
    }

    /// コードブロック右上のコピーボタン（とホバー時の言語名）。
    fn copy_overlay(&mut self, ui: &mut Ui, rect: Rect, id: egui::Id, lang: &str, code: &str) {
        let copied = self
            .copied
            .is_some_and(|(cid, t)| cid == id && self.now - t < 1.5);
        // 長い行を隠さないように、マウスが乗っているときだけ出す。
        if !(ui.rect_contains_pointer(rect) || copied) {
            return;
        }
        let label = if copied { self.s.copied } else { self.s.copy };
        let btn = Rect::from_min_size(rect.right_top() + vec2(-96.0, 6.0), vec2(90.0, 22.0));
        if !lang.is_empty() {
            ui.painter().text(
                btn.left_center() + vec2(-8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                lang,
                FontId::proportional(11.0),
                self.pal.muted,
            );
        }
        // `ui.put` で置くとレイアウトのカーソルが動き、後続のブロックがコードに重なるので使わない。
        let button_id = id.with("copy");
        if self
            .overlay_button(ui, btn, button_id, label, true)
            .clicked()
        {
            ui.ctx().copy_text(code.to_string());
            self.copied = Some((id, self.now));
        }
        if copied {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(1600));
        }
    }

    // ==== 機能: Mermaid 図 ====

    /// Mermaid 図を描く。対応していない図や構文エラーなら false を返す（呼び出し側がコード表示に切り替える）。
    fn mermaid(&mut self, ui: &mut Ui, src: &str) -> bool {
        if !crate::mermaid::is_supported(src) {
            return false;
        }
        let style = crate::mermaid::Style {
            font_size: self.size * 0.875,
            text: self.pal.text,
            line: self.pal.muted,
            node_fill: self.pal.code_bg,
            node_stroke: self.pal.border,
            accent: self.pal.accent,
            background: self.pal.bg,
            dark: self.pal.dark,
        };
        let id = self.id(ui, "mermaid");
        let mut ok = true;
        // 図は自然な大きさで描き、本文より広いときは横スクロールにする。
        Frame::new()
            .stroke(Stroke::new(1.0, self.pal.border))
            .corner_radius(6)
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::horizontal()
                    .id_salt(id)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            if crate::mermaid::show(ui, src, &style).is_err() {
                                ok = false;
                            }
                        });
                    });
            });
        ok
    }

    // ==== 機能: 数式ブロック（$$ … $$） ====

    /// 数式ブロックを描く（`math::show_display` で分数・根号などを組版。中央寄せ・背景付き）。
    /// 式が本文の幅より広いときは横スクロールにする。
    pub(super) fn math(&mut self, ui: &mut Ui, src: &str) {
        let id = self.id(ui, "math");
        let size = self.size * 1.1;
        let color = self.text_color;
        Frame::new()
            .fill(self.pal.code_bg)
            .corner_radius(6)
            .inner_margin(Margin::symmetric(14, 12))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::horizontal()
                    .id_salt(id)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down(Align::Center), |ui| {
                            crate::math::show_display(ui, src, size, color).on_hover_text(src);
                        });
                    });
            });
    }
}
