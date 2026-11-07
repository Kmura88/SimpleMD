//! # 機能: Markdown の解析（pulldown-cmark のイベント → [`Document`]）
//!
//! - 解析オプション（表・脚注・タスクリスト・数式・Wiki リンクなど）
//! - 行内要素の組み立て（書式の入れ子、CJK 文字間の改行の扱い、画像・リンク）
//! - ブロックの組み立て（引用・リスト・表・コード・脚注定義などをスタックで管理）
//! - 見出しのアンカー（GitHub 互換のスラッグ、重複時は `-1` などを付与）
//! - 脚注（参照順に番号付け）
//! - HTML ブロック（画像と文字の取り出し、`<details>` 折りたたみ）

use std::collections::HashMap;
use std::sync::LazyLock;

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, LinkType, Options, Parser, Tag,
    TagEnd,
};
use regex::Regex;

use super::ast::*;

// ==== 機能: 解析の入口・オプション ====

/// pulldown-cmark に渡す拡張機能の一式。
pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
        // 機能: Wiki リンク `[[ノート名]]` / `[[ノート名#見出し|表示名]]`
        | Options::ENABLE_WIKILINKS
}

/// Wiki リンクのリンク先に付ける接頭辞。アプリ側（`follow_link`）でノート名から解決する。
pub const WIKI_PREFIX: &str = "wiki:";

/// Markdown のソースを解析して [`Document`] にする。
pub fn parse(src: &str) -> Document {
    let mut b = Builder::default();
    // 最初にルートを積んでおく（文書が閉じていない `<details>` で始まる場合でも、
    // ルートとコンテナを取り違えないように）。
    b.stack.push((Frame::Root, Vec::new()));
    for event in Parser::new_ext(src, options()) {
        b.event(event);
    }
    b.finish()
}

// ==== 機能: 組み立て途中の状態（スタックの枠・葉・行内バッファ） ====

/// 子ブロックを持つ入れ物（コンテナ）。開始タグで積み、終了タグで降ろしてブロックにする。
enum Frame {
    Root,
    Quote(Option<Alert>),
    List(Option<u64>, Vec<ListItem>),
    Item(Option<bool>),
    Footnote(String),
    /// 機能: `<details>` 折りたたみ（開始タグと終了タグが別の HTML ブロックにある場合）
    Details {
        summary: Inline,
        open: bool,
    },
}

/// 子ブロックを持たない、組み立て中の末端ブロック（段落・見出し・コード・HTML・フロントマター・表）。
enum Leaf {
    Paragraph,
    Heading {
        level: u8,
        id: Option<String>,
    },
    Code {
        lang: String,
        code: String,
    },
    Html(String),
    Meta(String),
    Table {
        aligns: Vec<ColumnAlign>,
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
        row: Vec<Inline>,
    },
}

// ==== 機能: インライン要素の組み立て ====

/// 行内要素を貯めるバッファ。書式は入れ子になり得るので、開始/終了の回数を数えて管理する。
#[derive(Default)]
struct InlineBuf {
    runs: Vec<Run>,
    strong: u32,
    emphasis: u32,
    strike: u32,
    code: u32,
    superscript: u32,
    subscript: u32,
    mark: u32,
    links: Vec<String>,
    /// 読み取り中の画像の (url, title, alt)。画像の中のテキストは alt に入る。
    image: Option<(String, String, String)>,
    /// 直前にソフト改行があった（次のテキストを足すときに空白にするか決める）。
    pending_soft_break: bool,
}

impl InlineBuf {
    /// 現在の書式（各カウンタが 1 以上なら有効）。
    fn style(&self) -> Style {
        Style {
            strong: self.strong > 0,
            emphasis: self.emphasis > 0,
            strike: self.strike > 0,
            code: self.code > 0,
            math: false,
            superscript: self.superscript > 0,
            subscript: self.subscript > 0,
            mark: self.mark > 0,
            footnote_ref: false,
            link: self.links.last().cloned(),
        }
    }

    /// 現在の書式でテキストを追加する。
    fn push_text(&mut self, text: &str) {
        let style = self.style();
        self.push_styled(text, style);
    }

    /// 書式を指定してテキストを追加する。画像の読み取り中なら alt に足す。
    fn push_styled(&mut self, text: &str, style: Style) {
        if let Some((_, _, alt)) = &mut self.image {
            alt.push_str(text);
            return;
        }
        if text.is_empty() {
            return;
        }
        if std::mem::take(&mut self.pending_soft_break) {
            // ソフト改行は空白にする。ただし CJK 文字どうしの間ではつなげる
            // （日本語・中国語は単語の間に空白を入れないため）。
            let prev = self.last_char();
            let next = text.chars().next();
            let join = matches!((prev, next), (Some(a), Some(b)) if is_cjk(a) && is_cjk(b));
            if !join {
                self.append(" ", self.style());
            }
        }
        self.append(text, style);
    }

