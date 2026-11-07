//! # 機能: ナビゲーション
//! - ファイルを開く（前回のスクロール位置から再開）
//! - リンクをたどる（`#見出し` / 相対パスの .md / `[[Wiki リンク]]` / 外部 URL）
//! - 戻る / 進む（タブごとの履歴）
//! - スクロール位置の予約（見出し・アンカー・検索一致へのジャンプ）
//! - ページ内検索の前後移動、全文検索の結果を開く

use std::path::{Path, PathBuf};

use super::document::load_doc;
use super::tabs::HistoryEntry;
use super::{App, QuickOpen};
use crate::markdown::{self, WIKI_PREFIX};
use crate::render::{LinkClick, ViewState};
use crate::settings::{MAX_FONT, MIN_FONT};
use crate::textsearch::Needle;
use crate::{util, workspace};

/// スクロールの行き先。
#[derive(Debug, Clone, PartialEq)]
pub enum ScrollTarget {
    /// スクロール内容の先頭からの位置。
    Y(f32),
    /// アンカー名（見出しのスラッグや脚注）。
    Anchor(String),
    /// 見出しの番号（目次から）。
    Heading(usize),
    /// ページ内検索の一致の番号。
    Match(usize),
}

/// 予約中のスクロール。位置は描画時に測るので、分かるまで数フレーム待つことがある。
pub struct PendingScroll {
    pub target: ScrollTarget,
    /// 解決を始める前に待つフレーム数。
    pub wait: u8,
    /// 解決を試みた回数（諦めるまで）。
    pub tries: u8,
}

impl App {
    // ==== 機能: ファイルを開く ====

    /// アクティブなタブでファイルを開く。
    /// `record` なら履歴に積み、前回このファイルを見ていた位置から再開する。
    pub fn open(&mut self, ctx: &egui::Context, path: &Path, anchor: Option<String>, record: bool) {
        let path = absolute(path);
        self.remember_scroll();
        if record {
            self.record_history(path.clone());
        }
        if !path.starts_with(&self.root)
            && let Some(parent) = path.parent()
        {
            self.set_root(parent.to_path_buf(), false);
        }
        let resume = if record && anchor.is_none() {
            self.settings.position_of(&path)
        } else {
            None
        };
        let tab = self.tab_mut();
        tab.doc = Some(load_doc(&path));
        tab.view = ViewState::default();
        match (anchor, resume) {
            (Some(a), _) => tab.scroll_to(ScrollTarget::Anchor(a), 0),
            (None, Some(y)) => tab.scroll_to(ScrollTarget::Y(y), 1),
            (None, None) => tab.scroll_to(ScrollTarget::Y(0.0), 0),
        }
        if self.find.open {
            self.find.current = 0;
        }
        self.reveal = Some(path.clone());
        self.settings.last_file = Some(path.clone());
        self.settings.push_recent_file(path);
        self.update_title(ctx);
    }

    /// 開き方を選んで開く（新しいタブ or 現在のタブ）。
    pub fn open_with(
        &mut self,
        ctx: &egui::Context,
        path: &Path,
        anchor: Option<String>,
        new_tab: bool,
    ) {
        if new_tab {
            self.open_in_new_tab(ctx, path, anchor);
        } else {
            self.open(ctx, path, anchor, true);
        }
    }

    // ==== 機能: 戻る / 進む ====

    fn record_history(&mut self, path: PathBuf) {
        let tab = self.tab_mut();
        let offset = tab.scroll_offset;
        if let Some(cur) = tab.history.get_mut(tab.hist_pos) {
            cur.scroll = offset;
        }
        if !tab.history.is_empty() {
            tab.history.truncate(tab.hist_pos + 1);
        }
        tab.history.push(HistoryEntry { path, scroll: 0.0 });
        if tab.history.len() > 200 {
            tab.history.remove(0);
        }
        tab.hist_pos = tab.history.len() - 1;
    }

    pub fn can_go(&self, delta: isize) -> bool {
        let tab = self.tab();
        let target = tab.hist_pos as isize + delta;
        target >= 0 && (target as usize) < tab.history.len()
    }

