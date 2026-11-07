//! # 機能: タブ
//! 各タブが「開いている文書・描画状態・戻る/進むの履歴・スクロール位置」を持つ。
//! 新しいタブ（Ctrl+T）、閉じる（Ctrl+W）、切り替え（Ctrl+Tab）、
//! Ctrl+クリック / 中クリックでリンクを新しいタブで開く。

use std::path::{Path, PathBuf};

use super::document::OpenDoc;
use super::{App, PendingScroll, ScrollTarget};
use crate::i18n::Strings;
use crate::render::ViewState;

/// 履歴の 1 項目（どのファイルのどこを見ていたか）。
#[derive(Clone)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub scroll: f32,
}

/// 1 つのタブ。
#[derive(Default)]
pub struct Tab {
    /// 開いている文書（`None` ならようこそ画面）。
    pub doc: Option<OpenDoc>,
    pub view: ViewState,
    pub history: Vec<HistoryEntry>,
    pub hist_pos: usize,
    /// 次のフレームで行うスクロール。
    pub pending_scroll: Option<PendingScroll>,
    /// 現在のスクロール位置。
    pub scroll_offset: f32,
    /// 本文全体の高さ（末尾までスクロールしたかの判定に使う）。
    pub content_h: f32,
}

impl Tab {
    /// タブの見出し（ファイル名から拡張子を除いたもの）。
    pub fn title(&self, s: &Strings) -> String {
        match &self.doc {
            Some(d) => d
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            None => s.tab_welcome.to_string(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.doc.as_ref().map(|d| d.path.as_path())
    }

    /// 次のフレームでスクロールする位置を予約する。
    pub fn scroll_to(&mut self, target: ScrollTarget, wait: u8) {
        self.pending_scroll = Some(PendingScroll {
            target,
            wait,
            tries: 0,
        });
    }
}

impl App {
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// アクティブなタブの文書。
    pub fn doc(&self) -> Option<&OpenDoc> {
        self.tab().doc.as_ref()
    }

    /// 新しいタブ（ようこそ画面）を開いて切り替える。
    pub fn new_tab(&mut self, ctx: &egui::Context) {
        self.remember_scroll();
        self.tabs.push(Tab::default());
        self.active = self.tabs.len() - 1;
        self.on_tab_changed(ctx);
    }

    /// ファイルを新しいタブで開く。
    pub fn open_in_new_tab(&mut self, ctx: &egui::Context, path: &Path, anchor: Option<String>) {
        self.new_tab(ctx);
        self.open(ctx, path, anchor, true);
    }

    /// タブを閉じる。最後の 1 つは閉じずに空にする。
    pub fn close_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        if let Some(d) = &self.tabs[i].doc {
            let (path, y) = (d.path.clone(), self.tabs[i].scroll_offset);
            self.settings.remember_position(&path, y);
        }
        if self.tabs.len() == 1 {
            self.tabs[0] = Tab::default();
        } else {
            self.tabs.remove(i);
            if self.active > i || self.active >= self.tabs.len() {
                self.active = self.active.saturating_sub(1);
            }
        }
        self.on_tab_changed(ctx);
    }

    /// タブを切り替える。
    pub fn activate_tab(&mut self, ctx: &egui::Context, i: usize) {
        if i < self.tabs.len() && i != self.active {
            self.remember_scroll();
            self.active = i;
            self.on_tab_changed(ctx);
        }
    }

    /// 隣のタブへ（`delta` = ±1、端で一周する）。
    pub fn cycle_tab(&mut self, ctx: &egui::Context, delta: isize) {
        let n = self.tabs.len() as isize;
        let i = (self.active as isize + delta).rem_euclid(n) as usize;
        self.activate_tab(ctx, i);
    }

    /// タブが変わったときの後処理: 検索位置のリセット、フォルダ外のファイルならフォルダを切り替え、
    /// 裏にあった間にファイルが変わっていれば再読み込み。
    fn on_tab_changed(&mut self, ctx: &egui::Context) {
        self.find.current = 0;
        if let Some(path) = self.tab().path().map(Path::to_path_buf) {
            if !path.starts_with(&self.root)
                && let Some(parent) = path.parent()
            {
                self.set_root(parent.to_path_buf(), false);
            }
            self.reveal = Some(path);
            self.check_reload(ctx, false);
        }
        self.update_title(ctx);
    }

    /// アクティブなタブのスクロール位置を「ファイルごとの位置」として覚える。
    pub fn remember_scroll(&mut self) {
        let tab = self.tab();
        if let Some(d) = &tab.doc {
            let (path, y) = (d.path.clone(), tab.scroll_offset);
            self.settings.remember_position(&path, y);
        }
    }

    /// 終了時: 開いているタブと各タブの位置を設定に書き出す。
    pub fn store_session(&mut self) {
        for t in &self.tabs {
            if let Some(d) = &t.doc {
                self.settings.remember_position(&d.path, t.scroll_offset);
            }
        }
        self.settings.open_tabs = self
            .tabs
            .iter()
            .map(|t| t.path().map(Path::to_path_buf))
            .collect();
        self.settings.active_tab = self.active;
    }
}