    /// テキストを追加する。直前と同じ書式なら同じ [`Run`] にまとめる。
    fn append(&mut self, text: &str, style: Style) {
        if let Some(Run::Text { text: t, style: s }) = self.runs.last_mut()
            && *s == style
        {
            t.push_str(text);
            return;
        }
        self.runs.push(Run::Text {
            text: text.to_string(),
            style,
        });
    }

    /// 最後に追加した文字（直前が画像なら `None`）。
    fn last_char(&self) -> Option<char> {
        match self.runs.last()? {
            Run::Text { text, .. } => text.chars().last(),
            Run::Image(_) => None,
        }
    }

    /// 画像を追加する。
    fn push_image(&mut self, img: ImageRef) {
        self.pending_soft_break = false;
        self.runs.push(Run::Image(img));
    }

    /// 貯めた内容を [`Inline`] として取り出し、バッファを空にする。
    fn take(&mut self) -> Inline {
        self.pending_soft_break = false;
        Inline {
            runs: std::mem::take(&mut self.runs),
        }
    }

    /// 空白以外の文字か画像があれば true。
    fn has_content(&self) -> bool {
        self.runs.iter().any(|r| match r {
            Run::Text { text, .. } => !text.trim().is_empty(),
            Run::Image(_) => true,
        })
    }
}

// ==== 機能: ブロックの組み立て ====

/// イベントを受け取って文書を組み立てる本体。
#[derive(Default)]
struct Builder {
    /// 開いているコンテナと、その中に溜まった子ブロック。先頭は常にルート。
    stack: Vec<(Frame, Vec<Block>)>,
    leaf: Option<Leaf>,
    inline: InlineBuf,
    headings: Vec<Heading>,
    /// アンカー名ごとの使用回数（重複したアンカーに番号を付けるため）。
    anchors: HashMap<String, usize>,
    /// 脚注ラベル → 表示番号（最初に参照された順）。
    footnote_numbers: HashMap<String, usize>,
    footnote_defs: Vec<(String, Vec<Block>)>,
    front_matter: Option<String>,
}

impl Builder {
    /// 現在のコンテナの子ブロック一覧（スタックが空ならルートを積み直す）。
    fn blocks(&mut self) -> &mut Vec<Block> {
        if self.stack.is_empty() {
            self.stack.push((Frame::Root, Vec::new()));
        }
        &mut self.stack.last_mut().unwrap().1
    }

    /// 現在のコンテナにブロックを追加する。
    fn push_block(&mut self, block: Block) {
        self.blocks().push(block);
    }

    /// 明示的な段落の外で溜まったテキスト（詰めて書かれたリスト項目など）を段落として出す。
    fn flush_implicit_paragraph(&mut self) {
        if self.leaf.is_none() {
            if self.inline.has_content() {
                let inline = self.inline.take();
                self.push_block(Block::Paragraph(inline));
            } else {
                self.inline.take();
            }
        }
    }

