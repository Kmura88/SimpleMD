//! # 機能: サイドバー
//! - ファイル: フォルダツリー（開いているファイルまで自動展開）、ファイル名の絞り込み、フォルダを開く
//! - 検索: フォルダ内の全文検索（バックグラウンド）
//! - タグ: フロントマター / 本文の `#タグ` ごとのノート一覧
//!
//! どの一覧も、クリックで現在のタブ、Ctrl+クリック / 中クリックで新しいタブに開く。

use std::path::{Path, PathBuf};

use egui::text::LayoutJob;
use egui::{
    Align, Button, FontId, Key, Layout, RichText, ScrollArea, TextEdit, TextFormat, TextWrapMode,
    Ui,
};

use super::{
    clicked_with_tab, display_name, fuzzy_label, icon_button, rank_files, rel_display, thousands,
};
use crate::app::{App, LeftTab, absolute};
use crate::textsearch::Needle;
use crate::theme::Palette;
use crate::workspace::{Node, Tree};

impl App {
    /// サイドバーの上部タブと中身。
    pub(super) fn sidebar(&mut self, ui: &mut Ui) {
        let s = self.s;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.left_tab, LeftTab::Files, s.tab_files);
            ui.selectable_value(&mut self.left_tab, LeftTab::Search, s.tab_search);
            ui.selectable_value(&mut self.left_tab, LeftTab::Tags, s.tab_tags);
        });
        ui.add_space(4.0);
        match self.left_tab {
            LeftTab::Files => self.files_tab(ui),
            LeftTab::Search => self.search_tab(ui),
            LeftTab::Tags => self.tags_tab(ui),
        }
    }

    // ==== 機能: ファイルツリーと絞り込み ====

    fn files_tab(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.s;
        ui.horizontal(|ui| {
            ui.label(RichText::new(display_name(&self.root)).strong())
                .on_hover_text(self.root.display().to_string());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                if icon_button(ui, "⟳", true, s.tip_rescan).clicked() {
                    self.rescan();
                }
                ui.menu_button("📂", |ui| self.open_folder_menu(ui))
                    .response
                    .on_hover_text(s.tip_open_folder);
            });
        });
        ui.add(
            TextEdit::singleline(&mut self.tree_filter)
                .hint_text(s.filter_hint)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);

        let mut clicked: Option<(PathBuf, bool)> = None;
        ScrollArea::vertical()
            .id_salt("tree")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    // ツリーを借りたまま self を変更できないので、一時的に取り出す。
                    let Some(tree) = self.tree.take() else {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(s.scanning);
                        });
                        return;
                    };
                    if tree.files.is_empty() {
                        ui.label(RichText::new(s.no_md_files).color(self.palette.muted));
                    } else if self.tree_filter.trim().is_empty() {
                        let current = self.doc().map(|d| d.path.clone());
                        let reveal = self.reveal.take();
                        tree_nodes(
                            ui,
                            &tree.nodes,
                            current.as_deref(),
                            reveal.as_deref(),
                            &mut clicked,
                        );
                    } else {
                        self.filtered_files(ui, &tree, &mut clicked);
                    }
                    self.tree = Some(tree);
                });
            });
        if let Some((p, new_tab)) = clicked {
            self.open_with(&ctx, &p, None, new_tab);
        }
    }

    /// 絞り込み中: あいまい検索の結果を平らな一覧で出す。
    fn filtered_files(&mut self, ui: &mut Ui, tree: &Tree, clicked: &mut Option<(PathBuf, bool)>) {
        let query = self.tree_filter.clone();
        if self.filter_cache.as_ref().is_none_or(|(q, _)| *q != query) {
            self.filter_cache = Some((query.clone(), rank_files(&tree.files, &query, 300)));
        }
        let (_, results) = self.filter_cache.as_ref().unwrap();
        if results.is_empty() {
            ui.label(RichText::new(self.s.no_matching_files).color(self.palette.muted));
        }
        let current = self.doc().map(|d| d.path.clone());
        for (idx, positions) in results {
            let rel = &tree.files[*idx];
            let path = tree.root.join(rel);
            let job = fuzzy_label(&rel_display(rel), positions, &self.palette, 13.0);
            let selected = current.as_ref() == Some(&path);
            let resp = ui.add(Button::selectable(selected, job));
            if let Some(new_tab) = clicked_with_tab(ui, &resp) {
                *clicked = Some((path, new_tab));
            }
        }
    }

    // ==== 機能: フォルダを開く（パス入力・参照ダイアログ・最近のフォルダ） ====

    fn open_folder_menu(&mut self, ui: &mut Ui) {
        ui.set_min_width(320.0);
        if self.open_folder_form(ui) {
            ui.close();
        }
    }

    /// フォルダを開くフォーム（パス入力・参照ボタン・最近のフォルダ）。
    /// サイドバーの 📂 メニューと、ダイアログが使えない環境の「フォルダを開く」ウィンドウで共用する。
    /// 何かを開いたら true（呼び出し側がメニュー / ウィンドウを閉じる）。
    pub(super) fn open_folder_form(&mut self, ui: &mut Ui) -> bool {
        let ctx = ui.ctx().clone();
        let s = self.s;
        let mut done = false;
        ui.label(RichText::new(s.open_folder_title).strong());
        let resp = ui.add(
            TextEdit::singleline(&mut self.folder_input)
                .hint_text(s.path_hint)
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.open_dialog_focus) {
            resp.request_focus();
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.horizontal(|ui| {
            if (ui.button(s.open).clicked() || enter) && !self.folder_input.trim().is_empty() {
                let p = absolute(Path::new(self.folder_input.trim()));
                if p.is_dir() {
                    self.set_root(p, true);
                    done = true;
                } else if p.is_file() {
                    self.open(&ctx, &p, None, true);
                    done = true;
                } else {
                    self.toast(&ctx, s.not_found(&p.display().to_string()));
                }
            }
            if ui.button(s.browse).clicked() {
                done = true;
                self.pick_folder();
            }
        });
        ui.label(
            RichText::new(s.drop_tip)
                .size(11.0)
                .color(self.palette.muted),
        );
        let recents: Vec<PathBuf> = self
            .settings
            .recent_roots
            .iter()
            .filter(|p| **p != self.root && p.is_dir())
            .cloned()
            .collect();
        if !recents.is_empty() {
            ui.separator();
            ui.label(
                RichText::new(s.recent_folders)
                    .size(11.5)
                    .color(self.palette.muted),
            );
            for p in recents {
                if ui
                    .button(display_name(&p))
                    .on_hover_text(p.display().to_string())
                    .clicked()
                {
                    self.set_root(p, true);
                    done = true;
                }
            }
        }
        done
    }

    // ==== 機能: 全文検索 ====

    fn search_tab(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.s;
        let resp = ui.add(
            TextEdit::singleline(&mut self.search.query)
                .hint_text(s.search_hint)
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut self.search.focus) {
            resp.request_focus();
        }
        // 入力が変わったら検索を始め直す（実行はバックグラウンド、少し待ってから）。
        let query = self.search.query.trim().to_string();
        if query != self.search.last_started {
            self.search.last_started = query.clone();
            if query.is_empty() {
                self.search.results = None;
                self.search.running = false;
            } else if let Some(tree) = &self.tree {
                self.search.running = true;
                self.search.searcher.start(
                    tree.root.clone(),
                    tree.files.clone(),
                    query.clone(),
                    ctx.clone(),
                );
            }
        }
        ui.add_space(4.0);
        let muted = self.palette.muted;
        if self.search.running {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(s.searching).color(muted));
            });
        }
        let Some(results) = &self.search.results else {
            if query.is_empty() {
                ui.label(RichText::new(s.search_help).size(12.0).color(muted));
            }
            return;
        };
        let hits = format!(
            "{}{}",
            thousands(results.hits.len()),
            if results.truncated { "+" } else { "" }
        );
        ui.label(
            RichText::new(s.search_results(&hits, results.files_with_hits))
                .size(11.5)
                .color(muted),
        );
        ui.add_space(4.0);

        let mut open: Option<(PathBuf, usize, bool)> = None;
        let needle = results.query.clone();
        ScrollArea::vertical()
            .id_salt("search_results")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                    ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                    let mut last: Option<&Path> = None;
                    for hit in &results.hits {
                        if last != Some(hit.path.as_path()) {
                            last = Some(&hit.path);
                            ui.add_space(6.0);
                            let rel = hit.path.strip_prefix(&self.root).unwrap_or(&hit.path);
                            ui.label(RichText::new(rel_display(rel)).strong().size(12.5));
                        }
                        let job = highlight_snippet(&hit.text, &needle, hit.line, &self.palette);
                        let resp = ui.add(Button::selectable(false, job));
                        if let Some(new_tab) = clicked_with_tab(ui, &resp) {
                            open = Some((hit.path.clone(), hit.line, new_tab));
                        }
                    }
                });
            });
        if let Some((path, line, new_tab)) = open {
            self.open_hit(&ctx, &path, line, new_tab);
        }
    }

    // ==== 機能: タグ ====

    /// タグの一覧（チップ）と、選択中のタグが付いたノートの一覧。
    fn tags_tab(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.s;
        let Some(tree) = self.tree.take() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(s.scanning);
            });
            return;
        };
        let mut open: Option<(PathBuf, bool)> = None;
        if tree.index.tags.is_empty() {
            ui.label(
                RichText::new(s.no_tags)
                    .size(12.0)
                    .color(self.palette.muted),
            );
        } else {
            ScrollArea::vertical()
                .id_salt("tags")
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (tag, notes) in &tree.index.tags {
                            let selected = self.selected_tag.as_deref() == Some(tag.as_str());
                            let text = format!("#{tag}  {}", notes.len());
                            if ui.selectable_label(selected, text).clicked() {
                                self.selected_tag = if selected { None } else { Some(tag.clone()) };
                            }
                        }
                    });
                    if let Some(tag) = &self.selected_tag
                        && let Some(notes) = tree.index.tags.get(tag)
                    {
                        ui.separator();
                        ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                            ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                            for &i in notes {
                                if let Some(new_tab) = note_button(ui, &tree, i, &self.palette) {
                                    open = Some((tree.root.join(&tree.files[i]), new_tab));
                                }
                            }
                        });
                    }
                });
        }
        self.tree = Some(tree);
        if let Some((p, new_tab)) = open {
            self.open_with(&ctx, &p, None, new_tab);
        }
    }
}

