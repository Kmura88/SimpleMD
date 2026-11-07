//! # アプリ本体（状態と毎フレームの処理）
//!
//! | ファイル | 機能 |
//! |---|---|
//! | `mod.rs`        | アプリの状態、起動時の復元、毎フレームの更新（バックグラウンド処理の受け取り・自動再読み込み） |
//! | `document.rs`   | ファイルの読み込み（UTF-8 / Shift_JIS） |
//! | `tabs.rs`       | タブ（開く・閉じる・切り替え・セッションの保存） |
//! | `navigation.rs` | ファイルを開く、リンクをたどる、戻る/進む、スクロール位置の予約、ページ内検索の移動 |
//! | `input.rs`      | キーボードショートカット・マウスのサイドボタン・ドラッグ＆ドロップ |
//! | `external.rs`   | 外部エディタで開く、フォルダで表示、フォルダ選択ダイアログ |
//! | `export.rs`     | HTML への書き出し・印刷 |
//!
//! 画面の描画は `crate::ui` にある。

mod document;
mod export;
mod external;
mod fonts;
mod input;
mod navigation;
mod profile;
mod tabs;
pub use profile::slow as profile_slow;

use std::path::{Path, PathBuf};

use egui::{Theme, ThemePreference};

pub use document::{decode, load_doc};
pub use navigation::{PendingScroll, ScrollTarget, absolute, find_index};
pub use tabs::Tab;

use crate::bg::BgTask;
use crate::fonts::LoadedFonts;
use crate::i18n::Strings;
use crate::settings::Settings;
use crate::textsearch::Needle;
use crate::theme::{Palette, ThemeMode};
use crate::ui::{preview::PreviewState, slides::SlideState};
use crate::workspace::{self, FolderWatcher, SearchResults, Searcher, Tree};

// ==== 機能: ページ内検索の状態 ====

#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub query: String,
    /// 「Aa」: 完全一致（大文字小文字・ひらがな/カタカナ・全角/半角を区別する）。
    pub case_sensitive: bool,
    /// 選択中の一致の番号。
    pub current: usize,
    /// 次のフレームで入力欄にフォーカスする。
    pub focus: bool,
    needle: Option<Needle>,
    needle_key: (String, bool),
}

impl FindState {
    /// 検索語（日本語の表記ゆれを吸収する照合。入力が変わったときだけ作り直す）。
    pub fn needle(&mut self) -> Option<&Needle> {
        if !self.open || self.query.is_empty() {
            return None;
        }
        let key = (self.query.clone(), self.case_sensitive);
        if self.needle_key != key {
            self.needle = Needle::new(&self.query, self.case_sensitive);
            self.needle_key = key;
        }
        self.needle.as_ref()
    }
}

// ==== 機能: ファイル名のあいまい検索（Ctrl+P）の状態 ====

/// あいまい検索の結果: (`Tree::files` の添字, 一致した文字位置)。良い順。
pub type Ranked = Vec<(usize, Vec<usize>)>;

pub struct QuickOpen {
    pub query: String,
    pub selected: usize,
    pub focus: bool,
    pub results: Ranked,
    /// `results` を計算したときの検索語（変わったら再計算）。
    pub results_for: Option<String>,
}

// ==== 機能: 全文検索（Ctrl+Shift+F）の状態 ====

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub results: Option<SearchResults>,
    pub searcher: Searcher,
    pub running: bool,
    pub focus: bool,
    pub last_started: String,
}

/// サイドバーのタブ。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftTab {
    Files,
    Search,
    Tags,
}

/// アプリ全体の状態。
pub struct App {
    /// egui のコンテキスト（バックグラウンド処理から再描画を頼むために持っておく）。
    pub ctx: egui::Context,
    pub settings: Settings,
    pub palette: Palette,
    /// 機能: UI の言語 — 現在の文言テーブル。
    pub s: &'static Strings,
    /// 機能: フォントの読み込み（バックグラウンド）
    pub fonts: fonts::FontState,

    // ---- フォルダ ----
    pub root: PathBuf,
    pub tree: Option<Tree>,
    /// フォルダのスキャン（バックグラウンド）。
    tree_task: Option<BgTask<Tree>>,
    /// 機能: フォルダの変更監視
    watcher: Option<FolderWatcher>,
    pub tree_filter: String,
    /// サイドバーの絞り込み結果のキャッシュ: (検索語, 結果)。
    pub filter_cache: Option<(String, Ranked)>,
    /// 次のフレームでツリーをこのファイルまで展開する。
    pub reveal: Option<PathBuf>,
    /// 機能: タグ — 選択中のタグ。
    pub selected_tag: Option<String>,

    // ---- タブ ----
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub viewport_h: f32,

    // ---- 検索・ダイアログ ----
    pub find: FindState,
    pub quick: Option<QuickOpen>,
    pub search: SearchState,
    pub left_tab: LeftTab,
    pub settings_open: bool,
    pub folder_input: String,
    /// 機能: フォルダを開く — アプリ内のウィンドウを表示中か（OS のダイアログが使えないとき用）。
    pub open_dialog: bool,
    /// 次のフレームでパス入力欄にフォーカスする。
    pub open_dialog_focus: bool,
    pub toast: Option<(String, f64)>,