    /// イベント 1 つを処理する。
    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => match &mut self.leaf {
                Some(Leaf::Code { code, .. }) => code.push_str(&t),
                Some(Leaf::Meta(m)) => m.push_str(&t),
                Some(Leaf::Html(h)) => h.push_str(&t),
                _ => self.inline.push_text(&t),
            },
            Event::Code(t) => {
                let mut style = self.inline.style();
                style.code = true;
                self.inline.push_styled(&t, style);
            }
            Event::InlineMath(t) => {
                let mut style = self.inline.style();
                style.math = true;
                self.inline.push_styled(&t, style);
            }
            Event::DisplayMath(t) => {
                // ブロック数式は独立したブロックとして描くので、手前の行内要素を段落として確定させる。
                if self.inline.has_content() {
                    let inline = self.inline.take();
                    self.push_block(Block::Paragraph(inline));
                } else {
                    self.inline.take();
                }
                self.push_block(Block::Math(t.trim().to_string()));
            }
            Event::Html(t) => {
                if let Some(Leaf::Html(h)) = &mut self.leaf {
                    h.push_str(&t);
                } else {
                    self.inline_html(&t);
                }
            }
            Event::InlineHtml(t) => self.inline_html(&t),
            Event::FootnoteReference(label) => {
                let next = self.footnote_numbers.len() + 1;
                let n = *self
                    .footnote_numbers
                    .entry(label.to_string())
                    .or_insert(next);
                let mut style = self.inline.style();
                style.footnote_ref = true;
                style.link = Some(format!("#{}", footnote_anchor(&label)));
                self.inline.push_styled(&format!("[{n}]"), style);
            }
            Event::SoftBreak => {
                if !self.inline.runs.is_empty() {
                    self.inline.pending_soft_break = true;
                }
            }
            Event::HardBreak => {
                self.inline.pending_soft_break = false;
                self.inline.push_text("\n");
            }
            Event::Rule => {
                self.flush_implicit_paragraph();
                self.push_block(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some((Frame::Item(task), _)) = self
                    .stack
                    .iter_mut()
                    .rev()
                    .find(|(f, _)| matches!(f, Frame::Item(_)))
                {
                    *task = Some(checked);
                }
            }
        }
    }

    /// 開始タグ: 葉ブロックを始める、コンテナを積む、行内書式のカウンタを増やす。
    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.flush_implicit_paragraph();
                self.leaf = Some(Leaf::Paragraph);
            }
            Tag::Heading { level, id, .. } => {
                self.flush_implicit_paragraph();
                self.leaf = Some(Leaf::Heading {
                    level: heading_level(level),
                    id: id.map(|s| s.to_string()),
                });
            }
            Tag::BlockQuote(kind) => {
                self.flush_implicit_paragraph();
                self.stack
                    .push((Frame::Quote(kind.map(alert_kind)), Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.flush_implicit_paragraph();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split(|c: char| c.is_whitespace() || c == ',' || c == '{')
                        .next()
                        .unwrap_or("")
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.leaf = Some(Leaf::Code {
                    lang,
                    code: String::new(),
                });
            }
            Tag::HtmlBlock => {
                self.flush_implicit_paragraph();
                self.leaf = Some(Leaf::Html(String::new()));
            }
            Tag::List(start) => {
                self.flush_implicit_paragraph();
                self.stack
                    .push((Frame::List(start, Vec::new()), Vec::new()));
            }
            Tag::Item => {
                self.flush_implicit_paragraph();
                self.stack.push((Frame::Item(None), Vec::new()));
            }
            Tag::FootnoteDefinition(label) => {
                self.flush_implicit_paragraph();
                self.stack
                    .push((Frame::Footnote(label.to_string()), Vec::new()));
            }
            Tag::Table(aligns) => {
                self.flush_implicit_paragraph();
                self.leaf = Some(Leaf::Table {
                    aligns: aligns.into_iter().map(column_align).collect(),
                    head: Vec::new(),
                    rows: Vec::new(),
                    row: Vec::new(),
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(Leaf::Table { row, .. }) = &mut self.leaf {
                    row.clear();
                }
            }
            Tag::TableCell => {
                self.inline.take();
            }
            Tag::Emphasis => self.inline.emphasis += 1,
            Tag::Strong => self.inline.strong += 1,
            Tag::Strikethrough => self.inline.strike += 1,
            Tag::Superscript => self.inline.superscript += 1,
            Tag::Subscript => self.inline.subscript += 1,
            Tag::Link {
                link_type,
                dest_url,
                ..
            } => {
                let url = if matches!(link_type, LinkType::WikiLink { .. }) {
                    format!("{WIKI_PREFIX}{dest_url}")
                } else {
                    dest_url.to_string()
                };
                self.inline.links.push(url);
            }
            Tag::Image {
                dest_url, title, ..
            } => {
                self.inline.image = Some((dest_url.to_string(), title.to_string(), String::new()));
            }
            Tag::MetadataBlock(_) => {
                self.leaf = Some(Leaf::Meta(String::new()));
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {}
        }
    }

    /// 終了タグ: 葉ブロックやコンテナを確定させて親に追加する、行内書式のカウンタを減らす。
    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.leaf = None;
                if self.inline.has_content() {
                    let inline = self.inline.take();
                    self.push_block(Block::Paragraph(inline));
                } else {
                    self.inline.take();
                }
            }
            TagEnd::Heading(_) => {
                let Some(Leaf::Heading { level, id }) = self.leaf.take() else {
                    return;
                };
                let content = self.inline.take();
                let text = content.plain_text().trim().to_string();
                let base = id.unwrap_or_else(|| slugify(&text));
                let anchor = self.unique_anchor(base);
                let ordinal = self.headings.len();
                self.headings.push(Heading {
                    level,
                    text,
                    anchor: anchor.clone(),
                });
                self.push_block(Block::Heading {
                    level,
                    content,
                    anchor,
                    ordinal,
                });
            }
            TagEnd::BlockQuote(_) => {
                self.flush_implicit_paragraph();
                if let Some((Frame::Quote(alert), blocks)) = self.stack.pop() {
                    self.push_block(Block::Quote { alert, blocks });
                }
            }
            TagEnd::CodeBlock => {
                if let Some(Leaf::Code { lang, mut code }) = self.leaf.take() {
                    if code.ends_with('\n') {
                        code.pop();
                    }
                    self.push_block(Block::Code { lang, code });
                }
            }
            TagEnd::HtmlBlock => {
                if let Some(Leaf::Html(html)) = self.leaf.take() {
                    self.html_block(&html);
                }
            }
            TagEnd::List(_) => {
                self.flush_implicit_paragraph();
                if let Some((Frame::List(start, items), _)) = self.stack.pop() {
                    self.push_block(Block::List { start, items });
                }
            }
            TagEnd::Item => {
                self.flush_implicit_paragraph();
                if let Some((Frame::Item(task), blocks)) = self.stack.pop()
                    && let Some((Frame::List(_, items), _)) = self.stack.last_mut()
                {
                    items.push(ListItem { task, blocks });
                }
            }
            TagEnd::FootnoteDefinition => {
                self.flush_implicit_paragraph();
                if let Some((Frame::Footnote(label), blocks)) = self.stack.pop() {
                    self.footnote_defs.push((label, blocks));
                }
            }
            TagEnd::Table => {
                if let Some(Leaf::Table {
                    aligns, head, rows, ..
                }) = self.leaf.take()
                {
                    self.push_block(Block::Table { aligns, head, rows });
                }
            }
            TagEnd::TableHead => {
                if let Some(Leaf::Table { head, row, .. }) = &mut self.leaf {
                    *head = std::mem::take(row);
                }
            }
            TagEnd::TableRow => {
                if let Some(Leaf::Table { rows, row, .. }) = &mut self.leaf {
                    rows.push(std::mem::take(row));
                }
            }
            TagEnd::TableCell => {
                let cell = self.inline.take();
                if let Some(Leaf::Table { row, .. }) = &mut self.leaf {
                    row.push(cell);
                }
            }
            TagEnd::Emphasis => self.inline.emphasis = self.inline.emphasis.saturating_sub(1),
            TagEnd::Strong => self.inline.strong = self.inline.strong.saturating_sub(1),
            TagEnd::Strikethrough => self.inline.strike = self.inline.strike.saturating_sub(1),
            TagEnd::Superscript => {
                self.inline.superscript = self.inline.superscript.saturating_sub(1)
            }
            TagEnd::Subscript => self.inline.subscript = self.inline.subscript.saturating_sub(1),
            TagEnd::Link => {
                self.inline.links.pop();
            }
            TagEnd::Image => {
                if let Some((url, title, alt)) = self.inline.image.take() {
                    let link = self.inline.links.last().cloned();
                    self.inline.push_image(ImageRef {
                        url,
                        alt,
                        title,
                        link,
                    });
                }
            }
            TagEnd::MetadataBlock(_) => {
                if let Some(Leaf::Meta(m)) = self.leaf.take() {
                    self.front_matter = Some(m.trim_end().to_string());
                }
            }
            TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => {}
        }
    }

    // ==== 機能: インライン HTML（<b> / <kbd> / <br> / <a> / <img> など） ====

    /// 行内の HTML 書式タグを最低限サポートする（未対応のタグは無視）。
    fn inline_html(&mut self, html: &str) {
        let Some(c) = TAG.captures(html.trim()) else {
            return;
        };
        let closing = !c[1].is_empty();
        let name = c[2].to_ascii_lowercase();
        let attrs = c.get(3).map_or("", |m| m.as_str());
        let counter = match name.as_str() {
            "b" | "strong" => Some(&mut self.inline.strong),
            "i" | "em" => Some(&mut self.inline.emphasis),
            "s" | "del" | "strike" => Some(&mut self.inline.strike),
            "code" | "kbd" | "samp" | "tt" => Some(&mut self.inline.code),
            "sup" => Some(&mut self.inline.superscript),
            "sub" => Some(&mut self.inline.subscript),
            "mark" => Some(&mut self.inline.mark),
            _ => None,
        };
        if let Some(counter) = counter {
            if closing {
                *counter = counter.saturating_sub(1);
            } else {
                *counter += 1;
            }
            return;
        }
        match name.as_str() {
            "br" => {
                self.inline.pending_soft_break = false;
                self.inline.push_text("\n");
            }
            "a" if closing => {
                self.inline.links.pop();
            }
            "a" => {
                let href = attr(attrs, "href").unwrap_or_default();
                self.inline.links.push(href);
            }
            "img" => {
                if let Some(url) = attr(attrs, "src") {
                    let link = self.inline.links.last().cloned();
                    self.inline.push_image(ImageRef {
                        url,
                        alt: attr(attrs, "alt").unwrap_or_default(),
                        title: attr(attrs, "title").unwrap_or_default(),
                        link,
                    });
                }
            }
            _ => {}
        }
    }

    // ==== 機能: 見出しのアンカー ====

    /// 重複しないアンカー名を返す（2 回目以降は `base-1`, `base-2` …）。
    fn unique_anchor(&mut self, base: String) -> String {
        let count = self.anchors.entry(base.clone()).or_insert(0);
        let anchor = if *count == 0 {
            base.clone()
        } else {
            format!("{base}-{count}")
        };
        *count += 1;
        anchor
    }

    // ==== 機能: HTML ブロック（`<details>` 折りたたみを含む） ====

    /// HTML ブロックを処理する。`<details>` は開閉の位置に応じて次の 3 通りに分ける。
    /// 1. 1 つのブロック内で開いて閉じる → 中身を Markdown として再帰的に解析して `Block::Details`
    /// 2. 開始タグだけ → `Frame::Details` を積み、後続の Markdown ブロックをその中に入れる
    /// 3. 終了タグだけ（直前に 2 がある） → フレームを閉じて `Block::Details` にする
    fn html_block(&mut self, html: &str) {
        let open = DETAILS_OPEN.find(html);
        let close = DETAILS_CLOSE.find(html);
        match (open, close) {
            (Some(o), Some(c)) if c.start() >= o.end() => {
                self.plain_html(&html[..o.start()]);
                let is_open = DETAILS_OPEN_ATTR.is_match(o.as_str());
                let (summary, rest) = split_summary(&html[o.end()..c.start()]);
                let blocks = parse_nested(rest);
                self.push_block(Block::Details {
                    summary,
                    open: is_open,
                    blocks,
                });
                self.html_block(&html[c.end()..]);
            }
            (Some(o), _) => {
                self.plain_html(&html[..o.start()]);
                let is_open = DETAILS_OPEN_ATTR.is_match(o.as_str());
                let (summary, rest) = split_summary(&html[o.end()..]);
                let blocks = parse_nested(rest);
                self.stack.push((
                    Frame::Details {
                        summary,
                        open: is_open,
                    },
                    blocks,
                ));
            }
            (None, Some(c)) if matches!(self.stack.last(), Some((Frame::Details { .. }, _))) => {
                let before = parse_nested(&html[..c.start()]);
                self.blocks().extend(before);
                self.close_details();
                self.plain_html(&html[c.end()..]);
            }
            _ => self.plain_html(html),
        }
    }

    /// `Frame::Details` を閉じて親に `Block::Details` を追加する。
    fn close_details(&mut self) {
        self.flush_implicit_paragraph();
        if let Some((Frame::Details { summary, open }, blocks)) = self.stack.pop() {
            self.push_block(Block::Details {
                summary,
                open,
                blocks,
            });
        }
    }

    /// 普通の HTML ブロック（画像と文字だけ取り出す）。空なら何もしない。
    fn plain_html(&mut self, html: &str) {
        if html.trim().is_empty() {
            return;
        }
        let block = parse_html_block(html);
        if !block.images.is_empty() || !block.text.is_empty() {
            self.push_block(Block::Html(block));
        }
    }

    // ==== 機能: 文書の仕上げ・脚注 ====

    /// 解析の後始末をして [`Document`] を作る。
    /// 脚注は参照された順に番号を振り、参照されていない定義は末尾に続けて番号を振る。
    fn finish(mut self) -> Document {
        self.flush_implicit_paragraph();
        // 閉じられていないコンテナを片付ける（`<details>` は閉じ忘れでも折りたたみとして残す）。
        while self.stack.len() > 1 {
            if matches!(self.stack.last(), Some((Frame::Details { .. }, _))) {
                self.close_details();
                continue;
            }
            let (_, blocks) = self.stack.pop().unwrap();
            self.blocks().extend(blocks);
        }
        let blocks = self.stack.pop().map(|(_, b)| b).unwrap_or_default();

        let mut defs = self.footnote_defs;
        let mut next = self.footnote_numbers.len() + 1;
        let mut footnotes: Vec<Footnote> = defs
            .drain(..)
            .map(|(label, blocks)| {
                let number = *self
                    .footnote_numbers
                    .entry(label.clone())
                    .or_insert_with(|| {
                        next += 1;
                        next - 1
                    });
                Footnote {
                    anchor: footnote_anchor(&label),
                    label,
                    number,
                    blocks,
                }
            })
            .collect();
        footnotes.sort_by_key(|f| f.number);

        Document {
            blocks,
            footnotes,
            headings: self.headings,
            front_matter: self.front_matter,
        }
    }
}

