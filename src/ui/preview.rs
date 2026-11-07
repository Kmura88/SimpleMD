//! # 機能: リンク先プレビュー
//! ノート内リンク（`#見出し` / 相対パスの .md / `[[Wiki リンク]]`）にマウスを 0.35 秒乗せると、
//! リンク先の内容（アンカー付きならその節）を小さなポップアップで表示する。
//! 外部 URL はツールチップで URL を出すだけ（`render/text.rs`）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use egui::{Frame, Id, Margin, Pos2, RichText, ScrollArea, vec2};

use crate::app::{App, load_doc};
use crate::markdown::{Block, Document, WIKI_PREFIX, slugify};
use crate::render::{self, ViewState};
use crate::util;
use crate::workspace;

/// マウスを乗せてから表示するまでの時間（秒）。
const DELAY: f64 = 0.35;
/// プレビューに出すブロック数の上限。
const MAX_BLOCKS: usize = 14;

/// キャッシュの中身: (更新時刻, 切り出した文書, 見出し)。
type Cached = (Option<SystemTime>, Arc<Document>, String);

/// プレビューの状態。
#[derive(Default)]
pub struct PreviewState {
    /// 乗せているリンク、位置、乗せ始めた時刻。
    hover: Option<(String, Pos2, f64)>,
    /// (パス, アンカー) → (更新時刻, 切り出した文書, 見出し)。
    cache: HashMap<(PathBuf, Option<String>), Cached>,
    view: ViewState,
}

impl PreviewState {
    /// ファイルが変わったらキャッシュを捨てる。
    pub fn invalidate(&mut self, path: &Path) {
        self.cache.retain(|(p, _), _| p != path);
    }

    /// クリックなどでプレビューを消す。
    pub fn clear(&mut self) {
        self.hover = None;
    }
}

impl App {
    /// 描画結果からホバー中のリンクを受け取る。
    pub(super) fn update_preview(&mut self, ctx: &egui::Context, hovered: Option<(String, Pos2)>) {
        let now = ctx.input(|i| i.time);
        self.preview.hover = match (hovered, self.preview.hover.take()) {
            (Some((href, pos)), Some((old, _, since))) if old == href => Some((href, pos, since)),
            (Some((href, pos)), _) => Some((href, pos, now)),
            (None, _) => None,
        };
    }

    /// プレビューのポップアップを描く。
    pub(super) fn preview_popup(&mut self, ctx: &egui::Context) {
        let Some((href, pos, since)) = self.preview.hover.clone() else {
            return;
        };
        let now = ctx.input(|i| i.time);
        if now - since < DELAY {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                DELAY - (now - since) + 0.01,
            ));
            return;
        }
        let Some((path, anchor)) = self.preview_target(&href) else {
            return;
        };
        let Some((doc, title)) = self.preview_doc(&path, anchor) else {
            return;
        };
        let base_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let pal = self.palette.clone();
        let size = self.settings.font_size * 0.85;
        let strings = self.s;
        let view = &mut self.preview.view;
        egui::Area::new(Id::unique("link_preview"))
            .order(egui::Order::Tooltip)
            .fixed_pos(pos + vec2(14.0, 18.0))
            .interactable(false)
            .constrain(true)
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .fill(pal.bg)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_width(440.0);
                        ui.label(RichText::new(&title).size(11.5).color(pal.muted));
                        ui.separator();
                        ScrollArea::vertical()
                            .id_salt("preview_scroll")
                            .max_height(300.0)
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show_viewport(ui, |ui, viewport| {
                                let origin_y = ui.min_rect().top();
                                render::show(
                                    ui,
                                    &doc,
                                    view,
                                    &render::Options {
                                        palette: &pal,
                                        strings,
                                        font_size: size,
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
            });
    }

    /// リンクがノート内リンクなら (パス, アンカー) を返す。外部 URL などは None。
    fn preview_target(&self, href: &str) -> Option<(PathBuf, Option<String>)> {
        if let Some(anchor) = href.strip_prefix('#') {
            let path = self.tab().path()?.to_path_buf();
            return Some((path, Some(util::percent_decode(anchor))));
        }
        if let Some(name) = href.strip_prefix(WIKI_PREFIX) {
            return self.resolve_wiki(name);
        }
        if util::has_scheme(href) {
            return None;
        }
        let (path, anchor) = self.resolve_relative(href)?;
        (path.is_file() && workspace::is_markdown(&path)).then_some((path, anchor))
    }

    /// プレビュー用の文書（アンカーがあればその節だけ）を作る。キャッシュする。
    fn preview_doc(
        &mut self,
        path: &Path,
        anchor: Option<String>,
    ) -> Option<(Arc<Document>, String)> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let key = (path.to_path_buf(), anchor.clone());
        if let Some((m, doc, title)) = self.preview.cache.get(&key)
            && *m == modified
        {
            return Some((doc.clone(), title.clone()));
        }
        let full = load_doc(path);
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (blocks, title) = match &anchor {
            Some(a) => {
                let section = section_blocks(&full.doc, a)?;
                (section, format!("{name} › {a}"))
            }
            None => (
                full.doc.blocks.iter().take(MAX_BLOCKS).cloned().collect(),
                name,
            ),
        };
        let doc = Arc::new(Document {
            blocks,
            footnotes: Vec::new(),
            headings: full.doc.headings.clone(),
            front_matter: None,
        });
        self.preview.view = ViewState::default();
        self.preview
            .cache
            .insert(key, (modified, doc.clone(), title.clone()));
        Some((doc, title))
    }
}

/// アンカーの見出しから、同じか上のレベルの次の見出しまでのブロック（最大 `MAX_BLOCKS`）。
fn section_blocks(doc: &Document, anchor: &str) -> Option<Vec<Block>> {
    let wanted = [anchor.to_string(), anchor.to_lowercase(), slugify(anchor)];
    let start = doc
        .blocks
        .iter()
        .position(|b| matches!(b, Block::Heading { anchor: a, .. } if wanted.contains(a)))?;
    let Block::Heading { level, .. } = &doc.blocks[start] else {
        return None;
    };
    let level = *level;
    let end = doc.blocks[start + 1..]
        .iter()
        .position(|b| matches!(b, Block::Heading { level: l, .. } if *l <= level))
        .map_or(doc.blocks.len(), |i| start + 1 + i);
    Some(
        doc.blocks[start..end]
            .iter()
            .take(MAX_BLOCKS)
            .cloned()
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;

    #[test]
    fn extracts_sections() {
        let d = parse("# A\na\n## B\nb1\n\nb2\n### C\nc\n## D\nd");
        let s = section_blocks(&d, "b").unwrap();
        // B の見出し・b1・b2・C の見出し・c（D の手前まで）
        assert_eq!(s.len(), 5);
        assert!(section_blocks(&d, "nope").is_none());
        assert_eq!(section_blocks(&d, "D").unwrap().len(), 2);
    }
}
