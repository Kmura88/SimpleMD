//! # 機能: コードの構文ハイライト
//! syntect を使う。構文定義は two-face（bat と同じ定義集）を使い、TypeScript・TOML・
//! Dockerfile・Kotlin・Swift・Zig など標準の syntect にない言語も色付けできる。
//! 同じコードを毎フレーム解析しないよう、結果をキャッシュする。

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, LazyLock, Mutex, mpsc};

use egui::Color32;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// 色付けの単位: 元テキストのバイト範囲とスタイル。
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub range: std::ops::Range<usize>,
    pub color: Color32,
    pub bold: bool,
    pub italic: bool,
}

struct Highlighter {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
    cache: Mutex<HashMap<u64, Arc<Vec<Span>>>>,
}

/// 初回使用時に構文定義を読み込む（数十 ms かかるので遅延させる）。
static HL: LazyLock<Highlighter> = LazyLock::new(|| Highlighter {
    syntaxes: two_face::syntax::extra_newlines(),
    themes: ThemeSet::load_defaults(),
    cache: Mutex::new(HashMap::new()),
});

/// 機能: 起動を速く — 構文定義（数 MB）の読み込みを先にバックグラウンドで済ませておく。
/// 最初にコードブロックを表示したときの引っかかりがなくなる。
pub fn warm_up() {
    std::thread::spawn(|| {
        LazyLock::force(&HL);
    });
}

/// コードブロックの言語名の別名を、構文定義の名前に寄せる。
fn alias(lang: &str) -> &str {
    match lang {
        "sh" | "shell" | "zsh" | "console" | "shellsession" => "bash",
        "jsonc" | "json5" => "json",
        "yml" => "yaml",
        "c++" | "hpp" | "cc" => "cpp",
        "py3" | "python3" => "py",
        "golang" => "go",
        "docker" => "dockerfile",
        "ps1" | "pwsh" => "powershell",
        "md" => "markdown",
        "kt" => "kotlin",
        "rs" => "rust",
        other => other,
    }
}

/// 言語名から構文定義を探す。
fn syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let lang = lang.trim().to_ascii_lowercase();
    if lang.is_empty() {
        return None;
    }
    let hl = &*HL;
    hl.syntaxes
        .find_syntax_by_token(alias(&lang))
        .or_else(|| hl.syntaxes.find_syntax_by_token(&lang))
        .or_else(|| hl.syntaxes.find_syntax_by_extension(&lang))
        .or_else(|| hl.syntaxes.find_syntax_by_name(&lang))
}

// ==== 機能: バックグラウンドでの色付け ====
//
// 言語ごとの初回の色付けは、構文定義の正規表現のコンパイルで数十 ms かかる。
// UI のフレーム内でやると引っかかるので、専用のスレッドで順に処理し、終わったら再描画を頼む。
// 終わるまでの間は単色で表示する。

/// 色付けの依頼。
struct Job {
    key: u64,
    code: String,
    lang: String,
    dark: bool,
    ctx: egui::Context,
}

/// 色付けの結果: `Some(色)` / `None`（知らない言語 → 単色）。
type Highlighted = Option<Arc<Vec<Span>>>;

/// 依頼ごとの結果。
static RESULTS: LazyLock<Mutex<HashMap<u64, Highlighted>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// 処理待ち・処理中の依頼（同じ依頼を重ねて出さないため）。
static PENDING: LazyLock<Mutex<HashSet<u64>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
/// 色付け専用スレッドへの送り口。
static WORKER: LazyLock<Mutex<mpsc::Sender<Job>>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::spawn(move || {
        for job in rx {
            let result = highlight(&job.code, &job.lang, job.dark);
            if let Ok(mut r) = RESULTS.lock() {
                if r.len() > 2000 {
                    r.clear();
                }
                r.insert(job.key, result);
            }
            if let Ok(mut p) = PENDING.lock() {
                p.remove(&job.key);
            }
            job.ctx.request_repaint();
        }
    });
    Mutex::new(tx)
});

