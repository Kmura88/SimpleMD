//! # 機能: Markdown 文書の描画（egui）
//!
//! [`show`] が文書全体を描く入口。ブロックの種類ごとの描画は次のファイルに分かれている。
//!
//! | ファイル | 担当 |
//! |---|---|
//! | `blocks.rs` | 見出し・段落・画像・引用/注記・リスト・`<details>`・HTML・区切り線・フロントマター・脚注 |
//! | `code.rs`   | コードブロック（構文ハイライト・コピー）、数式ブロック、Mermaid 図 |
//! | `table.rs`  | 表（列幅の自動調整・横スクロール） |
//! | `text.rs`   | 行内テキストのレイアウト、リンクの当たり判定、ページ内検索のハイライト |
//!
//! ## 位置の記録
//! 見出し・アンカー・検索一致の位置（スクロール内容の先頭からの y）を [`ViewState`] に記録し、
//! アプリは次のフレームでその位置へスクロールする。
//!
//! ## 画面外の描画省略（カリング）
//! トップレベルのブロックごとに前回の高さを覚えておき、画面から十分離れたブロックは
//! 高さ分の空白だけ確保して描画を省く。長い文書でも軽く動くための仕組み。

mod blocks;
mod code;
mod table;
mod text;

use std::collections::HashMap;
use std::path::Path;

use egui::{Color32, Id, Pos2, Rect, Ui, vec2};

use crate::i18n::Strings;
use crate::markdown::*;
use crate::textsearch::Needle;
use crate::theme::Palette;

pub use text::TextBase;

/// ビューポートの上下この距離までは描画する（スクロール直後に空白が見えないように）。
const CULL_MARGIN: f32 = 600.0;

/// 描画の設定（アプリから毎フレーム渡す）。
pub struct Options<'a> {
    pub palette: &'a Palette,
    pub strings: &'a Strings,
    pub font_size: f32,
    /// Markdown ファイルのあるフォルダ（相対パスの画像の基準）。
    pub base_dir: &'a Path,
    /// ページ内検索の正規表現（検索バーが開いていて入力があるときだけ）。
    pub find: Option<&'a Needle>,
    /// 現在選択中の一致の番号（強調色で塗る）。
    pub find_current: usize,
    /// 画面に見えている範囲（スクリーン座標）。カリングに使う。
    pub visible: Rect,
    /// スクロール内容の先頭のスクリーン y。位置はここからの相対値で記録する。
    pub origin_y: f32,
    /// 見出しの横に「リンクをコピー」ボタンを出すか（プレビューやスライドでは出さない）。
    pub heading_links: bool,
}

/// 文書ごとにフレームをまたいで保持する描画状態。
#[derive(Default)]
pub struct ViewState {
    /// アンカー名 → y 位置。
    pub anchors: HashMap<String, f32>,
    /// 見出し番号（`Document::headings` の添字）→ y 位置。未計測は NaN。
    pub heading_ys: Vec<f32>,
    /// 検索一致の y 位置（文書順）。
    pub match_ys: Vec<f32>,
    /// 検索一致の総数。
    pub match_count: usize,
    /// トップレベルブロックの高さのキャッシュ（カリング用）。
    heights: Vec<Option<f32>>,
    /// 高さキャッシュが有効な条件（幅・文字サイズ）。変わったら作り直す。
    heights_key: (i32, i32),
    /// 「コピーしました」表示中のコードブロック（ID, 押した時刻）。
    copied: Option<(Id, f64)>,
}

/// リンクのクリック。
#[derive(Debug, Clone, PartialEq)]
pub struct LinkClick {
    pub href: String,
    /// Ctrl+クリック / 中クリック → 新しいタブで開く。
    pub new_tab: bool,
}

