//! # 機能: HTML への書き出し・印刷
//!
//! pulldown-cmark で HTML を作り、次の加工をして 1 ファイルの HTML にする。
//! - コードブロック: syntect で色付けした HTML（オフラインでも色が付く）
//! - Mermaid: `<pre class="mermaid">` にして mermaid.js（CDN）で描画
//! - 数式: KaTeX（CDN）で描画（オフラインでは TeX のまま表示）
//! - Wiki リンク: 実際のファイルへの相対リンクに変換
//! - フロントマター: 出力しない
//! - 印刷用: 画像を絶対パスにし、開いたら印刷ダイアログを出す
//!
//! 印刷は「一時フォルダに HTML を書き出してブラウザで開き、ブラウザの印刷機能を使う」方式。

use std::path::{Path, PathBuf};

use pulldown_cmark::{CodeBlockKind, CowStr, Event, LinkType, Parser, Tag, TagEnd, html};

use super::App;
use crate::markdown::parse::options;
use crate::{highlight, util};

/// 書き出しの設定。
pub struct ExportOptions<'a> {
    pub title: &'a str,
    /// Markdown ファイルのあるフォルダ。
    pub base_dir: &'a Path,
    /// 画像を絶対パス（file://）にする（一時フォルダに書き出す印刷用）。
    pub absolute_images: bool,
    /// 開いたら印刷ダイアログを出す。
    pub print: bool,
    /// Wiki リンク名 → リンク先（`base_dir` からの相対パス）。
    pub wiki: &'a dyn Fn(&str) -> Option<String>,
}

impl App {
    /// 機能: HTML に書き出す（保存先をダイアログで選ぶ。使えなければ .md の隣に保存）。
    pub fn export_html(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.doc() else { return };
        let default = doc.path.with_extension("html");
        let html = self.render_html(false);
        let target = rfd::FileDialog::new()
            .set_directory(default.parent().unwrap_or(Path::new(".")))
            .set_file_name(
                default
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
            .add_filter("HTML", &["html"])
            .save_file();
        // ダイアログが使えない環境（None）でも .md の隣に保存できるよう、キャンセルと区別しない。
        let target = target.unwrap_or(default);
        match std::fs::write(&target, html) {
            Ok(()) => self.toast(ctx, self.s.exported(&target.display().to_string())),
            Err(e) => self.toast(ctx, self.s.failed(&e.to_string())),
        }
    }

    /// 機能: 印刷 — 一時ファイルに書き出してブラウザで開く（印刷ダイアログが自動で出る）。
    pub fn print(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.doc() else { return };
        let stem = doc
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "print".into());
        let html = self.render_html(true);
        let target: PathBuf = std::env::temp_dir().join(format!("mdview-print-{stem}.html"));
        match std::fs::write(&target, html) {
            Ok(()) => ctx.open_url(egui::OpenUrl::new_tab(format!(
                "file://{}",
                target.to_string_lossy().replace('\\', "/")
            ))),
            Err(e) => self.toast(ctx, self.s.failed(&e.to_string())),
        }
    }

    /// 開いている文書を HTML にする。
    fn render_html(&self, print: bool) -> String {
        let Some(doc) = self.doc() else {
            return String::new();
        };
        let title = doc
            .doc
            .headings
            .first()
            .map(|h| h.text.clone())
            .unwrap_or_else(|| {
                doc.path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        let base = doc.base_dir.clone();
        let wiki = |name: &str| {
            let (path, anchor) = self.resolve_wiki(name)?;
            let rel = relative_path(&base, &path);
            Some(match anchor {
                Some(a) => format!("{rel}#{}", crate::markdown::slugify(&a)),
                None => rel,
            })
        };
        to_html(
            &doc.source,
            &ExportOptions {
                title: &title,
                base_dir: &doc.base_dir,
                absolute_images: print,
                print,
                wiki: &wiki,
            },
        )
    }
}

/// Markdown を 1 ファイルの HTML 文書にする。
pub fn to_html(src: &str, opts: &ExportOptions) -> String {
    let mut events: Vec<Event> = Vec::new();
    let mut in_meta = false;
    let mut code: Option<(String, String)> = None; // (言語, 中身)
    let mut has_math = false;
    let mut has_mermaid = false;

    for ev in Parser::new_ext(src, options()) {
        match ev {
            // フロントマターは出力しない
            Event::Start(Tag::MetadataBlock(_)) => in_meta = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_meta = false,
            _ if in_meta => {}
            // コードブロックは中身を集めてから、色付き HTML（または Mermaid）にする
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((lang, String::new()));
            }
            Event::Text(t) if code.is_some() => code.as_mut().unwrap().1.push_str(&t),
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, body)) = code.take() {
                    let html = if lang.eq_ignore_ascii_case("mermaid") {
                        has_mermaid = true;
                        format!("<pre class=\"mermaid\">{}</pre>\n", escape(&body))
                    } else {
                        highlight::to_html(&body, &lang).unwrap_or_else(|| {
                            format!("<pre><code>{}</code></pre>\n", escape(&body))
                        })
                    };
                    events.push(Event::Html(html.into()));
                }
            }
            // Wiki リンクは実際のファイルへ
            Event::Start(Tag::Link {
                link_type: LinkType::WikiLink { .. },
                dest_url,
                title,
                id,
            }) => {
                let href = (opts.wiki)(&dest_url).unwrap_or_else(|| dest_url.to_string());
                events.push(Event::Start(Tag::Link {
                    link_type: LinkType::Inline,
                    dest_url: CowStr::from(href),
                    title,
                    id,
                }));
            }
            // 印刷用は画像を絶対パスに
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) if opts.absolute_images => {
                let uri = util::image_uri(&dest_url, opts.base_dir);
                events.push(Event::Start(Tag::Image {
                    link_type,
                    dest_url: CowStr::from(uri),
                    title,
                    id,
                }));
            }
            Event::InlineMath(_) | Event::DisplayMath(_) => {
                has_math = true;
                events.push(ev);
            }
            other => events.push(other),
        }
    }

    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());

    let mut head_extra = String::new();
    let mut tail = String::new();
    if has_math {
        head_extra.push_str(KATEX_HEAD);
        tail.push_str(KATEX_SCRIPT);
    }
    if has_mermaid {
        tail.push_str(MERMAID_SCRIPT);
    }
    if opts.print {
        tail.push_str(PRINT_SCRIPT);
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"ja\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title}</title>\n<style>{CSS}</style>\n{head_extra}</head>\n\
         <body>\n<article class=\"markdown-body\">\n{body}</article>\n{tail}</body>\n</html>\n",
        title = escape(opts.title),
    )
}

