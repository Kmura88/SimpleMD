//! # 機能: パス / URL の小さな補助関数
//!
//! - `%XX` エスケープのデコード
//! - URL スキームの判定（Windows のドライブ文字と区別）
//! - パスの正規化と、Markdown ファイル基準の相対パス解決
//! - egui の画像ローダーに渡す URI の生成

use std::path::{Component, Path, PathBuf};

// ==== 機能: パーセントエンコードのデコード ====

/// `%XX` エスケープをデコードする（不正な並びはそのまま残す）。
pub fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// 16 進数 1 文字を数値にする（16 進数でなければ `None`）。
fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

// ==== 機能: URL スキームの判定 ====

/// `http:` や `mailto:` などのスキームが付いていれば true。
/// `C:` のような Windows のドライブ文字はスキームとみなさない（2 文字以上を要求）。
pub fn has_scheme(s: &str) -> bool {
    let Some(colon) = s.find(':') else {
        return false;
    };
    let scheme = &s[..colon];
    scheme.len() > 1
        && scheme
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

// ==== 機能: パスの正規化・解決 ====

/// `.` と `..` を字面の上だけで畳み込む（ファイルシステムには問い合わせない）。
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// リンク / 画像の参照先を、Markdown ファイルのあるフォルダ基準で解決する。
pub fn resolve_path(target: &str, base_dir: &Path) -> PathBuf {
    let decoded = percent_decode(target.trim());
    let p = Path::new(&decoded);
    if p.is_absolute() {
        normalize(p)
    } else {
        normalize(&base_dir.join(p))
    }
}

/// egui の画像ローダーに渡せる URI を作る。
/// スキーム付きの URL はそのまま、ローカルパスは `file://` 付きに変換する。
pub fn image_uri(url: &str, base_dir: &Path) -> String {
    let url = url.trim();
    if has_scheme(url) {
        return url.to_string();
    }
    let path = resolve_path(url, base_dir);
    format!("file://{}", path.to_string_lossy().replace('\\', "/"))
}

// ==== 機能: テスト ====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode() {
        assert_eq!(percent_decode("a%20b%E3%81%82"), "a bあ");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn schemes() {
        assert!(has_scheme("https://x"));
        assert!(has_scheme("mailto:a@b"));
        assert!(!has_scheme("C:\\x"));
        assert!(!has_scheme("docs/a.md"));
        assert!(!has_scheme("#top"));
    }

    #[test]
    fn resolve() {
        let base = Path::new("/notes/sub");
        assert_eq!(
            resolve_path("../a%20b.md", base),
            PathBuf::from("/notes/a b.md")
        );
        assert_eq!(image_uri("img/x.png", base), "file:///notes/sub/img/x.png");
        assert_eq!(image_uri("https://e/x.svg", base), "https://e/x.svg");
    }
}