/// 描画中に起きたユーザー操作。アプリ側で処理する。
#[derive(Default)]
pub struct Output {
    /// 機能: リンクのクリック（アンカー・他ノート・外部 URL）
    pub clicked_link: Option<LinkClick>,
    /// 機能: リンク先プレビュー — マウスが乗っているリンクと、その位置
    pub hovered_link: Option<(String, Pos2)>,
    /// 機能: 画像の拡大表示 — クリックされた画像の URI
    pub clicked_image: Option<String>,
    /// 機能: 見出しリンクのコピー — (見出しの文字列, アンカー)
    pub copy_heading: Option<(String, String)>,
}

/// 描画処理の本体。描画中の状態（文字サイズ・入れ子の深さ・出力）を持つ。
pub struct Renderer<'a> {
    pal: &'a Palette,
    s: &'a Strings,
    size: f32,
    base_dir: &'a Path,
    find: Option<&'a Needle>,
    find_current: usize,
    origin_y: f32,
    heading_links: bool,
    /// 現在の文字色（引用の中では薄くするなど、入れ子で切り替える）。
    text_color: Color32,
    /// リストの入れ子の深さ（段落間の余白や記号の形に使う）。
    list_depth: usize,
    /// ウィジェット ID を一意にするための連番。
    next_id: usize,
    now: f64,
    copied: Option<(Id, f64)>,
    anchors: HashMap<String, f32>,
    heading_ys: Vec<f32>,
    match_ys: Vec<f32>,
    match_count: usize,
    out: Output,
}

/// `doc` を `ui`（縦スクロール領域の中）に描画する。
pub fn show(ui: &mut Ui, doc: &Document, view: &mut ViewState, opts: &Options) -> Output {
    let key = (
        ui.available_width().round() as i32,
        (opts.font_size * 10.0) as i32,
    );
    if view.heights_key != key || view.heights.len() != doc.blocks.len() {
        view.heights = vec![None; doc.blocks.len()];
        view.heights_key = key;
    }

    let mut r = Renderer {
        pal: opts.palette,
        s: opts.strings,
        size: opts.font_size,
        base_dir: opts.base_dir,
        find: opts.find,
        find_current: opts.find_current,
        origin_y: opts.origin_y,
        heading_links: opts.heading_links,
        text_color: opts.palette.text,
        list_depth: 0,
        next_id: 0,
        now: ui.input(|i| i.time),
        copied: view.copied,
        anchors: HashMap::new(),
        heading_ys: vec![f32::NAN; doc.headings.len()],
        match_ys: Vec::new(),
        match_count: 0,
        out: Output::default(),
    };
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

    if let Some(fm) = &doc.front_matter {
        r.front_matter(ui, fm);
    }

    // 検索中は全一致の位置が必要なので、カリングしない。
    let cull = opts.find.is_none();
    for (i, block) in doc.blocks.iter().enumerate() {
        let top = ui.cursor().top();
        let gap = if i > 0 { r.gap_size(block) } else { 0.0 };
        if let Block::Heading {
            anchor, ordinal, ..
        } = block
        {
            // 実際に描画したときに `heading()` が記録する位置と同じ値（余白の後）にそろえる。
            let y = top + gap - r.origin_y;
            if let Some(h) = r.heading_ys.get_mut(*ordinal) {
                *h = y;
            }
            r.anchors.insert(anchor.clone(), y);
        }
        if cull
            && let Some(h) = view.heights[i]
            && (top + h < opts.visible.top() - CULL_MARGIN
                || top > opts.visible.bottom() + CULL_MARGIN)
        {
            ui.add_space(h);
            continue;
        }
        ui.add_space(gap);
        let t = std::time::Instant::now();
        r.block(ui, block);
        crate::app::profile_slow(&format!("block {i} {}", block_kind(block)), t);
        view.heights[i] = Some(ui.cursor().top() - top);
    }

    if !doc.footnotes.is_empty() {
        r.footnotes(ui, &doc.footnotes);
    }
    ui.add_space(r.size * 6.0);

    view.anchors = r.anchors;
    view.heading_ys = r.heading_ys;
    view.match_ys = r.match_ys;
    view.match_count = r.match_count;
    view.copied = r.copied;
    r.out
}

