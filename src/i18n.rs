//! # 機能: UI の多言語化（英語 / 日本語）
//!
//! 画面に出る文言はすべてここにまとめる。`Strings` は言語ごとの静的なテーブルで、
//! フィールドを増やすとコンパイラが両言語の定義漏れを教えてくれる。
//! 数を含む文言は `Strings` のメソッドで組み立てる。
//!
//! 言語は設定（`Language`）で選び、`Auto` のときは OS のロケールから判定する。

use serde::{Deserialize, Serialize};

/// 設定画面で選ぶ表示言語。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    /// OS のロケールに従う（`ja` で始まれば日本語、それ以外は英語）。
    #[default]
    Auto,
    English,
    Japanese,
}

impl Language {
    /// 実際に使うテーブルを返す。
    pub fn strings(self) -> &'static Strings {
        match self {
            Language::English => &EN,
            Language::Japanese => &JA,
            Language::Auto => {
                let ja = sys_locale::get_locale()
                    .is_some_and(|l| l.to_ascii_lowercase().starts_with("ja"));
                if ja { &JA } else { &EN }
            }
        }
    }
}

/// 画面の文言テーブル。
pub struct Strings {
    pub ja: bool,
    // ---- ツールバー ----
    pub tip_sidebar: &'static str,
    pub tip_back: &'static str,
    pub tip_forward: &'static str,
    pub tip_settings: &'static str,
    pub tip_outline: &'static str,
    pub tip_find: &'static str,
    pub tip_more: &'static str,
    pub go_to_file: &'static str,
    pub menu_open_editor: &'static str,
    pub menu_reveal: &'static str,
    pub menu_copy_path: &'static str,
    pub menu_export_html: &'static str,
    pub menu_print: &'static str,
    pub menu_present: &'static str,
    pub menu_open_folder: &'static str,
    // ---- タブ ----
    pub tab_welcome: &'static str,
    pub tip_new_tab: &'static str,
    pub tip_close_tab: &'static str,
    // ---- サイドバー ----
    pub tab_files: &'static str,
    pub tab_search: &'static str,
    pub tab_tags: &'static str,
    pub tip_rescan: &'static str,
    pub tip_open_folder: &'static str,
    pub filter_hint: &'static str,
    pub scanning: &'static str,
    pub no_md_files: &'static str,
    pub no_matching_files: &'static str,
    pub open_folder_title: &'static str,
    pub path_hint: &'static str,
    pub open: &'static str,
    pub browse: &'static str,
    pub drop_tip: &'static str,
    pub drop_here: &'static str,
    pub recent_folders: &'static str,
    pub search_hint: &'static str,
    pub searching: &'static str,
    pub search_help: &'static str,
    pub no_tags: &'static str,
    // ---- 目次・バックリンク ----
    pub outline: &'static str,
    pub backlinks: &'static str,
    pub no_backlinks: &'static str,
    // ---- ページ内検索 ----
    pub find_hint: &'static str,
    pub no_results: &'static str,
    pub tip_prev: &'static str,
    pub tip_next: &'static str,
    pub match_case: &'static str,
    pub tip_close: &'static str,
    // ---- 文書表示 ----
    pub could_not_open: &'static str,
    pub reloaded: &'static str,
    pub link_copied: &'static str,
    pub path_copied: &'static str,
    pub copy: &'static str,
    pub copied: &'static str,
    pub front_matter: &'static str,
    pub copy_heading_link: &'static str,
    pub alert_note: &'static str,
    pub alert_tip: &'static str,
    pub alert_important: &'static str,
    pub alert_warning: &'static str,
    pub alert_caution: &'static str,
    // ---- ようこそ画面 ----
    pub no_files_found: &'static str,
    pub open_another_folder: &'static str,
    pub recent: &'static str,
    pub shortcuts: &'static str,
    pub shortcut_list: &'static [(&'static str, &'static str)],
    // ---- ファイル検索 ----
    pub quick_hint: &'static str,
    pub quick_help: &'static str,
    // ---- 設定 ----
    pub settings: &'static str,
    pub theme: &'static str,
    pub theme_system: &'static str,
    pub theme_light: &'static str,
    pub theme_dark: &'static str,
    pub text_size: &'static str,
    pub line_width: &'static str,
    pub limit: &'static str,
    pub text_font: &'static str,
    pub code_font: &'static str,
    pub automatic: &'static str,
    pub auto_reload: &'static str,
    pub auto_reload_label: &'static str,
    pub language: &'static str,
    pub lang_auto: &'static str,
    pub editor: &'static str,
    pub editor_hint: &'static str,
    // ---- スライド ----
    pub slide_help: &'static str,
    // ---- 画像ビューア ----
    pub open_externally: &'static str,
    pub lightbox_help: &'static str,
}

