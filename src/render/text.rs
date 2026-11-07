//! # 機能: 行内テキストのレイアウト
//! - 文字の装飾（太字・斜体・取り消し線・インラインコード・マーカー・上付き/下付き・行内数式）
//! - リンクの当たり判定（ホバーで下線とツールチップ、クリック / Ctrl+クリック / 中クリック）
//! - ページ内検索の一致箇所のハイライトと位置の記録

use std::ops::Range;

use egui::text::{ByteIndex, CCursor, LayoutJob, LayoutSection};
use egui::{
    Align, Color32, CursorIcon, FontFamily, FontId, Label, Response, Stroke, TextFormat, Ui, pos2,
};

use super::{LinkClick, Renderer};
use crate::fonts;
use crate::markdown::{Run, Style, WIKI_PREFIX};

/// テキストブロック（段落・見出し・表のセルなど）の基本の見た目。
#[derive(Debug, Clone, Copy)]
pub struct TextBase {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color32,
    /// 行の高さ（`size` に対する倍率）。
    pub line_height: f32,
}

impl Renderer<'_> {
    // ==== 機能: 文字の装飾 ====

    /// 行内要素のスタイルから egui の TextFormat を作る。
    fn format(&self, style: &Style, base: &TextBase) -> TextFormat {
        let pal = self.pal;
        let mut size = base.size;
        if style.superscript || style.subscript || style.footnote_ref {
            size *= 0.75;
        }
        let family = if style.code {
            size *= 0.875;
            FontFamily::Monospace
        } else if style.strong || base.bold {
            fonts::bold()
        } else {
            FontFamily::Proportional
        };
        let color = if style.link.is_some() || style.footnote_ref {
            pal.link
        } else if style.math {
            pal.math
        } else {
            base.color
        };
        let mut f = TextFormat {
            font_id: FontId::new(size, family),
            color,
            italics: style.emphasis || base.italic,
            line_height: Some(base.size * base.line_height),
            valign: if style.superscript || style.footnote_ref {
                Align::TOP
            } else if style.subscript {
                Align::BOTTOM
            } else {
                Align::Center
            },
            ..Default::default()
        };
        if style.code {
            f.background = pal.inline_code_bg;
        }
        if style.mark {
            f.background = pal.mark_bg;
        }
        if style.strike {
            f.strikethrough = Stroke::new(1.0, color);
        }
        f
    }

    /// テキスト要素の並びから LayoutJob を作る。リンクの文字範囲（文字単位）も返す。
    pub(super) fn build_job(
        &self,
        runs: &[Run],
        base: &TextBase,
        wrap: f32,
    ) -> (LayoutJob, Vec<(Range<usize>, String)>) {
        let mut job = LayoutJob::default();
        job.wrap.max_width = wrap;
        let mut links: Vec<(Range<usize>, String)> = Vec::new();
        let mut chars = 0;
        let n = runs.len();
        for (i, run) in runs.iter().enumerate() {
            let Run::Text { text, style } = run else {
                continue;
            };
            let mut text: &str = text;
            // 改行由来の余分な空白をブロックの両端から取り除く。
            if i == 0 {
                text = text.trim_start_matches(' ');
            }
            if i + 1 == n {
                text = text.trim_end_matches(' ');
            }
            if text.is_empty() {
                continue;
            }
            let owned;
            let text = if style.code {
                // インラインコードの背景の左右に余白を作る。
                owned = format!("\u{a0}{text}\u{a0}");
                &owned
            } else if style.math {
                // 機能: 行内数式 — TeX を Unicode の近似表記にする（例: x^2 → x²）。
                owned = crate::math::to_unicode(text);
                &owned
            } else {
                text
            };
            let count = text.chars().count();
            if let Some(url) = &style.link {
                // 同じリンクが続く場合（太字を含むリンクなど）は 1 つの範囲にまとめる。
                match links.last_mut() {
                    Some((r, u)) if r.end == chars && u == url => r.end += count,
                    _ => links.push((chars..chars + count, url.clone())),
                }
            }
            job.append(text, 0.0, self.format(style, base));
            chars += count;
        }
        (job, links)
    }

    /// テキスト要素の並びをレイアウトして表示する。
    pub(super) fn text(&mut self, ui: &mut Ui, runs: &[Run], base: &TextBase) -> Response {
        let (mut job, links) = self.build_job(runs, base, ui.available_width());
        self.show_job(ui, &mut job, &links)
    }

    // ==== 機能: リンクの当たり判定・検索ハイライト ====

    /// 組み立て済みの LayoutJob を表示する（コード・数式でも使う共通処理）。
    /// 検索の一致箇所に色を塗り、リンクのホバー・クリックを処理する。
    pub(super) fn show_job(
        &mut self,
        ui: &mut Ui,
        job: &mut LayoutJob,
        links: &[(Range<usize>, String)],
    ) -> Response {
        let matches = self.highlight_matches(job);
        let galley = ui.ctx().fonts_mut(|f| f.layout_job(std::mem::take(job)));
        let response = ui.add(Label::new(galley.clone()).selectable(true));
        let origin = response.rect.min;

        for (start, _) in &matches {
            let r = galley.pos_from_cursor(CCursor::new(*start));
            self.match_ys.push(origin.y + r.top() - self.origin_y);
        }

        if !links.is_empty()
            && let Some(pointer) = response.hover_pos()
            && let Some(ci) = char_at(&galley, pointer - origin)
            && let Some((range, url)) = links.iter().find(|(r, _)| r.contains(&ci))
        {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            underline_range(ui, &galley, origin, range, self.pal.link);
            if response.clicked() || response.middle_clicked() {
                let new_tab = response.middle_clicked() || ui.input(|i| i.modifiers.command);
                self.out.clicked_link = Some(LinkClick {
                    href: url.clone(),
                    new_tab,
                });
            }
            // 機能: リンク先プレビュー — ホバー中のリンクをアプリに知らせる。
            self.out.hovered_link = Some((url.clone(), pointer));
            // 外部 URL はツールチップで行き先を見せる（ノート内リンクはプレビューが出る）。
            if crate::util::has_scheme(url) && !url.starts_with(WIKI_PREFIX) {
                response.clone().on_hover_text_at_pointer(url.as_str());
            }
        }
        response
    }

    /// LayoutJob 内の検索一致に背景色を付ける。戻り値は (先頭の文字位置, 通し番号)。
    fn highlight_matches(&mut self, job: &mut LayoutJob) -> Vec<(usize, usize)> {
        let Some(needle) = self.find else {
            return Vec::new();
        };
        let ranges = needle.find_all(&job.text);
        let mut out = Vec::with_capacity(ranges.len());
        for r in ranges {
            let index = self.match_count;
            self.match_count += 1;
            let color = if index == self.find_current {
                self.pal.find_current_bg
            } else {
                self.pal.find_bg
            };
            set_background(job, r.clone(), color);
            out.push((job.text[..r.start].chars().count(), index));
        }
        out
    }
}