/// ノート 1 件のボタン（タイトルと、薄い字の相対パス）。クリックされたら `Some(new_tab)`。
pub(super) fn note_button(ui: &mut Ui, tree: &Tree, i: usize, pal: &Palette) -> Option<bool> {
    let rel = rel_display(&tree.files[i]);
    let title = tree.index.notes[i]
        .title
        .clone()
        .unwrap_or_else(|| display_name(&tree.files[i]));
    let mut job = LayoutJob::default();
    job.append(
        &title,
        0.0,
        TextFormat::simple(FontId::proportional(13.0), pal.text),
    );
    job.append(
        &format!("  {rel}"),
        0.0,
        TextFormat::simple(FontId::proportional(11.0), pal.muted),
    );
    let resp = ui.add(Button::selectable(false, job));
    clicked_with_tab(ui, &resp)
}

/// フォルダツリーを再帰的に描く。`reveal` に含まれるフォルダは開き、そのファイルまでスクロールする。
fn tree_nodes(
    ui: &mut Ui,
    nodes: &[Node],
    current: Option<&Path>,
    reveal: Option<&Path>,
    clicked: &mut Option<(PathBuf, bool)>,
) {
    for node in nodes {
        match &node.children {
            Some(children) => {
                let mut header = egui::CollapsingHeader::new(&node.name)
                    .id_salt(&node.path)
                    .default_open(false);
                if reveal.is_some_and(|r| r.starts_with(&node.path)) {
                    header = header.open(Some(true));
                }
                header.show(ui, |ui| tree_nodes(ui, children, current, reveal, clicked));
            }
            None => {
                let selected = current == Some(node.path.as_path());
                let name = Path::new(&node.name)
                    .file_stem()
                    .map_or(node.name.clone(), |s| s.to_string_lossy().into_owned());
                let resp = ui
                    .add(Button::selectable(selected, format!("📄 {name}")))
                    .on_hover_text(&node.name);
                if selected && reveal.is_some() {
                    resp.scroll_to_me(Some(Align::Center));
                }
                if let Some(new_tab) = clicked_with_tab(ui, &resp) {
                    *clicked = Some((node.path.clone(), new_tab));
                }
            }
        }
    }
}

/// 全文検索の結果 1 行（行番号と、一致部分を強調した本文）。
fn highlight_snippet(line: &str, needle: &str, line_no: usize, pal: &Palette) -> LayoutJob {
    let mut job = LayoutJob::default();
    let size = 12.5;
    job.append(
        &format!("{line_no:>4}  "),
        0.0,
        TextFormat::simple(FontId::monospace(size * 0.9), pal.muted),
    );
    let normal = TextFormat::simple(FontId::proportional(size), pal.text);
    let mut hit = normal.clone();
    hit.background = pal.find_bg;
    // 一致箇所（日本語の表記ゆれも吸収）を強調する。
    let ranges = Needle::new(needle, false)
        .map(|n| n.find_all(line))
        .unwrap_or_default();
    let mut last = 0;
    for r in ranges {
        job.append(&line[last..r.start], 0.0, normal.clone());
        job.append(&line[r.clone()], 0.0, hit.clone());
        last = r.end;
    }
    job.append(&line[last..], 0.0, normal);
    job
}