/// HTML の特殊文字をエスケープする。
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `from` フォルダから `to` への相対パス（`/` 区切り）。
pub fn relative_path(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to_c: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to_c).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(
        to_c[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// GitHub 風のスタイル（ライト / ダーク / 印刷）。
const CSS: &str = r#"
:root { color-scheme: light dark; --fg:#1f2328; --muted:#59636e; --bg:#fff; --border:#d1d9e0; --code:#f6f8fa; --link:#0969da; }
@media (prefers-color-scheme: dark) { :root { --fg:#e6edf3; --muted:#9198a1; --bg:#0d1117; --border:#3d444d; --code:#151b23; --link:#4493f8; } }
body { margin:0; background:var(--bg); color:var(--fg); font-family:-apple-system,"Segoe UI","Noto Sans","Hiragino Sans","Yu Gothic UI",Meiryo,sans-serif; line-height:1.6; }
.markdown-body { max-width:860px; margin:0 auto; padding:32px 24px 64px; font-size:16px; }
h1,h2 { border-bottom:1px solid var(--border); padding-bottom:.3em; }
h1,h2,h3,h4,h5,h6 { margin:1.5em 0 .6em; line-height:1.25; }
a { color:var(--link); text-decoration:none; } a:hover { text-decoration:underline; }
code { font-family:"Cascadia Mono",Consolas,"SF Mono",Menlo,monospace; font-size:.875em; background:var(--code); padding:.15em .35em; border-radius:4px; }
pre { background:var(--code) !important; padding:14px 16px; border-radius:6px; overflow:auto; border:1px solid var(--border); font-size:.875em; line-height:1.5; }
pre code { background:none; padding:0; font-size:1em; }
blockquote { margin:0 0 1em; padding:0 1em; color:var(--muted); border-left:4px solid var(--border); }
table { border-collapse:collapse; display:block; overflow:auto; margin-bottom:1em; }
th,td { border:1px solid var(--border); padding:6px 13px; } th { background:var(--code); }
tr:nth-child(2n) td { background:color-mix(in srgb, var(--code) 60%, transparent); }
img { max-width:100%; } hr { border:none; height:3px; background:var(--border); }
ul.contains-task-list { list-style:none; padding-left:1.2em; }
.markdown-alert-note { border-color:#0969da; } .markdown-alert-tip { border-color:#1a7f37; }
.markdown-alert-important { border-color:#8250df; } .markdown-alert-warning { border-color:#9a6700; }
.markdown-alert-caution { border-color:#d1242f; }
.math-display { display:block; text-align:center; margin:1em 0; }
.footnote-definition { font-size:.875em; color:var(--muted); }
@media print { body { background:#fff; color:#000; } .markdown-body { max-width:none; padding:0; } pre { white-space:pre-wrap; } a { color:#000; } }
"#;

const KATEX_HEAD: &str = r#"<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css">
"#;

const KATEX_SCRIPT: &str = r#"<script src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js"></script>
<script>
document.querySelectorAll('.math').forEach(function (el) {
  try { katex.render(el.textContent, el, { displayMode: el.classList.contains('math-display'), throwOnError: false }); } catch (e) {}
});
</script>
"#;

const MERMAID_SCRIPT: &str = r#"<script type="module">
import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@10.9.1/dist/mermaid.esm.min.mjs';
mermaid.initialize({ startOnLoad: true, theme: matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'default' });
</script>
"#;

const PRINT_SCRIPT: &str = r#"<script>
window.addEventListener('load', function () { setTimeout(function () { window.print(); }, 600); });
</script>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_full_document() {
        let wiki = |name: &str| (name == "Other").then(|| "sub/other.md".to_string());
        let src = "---\ntitle: x\n---\n# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```rust\nfn main() {}\n```\n\n```mermaid\ngraph TD; A-->B\n```\n\nsee [[Other]] and $x^2$\n\n![i](img/p.png)\n";
        let html = to_html(
            src,
            &ExportOptions {
                title: "T <1>",
                base_dir: Path::new("/notes"),
                absolute_images: true,
                print: true,
                wiki: &wiki,
            },
        );
        assert!(html.contains("<title>T &lt;1&gt;</title>"));
        assert!(!html.contains("title: x"), "front matter must be dropped");
        assert!(html.contains("<table>"));
        assert!(html.contains("<pre style="), "code is highlighted: {html}");
        assert!(html.contains("<pre class=\"mermaid\">graph TD; A--&gt;B"));
        assert!(html.contains("href=\"sub/other.md\""));
        assert!(html.contains("katex"));
        assert!(html.contains("src=\"file:///notes/img/p.png\""));
        assert!(html.contains("window.print"));
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Path::new("/a/b"), Path::new("/a/c/d.md")),
            "../c/d.md"
        );
        assert_eq!(relative_path(Path::new("/a"), Path::new("/a/x.md")), "x.md");
    }
}