impl Strings {
    pub fn not_found(&self, what: &str) -> String {
        if self.ja {
            format!("見つかりません: {what}")
        } else {
            format!("Not found: {what}")
        }
    }

    pub fn note_not_found(&self, name: &str) -> String {
        if self.ja {
            format!("ノート「{name}」が見つかりません")
        } else {
            format!("No note named “{name}”")
        }
    }

    pub fn search_results(&self, hits: &str, files: usize) -> String {
        if self.ja {
            format!("{files} ファイル中 {hits} 件")
        } else {
            format!("{hits} results in {files} files")
        }
    }

    pub fn file_count(&self, n: &str) -> String {
        if self.ja {
            format!("{n} ファイル")
        } else {
            format!("{n} files")
        }
    }

    pub fn md_file_count(&self, n: &str) -> String {
        if self.ja {
            format!("Markdown ファイル {n} 件")
        } else {
            format!("{n} Markdown files")
        }
    }

    pub fn words_chars(&self, words: &str, chars: &str) -> String {
        if self.ja {
            format!("{words} 語 · {chars} 文字")
        } else {
            format!("{words} words · {chars} chars")
        }
    }

    /// 「◯分前に更新」
    pub fn modified_ago(&self, secs: u64) -> String {
        let (n, unit_en, unit_ja) = match secs {
            0..=59 => {
                return if self.ja {
                    "たった今更新".into()
                } else {
                    "modified just now".into()
                };
            }
            60..=3599 => (secs / 60, "min", "分"),
            3600..=86_399 => (secs / 3600, "h", "時間"),
            _ => (secs / 86_400, "days", "日"),
        };
        if self.ja {
            format!("{n}{unit_ja}前に更新")
        } else {
            format!("modified {n} {unit_en} ago")
        }
    }

    pub fn exported(&self, path: &str) -> String {
        if self.ja {
            format!("書き出しました: {path}")
        } else {
            format!("Exported: {path}")
        }
    }

    pub fn failed(&self, what: &str) -> String {
        if self.ja {
            format!("失敗しました: {what}")
        } else {
            format!("Failed: {what}")
        }
    }

    pub fn fonts_in_use(&self, body: &str, cjk: &str, mono: &str, bold: bool) -> String {
        let no_bold = if bold {
            ""
        } else if self.ja {
            "（太字フォントなし）"
        } else {
            " (no bold face found)"
        };
        if self.ja {
            format!("使用中: 本文 {body} · 日本語 {cjk} · コード {mono}{no_bold}")
        } else {
            format!("In use: text {body} · Japanese {cjk} · code {mono}{no_bold}")
        }
    }

    pub fn slide_counter(&self, cur: usize, total: usize) -> String {
        format!("{cur} / {total}")
    }

    pub fn backlinks_count(&self, n: usize) -> String {
        format!("{} ({n})", self.backlinks)
    }
}