// ==== 機能: pulldown-cmark の型からの変換 ====

/// 見出しレベルを数値（1〜6）にする。
fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// 引用の注記の種類を [`Alert`] にする。
fn alert_kind(kind: BlockQuoteKind) -> Alert {
    match kind {
        BlockQuoteKind::Note => Alert::Note,
        BlockQuoteKind::Tip => Alert::Tip,
        BlockQuoteKind::Important => Alert::Important,
        BlockQuoteKind::Warning => Alert::Warning,
        BlockQuoteKind::Caution => Alert::Caution,
    }
}

/// 表の列の揃え方を [`ColumnAlign`] にする。
fn column_align(a: Alignment) -> ColumnAlign {
    match a {
        Alignment::None => ColumnAlign::None,
        Alignment::Left => ColumnAlign::Left,
        Alignment::Center => ColumnAlign::Center,
        Alignment::Right => ColumnAlign::Right,
    }
}

// ==== 機能: アンカー名・スラッグ・CJK 判定 ====

/// 脚注ラベルからアンカー名（`fn-<スラッグ>`）を作る。
pub fn footnote_anchor(label: &str) -> String {
    format!("fn-{}", slugify(label))
}

/// GitHub 互換の見出しスラッグ: 小文字化し、空白は `-` に、記号は取り除く。
pub fn slugify(text: &str) -> String {
    text.trim()
        .chars()
        .filter_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                Some(c.to_lowercase().next().unwrap_or(c))
            } else if c.is_whitespace() {
                Some('-')
            } else {
                None
            }
        })
        .collect()
}