    // ---- オーバーレイ ----
    /// 機能: リンク先プレビュー
    pub preview: PreviewState,
    /// 機能: 画像の拡大表示（表示中の画像 URI）
    pub lightbox: Option<String>,
    /// 機能: スライド表示
    pub slides: Option<SlideState>,

    last_reload_check: f64,
    title: String,
    /// スキャンが終わったらフォルダの README / index を開く。
    pending_index: bool,
    /// 最初のフレームを描いたか（起動時間の計測用）。
    first_frame_done: bool,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        start: Option<PathBuf>,
        preloaded: Option<LoadedFonts>,
    ) -> Self {
        let settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, eframe::APP_KEY))
            .unwrap_or_default();
        let ctx = &cc.egui_ctx;
        let t0 = std::time::Instant::now();
        egui_extras::install_image_loaders(ctx);
        profile::phase("image loaders", t0);
        // Ctrl +/- は文字サイズの変更に使うので、egui の拡大縮小は止める。
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        for theme in [Theme::Light, Theme::Dark] {
            ctx.set_visuals_of(theme, Palette::new(theme == Theme::Dark).visuals());
            ctx.style_mut_of(theme, |s| {
                s.spacing.item_spacing = egui::vec2(6.0, 4.0);
                s.spacing.button_padding = egui::vec2(6.0, 3.0);
                s.interaction.selectable_labels = true;
            });
        }

        // フォントは main.rs で起動直後から別スレッドで読み込んでいる。ここで結果を待つ。
        let t0 = std::time::Instant::now();
        let loaded = preloaded.unwrap_or_else(|| {
            crate::fonts::preload()
                .join()
                .unwrap_or_else(|_| LoadedFonts::fallback())
        });
        profile::phase("wait for fonts", t0);

        let mut app = Self {
            ctx: ctx.clone(),
            palette: Palette::new(false),
            s: settings.language.strings(),
            settings,
            fonts: Default::default(),
            root: PathBuf::new(),
            tree: None,
            tree_task: None,
            watcher: None,
            tree_filter: String::new(),
            filter_cache: None,
            reveal: None,
            selected_tag: None,
            tabs: vec![Tab::default()],
            active: 0,
            viewport_h: 600.0,
            find: FindState::default(),
            quick: None,
            search: SearchState::default(),
            left_tab: LeftTab::Files,
            settings_open: false,
            folder_input: String::new(),
            open_dialog: false,
            open_dialog_focus: false,
            toast: None,
            preview: PreviewState::default(),
            lightbox: None,
            slides: None,
            last_reload_check: 0.0,
            title: String::new(),
            pending_index: false,
            first_frame_done: false,
        };
        app.install_fonts(ctx, loaded);
        app.apply_theme(ctx);
        let t0 = std::time::Instant::now();
        app.start(ctx, start);
        profile::phase("open start file", t0);
        app
    }

    // ==== 機能: 起動時の復元 ====

    /// コマンドライン引数、なければ前回のセッション（フォルダ・タブ）から開始する。
    fn start(&mut self, ctx: &egui::Context, start: Option<PathBuf>) {
        match start.map(|p| absolute(&p)) {
            Some(p) if p.is_file() => {
                let parent = p.parent().map(Path::to_path_buf).unwrap_or_default();
                self.set_root(parent, false);
                self.open(ctx, &p, None, true);
            }
            Some(p) if p.is_dir() => self.set_root(p, true),
            Some(p) => {
                self.toast(ctx, self.s.not_found(&p.display().to_string()));
                self.set_root(std::env::current_dir().unwrap_or_default(), true);
            }
            None => {
                let last_root = self.settings.last_root.clone().filter(|p| p.is_dir());
                match last_root {
                    Some(root) => {
                        self.set_root(root.clone(), false);
                        if !self.restore_tabs(ctx, &root) {
                            match self.settings.last_file.clone().filter(|f| f.is_file()) {
                                Some(f) => self.open(ctx, &f, None, true),
                                None => self.pending_index = true,
                            }
                        }
                    }
                    None => self.set_root(std::env::current_dir().unwrap_or_default(), true),
                }
            }
        }
    }

    /// 前回開いていたタブを開き直す。1 つでも開けたら true。
    fn restore_tabs(&mut self, ctx: &egui::Context, root: &Path) -> bool {
        let saved: Vec<PathBuf> = self
            .settings
            .open_tabs
            .iter()
            .flatten()
            .filter(|p| p.is_file() && p.starts_with(root))
            .cloned()
            .collect();
        if saved.is_empty() {
            return false;
        }
        let active = self.settings.active_tab.min(saved.len() - 1);
        for (i, p) in saved.iter().enumerate() {
            if i > 0 {
                self.tabs.push(Tab::default());
                self.active = i;
            }
            self.open(ctx, p, None, true);
        }
        self.active = active;
        self.update_title(ctx);
        true
    }

    // ==== 機能: テーマ・フォント・言語 ====

    pub fn apply_theme(&mut self, ctx: &egui::Context) {
        ctx.set_theme(match self.settings.theme {
            ThemeMode::System => ThemePreference::System,
            ThemeMode::Light => ThemePreference::Light,
            ThemeMode::Dark => ThemePreference::Dark,
        });
    }

    pub fn apply_language(&mut self) {
        self.s = self.settings.language.strings();
    }

    /// 右下に短いメッセージを出す。
    pub fn toast(&mut self, ctx: &egui::Context, msg: impl Into<String>) {
        self.toast = Some((msg.into(), ctx.input(|i| i.time)));
    }

    // ==== 機能: フォルダ（スキャンと監視） ====

    /// 表示するフォルダを変える。`open_index` なら README / index を自動で開く。
    pub fn set_root(&mut self, root: PathBuf, open_index: bool) {
        let root = absolute(&root);
        if root == self.root && self.tree.is_some() && !open_index {
            return;
        }
        self.root = root.clone();
        self.tree = None;
        self.tree_filter.clear();
        self.selected_tag = None;
        self.search.results = None;
        self.search.last_started.clear();
        self.settings.last_root = Some(root.clone());
        self.settings.push_recent_root(root);
        self.watcher = None;
        self.rescan();
        if open_index {
            self.pending_index = true;
        }
    }

    /// バックグラウンドでフォルダを読み直す（終わるまで古いツリーを表示し続ける）。
    pub fn rescan(&mut self) {
        let root = self.root.clone();
        // 同じフォルダなら前回の索引を渡し、変わったファイルだけ読み直させる。
        let prev = self
            .tree
            .as_ref()
            .filter(|t| t.root == root)
            .map(|t| t.index.clone());
        self.tree_task = Some(BgTask::spawn(&self.ctx, move || {
            workspace::scan(&root, prev.as_ref())
        }));
    }

    /// フォルダの README.md / index.md を開く。
    fn open_index(&mut self, ctx: &egui::Context) {
        if let Some(p) = find_index(&self.root) {
            self.open(ctx, &p, None, true);
        }
    }

    // ==== 毎フレームの処理 ====

    /// バックグラウンド処理の結果を受け取り、自動再読み込みを確認する。
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.poll_fonts(ctx);
        if let Some(tree) = self.tree_task.as_ref().and_then(BgTask::poll) {
            self.tree = Some(tree);
            self.tree_task = None;
            self.filter_cache = None;
            if let Some(q) = &mut self.quick {
                q.results_for = None;
            }
            if self.watcher.is_none() {
                self.watcher = Some(FolderWatcher::new(&self.root, ctx));
            }
            if self.pending_index {
                self.pending_index = false;
                if self.doc().is_none() {
                    self.open_index(ctx);
                }
            }
        }
        if self.tree_task.is_none()
            && let Some(w) = &mut self.watcher
            && w.should_rescan()
        {
            self.rescan();
        }
        if let Some(results) = self.search.searcher.poll() {
            self.search.running = false;
            self.search.results = Some(results);
        }
        let now = ctx.input(|i| i.time);
        if self.settings.auto_reload && self.doc().is_some() {
            if now - self.last_reload_check > 1.0 {
                self.last_reload_check = now;
                self.check_reload(ctx, false);
            }
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        self.palette = Palette::new(ctx.theme() == Theme::Dark);
        self.update_title(ctx);
    }

    /// ウィンドウのタイトルを「ファイル名 — mdview」にする。
    pub fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.doc() {
            Some(d) => format!(
                "{} — mdview",
                d.path.file_name().unwrap_or_default().to_string_lossy()
            ),
            None => "mdview".to_string(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    // ==== 機能: 自動再読み込み ====

    /// 開いているファイルが変わっていたら読み直す（スクロール位置は保つ）。
    pub fn check_reload(&mut self, ctx: &egui::Context, force: bool) {
        let Some(doc) = self.doc() else { return };
        let modified = std::fs::metadata(&doc.path).and_then(|m| m.modified()).ok();
        if force || modified != doc.modified {
            let path = doc.path.clone();
            let tab = self.tab_mut();
            tab.doc = Some(load_doc(&path));
            let offset = tab.scroll_offset;
            tab.view = Default::default();
            tab.scroll_to(ScrollTarget::Y(offset), 1);
            if force {
                ctx.forget_all_images();
            }
            self.preview.invalidate(&path);
            self.toast(ctx, self.s.reloaded);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let t = std::time::Instant::now();
        self.poll(&ctx);
        self.handle_input(&ctx);
        self.draw(ui);
        if !self.first_frame_done {
            self.first_frame_done = true;
            profile::phase("first frame", t);
        }
        profile::record(t.elapsed());
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.store_session();
        eframe::set_value(storage, eframe::APP_KEY, &self.settings);
    }
}