pub static EN: Strings = Strings {
    ja: false,
    tip_sidebar: "Sidebar (Ctrl+B)",
    tip_back: "Back (Alt+←)",
    tip_forward: "Forward (Alt+→)",
    tip_settings: "Settings (Ctrl+,)",
    tip_outline: "Outline & backlinks (Ctrl+Shift+O)",
    tip_find: "Find in page (Ctrl+F)",
    tip_more: "More",
    go_to_file: "Go to file…",
    menu_open_editor: "Open in editor (Ctrl+Shift+E)",
    menu_reveal: "Show in file manager",
    menu_copy_path: "Copy file path",
    menu_export_html: "Export as HTML… (Ctrl+E)",
    menu_print: "Print… (via browser)",
    menu_present: "Presentation mode (F11)",
    menu_open_folder: "Open folder… (Ctrl+Shift+N)",
    tab_welcome: "Welcome",
    tip_new_tab: "New tab (Ctrl+T)",
    tip_close_tab: "Close tab (Ctrl+W)",
    tab_files: "Files",
    tab_search: "Search",
    tab_tags: "Tags",
    tip_rescan: "Rescan folder (F5)",
    tip_open_folder: "Open folder",
    filter_hint: "Filter files…",
    scanning: "Scanning…",
    no_md_files: "No Markdown files in this folder.",
    no_matching_files: "No matching files",
    open_folder_title: "Open folder",
    path_hint: "Path to a folder or .md file",
    open: "Open",
    browse: "Browse…",
    drop_tip: "Tip: you can also drop a folder or file onto the window.",
    drop_here: "Drop a folder or Markdown file to open it",
    recent_folders: "Recent folders",
    search_hint: "Search in files…",
    searching: "Searching…",
    search_help: "Type to search every Markdown file in the folder.",
    no_tags: "No tags. Add `tags: [a, b]` to a note's front matter.",
    outline: "OUTLINE",
    backlinks: "BACKLINKS",
    no_backlinks: "No other note links here.",
    find_hint: "Find in page",
    no_results: "No results",
    tip_prev: "Previous (Shift+Enter)",
    tip_next: "Next (Enter)",
    match_case: "Match case",
    tip_close: "Close (Esc)",
    could_not_open: "Could not open file",
    reloaded: "Reloaded",
    link_copied: "Link copied",
    path_copied: "Path copied",
    copy: "📋 Copy",
    copied: "✔ Copied",
    front_matter: "Front matter",
    copy_heading_link: "Copy link to this heading",
    alert_note: "Note",
    alert_tip: "Tip",
    alert_important: "Important",
    alert_warning: "Warning",
    alert_caution: "Caution",
    no_files_found: "No Markdown files were found in this folder.",
    open_another_folder: "Open another folder from the ⋯ menu, or drop one onto this window.",
    recent: "RECENT",
    shortcuts: "SHORTCUTS",
    shortcut_list: &[
        ("Ctrl+P", "Go to file"),
        ("Ctrl+F", "Find in page"),
        ("Ctrl+Shift+F", "Search in all files"),
        ("Alt+← / →", "Back / forward"),
        ("Ctrl+T / Ctrl+W", "New tab / close tab"),
        ("Ctrl+click", "Open link in a new tab"),
        ("Ctrl+B", "Toggle sidebar"),
        ("Ctrl+Shift+O", "Toggle outline"),
        ("Ctrl + / − / 0", "Text size"),
        ("F11", "Presentation mode"),
        ("F5", "Reload"),
    ],
    quick_hint: "Go to file…",
    quick_help: "↑↓ to select · Enter to open · Ctrl+Enter in new tab · Esc to close",
    settings: "Settings",
    theme: "Theme",
    theme_system: "System",
    theme_light: "Light",
    theme_dark: "Dark",
    text_size: "Text size",
    line_width: "Line width",
    limit: "Limit",
    text_font: "Text font",
    code_font: "Code font",
    automatic: "Automatic",
    auto_reload: "Auto reload",
    auto_reload_label: "Reload when the file changes",
    language: "Language",
    lang_auto: "Auto",
    editor: "Editor",
    editor_hint: "e.g. code, notepad, vim (empty = system default)",
    slide_help: "← → / Space to move · Esc to exit",
    open_externally: "Open in default app",
    lightbox_help: "Click or Esc to close",
};

