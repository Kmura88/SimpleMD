//! # 機能: 表（GFM テーブル）
//! 各列の「折り返さない幅」を測り、画面に収まらなければ広い列から縮める。
//! それでも収まらない（列が多すぎる）ときは横スクロールにする。

use egui::{Align, Frame, Layout, Margin, Rect, Shape, Stroke, Ui, pos2, vec2};

use super::Renderer;
use crate::markdown::*;

impl Renderer<'_> {
    /// 表を描く。見出し行は太字・背景色付き、本文は 1 行おきに縞模様。
    #[allow(clippy::needless_range_loop)] // `c` は widths・aligns・セルの共通の添字
    pub(super) fn table(
        &mut self,
        ui: &mut Ui,
        aligns: &[ColumnAlign],
        head: &[Inline],
        rows: &[Vec<Inline>],
    ) {
        let ncols = rows
            .iter()
            .map(Vec::len)
            .chain([head.len()])
            .max()
            .unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let pad_x = self.size * 0.8;
        let pad_y = self.size * 0.4;
        let mut head_base = self.body();
        head_base.bold = true;
        let body_base = self.body();

        // 各列の自然な（折り返さない）幅を測る。
        let mut natural = vec![self.size * 2.0; ncols];
        let all_rows = std::iter::once((head, &head_base))
            .chain(rows.iter().map(|r| (r.as_slice(), &body_base)));
        for (row, base) in all_rows {
            for (c, cell) in row.iter().enumerate() {
                let (job, _) = self.build_job(&cell.runs, base, f32::INFINITY);
                let w = ui.ctx().fonts_mut(|f| f.layout_job(job)).size().x;
                natural[c] = natural[c].max(w + 1.0);
            }
        }
        let widths = fit_columns(
            &natural,
            ui.available_width() - pad_x * 2.0 * ncols as f32,
            self.size * 4.0,
        );
        let total_w: f32 = widths.iter().map(|w| w + pad_x * 2.0).sum();

        let id = self.id(ui, "table");
        egui::ScrollArea::horizontal()
            .id_salt(id)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let table_top = ui.cursor().top();
                let left = ui.cursor().left();
                let mut row_bottoms = Vec::new();
                let all = std::iter::once(head).chain(rows.iter().map(Vec::as_slice));
                for (ri, row) in all.enumerate() {
                    // 行の背景は中身を描いた後に高さが分かるので、先に場所だけ確保しておく。
                    let bg = ui.painter().add(Shape::Noop);
                    let top = ui.cursor().top();
                    ui.horizontal_top(|ui| {
                        for c in 0..ncols {
                            let align = match aligns.get(c) {
                                Some(ColumnAlign::Center) => Align::Center,
                                Some(ColumnAlign::Right) => Align::Max,
                                _ => Align::Min,
                            };
                            let w = widths[c];
                            ui.allocate_ui_with_layout(
                                vec2(w + pad_x * 2.0, 0.0),
                                Layout::top_down(Align::Min),
                                |ui| {
                                    ui.set_width(w + pad_x * 2.0);
                                    Frame::new()
                                        .inner_margin(Margin::symmetric(pad_x as i8, pad_y as i8))
                                        .show(ui, |ui| {
                                            ui.set_width(w);
                                            ui.with_layout(Layout::top_down(align), |ui| {
                                                if let Some(cell) = row.get(c) {
                                                    let base =
                                                        if ri == 0 { head_base } else { body_base };
                                                    self.inline_flow(ui, &cell.runs, &base);
                                                }
                                            });
                                        });
                                },
                            );
                        }
                    });
                    let bottom = ui.cursor().top();
                    let fill = if ri == 0 {
                        Some(self.pal.panel)
                    } else if ri % 2 == 0 {
                        Some(self.pal.table_stripe)
                    } else {
                        None
                    };
                    if let Some(fill) = fill {
                        ui.painter().set(
                            bg,
                            Shape::rect_filled(
                                Rect::from_min_max(pos2(left, top), pos2(left + total_w, bottom)),
                                0.0,
                                fill,
                            ),
                        );
                    }
                    row_bottoms.push(bottom);
                }
                // 罫線（外枠・行の区切り・列の区切り）
                let stroke = Stroke::new(1.0, self.pal.border);
                let bottom = *row_bottoms.last().unwrap_or(&table_top);
                let p = ui.painter();
                p.rect_stroke(
                    Rect::from_min_max(pos2(left, table_top), pos2(left + total_w, bottom)),
                    0.0,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                for y in &row_bottoms[..row_bottoms.len().saturating_sub(1)] {
                    p.hline(left..=left + total_w, *y, stroke);
                }
                let mut x = left;
                for w in &widths[..ncols - 1] {
                    x += w + pad_x * 2.0;
                    p.vline(x, table_top..=bottom, stroke);
                }
            });
    }
}

/// 列幅を `budget` に収める。狭い列は自然な幅のまま、広い列が残りを比例配分で分け合う
/// （ただし `min` より狭くはしない）。
pub(super) fn fit_columns(natural: &[f32], budget: f32, min: f32) -> Vec<f32> {
    let total: f32 = natural.iter().sum();
    if total <= budget || natural.is_empty() {
        return natural.to_vec();
    }
    let fair = budget / natural.len() as f32;
    let small: f32 = natural.iter().filter(|w| **w <= fair).sum();
    let large: f32 = natural.iter().filter(|w| **w > fair).sum();
    let remaining = (budget - small).max(0.0);
    natural
        .iter()
        .map(|w| {
            if *w <= fair {
                *w
            } else {
                (w / large * remaining).max(min).min(*w)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_fit() {
        assert_eq!(fit_columns(&[10.0, 20.0], 100.0, 5.0), vec![10.0, 20.0]);
        let w = fit_columns(&[10.0, 300.0, 100.0], 210.0, 20.0);
        assert_eq!(w[0], 10.0);
        assert!((w.iter().sum::<f32>() - 210.0).abs() < 0.01);
        assert!(w[1] > w[2]);
        // 最小幅より狭くはならない
        let w = fit_columns(&[500.0; 10], 100.0, 40.0);
        assert!(w.iter().all(|x| *x == 40.0));
    }
}
