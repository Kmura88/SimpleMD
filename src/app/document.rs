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
    let meta = std::fs::metadata(path);
    let modified = meta.as_ref().ok().and_then(|m| m.modified().ok());
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let (text, error, sjis) = if size > MAX_FILE_SIZE {
        // 巨大なファイルは解析しない（固まるのを防ぐ）
        (
            String::new(),
            Some(format!("file is too large ({} MB)", size / 1_000_000)),
            false,
        )
    } else {
        match std::fs::read(path) {
            // PDF・画像などのバイナリは解析しない（大量の文字化けで固まるのを防ぐ）
            Ok(bytes) if looks_binary(&bytes) => (
                String::new(),
                Some("not a text file (binary data)".to_string()),
                false,
            ),
            Ok(bytes) => {
                let (text, sjis) = decode(&bytes);
                (text, None, sjis)
            }
            Err(e) => (String::new(), Some(e.to_string()), false),
        }
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

/// これより大きいファイルは開かない（Markdown のメモとしては十分大きい）。
const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// よく使われるバイナリ形式のファイル先頭の識別子（PDF・画像・圧縮ファイル・Office 文書など）。
/// 普通の英単語と区別できないもの（`MZ` など）は入れない。それらは NUL 文字の判定で見分ける。
const BINARY_MAGIC: &[&[u8]] = &[
    b"%PDF",             // PDF
    b"\x89PNG",          // PNG
    b"\xFF\xD8\xFF",     // JPEG
    b"GIF8",             // GIF
    b"PK\x03\x04",       // ZIP / docx / xlsx / pptx / epub
    b"\xD0\xCF\x11\xE0", // 古い Office 文書（doc / xls）
    b"7z\xBC\xAF",       // 7-Zip
    b"\x1F\x8B",         // gzip
    b"\x7FELF",          // Linux の実行ファイル
];

/// バイナリファイルか。先頭の識別子が既知のバイナリ形式か、先頭 8KB に NUL 文字があれば true。
/// Shift_JIS や UTF-8 のテキストには NUL は現れない。
fn looks_binary(bytes: &[u8]) -> bool {
    BINARY_MAGIC.iter().any(|m| bytes.starts_with(m)) || bytes.iter().take(8192).any(|&b| b == 0)
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