impl Renderer<'_> {
    /// ブロックごとに一意なウィジェット ID を作る。
    fn id(&mut self, ui: &Ui, what: &str) -> Id {
        self.next_id += 1;
        ui.scope_id().with((what, self.next_id))
    }

    /// 機能: 重ねて置くボタン（コードのコピー・見出しリンクの 🔗）
    ///
    /// 領域を確保せずに、当たり判定と描画だけを行う。`ui.put` や `ui.add` で置くと
    /// レイアウトのカーソルや範囲が動いてしまい、後続のブロックがずれたり重なったりするため。
    fn overlay_button(
        &self,
        ui: &Ui,
        rect: Rect,
        id: Id,
        label: &str,
        framed: bool,
    ) -> egui::Response {
        let resp = ui.interact(rect, id, egui::Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        let hovered = resp.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let fill = if hovered {
            self.pal.inline_code_bg
        } else if framed {
            self.pal.bg
        } else {
            Color32::TRANSPARENT
        };
        let stroke = if framed {
            egui::Stroke::new(1.0, self.pal.border)
        } else {
            egui::Stroke::NONE
        };
        let p = ui.painter();
        p.rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);
        p.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(12.0),
            if framed {
                self.pal.text
            } else {
                self.pal.muted
            },
        );
        resp
    }

    /// 本文の文字設定。
    fn body(&self) -> TextBase {
        TextBase {
            size: self.size,
            bold: false,
            italic: false,
            color: self.text_color,
            line_height: 1.6,
        }
    }

    /// 段落間の余白（リストの中では狭くする）。
    fn para_gap(&self) -> f32 {
        if self.list_depth > 0 {
            self.size * 0.35
        } else {
            self.size * 0.9
        }
    }

    /// ブロックの前に空ける余白。見出しの前は広め。
    fn gap_size(&self, block: &Block) -> f32 {
        match block {
            Block::Heading { .. } if self.list_depth == 0 => self.size * 1.5,
            _ => self.para_gap(),
        }
    }

    /// ブロックの並びを描く（コンテナの中身用）。
    fn blocks(&mut self, ui: &mut Ui, blocks: &[Block]) {
        for (i, b) in blocks.iter().enumerate() {
            if i > 0 {
                ui.add_space(self.gap_size(b));
            }
            self.block(ui, b);
        }
    }

    /// ブロックの種類ごとに描画関数へ振り分ける。
    fn block(&mut self, ui: &mut Ui, block: &Block) {
        match block {
            Block::Heading {
                level,
                content,
                anchor,
                ordinal,
            } => self.heading(ui, *level, content, anchor, *ordinal),
            Block::Paragraph(inline) => self.paragraph(ui, inline),
            Block::Code { lang, code } => self.code_block(ui, lang, code),
            Block::Quote { alert, blocks } => self.quote(ui, *alert, blocks),
            Block::List { start, items } => self.list(ui, *start, items),
            Block::Table { aligns, head, rows } => self.table(ui, aligns, head, rows),
            Block::Math(src) => self.math(ui, src),
            Block::Html(html) => self.html(ui, html),
            Block::Rule => self.rule(ui),
            Block::Details {
                summary,
                open,
                blocks,
            } => self.details(ui, summary, *open, blocks),
        }
    }
}

/// ブロックの種類名（プロファイル表示用）。
fn block_kind(b: &Block) -> &'static str {
    match b {
        Block::Heading { .. } => "heading",
        Block::Paragraph(_) => "paragraph",
        Block::Code { .. } => "code",
        Block::Quote { .. } => "quote",
        Block::List { .. } => "list",
        Block::Table { .. } => "table",
        Block::Math(_) => "math",
        Block::Html(_) => "html",
        Block::Rule => "rule",
        Block::Details { .. } => "details",
    }
}
