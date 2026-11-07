//! # 機能: 文書モデル（描画用の Markdown 構文木）
//!
//! pulldown-cmark のイベントから組み立てた、描画にそのまま使えるデータ構造。
//! - 文書全体（[`Document`]）・見出し一覧・脚注
//! - ブロック要素（[`Block`]: 見出し・段落・コード・引用・リスト・表・数式・HTML・`<details>` など）
//! - 行内要素（[`Inline`] / [`Run`] / [`Style`]）と画像参照

// ==== 機能: 文書全体・見出し・脚注 ====

/// 解析済みの Markdown 文書。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
    /// 脚注の定義。最初に参照された順に並ぶ（参照されていないものは末尾）。
    pub footnotes: Vec<Footnote>,
    /// 文書内のすべての見出し（文書順）。アウトラインパネルで使う。
    pub headings: Vec<Heading>,
    /// YAML / TOML のフロントマター（生テキスト）。無ければ `None`。
    pub front_matter: Option<String>,
}

/// 見出し 1 件（アウトライン用）。`anchor` はページ内リンクの飛び先 ID。
#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub level: u8,
    pub text: String,
    pub anchor: String,
}

/// 脚注の定義 1 件。
#[derive(Debug, Clone, PartialEq)]
pub struct Footnote {
    pub label: String,
    /// 表示用の番号（1 始まり）。
    pub number: usize,
    pub anchor: String,
    pub blocks: Vec<Block>,
}

// ==== 機能: ブロック要素 ====

/// ブロック要素（段落・見出し・コードブロックなど、縦に積まれる単位）。
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading {
        level: u8,
        content: Inline,
        anchor: String,
        /// [`Document::headings`] 内での位置（添字）。
        ordinal: usize,
    },
    Paragraph(Inline),
    Code {
        lang: String,
        code: String,
    },
    /// 引用。`> [!NOTE]` などの注記なら `alert` に種類が入る。
    Quote {
        alert: Option<Alert>,
        blocks: Vec<Block>,
    },
    List {
        /// 番号付きリストなら `Some(n)`（`n` は開始番号）。
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Table {
        aligns: Vec<ColumnAlign>,
        head: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    /// ブロック数式（`$$ ... $$`）。整形済みテキストとして表示する。
    Math(String),
    /// 生の HTML ブロック。中の画像は取り出し、それ以外のタグは取り除く。
    Html(HtmlBlock),
    /// 区切り線（`---`）。
    Rule,
    /// 機能: `<details><summary>…</summary>…</details>` の折りたたみ。
    Details {
        summary: Inline,
        /// `<details open>` のとき最初から開いておく。
        open: bool,
        blocks: Vec<Block>,
    },
}

/// リストの項目 1 つ。
#[derive(Debug, Clone, PartialEq)]
pub struct ListItem {
    /// タスクリストの項目なら `Some(チェック済みか)`。
    pub task: Option<bool>,
    pub blocks: Vec<Block>,
}

/// 表の列の揃え方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAlign {
    None,
    Left,
    Center,
    Right,
}

// ==== 機能: 注記（> [!NOTE] など） ====

/// GitHub 形式の注記（`> [!NOTE]` など）の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl Alert {
    /// 注記の見出しに付けるアイコン文字。
    pub fn icon(self) -> &'static str {
        match self {
            Alert::Note => "ℹ",
            Alert::Tip => "💡",
            Alert::Important => "❗",
            Alert::Warning => "⚠",
            Alert::Caution => "⛔",
        }
    }
}

// ==== 機能: HTML ブロック・画像参照 ====

/// HTML ブロックから取り出した表示用の中身。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HtmlBlock {
    /// すべての `<img>` タグの `src`（alt テキスト付き）。
    pub images: Vec<ImageRef>,
    /// タグを取り除いた残りのテキスト（空のこともある）。
    pub text: String,
}

/// 画像 1 枚への参照。
#[derive(Debug, Clone, PartialEq)]
pub struct ImageRef {
    pub url: String,
    pub alt: String,
    pub title: String,
    /// 画像がリンクで囲まれているときのリンク先。
    pub link: Option<String>,
}

// ==== 機能: 行内要素（テキストの連なりと書式） ====

/// 行内要素の並び（段落・見出し・表のセルなどの中身）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Inline {
    pub runs: Vec<Run>,
}

impl Inline {
    /// プレーンテキストにする（画像は alt テキストとして含める）。
    pub fn plain_text(&self) -> String {
        let mut s = String::new();
        for r in &self.runs {
            match r {
                Run::Text { text, .. } => s.push_str(text),
                Run::Image(img) => s.push_str(&img.alt),
            }
        }
        s
    }

    /// 中身が画像（と空白）だけなら true。バッジが横に並んだ行などの判定に使う。
    pub fn is_image_only(&self) -> bool {
        self.runs.iter().any(|r| matches!(r, Run::Image(_)))
            && self.runs.iter().all(|r| match r {
                Run::Image(_) => true,
                Run::Text { text, .. } => text.trim().is_empty(),
            })
    }
}

/// 行内要素の 1 単位。同じ書式のテキスト片、または画像。
#[derive(Debug, Clone, PartialEq)]
pub enum Run {
    Text { text: String, style: Style },
    Image(ImageRef),
}

/// テキスト片の書式（太字・斜体・取り消し線・コード・数式・上付き/下付き・マーカー・リンクなど）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Style {
    pub strong: bool,
    pub emphasis: bool,
    pub strike: bool,
    pub code: bool,
    pub math: bool,
    pub superscript: bool,
    pub subscript: bool,
    pub mark: bool,
    /// 脚注参照の記号（`[^1]`）。
    pub footnote_ref: bool,
    pub link: Option<String>,
}