/// CJK（日本語・中国語・韓国語）の文字なら true。ソフト改行を空白にするかの判定に使う。
pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F   // CJK の句読点・記号
        | 0x3040..=0x30FF // ひらがな・カタカナ
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFFEF // 全角形
        | 0xAC00..=0xD7AF // ハングル
    )
}

// ==== 機能: HTML の補助（タグ・属性・文字参照） ====

/// 文字列の先頭にある HTML タグ 1 つ（終了タグか, タグ名, 属性）。
static TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^<(/?)([A-Za-z][A-Za-z0-9]*)\b([^>]*)>").unwrap());
/// 任意の位置の HTML タグまたはコメント。
static ANY_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->|<(/?)([A-Za-z][A-Za-z0-9]*)\b([^>]*)>").unwrap());

/// 属性文字列から `name` の値を取り出す（引用符あり/なしの両方に対応、文字参照はデコード）。
fn attr(attrs: &str, name: &str) -> Option<String> {
    let re = Regex::new(&format!(
        r#"(?i)\b{name}\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+))"#
    ))
    .ok()?;
    let c = re.captures(attrs)?;
    let v = c.get(1).or(c.get(2)).or(c.get(3))?.as_str();
    Some(decode_entities(v))
}

/// よく使う HTML 文字参照（`&amp;` `&lt;` `&nbsp;` など）をデコードする。
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&nbsp;", "\u{a0}")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