/// ホバー中のリンクに下線を引く（行をまたぐリンクは行ごとに）。
fn underline_range(
    ui: &Ui,
    galley: &egui::Galley,
    origin: egui::Pos2,
    range: &Range<usize>,
    color: Color32,
) {
    let mut idx = 0;
    for row in &galley.rows {
        let mut x: Option<(f32, f32)> = None;
        let mut bottom = 0.0f32;
        for (i, g) in row.glyphs.iter().enumerate() {
            if range.contains(&(idx + i)) {
                let r = g.logical_rect();
                let (a, b) = x.unwrap_or((f32::MAX, f32::MIN));
                x = Some((a.min(r.left()), b.max(r.right())));
                bottom = bottom.max(r.bottom());
            }
        }
        if let Some((a, b)) = x {
            let y = origin.y + row.pos.y + bottom - 1.0;
            ui.painter().line_segment(
                [
                    pos2(origin.x + row.pos.x + a, y),
                    pos2(origin.x + row.pos.x + b, y),
                ],
                Stroke::new(1.0, color),
            );
        }
        idx += row.char_count_including_newline().0;
    }
}

/// galley 上の位置 `pos` にある文字の番号（文字の上でなければ None）。
pub fn char_at(galley: &egui::Galley, pos: egui::Vec2) -> Option<usize> {
    let mut idx = 0;
    for row in &galley.rows {
        let rect = row.rect();
        if pos.y >= rect.top() && pos.y < rect.bottom() {
            let x = pos.x - row.pos.x;
            return row
                .glyphs
                .iter()
                .position(|g| {
                    let r = g.logical_rect();
                    x >= r.left() && x < r.right()
                })
                .map(|i| idx + i);
        }
        idx += row.char_count_including_newline().0;
    }
    None
}

/// `job.text` のバイト範囲 `range` に背景色を付ける（必要ならセクションを分割する）。
pub fn set_background(job: &mut LayoutJob, range: Range<usize>, color: Color32) {
    let mut out: Vec<LayoutSection> = Vec::with_capacity(job.sections.len() + 2);
    for s in job.sections.drain(..) {
        let (a, b) = (s.byte_range.start.0, s.byte_range.end.0);
        let cuts = [a, range.start.clamp(a, b), range.end.clamp(a, b), b];
        let mut first = true;
        for w in cuts.windows(2) {
            if w[0] >= w[1] {
                continue;
            }
            let mut piece = s.clone();
            piece.byte_range = ByteIndex(w[0])..ByteIndex(w[1]);
            if !first {
                piece.leading_space = 0.0;
            }
            first = false;
            if w[0] >= range.start && w[1] <= range.end {
                piece.format.background = color;
            }
            out.push(piece);
        }
    }
    job.sections = out;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_split() {
        let mut job = LayoutJob::default();
        job.append("hello ", 0.0, TextFormat::default());
        let blue = TextFormat {
            color: Color32::BLUE,
            ..Default::default()
        };
        job.append("world", 0.0, blue); // 2 つ目のセクション
        set_background(&mut job, 3..8, Color32::RED);
        let pieces: Vec<(Range<usize>, bool)> = job
            .sections
            .iter()
            .map(|s| {
                (
                    s.byte_range.start.0..s.byte_range.end.0,
                    s.format.background == Color32::RED,
                )
            })
            .collect();
        assert_eq!(
            pieces,
            vec![(0..3, false), (3..6, true), (6..8, true), (8..11, false)]
        );
    }
}
