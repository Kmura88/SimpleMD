//! # 機能: 設定の保存と復元
//! eframe の永続化機能で、終了時に保存して次回起動時に読み込む。
//! 項目を追加しても古い保存データを読めるよう、`#[serde(default)]` で既定値を補う。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::i18n::Language;
use crate::theme::ThemeMode;

pub const MIN_FONT: f32 = 10.0;
pub const MAX_FONT: f32 = 32.0;

/// スクロール位置を覚えておくファイル数の上限。
const MAX_POSITIONS: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    // ---- 表示 ----
    pub theme: ThemeMode,
    /// 機能: UI の言語
    pub language: Language,
    pub font_size: f32,
    /// 本文の幅を制限して読みやすくする。
    pub readable_width: bool,
    pub content_width: f32,
    pub show_sidebar: bool,
    pub show_outline: bool,
    pub auto_reload: bool,
    /// フォントの指定（空欄なら自動選択）。
    pub body_font: String,
    pub mono_font: String,
    /// 機能: 外部エディタで開く — コマンド（空欄ならシステム既定のアプリ）。
    /// `{file}` / `{line}` を含めるとそこに置き換え、含まなければ末尾にファイルパスを付ける。
    pub editor_command: String,

    // ---- 前回の状態 ----
    pub last_root: Option<PathBuf>,
    pub last_file: Option<PathBuf>,
    pub recent_roots: Vec<PathBuf>,
    pub recent_files: Vec<PathBuf>,
    /// 機能: タブ — 終了時に開いていたファイル（ようこそ画面のタブは `None`）。
    pub open_tabs: Vec<Option<PathBuf>>,
    pub active_tab: usize,
    /// 機能: ファイルごとのスクロール位置の記憶 — 新しい順の (パス, 位置)。
    pub positions: Vec<(String, f32)>,
    /// 機能: スライド表示 — 文字サイズの倍率。
    pub slide_scale: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            language: Language::Auto,
            font_size: 16.0,
            readable_width: true,
            content_width: 860.0,
            show_sidebar: true,
            show_outline: true,
            auto_reload: true,
            body_font: String::new(),
            mono_font: String::new(),
            editor_command: String::new(),
            last_root: None,
            last_file: None,
            recent_roots: Vec::new(),
            recent_files: Vec::new(),
            open_tabs: Vec::new(),
            active_tab: 0,
            positions: Vec::new(),
            slide_scale: 1.8,
        }
    }
}

impl Settings {
    pub fn push_recent_file(&mut self, path: PathBuf) {
        push_recent(&mut self.recent_files, path, 15);
    }

    pub fn push_recent_root(&mut self, path: PathBuf) {
        push_recent(&mut self.recent_roots, path, 10);
    }

    /// ファイルのスクロール位置を覚える（先頭付近なら記録を消す）。
    pub fn remember_position(&mut self, path: &std::path::Path, offset: f32) {
        let key = path.to_string_lossy().into_owned();
        self.positions.retain(|(p, _)| *p != key);
        if offset > 20.0 {
            self.positions.insert(0, (key, offset));
            self.positions.truncate(MAX_POSITIONS);
        }
    }

    /// 覚えているスクロール位置。
    pub fn position_of(&self, path: &std::path::Path) -> Option<f32> {
        let key = path.to_string_lossy();
        self.positions
            .iter()
            .find(|(p, _)| *p == key)
            .map(|(_, y)| *y)
    }
}

fn push_recent(list: &mut Vec<PathBuf>, path: PathBuf, max: usize) {
    list.retain(|p| *p != path);
    list.insert(0, path);
    list.truncate(max);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn positions_lru() {
        let mut s = Settings::default();
        s.remember_position(Path::new("/a.md"), 100.0);
        s.remember_position(Path::new("/b.md"), 200.0);
        s.remember_position(Path::new("/a.md"), 150.0);
        assert_eq!(s.position_of(Path::new("/a.md")), Some(150.0));
        assert_eq!(s.positions[0].0, "/a.md");
        // 先頭付近に戻ったら忘れる
        s.remember_position(Path::new("/b.md"), 0.0);
        assert_eq!(s.position_of(Path::new("/b.md")), None);
    }
}
