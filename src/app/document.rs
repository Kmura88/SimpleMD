//! # 機能: ファイルの読み込み
//! Markdown ファイルを読み、文字コードを判定して解析する。
//! UTF-8（BOM あり / なし）と Shift_JIS に対応する。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::markdown::{self, Document};

/// 開いている 1 ファイル。
pub struct OpenDoc {
    pub path: PathBuf,
    /// ファイルのあるフォルダ（相対リンク・画像の基準）。
    pub base_dir: PathBuf,
    /// 元のテキスト（HTML 書き出しに使う）。
    pub source: String,
    pub doc: Document,
    /// 読み込んだ時点の更新時刻（自動再読み込みの判定に使う）。
    pub modified: Option<SystemTime>,
    /// 読み込みに失敗したときのエラー。
    pub error: Option<String>,
    pub words: usize,
    pub chars: usize,
    /// UTF-8 ではなく Shift_JIS として読んだ。
    pub sjis: bool,
}

/// ファイルを読み込んで解析する。失敗しても `error` に理由を入れて返す。
pub fn load_doc(path: &Path) -> OpenDoc {
    let base_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let (text, error, sjis) = match std::fs::read(path) {
        Ok(bytes) => {
            let (text, sjis) = decode(&bytes);
            (text, None, sjis)
        }
        Err(e) => (String::new(), Some(e.to_string()), false),
    };
    let words = text.split_whitespace().count();
    let chars = text.chars().filter(|c| !c.is_whitespace()).count();
    OpenDoc {
        path: path.to_path_buf(),
        base_dir,
        doc: markdown::parse(&text),
        source: text,
        modified,
        error,
        words,
        chars,
        sjis,
    }
}

/// バイト列を文字列にする。UTF-8 として読めなければ Shift_JIS を試し、
/// それも壊れていれば UTF-8 として読める部分だけ読む。戻り値の bool は Shift_JIS だったか。
pub fn decode(bytes: &[u8]) -> (String, bool) {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), false),
        Err(_) => {
            let (text, _, had_errors) = encoding_rs::SHIFT_JIS.decode(bytes);
            if had_errors {
                (String::from_utf8_lossy(bytes).into_owned(), false)
            } else {
                (text.into_owned(), true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_bom_and_sjis() {
        assert_eq!(decode(b"\xEF\xBB\xBF# hi").0, "# hi");
        let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("# 日本語のメモ");
        let (text, was_sjis) = decode(&sjis);
        assert_eq!(text, "# 日本語のメモ");
        assert!(was_sjis);
    }
}