/// 描画用: 色付け済みならその結果を返し、まだなら依頼して `None`（単色で表示）を返す。
/// 構文定義の読み込みも待たないので、UI スレッドが止まることはない。
pub fn highlight_async(
    ctx: &egui::Context,
    code: &str,
    lang: &str,
    dark: bool,
) -> Option<Arc<Vec<Span>>> {
    if lang.trim().is_empty() {
        return None;
    }
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (code, lang, dark).hash(&mut h);
        h.finish()
    };
    if let Some(done) = RESULTS.lock().ok()?.get(&key) {
        return done.clone();
    }
    let mut pending = PENDING.lock().ok()?;
    if pending.insert(key) {
        let job = Job {
            key,
            code: code.to_string(),
            lang: lang.to_string(),
            dark,
            ctx: ctx.clone(),
        };
        if let Ok(tx) = WORKER.lock() {
            let _ = tx.send(job);
        }
    }
    None
}

/// `code` をその場で色付けする（時間がかかることがある。描画では [`highlight_async`] を使う）。
/// 知らない言語なら `None`（呼び出し側は単色で表示する）。
pub fn highlight(code: &str, lang: &str, dark: bool) -> Option<Arc<Vec<Span>>> {
    let syntax = syntax(lang)?;
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (code, &syntax.name, dark).hash(&mut h);
        h.finish()
    };
    let hl = &*HL;
    if let Some(spans) = hl.cache.lock().ok()?.get(&key) {
        return Some(spans.clone());
    }
    let theme_name = if dark {
        "base16-ocean.dark"
    } else {
        "InspiredGitHub"
    };
    let theme = hl.themes.themes.get(theme_name)?;
    let mut h = HighlightLines::new(syntax, theme);
    let mut spans = Vec::new();
    let mut offset = 0;
    for line in LinesWithEndings::from(code) {
        let Ok(regions) = h.highlight_line(line, &hl.syntaxes) else {
            return None;
        };
        for (style, text) in regions {
            let c = style.foreground;
            spans.push(Span {
                range: offset..offset + text.len(),
                color: Color32::from_rgb(c.r, c.g, c.b),
                bold: style.font_style.contains(FontStyle::BOLD),
                italic: style.font_style.contains(FontStyle::ITALIC),
            });
            offset += text.len();
        }
    }
    let spans = Arc::new(spans);
    if let Ok(mut cache) = hl.cache.lock() {
        if cache.len() > 2000 {
            cache.clear();
        }
        cache.insert(key, spans.clone());
    }
    Some(spans)
}

/// 機能: HTML 書き出し用 — 色付きの `<pre>`（インラインスタイル付き）を作る。
pub fn to_html(code: &str, lang: &str) -> Option<String> {
    let syntax = syntax(lang)?;
    let hl = &*HL;
    let theme = hl.themes.themes.get("InspiredGitHub")?;
    syntect::html::highlighted_html_for_string(code, &hl.syntaxes, syntax, theme).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_many_languages() {
        let code = "fn main() {\n    let x = 1;\n}";
        let spans = highlight(code, "rust", false).unwrap();
        assert_eq!(spans.last().unwrap().range.end, code.len());
        assert!(
            spans
                .iter()
                .map(|s| s.color)
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 1
        );
        // two-face で増えた言語
        for lang in ["ts", "typescript", "toml", "dockerfile", "kotlin", "tsx"] {
            assert!(highlight("x = 1", lang, true).is_some(), "{lang}");
        }
        assert!(highlight("x", "no-such-lang", true).is_none());
        assert!(highlight("x", "", false).is_none());
        assert!(to_html("let a = 1;", "js").unwrap().starts_with("<pre"));
    }

    #[test]
    fn async_highlight_completes_in_background() {
        let ctx = egui::Context::default();
        let code = "let unique_name_for_async_test = 42;";
        assert!(
            highlight_async(&ctx, code, "rust", false).is_none(),
            "first call only queues"
        );
        let mut done = None;
        for _ in 0..500 {
            done = highlight_async(&ctx, code, "rust", false);
            if done.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(done.is_some(), "finished in the background");
        assert!(highlight_async(&ctx, "x", "", false).is_none());
    }
}