// ==== 機能: HTML ブロック / <details> ====

/// `<details ...>` の開始タグ。
static DETAILS_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<details\b[^>]*>").unwrap());
/// `open` 属性付きの `<details>`。
static DETAILS_OPEN_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)<details\b[^>]*\bopen\b").unwrap());
/// `</details>` の終了タグ。
static DETAILS_CLOSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</details\s*>").unwrap());
/// 先頭の `<summary>…</summary>`。
static SUMMARY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)^\s*<summary\b[^>]*>(.*?)</summary\s*>").unwrap());

/// `<summary>…</summary>` を取り出し、(見出しのインライン, 残りの本文) を返す。
/// summary が無い場合は「Details」を見出しにする。
fn split_summary(html: &str) -> (Inline, &str) {
    let (text, rest) = match SUMMARY.captures(html) {
        Some(c) => (c[1].to_string(), &html[c.get(0).unwrap().end()..]),
        None => ("Details".to_string(), html),
    };
    // summary 内の Markdown（`**太字**` など）も効くように 1 段落として解析する。
    let summary = parse(text.trim())
        .blocks
        .into_iter()
        .find_map(|b| match b {
            Block::Paragraph(i) => Some(i),
            Block::Heading { content, .. } => Some(content),
            _ => None,
        })
        .unwrap_or_else(|| Inline {
            runs: vec![Run::Text {
                text: decode_entities(text.trim()),
                style: Style::default(),
            }],
        });
    (summary, rest)
}

/// `<details>` の中身を Markdown として解析する。
/// 入れ子の見出しは外側の目次に載らないので、目次番号を無効値にしておく。
fn parse_nested(src: &str) -> Vec<Block> {
    if src.trim().is_empty() {
        return Vec::new();
    }
    let mut blocks = parse(&dedent(src)).blocks;
    clear_ordinals(&mut blocks);
    blocks
}