pub static JA: Strings = Strings {
    ja: true,
    tip_sidebar: "サイドバー (Ctrl+B)",
    tip_back: "戻る (Alt+←)",
    tip_forward: "進む (Alt+→)",
    tip_settings: "設定 (Ctrl+,)",
    tip_outline: "目次とバックリンク (Ctrl+Shift+O)",
    tip_find: "ページ内検索 (Ctrl+F)",
    tip_more: "その他",
    go_to_file: "ファイルを開く…",
    menu_open_editor: "エディタで開く (Ctrl+Shift+E)",
    menu_reveal: "フォルダで表示",
    menu_copy_path: "ファイルのパスをコピー",
    menu_export_html: "HTML に書き出す… (Ctrl+E)",
    menu_print: "印刷…（ブラウザ経由）",
    menu_present: "スライド表示 (F11)",
    menu_open_folder: "フォルダを開く… (Ctrl+Shift+N)",
    tab_welcome: "ようこそ",
    tip_new_tab: "新しいタブ (Ctrl+T)",
    tip_close_tab: "タブを閉じる (Ctrl+W)",
    tab_files: "ファイル",
    tab_search: "検索",
    tab_tags: "タグ",
    tip_rescan: "フォルダを再読み込み (F5)",
    tip_open_folder: "フォルダを開く",
    filter_hint: "ファイル名で絞り込み…",
    scanning: "読み込み中…",
    no_md_files: "このフォルダに Markdown ファイルはありません。",
    no_matching_files: "一致するファイルはありません",
    open_folder_title: "フォルダを開く",
    path_hint: "フォルダまたは .md ファイルのパス",
    open: "開く",
    browse: "参照…",
    drop_tip: "ヒント: フォルダやファイルをウィンドウにドロップしても開けます。",
    drop_here: "ドロップしてフォルダ / Markdown ファイルを開く",
    recent_folders: "最近のフォルダ",
    search_hint: "全ファイルを検索…",
    searching: "検索中…",
    search_help: "入力するとフォルダ内のすべての Markdown を検索します。",
    no_tags: "タグはありません。フロントマターに `tags: [a, b]` と書くと表示されます。",
    outline: "目次",
    backlinks: "バックリンク",
    no_backlinks: "このノートへのリンクはありません。",
    find_hint: "ページ内を検索",
    no_results: "見つかりません",
    tip_prev: "前へ (Shift+Enter)",
    tip_next: "次へ (Enter)",
    match_case: "大文字と小文字を区別",
    tip_close: "閉じる (Esc)",
    could_not_open: "ファイルを開けませんでした",
    reloaded: "再読み込みしました",
    link_copied: "リンクをコピーしました",
    path_copied: "パスをコピーしました",
    copy: "📋 コピー",
    copied: "✔ コピー済み",
    front_matter: "フロントマター",
    copy_heading_link: "この見出しへのリンクをコピー",
    alert_note: "注記",
    alert_tip: "ヒント",
    alert_important: "重要",
    alert_warning: "警告",
    alert_caution: "注意",
    no_files_found: "このフォルダには Markdown ファイルがありません。",
    open_another_folder: "⋯ メニューから別のフォルダを開くか、ウィンドウにドロップしてください。",
    recent: "最近開いたファイル",
    shortcuts: "ショートカット",
    shortcut_list: &[
        ("Ctrl+P", "ファイルを開く"),
        ("Ctrl+F", "ページ内検索"),
        ("Ctrl+Shift+F", "全ファイル検索"),
        ("Alt+← / →", "戻る / 進む"),
        ("Ctrl+T / Ctrl+W", "新しいタブ / タブを閉じる"),
        ("Ctrl+クリック", "リンクを新しいタブで開く"),
        ("Ctrl+B", "サイドバーの表示切替"),
        ("Ctrl+Shift+O", "目次の表示切替"),
        ("Ctrl + / − / 0", "文字サイズ"),
        ("F11", "スライド表示"),
        ("F5", "再読み込み"),
    ],
    quick_hint: "ファイル名を入力…",
    quick_help: "↑↓ で選択 · Enter で開く · Ctrl+Enter で新しいタブ · Esc で閉じる",
    settings: "設定",
    theme: "テーマ",
    theme_system: "システム",
    theme_light: "ライト",
    theme_dark: "ダーク",
    text_size: "文字サイズ",
    line_width: "本文の幅",
    limit: "制限する",
    text_font: "本文フォント",
    code_font: "コードフォント",
    automatic: "自動",
    auto_reload: "自動再読み込み",
    auto_reload_label: "ファイルが変更されたら再読み込み",
    language: "言語",
    lang_auto: "自動",
    editor: "エディタ",
    editor_hint: "例: code, notepad, vim（空欄でシステム既定）",
    slide_help: "← → / Space で移動 · Esc で終了",
    open_externally: "既定のアプリで開く",
    lightbox_help: "クリックまたは Esc で閉じる",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_languages() {
        assert!(Language::Japanese.strings().ja);
        assert!(!Language::English.strings().ja);
        assert_eq!(JA.modified_ago(120), "2分前に更新");
        assert_eq!(EN.modified_ago(7200), "modified 2 h ago");
        assert_eq!(EN.shortcut_list.len(), JA.shortcut_list.len());
    }
}
