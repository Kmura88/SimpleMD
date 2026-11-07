//! # 機能: 外部アプリとの連携
//! - 外部エディタで開く（設定のコマンド、なければ OS の既定アプリ）
//! - ファイルマネージャーで表示（Windows はファイルを選択した状態で開く）
//! - ファイルのパスをコピー
//! - フォルダ選択ダイアログ（OS ネイティブ。使えない環境ではパス入力にフォールバック）

use std::path::Path;
use std::process::Command;

use super::App;

impl App {
    /// 開いているファイルを外部エディタで開く。
    pub fn open_current_in_editor(&mut self, ctx: &egui::Context) {
        let Some(path) = self.tab().path().map(Path::to_path_buf) else {
            return;
        };
        if let Err(e) = open_in_editor(&self.settings.editor_command, &path) {
            self.toast(ctx, self.s.failed(&e));
        }
    }

    /// 開いているファイルをファイルマネージャーで表示する。
    pub fn reveal_current(&mut self, ctx: &egui::Context) {
        let Some(path) = self.tab().path().map(Path::to_path_buf) else {
            return;
        };
        if let Err(e) = reveal_in_file_manager(&path) {
            self.toast(ctx, self.s.failed(&e));
        }
    }

    /// 開いているファイルの絶対パスをクリップボードへ。
    pub fn copy_current_path(&mut self, ctx: &egui::Context) {
        if let Some(path) = self.tab().path() {
            ctx.copy_text(path.display().to_string());
            self.toast(ctx, self.s.path_copied);
        }
    }

    /// OS のフォルダ選択ダイアログを出す。
    /// ダイアログが使えない環境（WSL など。すぐに「選択なし」で戻ってくる）では、
    /// アプリ内の「フォルダを開く」ウィンドウ（パス入力）を代わりに出す。
    pub fn pick_folder(&mut self) {
        let start = self.root.clone();
        let t0 = std::time::Instant::now();
        let picked = rfd::FileDialog::new().set_directory(start).pick_folder();
        match picked {
            Some(dir) => self.set_root(dir, true),
            // 人が操作したにしては早すぎる → ダイアログが出なかったとみなす。
            None if t0.elapsed() < std::time::Duration::from_millis(500) => self.show_open_dialog(),
            None => {}
        }
    }

    /// アプリ内の「フォルダを開く」ウィンドウを出す。
    pub fn show_open_dialog(&mut self) {
        self.open_dialog = true;
        self.open_dialog_focus = true;
    }
}

/// エディタでファイルを開く。
/// - `command` が空: OS の既定アプリ（`open` クレート）
/// - `{file}` を含む: そこをパスに置き換える（例: `code -g {file}`）
/// - 含まない: 末尾にパスを付ける（例: `notepad` → `notepad <path>`）
pub fn open_in_editor(command: &str, path: &Path) -> Result<(), String> {
    let command = command.trim();
    if command.is_empty() {
        return open::that_detached(path).map_err(|e| e.to_string());
    }
    let mut args = split_command(command);
    let file = path.to_string_lossy().into_owned();
    let mut used = false;
    for a in &mut args {
        if a.contains("{file}") {
            *a = a.replace("{file}", &file);
            used = true;
        }
        if a.contains("{line}") {
            *a = a.replace("{line}", "1");
        }
    }
    if !used {
        args.push(file);
    }
    let (program, rest) = args.split_first().ok_or("empty command")?;
    Command::new(program)
        .args(rest)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{program}: {e}"))
}

/// ファイルマネージャーで表示する。
pub fn reveal_in_file_manager(path: &Path) -> Result<(), String> {
    let result = if cfg!(target_os = "windows") {
        Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
            .map(|_| ())
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg("-R").arg(path).spawn().map(|_| ())
    } else {
        let dir = path.parent().unwrap_or(path);
        open::that_detached(dir)
    };
    result.map_err(|e| e.to_string())
}

/// コマンド文字列を引数に分ける（`"..."` / `'...'` で空白を含められる）。
pub fn split_command(s: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(c);
                has = true;
            }
            (None, c) if c.is_whitespace() => {
                if has || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        args.push(cur);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_splitting() {
        assert_eq!(split_command("code -g {file}"), ["code", "-g", "{file}"]);
        assert_eq!(
            split_command(r#""C:\Program Files\x.exe" --n  'a b'"#),
            [r"C:\Program Files\x.exe", "--n", "a b"]
        );
        assert_eq!(split_command("  "), Vec::<String>::new());
    }
}