/// 共通のインデントを取り除く（HTML 内で字下げされた Markdown をコードブロック扱いさせない）。
fn dedent(src: &str) -> String {
    let indent = src
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    src.lines()
        .map(|l| {
            if l.len() >= indent {
                &l[indent..]
            } else {
                l.trim_start()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 見出しの目次番号を `usize::MAX`（＝目次に無い）にする。
fn clear_ordinals(blocks: &mut [Block]) {
    for b in blocks {
        match b {
            Block::Heading { ordinal, .. } => *ordinal = usize::MAX,
            Block::Quote { blocks, .. } | Block::Details { blocks, .. } => clear_ordinals(blocks),
            Block::List { items, .. } => {
                for item in items {
                    clear_ordinals(&mut item.blocks);
                }
            }
            _ => {}
        }
    }
}

/// 生の HTML ブロックから画像と読める文字を取り出す。
/// `<script>` / `<style>` の中身は捨て、ブロック要素のタグは改行に置き換える。
fn parse_html_block(html: &str) -> HtmlBlock {
    let mut block = HtmlBlock::default();
    let mut links: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut last = 0;
    let mut skip_depth = 0u32; // <script>/<style> の中にいる深さ
    for c in ANY_TAG.captures_iter(html) {
        let m = c.get(0).unwrap();
        if skip_depth == 0 {
            text.push_str(&html[last..m.start()]);
        }
        last = m.end();
        let Some(name) = c.get(2) else { continue }; // コメント
        let name = name.as_str().to_ascii_lowercase();
        let closing = !c[1].is_empty();
        let attrs = c.get(3).map_or("", |m| m.as_str());
        match name.as_str() {
            "script" | "style" => {
                if closing {
                    skip_depth = skip_depth.saturating_sub(1);
                } else {
                    skip_depth += 1;
                }
            }
            "a" if closing => {
                links.pop();
            }
            "a" => links.push(attr(attrs, "href").unwrap_or_default()),
            "img" => {
                if let Some(url) = attr(attrs, "src") {
                    block.images.push(ImageRef {
                        url,
                        alt: attr(attrs, "alt").unwrap_or_default(),
                        title: attr(attrs, "title").unwrap_or_default(),
                        link: links.last().cloned().filter(|l| !l.is_empty()),
                    });
                }
            }
            "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            | "summary" | "details" | "table" | "ul" | "ol" | "pre" | "blockquote" => {
                text.push('\n')
            }
            _ => {}
        }
    }
    if skip_depth == 0 {
        text.push_str(&html[last..]);
    }
    let text = decode_entities(&text);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    block.text = lines.join("\n");
    block
}

// ==== 機能: テスト ====

#[cfg(test)]
mod tests {
    use super::*;

    /// 段落・見出しのプレーンテキスト（それ以外のブロックなら panic）。
    fn text_of(b: &Block) -> String {
        match b {
            Block::Paragraph(i) | Block::Heading { content: i, .. } => i.plain_text(),
            other => panic!("not text: {other:?}"),
        }
    }

    #[test]
    fn headings_and_anchors() {
        let d = parse("# Hello World!\n## Hello World\n## 日本語 見出し\n# Custom {#my-id}");
        let anchors: Vec<_> = d.headings.iter().map(|h| h.anchor.as_str()).collect();
        assert_eq!(
            anchors,
            ["hello-world", "hello-world-1", "日本語-見出し", "my-id"]
        );
        assert_eq!(d.headings[2].level, 2);
        assert!(matches!(d.blocks[3], Block::Heading { ordinal: 3, .. }));
    }

    #[test]
    fn inline_styles() {
        let d = parse("a **b *c*** ~~d~~ `e` [f](http://x)");
        let Block::Paragraph(i) = &d.blocks[0] else {
            panic!()
        };
        let styled: Vec<(String, bool, bool, bool, bool, Option<String>)> = i
            .runs
            .iter()
            .map(|r| match r {
                Run::Text { text, style } => (
                    text.clone(),
                    style.strong,
                    style.emphasis,
                    style.strike,
                    style.code,
                    style.link.clone(),
                ),
                _ => panic!(),
            })
            .collect();
        assert_eq!(styled[0], ("a ".into(), false, false, false, false, None));
        assert_eq!(styled[1], ("b ".into(), true, false, false, false, None));
        assert_eq!(styled[2], ("c".into(), true, true, false, false, None));
        assert_eq!(styled[4], ("d".into(), false, false, true, false, None));
        assert_eq!(styled[6], ("e".into(), false, false, false, true, None));
        assert_eq!(styled[8].5.as_deref(), Some("http://x"));
    }

    #[test]
    fn soft_breaks_join_cjk() {
        let d = parse("これは\n日本語です。\nEnglish\ntext");
        assert_eq!(text_of(&d.blocks[0]), "これは日本語です。 English text");
    }

    #[test]
    fn tight_and_task_lists() {
        let d = parse("- [ ] todo\n- [x] done\n  - nested\n\n1. one\n2. two");
        let Block::List { start: None, items } = &d.blocks[0] else {
            panic!("{:?}", d.blocks[0])
        };
        assert_eq!(items[0].task, Some(false));
        assert_eq!(items[1].task, Some(true));
        assert_eq!(text_of(&items[0].blocks[0]), "todo");
        assert!(matches!(items[1].blocks[1], Block::List { .. }));
        assert!(matches!(d.blocks[1], Block::List { start: Some(1), .. }));
    }

    #[test]
    fn table() {
        let d = parse("| a | b |\n|:--|--:|\n| 1 | **2** |\n| 3 | 4 |");
        let Block::Table { aligns, head, rows } = &d.blocks[0] else {
            panic!()
        };
        assert_eq!(aligns, &[ColumnAlign::Left, ColumnAlign::Right]);
        assert_eq!(head[1].plain_text(), "b");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][1].plain_text(), "2");
    }

    #[test]
    fn alerts_and_quotes() {
        let d = parse("> [!WARNING]\n> careful\n\n> plain");
        assert!(matches!(
            &d.blocks[0],
            Block::Quote { alert: Some(Alert::Warning), blocks } if text_of(&blocks[0]) == "careful"
        ));
        assert!(matches!(&d.blocks[1], Block::Quote { alert: None, .. }));
    }

    #[test]
    fn footnotes_numbered_by_reference() {
        let d = parse("x[^b] y[^a]\n\n[^a]: A\n[^b]: B\n[^c]: C");
        let nums: Vec<_> = d
            .footnotes
            .iter()
            .map(|f| (f.label.as_str(), f.number))
            .collect();
        assert_eq!(nums, [("b", 1), ("a", 2), ("c", 3)]);
        let Block::Paragraph(i) = &d.blocks[0] else {
            panic!()
        };
        assert!(i.runs.iter().any(|r| matches!(r,
            Run::Text { text, style } if text == "[1]" && style.footnote_ref
                && style.link.as_deref() == Some("#fn-b"))));
    }

    #[test]
    fn code_blocks() {
        let d = parse("```rust title=x\nfn main() {}\n```\n\n    indented\n");
        assert_eq!(
            d.blocks[0],
            Block::Code {
                lang: "rust".into(),
                code: "fn main() {}".into()
            }
        );
        assert_eq!(
            d.blocks[1],
            Block::Code {
                lang: String::new(),
                code: "indented".into()
            }
        );
    }

    #[test]
    fn images() {
        let d = parse("[![badge](b.svg)](http://ci) ![alt *x*](pic.png \"t\")");
        let Block::Paragraph(i) = &d.blocks[0] else {
            panic!()
        };
        assert!(i.is_image_only());
        let imgs: Vec<_> = i
            .runs
            .iter()
            .filter_map(|r| match r {
                Run::Image(img) => Some(img.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(imgs[0].link.as_deref(), Some("http://ci"));
        assert_eq!(imgs[1].alt, "alt x");
        assert_eq!(imgs[1].title, "t");
    }

    #[test]
    fn html_block() {
        let d = parse(
            "<p align=\"center\">\n  <a href=\"https://x\"><img src=\"logo.png\" alt=\"Logo\" width=100></a>\n  <br>Hello &amp; welcome<!-- hidden -->\n</p>\n",
        );
        let Block::Html(h) = &d.blocks[0] else {
            panic!("{:?}", d.blocks)
        };
        assert_eq!(h.images[0].url, "logo.png");
        assert_eq!(h.images[0].link.as_deref(), Some("https://x"));
        assert_eq!(h.text, "Hello & welcome");
        // コメントだけのブロックは消える。
        assert!(parse("<!-- note -->\n").blocks.is_empty());
    }

    #[test]
    fn wiki_links() {
        let d = parse("see [[My Note]] and [[dir/Other#Part|alias]]");
        let Block::Paragraph(i) = &d.blocks[0] else {
            panic!()
        };
        let links: Vec<(String, String)> = i
            .runs
            .iter()
            .filter_map(|r| match r {
                Run::Text { text, style } => style.link.clone().map(|l| (text.clone(), l)),
                _ => None,
            })
            .collect();
        assert_eq!(links[0], ("My Note".into(), "wiki:My Note".into()));
        assert_eq!(links[1], ("alias".into(), "wiki:dir/Other#Part".into()));
    }

    #[test]
    fn details_in_one_block() {
        let d = parse(
            "<details open><summary>**More**</summary>\n\n  - a\n  - b\n\n</details>\n\nafter",
        );
        let Block::Details {
            summary,
            open,
            blocks,
        } = &d.blocks[0]
        else {
            panic!("{:?}", d.blocks)
        };
        assert!(*open);
        assert_eq!(summary.plain_text(), "More");
        assert!(matches!(blocks[0], Block::List { .. }));
        assert_eq!(text_of(&d.blocks[1]), "after");
    }

    #[test]
    fn details_spanning_blocks() {
        let d = parse(
            "<details>\n<summary>Click</summary>\n\n## Inside\n\ntext\n\n</details>\n\nafter",
        );
        let Block::Details {
            summary,
            open,
            blocks,
        } = &d.blocks[0]
        else {
            panic!("{:?}", d.blocks)
        };
        assert!(!*open);
        assert_eq!(summary.plain_text(), "Click");
        assert_eq!(blocks.len(), 2);
        // 複数ブロックにまたがる場合は通常の見出しとして目次に載る。
        assert_eq!(d.headings.len(), 1);
        assert_eq!(text_of(&d.blocks[1]), "after");
        // 閉じ忘れても落ちない
        let d = parse("<details><summary>x</summary>\n\nbody");
        assert!(matches!(d.blocks[0], Block::Details { .. }));
    }

    #[test]
    fn inline_html_tags() {
        let d = parse("press <kbd>Ctrl</kbd>+<b>B</b><br>next");
        let Block::Paragraph(i) = &d.blocks[0] else {
            panic!()
        };
        assert_eq!(i.plain_text(), "press Ctrl+B\nnext");
        assert!(
            i.runs
                .iter()
                .any(|r| matches!(r, Run::Text { text, style } if text == "Ctrl" && style.code))
        );
        assert!(
            i.runs
                .iter()
                .any(|r| matches!(r, Run::Text { text, style } if text == "B" && style.strong))
        );
    }

    #[test]
    fn front_matter_and_math() {
        let d = parse("---\ntitle: x\n---\n# T\n\n$$\na^2\n$$\n\ninline $b$ ok");
        assert_eq!(d.front_matter.as_deref(), Some("title: x"));
        assert!(d.blocks.iter().any(|b| *b == Block::Math("a^2".into())));
        let Block::Paragraph(i) = d.blocks.last().unwrap() else {
            panic!()
        };
        assert!(
            i.runs
                .iter()
                .any(|r| matches!(r, Run::Text { text, style } if text == "b" && style.math))
        );
    }
}