    /// 履歴を `delta` 進む（負なら戻る）。スクロール位置も復元する。
    pub fn go(&mut self, ctx: &egui::Context, delta: isize) {
        if !self.can_go(delta) {
            return;
        }
        let tab = self.tab_mut();
        let offset = tab.scroll_offset;
        if let Some(cur) = tab.history.get_mut(tab.hist_pos) {
            cur.scroll = offset;
        }
        tab.hist_pos = (tab.hist_pos as isize + delta) as usize;
        let entry = tab.history[tab.hist_pos].clone();
        let same = tab.path() == Some(entry.path.as_path());
        if !same {
            self.open(ctx, &entry.path, None, false);
        }
        self.tab_mut()
            .scroll_to(ScrollTarget::Y(entry.scroll), if same { 0 } else { 1 });
    }

    // ==== 機能: リンクをたどる ====

    /// クリックされたリンクを処理する。
    pub fn follow_link(&mut self, ctx: &egui::Context, click: &LinkClick) {
        let href = click.href.trim();
        // ページ内の見出しへ
        if let Some(anchor) = href.strip_prefix('#') {
            if let Some(path) = self.tab().path().map(Path::to_path_buf) {
                if click.new_tab {
                    self.open_in_new_tab(ctx, &path, Some(util::percent_decode(anchor)));
                    return;
                }
                self.record_history(path);
            }
            self.scroll_to(ScrollTarget::Anchor(util::percent_decode(anchor)));
            return;
        }
        // Wiki リンク
        if let Some(name) = href.strip_prefix(WIKI_PREFIX) {
            match self.resolve_wiki(name) {
                Some((path, anchor)) => self.open_with(ctx, &path, anchor, click.new_tab),
                None => {
                    let name = name.split('#').next().unwrap_or(name).to_string();
                    self.toast(ctx, self.s.note_not_found(&name));
                }
            }
            return;
        }
        // 外部 URL はブラウザなどで開く
        if util::has_scheme(href) && !href.starts_with("file:") {
            ctx.open_url(egui::OpenUrl::new_tab(href));
            return;
        }
        let Some((path, anchor)) = self.resolve_relative(href) else {
            return;
        };
        if path.is_dir() {
            match find_index(&path) {
                Some(index) => self.open_with(ctx, &index, anchor, click.new_tab),
                None => self.set_root(path, true),
            }
        } else if path.is_file() && workspace::is_markdown(&path) {
            self.open_with(ctx, &path, anchor, click.new_tab);
        } else if path.exists() {
            // Markdown 以外のファイルは既定のアプリで開く。
            ctx.open_url(egui::OpenUrl::new_tab(format!(
                "file://{}",
                path.to_string_lossy().replace('\\', "/")
            )));
        } else {
            self.toast(ctx, self.s.not_found(&path.display().to_string()));
        }
    }

    /// 相対リンク（`dir/note.md#見出し`）を (絶対パス, アンカー) にする。
    pub fn resolve_relative(&self, href: &str) -> Option<(PathBuf, Option<String>)> {
        let href = href.strip_prefix("file://").unwrap_or(href);
        let (target, anchor) = match href.split_once('#') {
            Some((p, a)) => (p, Some(util::percent_decode(a))),
            None => (href, None),
        };
        if target.is_empty() {
            return None;
        }
        let base = self.doc().map_or(self.root.clone(), |d| d.base_dir.clone());
        Some((util::resolve_path(target, &base), anchor))
    }

    /// Wiki リンク名 → (絶対パス, アンカー)。
    pub fn resolve_wiki(&self, name: &str) -> Option<(PathBuf, Option<String>)> {
        let tree = self.tree.as_ref()?;
        let i = tree.index.resolve_wiki(&tree.files, name)?;
        let anchor = name
            .split('|')
            .next()
            .and_then(|n| n.split_once('#'))
            .map(|(_, a)| a.trim().to_string())
            .filter(|a| !a.is_empty());
        Some((tree.root.join(&tree.files[i]), anchor))
    }

