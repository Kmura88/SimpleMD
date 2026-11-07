//! テキスト処理: ソースの前処理、ラベルの整形、文字列の計測。

use egui::{Align2, Pos2, Vec2, pos2, vec2};

use super::scene::{Item, Role};

// ==== 機能: ソースの前処理 ====

/// ソースを「(行番号, 内容)」の列に変換する。
///
/// - 改行コードと BOM の正規化
/// - 先頭の YAML front matter（`---` で囲まれた部分）の除去
/// - `%%{init: ...}%%` ディレクティブと `%%` コメント行の除去
/// - 空行の除去と前後空白のトリム
pub fn preprocess(src: &str) -> Vec<(usize, String)> {
    let src = src.trim_start_matches('\u{feff}');
    let mut out = Vec::new();
    let mut in_front_matter = false;
    let mut seen_content = false;
    for (no, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if !seen_content && out.is_empty() && line == "---" {
            in_front_matter = !in_front_matter;
            if !in_front_matter {
                seen_content = true;
            }
            continue;
        }
        if in_front_matter {
            continue;
        }
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        seen_content = true;
        out.push((no + 1, line.to_string()));
    }
    out
}

/// 引用符・ブラケットを考慮せず、行を `;` で文に分割する（`"..."` 内の `;` は除く）。
pub fn split_statements(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for c in line.chars() {
        match c {
            '"' => {
                in_quote = !in_quote;
                cur.push(c);
            }
            ';' if !in_quote => {
                if !cur.trim().is_empty() {
                    out.push(cur.trim().to_string());
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

// ==== 機能: ラベルの整形 ====

/// 生のラベル文字列を表示用の行の列に整形する。
///
/// 引用符・マークダウン文字列の除去、`<br>` による改行、簡単な HTML タグの除去、
/// 実体参照（`&lt;` `#quot;` など）の復元、`**強調**` 記号の除去を行う。
pub fn clean_label(raw: &str) -> Vec<String> {
    let mut s = raw.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s = &s[1..s.len() - 1];
    }
    let s = s.trim();
    let s = s
        .strip_prefix('`')
        .and_then(|x| x.strip_suffix('`'))
        .unwrap_or(s);
    let s = replace_br(s);
    let s = s.replace("\\n", "\n");
    let s = strip_tags(&s);
    let s = decode_entities(&s);
    let s = s.replace("**", "");
    let lines: Vec<String> = s.split('\n').map(|l| l.trim().to_string()).collect();
    if lines.iter().all(|l| l.is_empty()) {
        Vec::new()
    } else {
        lines
    }
}

/// `<br>` `<br/>` `<br />`（大文字小文字を問わない）を改行に置き換える。
fn replace_br(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('<') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let close = tail.find('>');
        let is_br = close.is_some_and(|c| {
            let inner: String = tail[1..c].chars().filter(|c| !c.is_whitespace()).collect();
            matches!(inner.to_ascii_lowercase().as_str(), "br" | "br/")
        });
        match (is_br, close) {
            (true, Some(c)) => {
                out.push('\n');
                rest = &tail[c + 1..];
            }
            _ => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `<b>` や `</i>` のようなタグらしい部分を取り除く（`a < b` のような比較は残す）。
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('<') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let tag_end = tail.find('>').filter(|&c| looks_like_tag(&tail[1..c]));
        match tag_end {
            Some(c) => rest = &tail[c + 1..],
            None => {
                out.push('<');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// タグの中身（`<` と `>` の間）が HTML タグらしいかどうか。
fn looks_like_tag(inner: &str) -> bool {
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    let mut chars = inner.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && !inner.contains('<')
}

/// HTML 実体参照と Mermaid 独自の `#name;` 形式を復元する。
fn decode_entities(s: &str) -> String {
    if !s.contains('&') && !s.contains('#') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find(['&', '#']) {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let decoded = tail.find(';').filter(|&e| e <= 10).and_then(|e| {
            let name = &tail[1..e];
            let c = match name {
                "lt" => Some('<'),
                "gt" => Some('>'),
                "amp" => Some('&'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => {
                    let num = name.strip_prefix('#').unwrap_or(name);
                    num.parse::<u32>().ok().and_then(char::from_u32)
                }
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &tail[e + 1..];
            }
            None => {
                out.push_str(&tail[..1]);
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// 先頭と末尾の引用符を 1 組だけ取り除く。
pub fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

// ==== 機能: 文字列の計測 ====

/// 計測関数とフォントサイズをまとめたもの。
pub struct Measurer<'a> {
    /// 1 行のテキストの大きさを返す関数。
    f: &'a mut dyn FnMut(&str, f32) -> Vec2,
    /// 図全体の基本フォントサイズ。
    pub font: f32,
}

impl<'a> Measurer<'a> {
    /// 計測関数と基本フォントサイズから作る。
    pub fn new(f: &'a mut dyn FnMut(&str, f32) -> Vec2, font: f32) -> Self {
        Self { f, font }
    }

    /// 1 行を計測する（異常値は概算値で置き換える）。
    pub fn line(&mut self, s: &str, size: f32) -> Vec2 {
        let v = (self.f)(s, size);
        if v.x.is_finite() && v.y.is_finite() && v.x >= 0.0 && v.y >= 0.0 {
            v
        } else {
            vec2(s.chars().count() as f32 * size * 0.6, size * 1.2)
        }
    }

    /// 複数行のテキストブロックを計測する。
    pub fn block(&mut self, lines: &[String], size: f32) -> TextBlock {
        let sizes: Vec<Vec2> = lines.iter().map(|l| self.line(l, size)).collect();
        let w = sizes.iter().map(|s| s.x).fold(0.0, f32::max);
        let h = sizes.iter().map(|s| s.y).sum();
        TextBlock {
            lines: lines.to_vec(),
            sizes,
            size: vec2(w, h),
            font: size,
        }
    }

    /// 基本フォントサイズで複数行を計測する。
    pub fn text(&mut self, lines: &[String]) -> TextBlock {
        let f = self.font;
        self.block(lines, f)
    }
}

/// 計測済みの複数行テキスト。
#[derive(Clone, Debug, Default)]
pub struct TextBlock {
    /// 各行の文字列。
    pub lines: Vec<String>,
    /// 各行の大きさ。
    pub sizes: Vec<Vec2>,
    /// ブロック全体の大きさ（最大幅 × 高さ合計）。
    pub size: Vec2,
    /// フォントサイズ。
    pub font: f32,
}

impl TextBlock {
    /// 行がないかどうか。
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 各行を `center` を中心に中央揃えで描画命令として追加する。
    pub fn emit_centered(&self, items: &mut Vec<Item>, center: Pos2, color: Role) {
        self.emit(items, center, Align2::CENTER_CENTER, color);
    }

    /// ブロックを `anchor` 位置合わせで `pos` に置き、各行を同じ水平揃えで追加する。
    pub fn emit(&self, items: &mut Vec<Item>, pos: Pos2, anchor: Align2, color: Role) {
        let top = pos.y - self.size.y * anchor.y().to_factor();
        let mut y = top;
        for (line, sz) in self.lines.iter().zip(&self.sizes) {
            let x = pos.x;
            let line_anchor = Align2([anchor.x(), egui::Align::Center]);
            items.push(Item::Text {
                pos: pos2(x, y + sz.y / 2.0),
                text: line.clone(),
                size: self.font,
                color,
                anchor: line_anchor,
            });
            y += sz.y;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_label_handles_quotes_br_and_entities() {
        assert_eq!(clean_label("\"a<br>b\""), vec!["a", "b"]);
        assert_eq!(clean_label("x <br/> y"), vec!["x", "y"]);
        assert_eq!(clean_label("<b>bold</b> #quot;q#quot;"), vec!["bold \"q\""]);
        assert_eq!(clean_label("a < b"), vec!["a < b"]);
        assert_eq!(clean_label("&lt;tag&gt;"), vec!["<tag>"]);
        assert!(clean_label("  ").is_empty());
        assert_eq!(clean_label("日本語<BR>テスト"), vec!["日本語", "テスト"]);
    }

    #[test]
    fn preprocess_strips_comments_and_front_matter() {
        let src = "---\ntitle: x\n---\n%%{init: {}}%%\ngraph TD\n  %% comment\n  A-->B\n";
        let lines = preprocess(src);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].1, "graph TD");
        assert_eq!(lines[1].1, "A-->B");
    }

    #[test]
    fn split_statements_respects_quotes() {
        assert_eq!(split_statements("A-->B; B-->C"), vec!["A-->B", "B-->C"]);
        assert_eq!(split_statements("A[\"a;b\"]"), vec!["A[\"a;b\"]"]);
    }
}
