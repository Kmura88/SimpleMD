//! # 機能: フォントの読み込み（アプリ側）
//! 重い処理（全フォントのスキャン・ファイルの読み込み）はすべてバックグラウンドで行い、
//! 終わったら次のフレームで egui に反映する。UI は固まらない。
//! - 起動時: `main.rs` が先に始めた [`fonts::preload`] の結果を受け取る
//! - 設定画面を開いたとき: フォント一覧を作る（全スキャン）
//! - 設定でフォントを変えたとき: 選び直して読み込む

use std::sync::Arc;

use super::App;
use crate::bg::BgTask;
use crate::fonts::{self, FontReport, LoadedFonts, Scanner};

/// フォント関係の状態。
#[derive(Default)]
pub struct FontState {
    /// 実際に使っているフォント（設定画面に表示）。
    pub report: FontReport,
    /// 設定画面の選択肢（空ならまだ作っていない）。
    pub families: Vec<String>,
    /// 一覧を作っている最中か。
    pub loading_families: bool,
    scanner: Option<Arc<Scanner>>,
    /// フォント一覧づくり（全スキャン。結果のスキャナは読み直しにも使い回す）。
    families_task: Option<BgTask<(Vec<String>, Arc<Scanner>)>>,
    /// 設定に合わせた読み直し。
    load_task: Option<BgTask<LoadedFonts>>,
}

impl App {
    /// 起動時に読み込んだフォントを反映する。設定と違うフォントだった場合は読み直しを始める。
    pub(super) fn install_fonts(&mut self, ctx: &egui::Context, loaded: LoadedFonts) {
        let wanted = (
            self.settings.body_font.clone(),
            self.settings.mono_font.clone(),
        );
        let stale = loaded.choice.key != wanted;
        ctx.set_fonts(loaded.defs);
        self.fonts.report = loaded.report;
        if stale {
            self.apply_fonts(ctx);
        }
    }

    /// 設定のフォント指定に合わせて読み直す（バックグラウンド）。
    pub fn apply_fonts(&mut self, ctx: &egui::Context) {
        let scanner = self.fonts.scanner.clone();
        let (body, mono) = (
            self.settings.body_font.clone(),
            self.settings.mono_font.clone(),
        );
        self.fonts.load_task = Some(BgTask::spawn(ctx, move || {
            let scanner = scanner.unwrap_or_else(|| Arc::new(Scanner::new()));
            fonts::load_with(&scanner, &body, &mono)
        }));
    }

    /// 設定画面のフォント一覧を用意する（まだなら作り始める）。
    pub fn request_font_families(&mut self, ctx: &egui::Context) {
        if !self.fonts.families.is_empty() || self.fonts.loading_families {
            return;
        }
        self.fonts.loading_families = true;
        self.fonts.families_task = Some(BgTask::spawn(ctx, || {
            let scanner = Arc::new(Scanner::new());
            (scanner.families(), scanner)
        }));
    }

    /// バックグラウンド処理の結果を受け取る（毎フレーム）。
    pub(super) fn poll_fonts(&mut self, ctx: &egui::Context) {
        if let Some((list, scanner)) = self.fonts.families_task.as_ref().and_then(BgTask::poll) {
            self.fonts.families = list;
            self.fonts.loading_families = false;
            self.fonts.scanner = Some(scanner);
            self.fonts.families_task = None;
        }
        if let Some(loaded) = self.fonts.load_task.as_ref().and_then(BgTask::poll) {
            ctx.set_fonts(loaded.defs);
            self.fonts.report = loaded.report;
            self.fonts.load_task = None;
        }
    }
}