    // ==== 機能: スクロール位置の予約 ====

    pub fn scroll_to(&mut self, target: ScrollTarget) {
        self.tab_mut().scroll_to(target, 0);
    }

    /// 予約中のスクロールを、位置が分かっていれば y に変換する（描画の直前に呼ぶ）。
    pub fn take_scroll(&mut self) -> Option<f32> {
        let viewport_h = self.viewport_h;
        let tab = self.tab_mut();
        let p = tab.pending_scroll.as_mut()?;
        if p.wait > 0 {
            p.wait -= 1;
            return None;
        }
        let view = &tab.view;
        let y = match &p.target {
            ScrollTarget::Y(y) => Some(*y),
            ScrollTarget::Anchor(a) => view
                .anchors
                .get(a)
                .or_else(|| view.anchors.get(&a.to_lowercase()))
                .or_else(|| view.anchors.get(&markdown::slugify(a)))
                .map(|y| y - 12.0),
            ScrollTarget::Heading(i) => view
                .heading_ys
                .get(*i)
                .filter(|y| y.is_finite())
                .map(|y| y - 12.0),
            ScrollTarget::Match(i) => view.match_ys.get(*i).map(|y| y - viewport_h * 0.35),
        };
        if y.is_some() || p.tries > 6 {
            tab.pending_scroll = None;
        } else {
            p.tries += 1;
        }
        y.map(|y| y.max(0.0))
    }

    // ==== 機能: ページ内検索の移動 ====

    pub fn find_step(&mut self, delta: isize) {
        let n = self.tab().view.match_count;
        if n == 0 {
            return;
        }
        self.find.current = (self.find.current as isize + delta).rem_euclid(n as isize) as usize;
        self.scroll_to(ScrollTarget::Match(self.find.current));
    }

    pub fn open_find(&mut self) {
        self.find.open = true;
        self.find.focus = true;
    }

    pub fn close_find(&mut self) {
        self.find.open = false;
    }

    pub fn set_font_size(&mut self, size: f32) {
        self.settings.font_size = size.clamp(MIN_FONT, MAX_FONT).round();
    }

    pub fn open_quick(&mut self) {
        self.quick = Some(QuickOpen {
            query: String::new(),
            selected: 0,
            focus: true,
            results: Vec::new(),
            results_for: None,
        });
    }

    // ==== 機能: 全文検索の結果を開く ====

    /// 全文検索のヒットを開き、その行に近い一致へスクロールする。
    pub fn open_hit(&mut self, ctx: &egui::Context, path: &Path, line: usize, new_tab: bool) {
        let query = self.search.query.clone();
        let same = !new_tab && self.tab().path() == Some(path);
        if !same {
            self.open_with(ctx, path, None, new_tab);
        }
        // その行より前に一致がいくつあるかで、表示上の何番目の一致かを推定する。
        let before = match (std::fs::read(path), Needle::new(query.trim(), false)) {
            (Ok(bytes), Some(needle)) => super::decode(&bytes)
                .0
                .lines()
                .take(line.saturating_sub(1))
                .map(|l| needle.count(l))
                .sum::<usize>(),
            _ => 0,
        };
        self.find.open = true;
        self.find.query = query;
        self.find.case_sensitive = false;
        self.find.current = before;
        self.tab_mut()
            .scroll_to(ScrollTarget::Match(before), if same { 1 } else { 2 });
    }
}

/// パスを絶対パスにして `.` / `..` を取り除く。
pub fn absolute(p: &Path) -> PathBuf {
    util::normalize(&std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()))
}

/// フォルダの「入口」になるファイル（README / index / _index / home）を探す。
pub fn find_index(dir: &Path) -> Option<PathBuf> {
    let entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && workspace::is_markdown(p))
        .collect();
    for want in ["readme", "index", "_index", "home"] {
        if let Some(p) = entries.iter().find(|p| {
            p.file_stem()
                .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(want))
        }) {
            return Some(p.clone());
        }
    }
    None
}
