//! TeX 数式レンダラ (egui 0.37 用・単一ファイル・外部依存は egui のみ)
//!
//! Markdown ビューア向けの自己完結した TeX 数式モジュール。
//!
//! # 公開 API
//! - [`to_unicode`]: インライン数式 (`$...$`) を Unicode テキストで近似する。
//! - [`show_display`]: ディスプレイ数式 (`$$...$$`) を 2 次元レイアウトして描画する。
//!
//! # 処理の流れ
//! 1. 字句解析 (トークナイザ): TeX ソース → [`Token`] 列
//! 2. 構文解析 (パーサ): [`Token`] 列 → [`Node`] 木。不正な入力からも回復し、決してパニックしない。
//! 3. レイアウト: [`Node`] 木 → [`LayoutBox`] (幅・高さ・深さ + 描画アイテム)。文字は egui の Galley で計測。
//! 4. 描画: [`LayoutBox`] の描画アイテムを `egui::Painter` で描く。
//!
//! Unicode 変換 ([`to_unicode`]) は同じ [`Node`] 木から文字列を組み立てる。
//!
//! # 対応機能
//! - 上付き・下付き (入れ子、`x_i^2` の同時指定、プライム `f'`)。添字は 0.7 倍、二段目は 0.5 倍。
//! - 分数 `\frac` `\dfrac` `\tfrac` `\cfrac`、二項係数 `\binom` `\dbinom` `\tbinom`、中置の `\over` `\choose` `\atop`
//! - 根号 `\sqrt`、`\sqrt[n]` (中身に合わせて伸びる根号をパスで描画)
//! - 大型演算子 `\sum` `\prod` `\coprod` `\int` `\iint` `\iiint` `\oint` `\bigcup` `\bigcap` `\bigoplus` 等と
//!   `\lim` `\max` `\min` `\sup` `\inf` `\det` `\gcd` 等。ディスプレイ形式では添字を上下に置く
//!   (積分は右側)。`\limits` `\nolimits` で切替可能。
//! - 伸縮括弧 `\left` `\right` `\middle` (`( ) [ ] \{ \} | \| \langle \rangle \lfloor \lceil / .` 等)。
//!   1 行より高い場合は painter のパスで描画する。`\big` `\Big` `\bigg` `\Bigg` (`l` `r` `m` 付きも)。
//! - 環境: matrix pmatrix bmatrix Bmatrix vmatrix Vmatrix smallmatrix cases dcases rcases
//!   aligned align align* gathered gather gather* split array (列指定 `{lcr|}`・`\hline`) equation、`\substack`
//! - トップレベルの `\\` による複数行 (`&` を含めば aligned と同様に整列)
//! - 書体: `\text` `\textbf` `\textit` `\mathrm` `\mathbf` `\mathit` `\mathbb` `\mathcal` `\mathscr`
//!   `\mathfrak` `\mathsf` `\mathtt` `\boldsymbol` `\operatorname` (`\operatorname*` も)。太字は重ね描きで近似。
//! - アクセント: `\hat` `\widehat` `\check` `\bar` `\overline` `\underline` `\vec` `\overrightarrow`
//!   `\overleftarrow` `\dot` `\ddot` `\dddot` `\tilde` `\widetilde` `\acute` `\grave` `\breve` `\mathring`、
//!   `\overbrace` `\underbrace` (添字は上下に配置)
//! - 空白: `\,` `\:` `\>` `\;` `\!` `\quad` `\qquad` `~` `\ ` `\hspace{..}` 等
//! - ギリシャ文字 (小文字はイタリック、大文字は立体)、関係演算子・二項演算子・矢印・各種記号、
//!   関数名 `\sin` `\cos` `\log` `\ln` `\exp` ... (立体)
//! - その他: `\overset` `\underset` `\stackrel` `\xrightarrow` `\xleftarrow` `\not` `\pmod` `\bmod`
//!   `\boxed` `\phantom` `\color` `\textcolor` `\displaystyle` `\textstyle` `\scriptstyle` `\tag`
//! - 変数 (ラテン文字 1 字) はイタリック、数字・演算子は立体。TeX と同じ原子クラス表で演算子周りの空白を決める。
//! - 未知のコマンドは等幅フォント・警告色で `\name` と表示する。波括弧の不釣り合いは回復する。
//!
//! # 既知の制限
//! - 字形の寸法はフォントの実インク範囲から計測するため、TeX のフォントメトリクスとは一致しない。
//! - 太字は同じ字形を少しずらして重ね描きする疑似太字。
//! - `\mathcal` などは Unicode の数学用英数字に置換するため、フォントに字形が無いと豆腐になる。
//! - 数式番号 (`\tag`) は式の右に括弧付きで並べるだけで、右寄せはしない。
//! - `\hline` 以外の表罫線指定 (`@{}` `p{}` 等) は無視する。`%` は TeX と同じくコメント扱い。

use std::sync::Arc;

use egui::epaint::{FontsView, Mesh};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Color32, Context, FontFamily, FontId, Galley, Painter, Pos2, Rect, Response, Sense, Shape,
    Stroke, Ui, Vec2, pos2, vec2,
};

// ============================================================================
// ==== 機能: 公開 API ====
// ============================================================================

/// インライン数式用: TeX を Unicode テキストで近似する。
///
/// 例: `"E = mc^2"` → `"E = mc²"`、`"\alpha + \beta"` → `"α + β"`、`"\frac{a}{b}"` → `"a/b"`、
/// `"x_i"` → `"xᵢ"` (対応する上付き・下付き文字が無ければ `"x_(ij)"` 形式)、`"\sqrt{x+1}"` → `"√(x+1)"`、
/// `"\mathbb{R}"` → `"ℝ"`、`"\text{if }"` → `"if "`。未知のコマンドは `"\name"` のまま残す。
pub fn to_unicode(tex: &str) -> String {
    let root = parse(tex);
    uni_node(&root, false)
}

/// ディスプレイ数式 (`$$...$$`) を 2 次元レイアウトして描画する。
///
/// 必要な大きさだけを `ui` に確保し (水平方向の中央寄せは呼び出し側の責任)、その領域に描画する。
/// どのような入力でもパニックしない。
pub fn show_display(ui: &mut Ui, tex: &str, font_size: f32, color: Color32) -> Response {
    let layout = layout_tex(ui.ctx(), tex, font_size, color);
    let size = vec2(
        layout.width.max(0.0).ceil(),
        (layout.ascent + layout.descent).max(0.0).ceil(),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let origin = rect.left_top() + vec2(0.0, layout.ascent);
        paint_box(ui.painter(), &layout, origin);
    }
    response
}

/// TeX ソースを解析してレイアウト済みのボックスを返す (描画はしない)。テストからも使う。
fn layout_tex(ctx: &Context, tex: &str, font_size: f32, color: Color32) -> LayoutBox {
    let size = if font_size.is_finite() {
        font_size.clamp(4.0, 300.0)
    } else {
        16.0
    };
    let root = parse(tex);
    ctx.fonts_mut(|fonts| {
        let mut layouter = Layouter::new(fonts, size);
        let env = Env {
            style: MathStyle::Display,
            color,
        };
        layouter.layout_node(&root, env)
    })
}

// ============================================================================
// ==== 機能: 字句解析 (トークナイザ) ====
// ============================================================================

/// TeX ソースを分割した字句。
#[derive(Debug, Clone, PartialEq)]
enum Token {
    /// `\name` (英字の並び) または `\` + 記号 1 文字 (`\,` `\{` `\\` など)。
    Command(String),
    /// 通常の 1 文字。
    Char(char),
    /// `{`
    Open,
    /// `}`
    Close,
    /// `^`
    Sup,
    /// `_`
    Sub,
    /// `&` (表のセル区切り)
    Amp,
    /// 連続する空白 (数式モードでは無視、`\text` 内では保持)。
    Space,
}

/// TeX ソースをトークン列に変換する。`%` から行末まではコメントとして捨てる。
fn tokenize(src: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.peek().copied() {
                Some(n) if n.is_ascii_alphabetic() => {
                    let mut name = String::new();
                    while let Some(&n) = chars.peek() {
                        if !n.is_ascii_alphabetic() {
                            break;
                        }
                        name.push(n);
                        chars.next();
                    }
                    out.push(Token::Command(name));
                }
                Some(n) => {
                    chars.next();
                    out.push(Token::Command(n.to_string()));
                }
                None => out.push(Token::Char('\\')),
            },
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '^' => out.push(Token::Sup),
            '_' => out.push(Token::Sub),
            '&' => out.push(Token::Amp),
            '%' => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {
                if out.last() != Some(&Token::Space) {
                    out.push(Token::Space);
                }
            }
            c => out.push(Token::Char(c)),
        }
    }
    out
}

// ============================================================================
// ==== 機能: 構文木 (AST) の型 ====
// ============================================================================

/// TeX の原子クラス。演算子周りの空白量を決めるのに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// 通常の記号 (変数・数字など)
    Ord,
    /// 大型演算子・関数名 (`\sum` `\sin`)
    Op,
    /// 二項演算子 (`+` `\times`)
    Bin,
    /// 関係演算子 (`=` `\leq` `\to`)
    Rel,
    /// 開き括弧
    Open,
    /// 閉じ括弧
    Close,
    /// 句読点 (`,` `;`)
    Punct,
    /// 内部式 (分数・`\left..\right`)
    Inner,
}

/// 数式のスタイル (文字サイズと配置規則の段階)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MathStyle {
    /// ディスプレイ形式 (`$$`)
    Display,
    /// テキスト形式 (インライン・分数の中身)
    Text,
    /// 添字 (0.7 倍)
    Script,
    /// 添字の添字 (0.5 倍)
    ScriptScript,
}

impl MathStyle {
    /// 基準サイズに対する文字サイズの倍率。
    fn scale(self) -> f32 {
        match self {
            Self::Display | Self::Text => 1.0,
            Self::Script => 0.7,
            Self::ScriptScript => 0.5,
        }
    }

    /// 上付き・下付き添字のスタイル。
    fn script(self) -> Self {
        match self {
            Self::Display | Self::Text => Self::Script,
            _ => Self::ScriptScript,
        }
    }

    /// 分数の分子・分母のスタイル。
    fn fraction(self) -> Self {
        match self {
            Self::Display => Self::Text,
            Self::Text => Self::Script,
            _ => Self::ScriptScript,
        }
    }

    /// 添字系のスタイルかどうか (演算子周りの空白を省く)。
    fn is_script(self) -> bool {
        matches!(self, Self::Script | Self::ScriptScript)
    }
}

/// 書体の切替 (`\mathbb` など)。パース時に文字へ適用する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Variant {
    /// 既定: ラテン文字と小文字ギリシャ文字はイタリック
    #[default]
    Normal,
    /// 立体 (`\mathrm` `\mathsf` `\mathtt`)
    Roman,
    /// 太字立体 (`\mathbf`)
    Bold,
    /// 全てイタリック (`\mathit`)
    Italic,
    /// 太字イタリック (`\boldsymbol`)
    BoldItalic,
    /// 黒板太字 (`\mathbb`)
    Blackboard,
    /// 筆記体 (`\mathcal` `\mathscr`)
    Calligraphic,
    /// フラクトゥール (`\mathfrak`)
    Fraktur,
}

/// 1 つの記号 (文字列 + クラス + 書体)。
#[derive(Debug, Clone, PartialEq)]
struct Atom {
    text: String,
    class: Class,
    italic: bool,
    bold: bool,
}

/// 大型演算子の添字配置指定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Limits {
    /// ディスプレイ形式なら上下、それ以外は右側
    Auto,
    /// 常に上下 (`\limits`)
    Above,
    /// 常に右側 (`\nolimits`、関数名 `\sin` など)
    Side,
}

/// 大型演算子 (`\sum`) または関数名 (`\sin` `\lim`)。
#[derive(Debug, Clone, PartialEq)]
struct BigOp {
    text: String,
    /// 関数名 (立体の文字列) なら true、記号 (∑ 等) なら false
    named: bool,
    /// 積分記号 (既定で添字を右側に置く)
    integral: bool,
    limits: Limits,
}

/// 分数・二項係数。
#[derive(Debug, Clone)]
struct Frac {
    num: Node,
    den: Node,
    /// 分数線を引くか (`\binom` `\atop` では引かない)
    bar: bool,
    left: Option<char>,
    right: Option<char>,
    /// `\dfrac` `\tfrac` によるスタイル強制
    style: Option<MathStyle>,
}

/// アクセントの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccentKind {
    Hat,
    WideHat,
    Check,
    Bar,
    Overline,
    Underline,
    Vec,
    RightArrow,
    LeftArrow,
    LeftRightArrow,
    Dot,
    Ddot,
    Dddot,
    Tilde,
    WideTilde,
    Acute,
    Grave,
    Breve,
    Ring,
}

/// 表系環境の配置規則。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableKind {
    /// matrix 系: 全列中央揃え
    Matrix,
    /// cases: 左揃え
    Cases,
    /// aligned 系: 右・左の交互揃え
    Aligned,
    /// gathered 系・トップレベル複数行: 1 列中央揃え
    Gathered,
    /// array: 列指定に従う
    Array,
    /// \substack: 中央揃え・現在のスタイルのまま
    Substack,
}

/// 列の揃え方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColAlign {
    Left,
    Center,
    Right,
}

/// セルを組むときのスタイル。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CellStyle {
    Text,
    Display,
    Script,
    Inherit,
}

/// 表 (行列・cases・aligned など)。
#[derive(Debug, Clone)]
struct Matrix {
    /// rows[行][列] = セル内のノード列
    rows: Vec<Vec<Vec<Node>>>,
    kind: TableKind,
    /// array の列指定
    cols: Vec<ColAlign>,
    /// 縦罫線の位置 (その列番号の左側。列数と同じ値なら右端)
    vlines: Vec<usize>,
    /// 横罫線の位置 (その行番号の上側。行数と同じ値なら下端)
    hlines: Vec<usize>,
    left: Option<char>,
    right: Option<char>,
    style: CellStyle,
}

impl Matrix {
    /// 列数 (最も長い行のセル数)。
    fn columns(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    /// 列 `j` の揃え方。
    fn align(&self, j: usize) -> ColAlign {
        match self.kind {
            TableKind::Cases => ColAlign::Left,
            TableKind::Aligned if j.is_multiple_of(2) => ColAlign::Right,
            TableKind::Aligned => ColAlign::Left,
            TableKind::Array => self.cols.get(j).copied().unwrap_or(ColAlign::Center),
            _ => ColAlign::Center,
        }
    }

    /// 列 `j` と列 `j + 1` の間隔 (em 単位)。
    fn col_gap(&self, j: usize) -> f32 {
        match self.kind {
            TableKind::Aligned if j.is_multiple_of(2) => 0.0,
            TableKind::Aligned => 1.5,
            TableKind::Matrix if self.style == CellStyle::Script => 0.5,
            _ => 1.0,
        }
    }
}

/// 構文木のノード。
#[derive(Debug, Clone)]
enum Node {
    /// 記号 1 つ
    Atom(Atom),
    /// `{...}` などのまとまり (クラスは Ord)
    Group(Vec<Node>),
    /// 添字付きの式
    Scripts {
        base: Box<Node>,
        sup: Option<Box<Node>>,
        sub: Option<Box<Node>>,
    },
    /// 分数・二項係数
    Frac(Box<Frac>),
    /// 根号
    Sqrt {
        index: Option<Box<Node>>,
        body: Box<Node>,
    },
    /// 大型演算子・関数名
    BigOp(BigOp),
    /// `\left ... \right`
    Delimited {
        left: Option<char>,
        right: Option<char>,
        body: Vec<Node>,
    },
    /// `\middle` (外側の `\left..\right` と同じ高さに伸びる)
    Middle(Option<char>),
    /// `\big` 系 (サイズは em 単位)
    BigDelim {
        delim: Option<char>,
        size: f32,
        class: Class,
    },
    /// 表系環境
    Matrix(Box<Matrix>),
    /// アクセント
    Accent { kind: AccentKind, body: Box<Node> },
    /// `\overbrace` / `\underbrace`
    Brace { over: bool, body: Box<Node> },
    /// `\overset` `\underset` `\stackrel` `\xrightarrow`
    OverUnder {
        base: Box<Node>,
        over: Option<Box<Node>>,
        under: Option<Box<Node>>,
    },
    /// `\text{...}` (空白を保持する立体テキスト)
    Text {
        text: String,
        italic: bool,
        bold: bool,
    },
    /// 明示的な空白 (em 単位、負もあり)
    Space(f32),
    /// `\displaystyle` などのスタイル切替 (以降のリスト全体に適用)
    Styled { style: MathStyle, body: Vec<Node> },
    /// `\color` / `\textcolor`
    Color { color: Color32, body: Vec<Node> },
    /// `\boxed`
    Boxed(Box<Node>),
    /// `\phantom` (場所だけ取る)
    Phantom(Box<Node>),
    /// 未知のコマンドなど (表示文字列)
    Error(String),
}

/// ノード列を 1 つのノードにまとめる (1 要素ならそのまま取り出す)。
fn group_or_single(mut nodes: Vec<Node>) -> Node {
    if nodes.len() == 1 {
        nodes.pop().unwrap_or(Node::Group(Vec::new()))
    } else {
        Node::Group(nodes)
    }
}

/// レイアウト用の原子クラス。空白は `None` (空白計算から除外)。
fn node_class(node: &Node) -> Option<Class> {
    Some(match node {
        Node::Atom(a) => a.class,
        Node::Scripts { base, .. } | Node::OverUnder { base, .. } => {
            node_class(base).unwrap_or(Class::Ord)
        }
        Node::Frac(_) | Node::Delimited { .. } | Node::Matrix(_) => Class::Inner,
        Node::BigOp(_) => Class::Op,
        Node::BigDelim { class, .. } => *class,
        Node::Space(_) => return None,
        Node::Styled { body, .. } | Node::Color { body, .. } => {
            body.iter().find_map(node_class).unwrap_or(Class::Ord)
        }
        _ => Class::Ord,
    })
}

// ============================================================================
// ==== 機能: 記号表 (コマンド名 → 文字) ====
// ============================================================================

/// 記号コマンドを (表示文字列, クラス) に変換する。未知なら `None`。
fn symbol(name: &str) -> Option<(&'static str, Class)> {
    use Class::*;
    Some(match name {
        // ---- ギリシャ文字 (小文字) ----
        "alpha" => ("α", Ord),
        "beta" => ("β", Ord),
        "gamma" => ("γ", Ord),
        "delta" => ("δ", Ord),
        "epsilon" => ("ϵ", Ord),
        "varepsilon" => ("ε", Ord),
        "zeta" => ("ζ", Ord),
        "eta" => ("η", Ord),
        "theta" => ("θ", Ord),
        "vartheta" => ("ϑ", Ord),
        "iota" => ("ι", Ord),
        "kappa" => ("κ", Ord),
        "varkappa" => ("ϰ", Ord),
        "lambda" => ("λ", Ord),
        "mu" => ("μ", Ord),
        "nu" => ("ν", Ord),
        "xi" => ("ξ", Ord),
        "omicron" => ("ο", Ord),
        "pi" => ("π", Ord),
        "varpi" => ("ϖ", Ord),
        "rho" => ("ρ", Ord),
        "varrho" => ("ϱ", Ord),
        "sigma" => ("σ", Ord),
        "varsigma" => ("ς", Ord),
        "tau" => ("τ", Ord),
        "upsilon" => ("υ", Ord),
        "phi" => ("ϕ", Ord),
        "varphi" => ("φ", Ord),
        "chi" => ("χ", Ord),
        "psi" => ("ψ", Ord),
        "omega" => ("ω", Ord),
        "digamma" => ("ϝ", Ord),
        // ---- ギリシャ文字 (大文字) ----
        "Gamma" | "varGamma" => ("Γ", Ord),
        "Delta" | "varDelta" => ("Δ", Ord),
        "Theta" | "varTheta" => ("Θ", Ord),
        "Lambda" | "varLambda" => ("Λ", Ord),
        "Xi" | "varXi" => ("Ξ", Ord),
        "Pi" | "varPi" => ("Π", Ord),
        "Sigma" | "varSigma" => ("Σ", Ord),
        "Upsilon" | "varUpsilon" => ("Υ", Ord),
        "Phi" | "varPhi" => ("Φ", Ord),
        "Psi" | "varPsi" => ("Ψ", Ord),
        "Omega" | "varOmega" => ("Ω", Ord),
        // ---- 二項演算子 ----
        "pm" => ("±", Bin),
        "mp" => ("∓", Bin),
        "times" => ("×", Bin),
        "div" => ("÷", Bin),
        "cdot" | "cdotp" | "centerdot" => ("·", Bin),
        "ast" => ("∗", Bin),
        "star" => ("⋆", Bin),
        "circ" => ("∘", Bin),
        "bullet" => ("∙", Bin),
        "cap" => ("∩", Bin),
        "cup" => ("∪", Bin),
        "uplus" => ("⊎", Bin),
        "sqcap" => ("⊓", Bin),
        "sqcup" => ("⊔", Bin),
        "vee" | "lor" => ("∨", Bin),
        "wedge" | "land" => ("∧", Bin),
        "setminus" | "smallsetminus" => ("∖", Bin),
        "wr" => ("≀", Bin),
        "diamond" => ("⋄", Bin),
        "oplus" => ("⊕", Bin),
        "ominus" => ("⊖", Bin),
        "otimes" => ("⊗", Bin),
        "oslash" => ("⊘", Bin),
        "odot" => ("⊙", Bin),
        "dagger" => ("†", Bin),
        "ddagger" => ("‡", Bin),
        "amalg" => ("⨿", Bin),
        "triangleleft" => ("◁", Bin),
        "triangleright" => ("▷", Bin),
        "bigtriangleup" => ("△", Bin),
        "bigtriangledown" => ("▽", Bin),
        "bigcirc" => ("◯", Bin),
        // ---- 関係演算子 ----
        "leq" | "le" => ("≤", Rel),
        "geq" | "ge" => ("≥", Rel),
        "leqq" => ("≦", Rel),
        "geqq" => ("≧", Rel),
        "leqslant" => ("⩽", Rel),
        "geqslant" => ("⩾", Rel),
        "neq" | "ne" => ("≠", Rel),
        "equiv" => ("≡", Rel),
        "approx" => ("≈", Rel),
        "approxeq" => ("≊", Rel),
        "sim" => ("∼", Rel),
        "simeq" => ("≃", Rel),
        "cong" => ("≅", Rel),
        "propto" | "varpropto" => ("∝", Rel),
        "ll" => ("≪", Rel),
        "gg" => ("≫", Rel),
        "lll" => ("⋘", Rel),
        "ggg" => ("⋙", Rel),
        "prec" => ("≺", Rel),
        "succ" => ("≻", Rel),
        "preceq" => ("⪯", Rel),
        "succeq" => ("⪰", Rel),
        "subset" => ("⊂", Rel),
        "supset" => ("⊃", Rel),
        "subseteq" => ("⊆", Rel),
        "supseteq" => ("⊇", Rel),
        "subsetneq" => ("⊊", Rel),
        "supsetneq" => ("⊋", Rel),
        "nsubseteq" => ("⊈", Rel),
        "nsupseteq" => ("⊉", Rel),
        "sqsubset" => ("⊏", Rel),
        "sqsupset" => ("⊐", Rel),
        "sqsubseteq" => ("⊑", Rel),
        "sqsupseteq" => ("⊒", Rel),
        "in" | "isin" => ("∈", Rel),
        "ni" | "owns" => ("∋", Rel),
        "notin" => ("∉", Rel),
        "vdash" => ("⊢", Rel),
        "dashv" => ("⊣", Rel),
        "models" => ("⊨", Rel),
        "mid" => ("∣", Rel),
        "nmid" => ("∤", Rel),
        "parallel" => ("∥", Rel),
        "nparallel" => ("∦", Rel),
        "perp" => ("⊥", Rel),
        "asymp" => ("≍", Rel),
        "doteq" => ("≐", Rel),
        "bowtie" | "Join" => ("⋈", Rel),
        "smile" => ("⌣", Rel),
        "frown" => ("⌢", Rel),
        "lesssim" => ("≲", Rel),
        "gtrsim" => ("≳", Rel),
        "coloneqq" | "coloneq" => ("≔", Rel),
        "eqqcolon" | "eqcolon" => ("≕", Rel),
        "triangleq" => ("≜", Rel),
        "nleq" => ("≰", Rel),
        "ngeq" => ("≱", Rel),
        "nless" => ("≮", Rel),
        "ngtr" => ("≯", Rel),
        "nsim" => ("≁", Rel),
        "ncong" => ("≇", Rel),
        "lt" => ("<", Rel),
        "gt" => (">", Rel),
        "vartriangleleft" | "lhd" => ("⊲", Rel),
        "vartriangleright" | "rhd" => ("⊳", Rel),
        "unlhd" | "trianglelefteq" => ("⊴", Rel),
        "unrhd" | "trianglerighteq" => ("⊵", Rel),
        // ---- 矢印 (関係演算子扱い) ----
        "to" | "rightarrow" => ("→", Rel),
        "gets" | "leftarrow" => ("←", Rel),
        "leftrightarrow" => ("↔", Rel),
        "Rightarrow" => ("⇒", Rel),
        "Leftarrow" => ("⇐", Rel),
        "Leftrightarrow" => ("⇔", Rel),
        "iff" | "Longleftrightarrow" => ("⟺", Rel),
        "implies" | "Longrightarrow" => ("⟹", Rel),
        "impliedby" | "Longleftarrow" => ("⟸", Rel),
        "longrightarrow" => ("⟶", Rel),
        "longleftarrow" => ("⟵", Rel),
        "longleftrightarrow" => ("⟷", Rel),
        "mapsto" => ("↦", Rel),
        "longmapsto" => ("⟼", Rel),
        "uparrow" => ("↑", Rel),
        "downarrow" => ("↓", Rel),
        "updownarrow" => ("↕", Rel),
        "Uparrow" => ("⇑", Rel),
        "Downarrow" => ("⇓", Rel),
        "Updownarrow" => ("⇕", Rel),
        "nearrow" => ("↗", Rel),
        "searrow" => ("↘", Rel),
        "swarrow" => ("↙", Rel),
        "nwarrow" => ("↖", Rel),
        "hookrightarrow" => ("↪", Rel),
        "hookleftarrow" => ("↩", Rel),
        "rightharpoonup" => ("⇀", Rel),
        "rightharpoondown" => ("⇁", Rel),
        "leftharpoonup" => ("↼", Rel),
        "leftharpoondown" => ("↽", Rel),
        "rightleftharpoons" => ("⇌", Rel),
        "leftrightharpoons" => ("⇋", Rel),
        "leadsto" | "rightsquigarrow" => ("⇝", Rel),
        "rightrightarrows" => ("⇉", Rel),
        "leftleftarrows" => ("⇇", Rel),
        "rightleftarrows" => ("⇄", Rel),
        "twoheadrightarrow" => ("↠", Rel),
        "twoheadleftarrow" => ("↞", Rel),
        "rightarrowtail" => ("↣", Rel),
        "circlearrowleft" => ("↺", Rel),
        "circlearrowright" => ("↻", Rel),
        "nrightarrow" => ("↛", Rel),
        "nleftarrow" => ("↚", Rel),
        "nRightarrow" => ("⇏", Rel),
        "nLeftrightarrow" => ("⇎", Rel),
        // ---- その他の記号 (Ord) ----
        "infty" => ("∞", Ord),
        "partial" => ("∂", Ord),
        "nabla" => ("∇", Ord),
        "forall" => ("∀", Ord),
        "exists" => ("∃", Ord),
        "nexists" => ("∄", Ord),
        "neg" | "lnot" => ("¬", Ord),
        "emptyset" | "varnothing" => ("∅", Ord),
        "angle" => ("∠", Ord),
        "measuredangle" => ("∡", Ord),
        "sphericalangle" => ("∢", Ord),
        "prime" => ("′", Ord),
        "dprime" => ("″", Ord),
        "backprime" => ("‵", Ord),
        "degree" => ("°", Ord),
        "hbar" | "hslash" => ("ℏ", Ord),
        "ell" => ("ℓ", Ord),
        "wp" => ("℘", Ord),
        "Re" => ("ℜ", Ord),
        "Im" => ("ℑ", Ord),
        "aleph" => ("ℵ", Ord),
        "beth" => ("ℶ", Ord),
        "gimel" => ("ℷ", Ord),
        "top" => ("⊤", Ord),
        "bot" => ("⊥", Ord),
        "triangle" | "vartriangle" => ("△", Ord),
        "Box" | "square" => ("□", Ord),
        "blacksquare" => ("■", Ord),
        "lozenge" => ("◊", Ord),
        "blacklozenge" => ("⧫", Ord),
        "clubsuit" => ("♣", Ord),
        "diamondsuit" => ("♢", Ord),
        "heartsuit" => ("♡", Ord),
        "spadesuit" => ("♠", Ord),
        "flat" => ("♭", Ord),
        "natural" => ("♮", Ord),
        "sharp" => ("♯", Ord),
        "checkmark" => ("✓", Ord),
        "surd" => ("√", Ord),
        "imath" => ("ı", Ord),
        "jmath" => ("ȷ", Ord),
        "complement" => ("∁", Ord),
        "mho" => ("℧", Ord),
        "eth" => ("ð", Ord),
        "S" | "sect" => ("§", Ord),
        "P" => ("¶", Ord),
        "copyright" => ("©", Ord),
        "pounds" => ("£", Ord),
        "yen" => ("¥", Ord),
        "euro" => ("€", Ord),
        "ldots" | "dots" | "dotsc" | "dotso" | "mathellipsis" => ("…", Inner),
        "cdots" | "dotsb" | "dotsm" | "dotsi" | "cdotscentered" => ("⋯", Inner),
        "vdots" => ("⋮", Ord),
        "ddots" => ("⋱", Inner),
        "therefore" => ("∴", Rel),
        "because" => ("∵", Rel),
        "colon" => (":", Punct),
        "backslash" | "setminusbackslash" => ("\\", Ord),
        "vert" | "lvert" | "rvert" => ("|", Ord),
        "Vert" | "lVert" | "rVert" | "|" => ("‖", Ord),
        "lbrace" | "{" => ("{", Open),
        "rbrace" | "}" => ("}", Close),
        "lbrack" => ("[", Open),
        "rbrack" => ("]", Close),
        "langle" => ("⟨", Open),
        "rangle" => ("⟩", Close),
        "lfloor" => ("⌊", Open),
        "rfloor" => ("⌋", Close),
        "lceil" => ("⌈", Open),
        "rceil" => ("⌉", Close),
        "lgroup" => ("⟮", Open),
        "rgroup" => ("⟯", Close),
        "#" => ("#", Ord),
        "$" => ("$", Ord),
        "%" => ("%", Ord),
        "&" | "And" => ("&", Ord),
        "_" => ("_", Ord),
        "textbackslash" => ("\\", Ord),
        _ => return None,
    })
}

/// 大型演算子コマンドを (記号, 積分か) に変換する。
fn big_op_symbol(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "sum" => ("∑", false),
        "prod" => ("∏", false),
        "coprod" => ("∐", false),
        "bigcup" => ("⋃", false),
        "bigcap" => ("⋂", false),
        "bigvee" => ("⋁", false),
        "bigwedge" => ("⋀", false),
        "bigoplus" => ("⨁", false),
        "bigotimes" => ("⨂", false),
        "bigodot" => ("⨀", false),
        "biguplus" => ("⨄", false),
        "bigsqcup" => ("⨆", false),
        "int" | "intop" => ("∫", true),
        "iint" => ("∬", true),
        "iiint" => ("∭", true),
        "oint" => ("∮", true),
        "oiint" => ("∯", true),
        "oiiint" => ("∰", true),
        _ => return None,
    })
}

/// 関数名コマンドを (表示名, 添字を上下に置けるか) に変換する。
fn function_name(name: &str) -> Option<(&'static str, bool)> {
    Some(match name {
        "sin" => ("sin", false),
        "cos" => ("cos", false),
        "tan" => ("tan", false),
        "cot" => ("cot", false),
        "sec" => ("sec", false),
        "csc" => ("csc", false),
        "arcsin" => ("arcsin", false),
        "arccos" => ("arccos", false),
        "arctan" => ("arctan", false),
        "sinh" => ("sinh", false),
        "cosh" => ("cosh", false),
        "tanh" => ("tanh", false),
        "coth" => ("coth", false),
        "sech" => ("sech", false),
        "csch" => ("csch", false),
        "log" => ("log", false),
        "ln" => ("ln", false),
        "lg" => ("lg", false),
        "exp" => ("exp", false),
        "deg" => ("deg", false),
        "dim" => ("dim", false),
        "hom" => ("hom", false),
        "ker" => ("ker", false),
        "arg" => ("arg", false),
        "sgn" => ("sgn", false),
        "tr" => ("tr", false),
        "det" => ("det", true),
        "gcd" => ("gcd", true),
        "Pr" => ("Pr", true),
        "lim" => ("lim", true),
        "liminf" => ("lim inf", true),
        "limsup" => ("lim sup", true),
        "max" => ("max", true),
        "min" => ("min", true),
        "sup" => ("sup", true),
        "inf" => ("inf", true),
        "argmax" => ("arg max", true),
        "argmin" => ("arg min", true),
        _ => return None,
    })
}

/// 空白コマンドの幅 (em 単位)。
fn spacing_command(name: &str) -> Option<f32> {
    Some(match name {
        "," | "thinspace" => 3.0 / 18.0,
        ":" | ">" | "medspace" => 4.0 / 18.0,
        ";" | "thickspace" => 5.0 / 18.0,
        "!" | "negthinspace" => -3.0 / 18.0,
        "negmedspace" => -4.0 / 18.0,
        "negthickspace" => -5.0 / 18.0,
        " " | "space" | "nobreakspace" => 0.3,
        "enspace" | "enskip" => 0.5,
        "quad" => 1.0,
        "qquad" => 2.0,
        _ => return None,
    })
}

/// アクセントコマンドの種類。
fn accent_kind(name: &str) -> Option<AccentKind> {
    use AccentKind::*;
    Some(match name {
        "hat" => Hat,
        "widehat" => WideHat,
        "check" | "widecheck" => Check,
        "bar" => Bar,
        "overline" => Overline,
        "underline" => Underline,
        "vec" => Vec,
        "overrightarrow" => RightArrow,
        "overleftarrow" => LeftArrow,
        "overleftrightarrow" => LeftRightArrow,
        "dot" => Dot,
        "ddot" => Ddot,
        "dddot" => Dddot,
        "tilde" => Tilde,
        "widetilde" => WideTilde,
        "acute" => Acute,
        "grave" => Grave,
        "breve" => Breve,
        "mathring" => Ring,
        _ => return None,
    })
}

/// 直接入力された Unicode 文字のクラスを推定する。
fn char_class(c: char) -> Class {
    use Class::*;
    match c {
        '+' | '-' | '*' | '−' | '±' | '∓' | '×' | '÷' | '·' | '∗' | '⋆' | '∘' | '∙' | '∪' | '∩'
        | '∧' | '∨' | '⊕' | '⊖' | '⊗' | '⊘' | '⊙' | '∖' | '⋅' | '⊎' | '⊓' | '⊔' | '†' | '‡'
        | '⋄' => Bin,
        '=' | '<' | '>' | ':' | '≤' | '≥' | '≦' | '≧' | '⩽' | '⩾' | '≠' | '≡' | '≈' | '∼' | '≃'
        | '≅' | '∝' | '≪' | '≫' | '≺' | '≻' | '⪯' | '⪰' | '⊂' | '⊃' | '⊆' | '⊇' | '⊊' | '⊋'
        | '⊈' | '⊉' | '⊑' | '⊒' | '∈' | '∉' | '∋' | '⊢' | '⊣' | '⊨' | '∣' | '∥' | '∦' | '≍'
        | '≐' | '≲' | '≳' | '≔' | '≜' | '≰' | '≱' | '≮' | '≯' | '≢' | '≁' | '∴' | '∵' => {
            Rel
        }
        '\u{2190}'..='\u{21FF}' | '\u{27F0}'..='\u{27FF}' => Rel,
        '(' | '[' | '⟨' | '⌊' | '⌈' => Open,
        ')' | ']' | '⟩' | '⌋' | '⌉' | '!' | '?' => Close,
        ',' | ';' => Punct,
        '∑' | '∏' | '∫' | '∮' | '⋃' | '⋂' => Op,
        _ => Ord,
    }
}

/// 小文字ギリシャ文字か (既定書体でイタリックにする)。
fn is_lower_greek(c: char) -> bool {
    ('α'..='ω').contains(&c) || matches!(c, 'ϵ' | 'ϑ' | 'ϰ' | 'ϕ' | 'ϱ' | 'ϖ' | 'ϝ')
}

/// 書体を文字に適用し、(表示文字, イタリック, 太字) を返す。
fn styled_char(c: char, variant: Variant) -> (char, bool, bool) {
    let math_italic = c.is_ascii_alphabetic() || is_lower_greek(c);
    match variant {
        Variant::Normal => (c, math_italic, false),
        Variant::Roman => (c, false, false),
        Variant::Bold => (c, false, true),
        Variant::Italic => (c, c.is_alphabetic(), false),
        Variant::BoldItalic => (c, math_italic, true),
        Variant::Blackboard => (blackboard(c), false, false),
        Variant::Calligraphic => (calligraphic(c), false, false),
        Variant::Fraktur => (fraktur(c), false, false),
    }
}

/// 英数字を数学用英数字ブロック (U+1D400〜) の文字に写す。BMP にある例外を先に処理する。
fn math_alnum(
    c: char,
    upper: u32,
    lower: u32,
    digit: Option<u32>,
    exceptions: &[(char, char)],
) -> char {
    if let Some(&(_, m)) = exceptions.iter().find(|(k, _)| *k == c) {
        return m;
    }
    let code = match c {
        'A'..='Z' => upper + (c as u32 - 'A' as u32),
        'a'..='z' => lower + (c as u32 - 'a' as u32),
        '0'..='9' => match digit {
            Some(d) => d + (c as u32 - '0' as u32),
            None => return c,
        },
        _ => return c,
    };
    char::from_u32(code).unwrap_or(c)
}

/// `\mathbb` の文字 (ℝ など)。
fn blackboard(c: char) -> char {
    const EXC: &[(char, char)] = &[
        ('C', 'ℂ'),
        ('H', 'ℍ'),
        ('N', 'ℕ'),
        ('P', 'ℙ'),
        ('Q', 'ℚ'),
        ('R', 'ℝ'),
        ('Z', 'ℤ'),
    ];
    math_alnum(c, 0x1D538, 0x1D552, Some(0x1D7D8), EXC)
}

/// `\mathcal` / `\mathscr` の文字 (ℒ など)。
fn calligraphic(c: char) -> char {
    const EXC: &[(char, char)] = &[
        ('B', 'ℬ'),
        ('E', 'ℰ'),
        ('F', 'ℱ'),
        ('H', 'ℋ'),
        ('I', 'ℐ'),
        ('L', 'ℒ'),
        ('M', 'ℳ'),
        ('R', 'ℛ'),
        ('e', 'ℯ'),
        ('g', 'ℊ'),
        ('o', 'ℴ'),
    ];
    math_alnum(c, 0x1D49C, 0x1D4B6, None, EXC)
}

/// `\mathfrak` の文字 (𝔤 など)。
fn fraktur(c: char) -> char {
    const EXC: &[(char, char)] = &[('C', 'ℭ'), ('H', 'ℌ'), ('I', 'ℑ'), ('R', 'ℜ'), ('Z', 'ℨ')];
    math_alnum(c, 0x1D504, 0x1D51E, None, EXC)
}

/// `\not` で否定した関係演算子の文字列。
fn negate_text(text: &str) -> String {
    match text {
        "=" => "≠",
        "<" => "≮",
        ">" => "≯",
        "∈" => "∉",
        "≡" => "≢",
        "∼" => "≁",
        "≤" => "≰",
        "≥" => "≱",
        "⊂" => "⊄",
        "⊃" => "⊅",
        "⊆" => "⊈",
        "⊇" => "⊉",
        "∣" | "|" => "∤",
        "∥" => "∦",
        "≅" => "≇",
        "→" => "↛",
        "←" => "↚",
        "⇒" => "⇏",
        "⇔" => "⇎",
        other => return format!("{other}\u{338}"),
    }
    .to_owned()
}

/// `\color` の色名・16 進表記を解釈する (不明なら None)。
fn parse_color(spec: &str) -> Option<Color32> {
    let s = spec.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return Color32::from_hex(&format!("#{hex}")).ok();
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "red" => Color32::from_rgb(220, 50, 47),
        "green" => Color32::from_rgb(40, 160, 60),
        "blue" => Color32::from_rgb(38, 110, 220),
        "orange" => Color32::from_rgb(240, 140, 30),
        "purple" | "violet" => Color32::from_rgb(150, 80, 200),
        "magenta" => Color32::from_rgb(210, 50, 170),
        "cyan" => Color32::from_rgb(30, 170, 190),
        "teal" => Color32::from_rgb(20, 140, 140),
        "brown" => Color32::from_rgb(150, 90, 40),
        "yellow" => Color32::from_rgb(220, 190, 20),
        "gray" | "grey" => Color32::GRAY,
        "black" => Color32::BLACK,
        "white" => Color32::WHITE,
        _ => {
            let hex = s.trim_start_matches('#');
            if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Color32::from_hex(&format!("#{hex}")).ok();
            }
            return None;
        }
    })
}

/// `\hspace{..}` の長さ指定を em 単位に変換する。
fn parse_length(spec: &str) -> f32 {
    let s = spec.trim();
    let split = s
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(s.len());
    let value: f32 = s[..split].parse().unwrap_or(0.0);
    let unit = match s[split..].trim() {
        "ex" => 0.45,
        "pt" | "bp" => 0.1,
        "mu" => 1.0 / 18.0,
        "px" => 0.07,
        "mm" => 0.285,
        "cm" => 2.85,
        "in" => 7.23,
        _ => 1.0,
    };
    (value * unit).clamp(-20.0, 20.0)
}

// ============================================================================
// ==== 機能: 構文解析 (パーサ) ====
// ============================================================================

/// 入れ子の上限。これを超える部分は読み飛ばす (スタックオーバーフロー防止)。
const MAX_DEPTH: usize = 40;

/// 解析中の文脈。終端トークンをどのレベルで受け止めるかの判定に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// トップレベル (`&` `\\` で区切る)
    Top,
    /// `{...}`
    Group,
    /// `\left ... \right`
    Left,
    /// `\begin ... \end` (`&` `\\` で区切る)
    Env,
    /// `\sqrt[...]` の省略可能引数
    Bracket,
}

/// `parse_list` が止まった理由 (終端トークンは消費しない。`Eof` を除く)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Eof,
    Close,
    Right,
    Env,
    Amp,
    Row,
    Bracket,
}

/// 表の解析結果: (行×列のセル, 横罫線位置, 終了理由)。
type Rows = (Vec<Vec<Vec<Node>>>, Vec<usize>, End);

/// 再帰下降パーサ。
struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: usize,
    frames: Vec<Frame>,
    variant: Variant,
}

/// TeX ソース全体を解析する。トップレベルの `\\` と `&` は表として扱う。
fn parse(tex: &str) -> Node {
    let mut p = Parser {
        toks: tokenize(tex),
        pos: 0,
        depth: 0,
        frames: vec![Frame::Top],
        variant: Variant::Normal,
    };
    // 終了理由は必ず Eof (Top では Close 等で止まらない)
    let (mut rows, hlines, _) = p.parse_rows();
    if rows.iter().any(|r| r.len() > 1) {
        Node::Matrix(Box::new(Matrix {
            rows,
            kind: TableKind::Aligned,
            cols: Vec::new(),
            vlines: Vec::new(),
            hlines,
            left: None,
            right: None,
            style: CellStyle::Display,
        }))
    } else if rows.len() > 1 {
        Node::Matrix(Box::new(Matrix {
            rows,
            kind: TableKind::Gathered,
            cols: Vec::new(),
            vlines: Vec::new(),
            hlines,
            left: None,
            right: None,
            style: CellStyle::Display,
        }))
    } else {
        let cell = rows.pop().and_then(|mut r| r.pop()).unwrap_or_default();
        Node::Group(cell)
    }
}

impl Parser {
    /// 次のトークンを覗く。
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    /// 空白トークンを読み飛ばす。
    fn skip_spaces(&mut self) {
        while matches!(self.peek(), Some(Token::Space)) {
            self.pos += 1;
        }
    }

    /// 指定の文脈がスタック中にあるか。
    fn in_frame(&self, frame: Frame) -> bool {
        self.frames.contains(&frame)
    }

    /// 最も内側の文脈が表 (`&` `\\` を区切りとして受ける) か。
    fn in_table(&self) -> bool {
        matches!(self.frames.last(), Some(Frame::Env | Frame::Top))
    }

    // ---- リスト・項の解析 ----

    /// 終端に達するまでノード列を読む。終端トークン自体は消費しない。
    fn parse_list(&mut self) -> (Vec<Node>, End) {
        if self.depth >= MAX_DEPTH {
            return self.skip_deep();
        }
        self.depth += 1;
        let saved_variant = self.variant;
        let mut nodes = Vec::new();
        // 中置分数 (\over など): (分子の長さ, 線の有無, 左括弧, 右括弧)
        let mut infix: Option<(usize, bool, Option<char>, Option<char>)> = None;
        let end = loop {
            let Some(tok) = self.peek().cloned() else {
                break End::Eof;
            };
            match tok {
                Token::Space => self.pos += 1,
                Token::Close => {
                    if self.in_frame(Frame::Group) {
                        break End::Close;
                    }
                    self.pos += 1; // 対応しない `}` は無視して回復
                }
                Token::Char(']') if self.frames.last() == Some(&Frame::Bracket) => {
                    break End::Bracket;
                }
                Token::Amp => {
                    if self.in_table() {
                        break End::Amp;
                    }
                    self.pos += 1;
                }
                Token::Command(name) => match name.as_str() {
                    "\\" | "cr" | "newline" => {
                        if self.in_table() {
                            break End::Row;
                        }
                        self.pos += 1;
                    }
                    "right" => {
                        if self.in_frame(Frame::Left) {
                            break End::Right;
                        }
                        // 対応する \left が無い: 区切り記号を普通の記号として出す
                        self.pos += 1;
                        if let Some(d) = self.parse_delim() {
                            nodes.push(atom(d.to_string(), Class::Close));
                        }
                    }
                    "end" => {
                        if self.in_frame(Frame::Env) {
                            break End::Env;
                        }
                        self.pos += 1;
                        let _ = self.raw_arg();
                    }
                    "over" | "choose" | "atop" | "brace" | "brack" => {
                        self.pos += 1;
                        if infix.is_none() {
                            let (bar, l, r) = match name.as_str() {
                                "over" => (true, None, None),
                                "choose" => (false, Some('('), Some(')')),
                                "brace" => (false, Some('{'), Some('}')),
                                "brack" => (false, Some('['), Some(']')),
                                _ => (false, None, None),
                            };
                            infix = Some((nodes.len(), bar, l, r));
                        }
                    }
                    "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" => {
                        self.pos += 1;
                        let style = match name.as_str() {
                            "displaystyle" => MathStyle::Display,
                            "textstyle" => MathStyle::Text,
                            "scriptstyle" => MathStyle::Script,
                            _ => MathStyle::ScriptScript,
                        };
                        let (body, e) = self.parse_list();
                        nodes.push(Node::Styled { style, body });
                        break e;
                    }
                    "color" => {
                        self.pos += 1;
                        let color = parse_color(&self.raw_arg());
                        let (body, e) = self.parse_list();
                        match color {
                            Some(color) => nodes.push(Node::Color { color, body }),
                            None => nodes.extend(body),
                        }
                        break e;
                    }
                    "rm" | "bf" | "it" | "cal" | "sf" | "tt" | "mit" => {
                        self.pos += 1;
                        self.variant = match name.as_str() {
                            "bf" => Variant::Bold,
                            "it" => Variant::Italic,
                            "cal" => Variant::Calligraphic,
                            "mit" => Variant::Normal,
                            _ => Variant::Roman,
                        };
                    }
                    _ => self.push_item(&mut nodes),
                },
                _ => self.push_item(&mut nodes),
            }
        };
        self.variant = saved_variant;
        self.depth -= 1;
        if let Some((split, bar, left, right)) = infix {
            let den = nodes.split_off(split.min(nodes.len()));
            nodes = vec![Node::Frac(Box::new(Frac {
                num: Node::Group(nodes),
                den: Node::Group(den),
                bar,
                left,
                right,
                style: None,
            }))];
        }
        (nodes, end)
    }

    /// 入れ子が深すぎるとき、対応する `}` (または末尾) までを読み飛ばす。
    fn skip_deep(&mut self) -> (Vec<Node>, End) {
        let mut level = 0usize;
        while let Some(tok) = self.peek() {
            match tok {
                Token::Open => level += 1,
                Token::Close if level == 0 && self.in_frame(Frame::Group) => {
                    return (vec![Node::Error("…".into())], End::Close);
                }
                Token::Close => level = level.saturating_sub(1),
                _ => {}
            }
            self.pos += 1;
        }
        (vec![Node::Error("…".into())], End::Eof)
    }

    /// 1 項を読んでリストに追加する。必ず 1 トークン以上進める (無限ループ防止)。
    fn push_item(&mut self, nodes: &mut Vec<Node>) {
        let start = self.pos;
        let node = self.parse_item();
        nodes.push(node);
        if self.pos == start {
            self.pos += 1;
        }
    }

    /// 1 項 (基本要素 + 後置の添字) を読む。
    fn parse_item(&mut self) -> Node {
        let base = self.parse_primary(true);
        self.parse_postfix(base)
    }

    /// 後置要素 (`^` `_` `'` `\limits`) を読んで添字ノードを作る。
    fn parse_postfix(&mut self, mut base: Node) -> Node {
        let mut sup: Option<Vec<Node>> = None;
        let mut sub: Option<Node> = None;
        let mut primes_only = false;
        loop {
            let save = self.pos;
            self.skip_spaces();
            match self.peek() {
                Some(Token::Sup) => {
                    if sup.is_some() && !primes_only {
                        // 二重の上付き: 次の項 (空の基底) に回す
                        self.pos = save;
                        break;
                    }
                    self.pos += 1;
                    let arg = self.parse_arg();
                    sup.get_or_insert_with(Vec::new).push(arg);
                    primes_only = false;
                }
                Some(Token::Sub) => {
                    if sub.is_some() {
                        self.pos = save;
                        break;
                    }
                    self.pos += 1;
                    sub = Some(self.parse_arg());
                }
                Some(Token::Char('\'')) => {
                    if sup.is_some() && !primes_only {
                        self.pos = save;
                        break;
                    }
                    self.pos += 1;
                    sup.get_or_insert_with(Vec::new)
                        .push(atom("′".into(), Class::Ord));
                    primes_only = true;
                }
                Some(Token::Command(n))
                    if matches!(n.as_str(), "limits" | "nolimits" | "displaylimits") =>
                {
                    let limits = match n.as_str() {
                        "limits" => Limits::Above,
                        "nolimits" => Limits::Side,
                        _ => Limits::Auto,
                    };
                    self.pos += 1;
                    if let Node::BigOp(op) = &mut base {
                        op.limits = limits;
                    }
                }
                _ => {
                    self.pos = save;
                    break;
                }
            }
        }
        if sup.is_none() && sub.is_none() {
            return base;
        }
        Node::Scripts {
            base: Box::new(base),
            sup: sup.map(|v| Box::new(group_or_single(v))),
            sub: sub.map(Box::new),
        }
    }

    /// 基本要素を 1 つ読む。`merge_digits` が true なら連続する数字を 1 つにまとめる。
    fn parse_primary(&mut self, merge_digits: bool) -> Node {
        self.skip_spaces();
        match self.peek().cloned() {
            None | Some(Token::Sup | Token::Sub) => Node::Group(Vec::new()),
            Some(Token::Open) => self.parse_group(),
            Some(Token::Char(c)) => {
                self.pos += 1;
                if merge_digits && c.is_ascii_digit() {
                    return self.number(c);
                }
                self.char_node(c)
            }
            Some(Token::Command(name)) => {
                self.pos += 1;
                self.command(&name)
            }
            Some(Token::Close | Token::Amp | Token::Space) => {
                self.pos += 1;
                Node::Group(Vec::new())
            }
        }
    }

    /// 数字列 (`3.14` など) を 1 つの記号にまとめる。
    fn number(&mut self, first: char) -> Node {
        let mut digits = String::from(first);
        loop {
            match (self.peek(), self.toks.get(self.pos + 1)) {
                (Some(Token::Char(d)), _) if d.is_ascii_digit() => digits.push(*d),
                (Some(Token::Char('.')), Some(Token::Char(d))) if d.is_ascii_digit() => {
                    digits.push('.')
                }
                _ => break,
            }
            self.pos += 1;
        }
        let mut text = String::new();
        let mut bold = false;
        for c in digits.chars() {
            let (m, _, b) = styled_char(c, self.variant);
            text.push(m);
            bold = b;
        }
        Node::Atom(Atom {
            text,
            class: Class::Ord,
            italic: false,
            bold,
        })
    }

    /// `{...}` を読む (`{` の位置で呼ぶ)。閉じ括弧が無くても回復する。
    fn parse_group(&mut self) -> Node {
        self.pos += 1;
        self.frames.push(Frame::Group);
        let (nodes, end) = self.parse_list();
        self.frames.pop();
        if end == End::Close {
            self.pos += 1;
        }
        Node::Group(nodes)
    }

    /// コマンドの必須引数を 1 つ読む (`{...}` または 1 トークン)。
    fn parse_arg(&mut self) -> Node {
        self.skip_spaces();
        if self.depth >= MAX_DEPTH {
            self.pos += 1;
            return Node::Error("…".into());
        }
        self.depth += 1;
        let node = match self.peek().cloned() {
            Some(Token::Open) => match self.parse_group() {
                Node::Group(v) => group_or_single(v),
                other => other,
            },
            Some(Token::Char(c)) => {
                self.pos += 1;
                self.char_node(c)
            }
            Some(Token::Command(name))
                if !matches!(name.as_str(), "right" | "end" | "\\" | "cr" | "newline") =>
            {
                self.parse_primary(false)
            }
            _ => Node::Group(Vec::new()),
        };
        self.depth -= 1;
        node
    }

    /// 省略可能引数 `[...]` を読む (無ければ None で位置を戻す)。
    fn parse_optional(&mut self) -> Option<Node> {
        let save = self.pos;
        self.skip_spaces();
        if self.peek() != Some(&Token::Char('[')) {
            self.pos = save;
            return None;
        }
        self.pos += 1;
        self.frames.push(Frame::Bracket);
        let (nodes, end) = self.parse_list();
        self.frames.pop();
        if end == End::Bracket {
            self.pos += 1;
        }
        Some(group_or_single(nodes))
    }

    /// 省略可能引数 `[...]` を文字列として読み飛ばす。
    fn skip_optional_raw(&mut self) {
        let save = self.pos;
        self.skip_spaces();
        if self.peek() != Some(&Token::Char('[')) {
            self.pos = save;
            return;
        }
        while let Some(tok) = self.peek().cloned() {
            self.pos += 1;
            if tok == Token::Char(']') {
                break;
            }
        }
    }

    /// 引数を生の文字列として読む (`\text` や環境名で使う)。
    fn raw_arg(&mut self) -> String {
        self.skip_spaces();
        let mut out = String::new();
        match self.peek().cloned() {
            Some(Token::Open) => {
                self.pos += 1;
                let mut level = 0usize;
                while let Some(tok) = self.peek().cloned() {
                    self.pos += 1;
                    match tok {
                        Token::Open => level += 1,
                        Token::Close if level == 0 => break,
                        Token::Close => level -= 1,
                        other => push_token_text(&mut out, &other),
                    }
                }
            }
            Some(Token::Close) | None => {}
            Some(tok) => {
                self.pos += 1;
                push_token_text(&mut out, &tok);
            }
        }
        out
    }

    /// `\left` 等の後の区切り記号を読む。`.` は None (見えない区切り)。
    fn parse_delim(&mut self) -> Option<char> {
        self.skip_spaces();
        match self.peek().cloned() {
            Some(Token::Char(c)) => {
                self.pos += 1;
                match c {
                    '.' => None,
                    '<' => Some('⟨'),
                    '>' => Some('⟩'),
                    c => Some(c),
                }
            }
            Some(Token::Command(name)) => {
                self.pos += 1;
                match name.as_str() {
                    "{" | "lbrace" => Some('{'),
                    "}" | "rbrace" => Some('}'),
                    "|" | "Vert" | "lVert" | "rVert" => Some('‖'),
                    "vert" | "lvert" | "rvert" | "mid" => Some('|'),
                    "langle" => Some('⟨'),
                    "rangle" => Some('⟩'),
                    "lfloor" => Some('⌊'),
                    "rfloor" => Some('⌋'),
                    "lceil" => Some('⌈'),
                    "rceil" => Some('⌉'),
                    "lbrack" => Some('['),
                    "rbrack" => Some(']'),
                    "backslash" => Some('\\'),
                    "lgroup" => Some('⟮'),
                    "rgroup" => Some('⟯'),
                    other => symbol(other).and_then(|(s, _)| s.chars().next()),
                }
            }
            _ => None,
        }
    }

    // ---- 文字・コマンド ----

    /// 1 文字を記号ノードにする (現在の書体を適用)。
    fn char_node(&self, c: char) -> Node {
        match c {
            '~' => Node::Space(0.3),
            '\'' => atom("′".into(), Class::Ord),
            _ if c.is_alphabetic() => {
                let (m, italic, bold) = styled_char(c, self.variant);
                Node::Atom(Atom {
                    text: m.to_string(),
                    class: Class::Ord,
                    italic,
                    bold,
                })
            }
            _ => {
                let (m, _, bold) = styled_char(c, self.variant);
                let text = match m {
                    '-' => "−".to_owned(),
                    '*' => "∗".to_owned(),
                    other => other.to_string(),
                };
                Node::Atom(Atom {
                    text,
                    class: char_class(c),
                    italic: false,
                    bold,
                })
            }
        }
    }

    /// 書体を切り替えて引数を読む (`\mathbb{R}` など)。
    fn with_variant(&mut self, variant: Variant) -> Node {
        let saved = self.variant;
        self.variant = variant;
        let arg = self.parse_arg();
        self.variant = saved;
        arg
    }

    /// コマンド (`\` の後の名前は消費済み) を解釈する。
    fn command(&mut self, name: &str) -> Node {
        match name {
            // ---- 分数・二項係数 ----
            "frac" | "dfrac" | "tfrac" | "cfrac" | "binom" | "dbinom" | "tbinom" => {
                let num = self.parse_arg();
                let den = self.parse_arg();
                let style = match name {
                    "dfrac" | "cfrac" | "dbinom" => Some(MathStyle::Display),
                    "tfrac" | "tbinom" => Some(MathStyle::Text),
                    _ => None,
                };
                let binom = name.ends_with("binom");
                Node::Frac(Box::new(Frac {
                    num,
                    den,
                    bar: !binom,
                    left: binom.then_some('('),
                    right: binom.then_some(')'),
                    style,
                }))
            }
            // ---- 根号 ----
            "sqrt" => {
                let index = self.parse_optional().map(Box::new);
                let body = Box::new(self.parse_arg());
                Node::Sqrt { index, body }
            }
            // ---- 伸縮括弧 ----
            "left" => {
                let left = self.parse_delim();
                self.frames.push(Frame::Left);
                let (body, end) = self.parse_list();
                self.frames.pop();
                let right = if end == End::Right {
                    self.pos += 1;
                    self.parse_delim()
                } else {
                    None
                };
                Node::Delimited { left, right, body }
            }
            "middle" => Node::Middle(self.parse_delim()),
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "Bigl" | "biggl" | "Biggl" | "bigr"
            | "Bigr" | "biggr" | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => {
                let size = match name.trim_end_matches(['l', 'r', 'm']) {
                    "big" => 1.2,
                    "Big" => 1.8,
                    "bigg" => 2.4,
                    _ => 3.0,
                };
                let class = match name.chars().last() {
                    Some('l') => Class::Open,
                    Some('r') => Class::Close,
                    Some('m') => Class::Rel,
                    _ => Class::Ord,
                };
                Node::BigDelim {
                    delim: self.parse_delim(),
                    size,
                    class,
                }
            }
            // ---- 環境 ----
            "begin" => {
                let env = self.raw_arg();
                self.parse_env(env.trim())
            }
            "substack" => self.parse_substack(),
            // ---- テキスト ----
            "text" | "textrm" | "textup" | "textnormal" | "mbox" | "hbox" | "textsf" | "texttt"
            | "textmd" => Node::Text {
                text: self.raw_arg(),
                italic: false,
                bold: false,
            },
            "textit" | "emph" | "textsl" => Node::Text {
                text: self.raw_arg(),
                italic: true,
                bold: false,
            },
            "textbf" => Node::Text {
                text: self.raw_arg(),
                italic: false,
                bold: true,
            },
            // ---- 書体 ----
            "mathrm" | "mathup" | "mathsf" | "mathtt" | "mathsfit" => {
                self.with_variant(Variant::Roman)
            }
            "mathbf" | "bold" => self.with_variant(Variant::Bold),
            "mathit" => self.with_variant(Variant::Italic),
            "mathnormal" => self.with_variant(Variant::Normal),
            "boldsymbol" | "bm" | "pmb" => self.with_variant(Variant::BoldItalic),
            "mathbb" | "Bbb" => self.with_variant(Variant::Blackboard),
            "mathcal" | "mathscr" => self.with_variant(Variant::Calligraphic),
            "mathfrak" => self.with_variant(Variant::Fraktur),
            "operatorname" | "mathop" => {
                let mut limits = if name == "mathop" {
                    Limits::Auto
                } else {
                    Limits::Side
                };
                if self.peek() == Some(&Token::Char('*')) {
                    self.pos += 1;
                    limits = Limits::Auto;
                }
                Node::BigOp(BigOp {
                    text: self.raw_arg(),
                    named: true,
                    integral: false,
                    limits,
                })
            }
            // ---- 上下の飾り ----
            "overbrace" | "underbrace" => Node::Brace {
                over: name == "overbrace",
                body: Box::new(self.parse_arg()),
            },
            "overset" | "stackrel" | "underset" => {
                let script = Box::new(self.parse_arg());
                let base = Box::new(self.parse_arg());
                if name == "underset" {
                    Node::OverUnder {
                        base,
                        over: None,
                        under: Some(script),
                    }
                } else {
                    Node::OverUnder {
                        base,
                        over: Some(script),
                        under: None,
                    }
                }
            }
            "xrightarrow" | "xleftarrow" | "xRightarrow" | "xLeftarrow" | "xleftrightarrow"
            | "xmapsto" => {
                let under = self.parse_optional().map(Box::new);
                let over = Some(Box::new(self.parse_arg()));
                let arrow = match name {
                    "xleftarrow" => "⟵",
                    "xRightarrow" => "⟹",
                    "xLeftarrow" => "⟸",
                    "xleftrightarrow" => "⟷",
                    "xmapsto" => "⟼",
                    _ => "⟶",
                };
                Node::OverUnder {
                    base: Box::new(atom(arrow.into(), Class::Rel)),
                    over,
                    under,
                }
            }
            // ---- 空白 ----
            "hspace" | "hskip" | "kern" | "mkern" | "mskip" => {
                if self.peek() == Some(&Token::Char('*')) {
                    self.pos += 1;
                }
                let spec = self.raw_arg();
                let em = if name.starts_with('m') {
                    parse_length(&spec) / 18.0
                } else {
                    parse_length(&spec)
                };
                Node::Space(em)
            }
            // ---- その他 ----
            "not" => match self.parse_arg() {
                Node::Atom(a) => Node::Atom(Atom {
                    text: negate_text(&a.text),
                    class: Class::Rel,
                    ..a
                }),
                other => other,
            },
            "pmod" => {
                let arg = self.parse_arg();
                Node::Group(vec![
                    Node::Space(0.6),
                    atom("(".into(), Class::Open),
                    named_op("mod"),
                    Node::Space(0.3),
                    arg,
                    atom(")".into(), Class::Close),
                ])
            }
            "bmod" => Node::Atom(Atom {
                text: "mod".into(),
                class: Class::Bin,
                italic: false,
                bold: false,
            }),
            "mod" => Node::Group(vec![Node::Space(0.6), named_op("mod"), Node::Space(0.3)]),
            "boxed" | "fbox" => Node::Boxed(Box::new(self.parse_arg())),
            "phantom" | "vphantom" | "hphantom" => Node::Phantom(Box::new(self.parse_arg())),
            "textcolor" => {
                let color = parse_color(&self.raw_arg());
                let body = self.parse_arg();
                match color {
                    Some(color) => Node::Color {
                        color,
                        body: vec![body],
                    },
                    None => body,
                }
            }
            "tag" => {
                if self.peek() == Some(&Token::Char('*')) {
                    self.pos += 1;
                }
                let label = self.raw_arg();
                Node::Group(vec![
                    Node::Space(2.0),
                    Node::Text {
                        text: format!("({label})"),
                        italic: false,
                        bold: false,
                    },
                ])
            }
            "label" => {
                let _ = self.raw_arg();
                Node::Group(Vec::new())
            }
            "nonumber" | "notag" | "limits" | "nolimits" | "displaylimits" | "hline"
            | "midrule" | "toprule" | "bottomrule" | "relax" | "strut" | "allowbreak" => {
                Node::Group(Vec::new())
            }
            _ => self.simple_command(name),
        }
    }

    /// 引数を取らない単純なコマンド (記号・関数名・空白・アクセント) を解釈する。
    fn simple_command(&mut self, name: &str) -> Node {
        if let Some(em) = spacing_command(name) {
            return Node::Space(em);
        }
        if let Some(kind) = accent_kind(name) {
            return Node::Accent {
                kind,
                body: Box::new(self.parse_arg()),
            };
        }
        if let Some((text, integral)) = big_op_symbol(name) {
            return Node::BigOp(BigOp {
                text: text.into(),
                named: false,
                integral,
                limits: Limits::Auto,
            });
        }
        if let Some((text, limits)) = function_name(name) {
            return Node::BigOp(BigOp {
                text: text.into(),
                named: true,
                integral: false,
                limits: if limits { Limits::Auto } else { Limits::Side },
            });
        }
        if let Some((text, class)) = symbol(name) {
            let c = text.chars().next().unwrap_or(' ');
            let (italic, bold) = match self.variant {
                Variant::Bold => (false, true),
                Variant::BoldItalic => (is_lower_greek(c), true),
                Variant::Roman => (false, false),
                Variant::Italic => (c.is_alphabetic(), false),
                _ => (is_lower_greek(c), false),
            };
            return Node::Atom(Atom {
                text: text.into(),
                class,
                italic,
                bold,
            });
        }
        Node::Error(format!("\\{name}"))
    }

    // ---- 表 (環境) ----

    /// セル・行を読み、`\end` などの終端まで表を組み立てる。
    fn parse_rows(&mut self) -> Rows {
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let mut hlines = Vec::new();
        loop {
            // 行頭の \hline を罫線として記録
            loop {
                self.skip_spaces();
                match self.peek() {
                    Some(Token::Command(n))
                        if matches!(n.as_str(), "hline" | "midrule" | "toprule" | "bottomrule") =>
                    {
                        self.pos += 1;
                        if row.is_empty() && !hlines.contains(&rows.len()) {
                            hlines.push(rows.len());
                        }
                    }
                    _ => break,
                }
            }
            let (cell, end) = self.parse_list();
            row.push(cell);
            match end {
                End::Amp => self.pos += 1,
                End::Row => {
                    self.pos += 1;
                    self.skip_optional_raw();
                    rows.push(std::mem::take(&mut row));
                }
                other => {
                    rows.push(row);
                    // 末尾の `\\` の後の空行は捨てる
                    if rows.len() > 1
                        && rows.last().is_some_and(|r| r.len() == 1 && r[0].is_empty())
                    {
                        rows.pop();
                    }
                    return (rows, hlines, other);
                }
            }
        }
    }

    /// `\begin{name}` の後を読み、環境ノードを作る。
    fn parse_env(&mut self, name: &str) -> Node {
        let base = name.trim_end_matches('*');
        let (kind, left, right, style) = match base {
            "matrix" => (TableKind::Matrix, None, None, CellStyle::Text),
            "pmatrix" => (TableKind::Matrix, Some('('), Some(')'), CellStyle::Text),
            "bmatrix" => (TableKind::Matrix, Some('['), Some(']'), CellStyle::Text),
            "Bmatrix" => (TableKind::Matrix, Some('{'), Some('}'), CellStyle::Text),
            "vmatrix" => (TableKind::Matrix, Some('|'), Some('|'), CellStyle::Text),
            "Vmatrix" => (TableKind::Matrix, Some('‖'), Some('‖'), CellStyle::Text),
            "smallmatrix" => (TableKind::Matrix, None, None, CellStyle::Script),
            "cases" => (TableKind::Cases, Some('{'), None, CellStyle::Text),
            "dcases" => (TableKind::Cases, Some('{'), None, CellStyle::Display),
            "rcases" => (TableKind::Cases, None, Some('}'), CellStyle::Text),
            "aligned" | "align" | "alignat" | "alignedat" | "split" | "eqnarray" | "flalign" => {
                (TableKind::Aligned, None, None, CellStyle::Display)
            }
            "gathered" | "gather" | "multline" | "equation" | "displaymath" => {
                (TableKind::Gathered, None, None, CellStyle::Display)
            }
            "array" | "darray" | "tabular" => (TableKind::Array, None, None, CellStyle::Text),
            _ => (TableKind::Matrix, None, None, CellStyle::Text),
        };
        let mut cols = Vec::new();
        let mut vlines = Vec::new();
        if base.starts_with("alignat") || base == "alignedat" {
            let _ = self.raw_arg();
        }
        if kind == TableKind::Array {
            self.skip_optional_raw();
            for c in self.raw_arg().chars() {
                match c {
                    'l' => cols.push(ColAlign::Left),
                    'c' => cols.push(ColAlign::Center),
                    'r' => cols.push(ColAlign::Right),
                    '|' => vlines.push(cols.len()),
                    _ => {}
                }
            }
        }
        self.frames.push(Frame::Env);
        let (rows, hlines, end) = self.parse_rows();
        self.frames.pop();
        if end == End::Env {
            self.pos += 1;
            let _ = self.raw_arg();
        }
        Node::Matrix(Box::new(Matrix {
            rows,
            kind,
            cols,
            vlines,
            hlines,
            left,
            right,
            style,
        }))
    }

    /// `\substack{a \\ b}` を 1 列の表として読む。
    fn parse_substack(&mut self) -> Node {
        self.skip_spaces();
        if self.peek() != Some(&Token::Open) {
            return self.parse_arg();
        }
        self.pos += 1;
        self.frames.push(Frame::Group);
        self.frames.push(Frame::Env);
        let (rows, _, end) = self.parse_rows();
        self.frames.pop();
        self.frames.pop();
        if end == End::Close {
            self.pos += 1;
        }
        Node::Matrix(Box::new(Matrix {
            rows,
            kind: TableKind::Substack,
            cols: Vec::new(),
            vlines: Vec::new(),
            hlines: Vec::new(),
            left: None,
            right: None,
            style: CellStyle::Inherit,
        }))
    }
}

/// 立体の記号ノードを作る。
fn atom(text: String, class: Class) -> Node {
    Node::Atom(Atom {
        text,
        class,
        italic: false,
        bold: false,
    })
}

/// 添字を右側に置く関数名ノードを作る。
fn named_op(text: &str) -> Node {
    Node::BigOp(BigOp {
        text: text.into(),
        named: true,
        integral: false,
        limits: Limits::Side,
    })
}

/// 生テキスト引数 (`\text{...}`) 用にトークンを文字列へ戻す。
fn push_token_text(out: &mut String, tok: &Token) {
    match tok {
        Token::Char('~') => out.push(' '),
        Token::Char(c) => out.push(*c),
        Token::Space => out.push(' '),
        Token::Sup => out.push('^'),
        Token::Sub => out.push('_'),
        Token::Amp => out.push('&'),
        Token::Open | Token::Close => {}
        Token::Command(name) => match name.as_str() {
            "\\" | " " | "," | ":" | ";" | ">" => out.push(' '),
            "!" => {}
            "quad" => out.push_str("  "),
            "qquad" => out.push_str("    "),
            other => match symbol(other) {
                Some((s, _)) => out.push_str(s),
                None if other.chars().count() == 1 => out.push_str(other),
                None => {
                    out.push('\\');
                    out.push_str(other);
                }
            },
        },
    }
}

// ============================================================================
// ==== 機能: 原子クラスと空白規則 ====
// ============================================================================

/// 二項演算子が単項として使われている場合などに Ord へ変換する (TeX の規則)。
/// `lead` はリストの直前にある (仮想の) 要素のクラス。
fn resolve_classes(classes: &mut [Option<Class>], lead: Option<Class>) {
    use Class::*;
    let mut prev: Option<(Option<usize>, Class)> = lead.map(|c| (None, c));
    for i in 0..classes.len() {
        let Some(mut c) = classes[i] else { continue };
        if c == Bin && matches!(prev, None | Some((_, Bin | Op | Rel | Open | Punct))) {
            c = Ord;
        }
        if matches!(c, Rel | Close | Punct)
            && let Some((Some(j), Bin)) = prev
        {
            classes[j] = Some(Ord);
        }
        classes[i] = Some(c);
        prev = Some((Some(i), c));
    }
    if let Some((Some(j), Bin)) = prev {
        classes[j] = Some(Ord);
    }
}

/// 隣接する 2 要素間の空白量 (mu = 1/18 em) と、添字スタイルでも入れるか。
fn spacing_mu(left: Class, right: Class) -> (u8, bool) {
    use Class::*;
    match (left, right) {
        (Ord | Op | Close | Inner, Op) | (Op, Ord) => (3, true),
        (Ord | Close | Inner, Bin) | (Bin, Ord | Op | Open | Inner) => (4, false),
        (Ord | Op | Close | Inner, Rel) | (Rel, Ord | Op | Open | Inner) => (5, false),
        (Ord | Op | Close, Inner)
        | (Inner, Ord | Open | Punct | Inner)
        | (Punct, Ord | Op | Rel | Open | Close | Punct | Inner) => (3, false),
        _ => (0, false),
    }
}

// ============================================================================
// ==== 機能: レイアウト (ボックスと描画アイテム) ====
// ============================================================================

/// 描画アイテム。座標はボックス原点 (左端・ベースライン) からの相対値で、y は下向き。
#[derive(Debug, Clone)]
enum Item {
    /// テキスト (Galley の左上位置)。`bold` > 0 なら x 方向にずらして重ね描きする。
    Text {
        pos: Vec2,
        galley: Arc<Galley>,
        color: Color32,
        bold: f32,
    },
    /// 塗りつぶし矩形 (分数線・罫線)
    Rect { rect: Rect, color: Color32 },
    /// 折れ線
    Line {
        points: Vec<Pos2>,
        width: f32,
        color: Color32,
    },
    /// 太さが変化する線 (外側・内側の輪郭で囲まれた帯)
    Band {
        outer: Vec<Pos2>,
        inner: Vec<Pos2>,
        color: Color32,
    },
    /// 塗りつぶし円 (`\dot`)
    Dot {
        center: Pos2,
        radius: f32,
        color: Color32,
    },
}

impl Item {
    /// アイテムを平行移動する。
    fn translate(&mut self, d: Vec2) {
        match self {
            Item::Text { pos, .. } => *pos += d,
            Item::Rect { rect, .. } => *rect = rect.translate(d),
            Item::Line { points, .. } => points.iter_mut().for_each(|p| *p += d),
            Item::Band { outer, inner, .. } => {
                outer
                    .iter_mut()
                    .chain(inner.iter_mut())
                    .for_each(|p| *p += d);
            }
            Item::Dot { center, .. } => *center += d,
        }
    }
}

/// レイアウト済みのボックス。原点は左端のベースライン上。
#[derive(Debug, Clone, Default)]
struct LayoutBox {
    /// 送り幅
    width: f32,
    /// ベースラインより上の高さ
    ascent: f32,
    /// ベースラインより下の深さ
    descent: f32,
    /// イタリック補正 (傾いた文字の右上のはみ出し)
    italic: f32,
    /// 描画アイテム
    items: Vec<Item>,
}

impl LayoutBox {
    /// 幅だけを持つ空のボックス。
    fn space(width: f32) -> Self {
        Self {
            width,
            ..Self::default()
        }
    }

    /// 全体の高さ (高さ + 深さ)。
    fn height(&self) -> f32 {
        self.ascent + self.descent
    }

    /// 別のボックスを (dx, dy) (dy は下向き正) の位置に重ねる。幅は変えない。
    fn place(&mut self, other: LayoutBox, dx: f32, dy: f32) {
        self.ascent = self.ascent.max(other.ascent - dy);
        self.descent = self.descent.max(other.descent + dy);
        let d = vec2(dx, dy);
        self.items.extend(other.items.into_iter().map(|mut it| {
            it.translate(d);
            it
        }));
    }

    /// 描画アイテムを追加し、上下の範囲を広げる (`top`/`bottom` は y 下向き座標)。
    fn add_item(&mut self, item: Item, top: f32, bottom: f32) {
        self.ascent = self.ascent.max(-top);
        self.descent = self.descent.max(bottom);
        self.items.push(item);
    }

    /// ベースラインを `up` だけ持ち上げる (負なら下げる)。
    fn raise(&mut self, up: f32) {
        self.ascent += up;
        self.descent -= up;
        let d = vec2(0.0, -up);
        self.items.iter_mut().for_each(|it| it.translate(d));
    }
}

/// レイアウト中の環境 (スタイルと色)。
#[derive(Debug, Clone, Copy)]
struct Env {
    style: MathStyle,
    color: Color32,
}

impl Env {
    /// スタイルだけを変えた環境。
    fn with_style(self, style: MathStyle) -> Self {
        Self { style, ..self }
    }
}

/// テキスト計測時の書式。
#[derive(Debug, Clone, Copy)]
struct Font {
    size: f32,
    italic: bool,
    bold: bool,
    mono: bool,
}

/// 未知のコマンドの表示色。
const ERROR_COLOR: Color32 = Color32::from_rgb(230, 90, 70);

/// Node 木から LayoutBox を組み立てるレイアウタ。
struct Layouter<'a, 'f> {
    fonts: &'a mut FontsView<'f>,
    /// 基準文字サイズ (pt)
    base: f32,
    /// x の高さ (em 比)
    x_height: f32,
    /// 数式の軸 (分数線の高さ、em 比)
    axis: f32,
}

// ---- 基本計測 ----
impl<'a, 'f> Layouter<'a, 'f> {
    /// フォントを計測して x-height と数式軸を求める。
    fn new(fonts: &'a mut FontsView<'f>, base: f32) -> Self {
        let mut l = Self {
            fonts,
            base,
            x_height: 0.45,
            axis: 0.25,
        };
        let font = Font {
            size: base,
            italic: false,
            bold: false,
            mono: false,
        };
        let x = l.text_box("x", font, Color32::WHITE);
        if x.ascent > 0.0 {
            l.x_height = (x.ascent / base).clamp(0.3, 0.75);
        }
        let plus = l.text_box("+", font, Color32::WHITE);
        if plus.ascent > 0.0 {
            l.axis = ((plus.ascent - plus.descent) / 2.0 / base).clamp(0.12, 0.45);
        }
        l
    }

    /// 現在のスタイルでの 1em (pt)。
    fn em(&self, env: Env) -> f32 {
        self.base * env.style.scale()
    }

    /// 線の太さ (分数線など)。
    fn rule(&self, em: f32) -> f32 {
        (em * 0.05).max(1.0)
    }

    /// 文字列を Galley にして計測する。高さ・深さは実際のインク範囲から求める。
    fn text_box(&mut self, text: &str, font: Font, color: Color32) -> LayoutBox {
        let family = if font.mono {
            FontFamily::Monospace
        } else {
            FontFamily::Proportional
        };
        let mut job = LayoutJob::default();
        job.append(
            text,
            0.0,
            TextFormat {
                font_id: FontId::new(font.size, family),
                color,
                italics: font.italic,
                ..Default::default()
            },
        );
        let galley = self.fonts.layout_job(job);

        let mut baseline: Option<f32> = None;
        let (mut top, mut bottom) = (f32::INFINITY, f32::NEG_INFINITY);
        let mut advance = 0.0f32;
        let mut italic = 0.0f32;
        for row in &galley.rows {
            for g in &row.glyphs {
                let base_y = row.pos.y + g.pos.y;
                baseline.get_or_insert(base_y);
                let right = row.pos.x + g.pos.x + g.advance_width;
                advance = advance.max(right);
                if !g.uv_rect.is_nothing() {
                    let t = base_y + g.uv_rect.offset.y;
                    let b = t + g.uv_rect.size.y;
                    top = top.min(t);
                    bottom = bottom.max(b);
                    if font.italic {
                        // egui のイタリックはインク上端を高さ×0.25 だけ右へずらす
                        let ink_right = row.pos.x + g.pos.x + g.uv_rect.offset.x + g.uv_rect.size.x;
                        italic = (ink_right + g.uv_rect.size.y * 0.25 - right).max(0.0);
                    }
                }
            }
        }
        let baseline = baseline.unwrap_or(galley.size().y * 0.8);
        let (ascent, descent) = if top.is_finite() {
            ((baseline - top).max(0.0), (bottom - baseline).max(0.0))
        } else {
            (0.0, 0.0)
        };
        let bold = if font.bold {
            (font.size * 0.035).max(0.5)
        } else {
            0.0
        };
        LayoutBox {
            width: advance + bold,
            ascent,
            descent,
            italic,
            items: vec![Item::Text {
                pos: vec2(0.0, -baseline),
                galley,
                color,
                bold,
            }],
        }
    }

    /// インクの高さが `target` になるよう文字サイズを調整して文字を組む。
    fn sized_glyph(&mut self, text: &str, em: f32, target: f32, color: Color32) -> LayoutBox {
        let mut font = Font {
            size: em,
            italic: false,
            bold: false,
            mono: false,
        };
        let probe = self.text_box(text, font, color);
        if probe.height() <= 0.0 {
            return probe;
        }
        let scale = (target / probe.height()).clamp(0.5, 6.0);
        font.size = (em * scale).min(400.0);
        self.text_box(text, font, color)
    }

    /// ボックスを数式の軸に対して上下中央に揃える。
    fn center_on_axis(&self, b: &mut LayoutBox, em: f32) {
        let center = (b.ascent - b.descent) / 2.0;
        b.raise(self.axis * em - center);
    }
}

// ---- ノードごとのレイアウト ----
impl Layouter<'_, '_> {
    /// ノード 1 つをレイアウトする。
    fn layout_node(&mut self, node: &Node, env: Env) -> LayoutBox {
        let em = self.em(env);
        match node {
            Node::Atom(a) => self.text_box(
                &a.text,
                Font {
                    size: em,
                    italic: a.italic,
                    bold: a.bold,
                    mono: false,
                },
                env.color,
            ),
            Node::Group(nodes) => self.layout_list(nodes, env, None),
            Node::Scripts { base, sup, sub } => {
                self.layout_scripts(base, sup.as_deref(), sub.as_deref(), env)
            }
            Node::Frac(f) => self.layout_frac(f, env),
            Node::Sqrt { index, body } => self.layout_sqrt(index.as_deref(), body, env),
            Node::BigOp(op) => self.layout_big_op(op, env),
            Node::Delimited { left, right, body } => {
                self.layout_delimited(*left, *right, body, env)
            }
            Node::Middle(d) => self.delim_box(*d, em, env),
            Node::BigDelim { delim, size, .. } => self.delim_box(*delim, size * em, env),
            Node::Matrix(m) => self.layout_matrix(m, env),
            Node::Accent { kind, body } => self.layout_accent(*kind, body, env),
            Node::Brace { over, body } => self.layout_brace(*over, body, env),
            Node::OverUnder { base, over, under } => {
                let b = self.layout_node(base, env);
                let small = env.with_style(env.style.script());
                let o = over.as_deref().map(|n| self.layout_node(n, small));
                let u = under.as_deref().map(|n| self.layout_node(n, small));
                self.stack_limits(b, o, u, em, 0.12 * em, 0.12 * em)
            }
            Node::Text { text, italic, bold } => self.text_box(
                text,
                Font {
                    size: em,
                    italic: *italic,
                    bold: *bold,
                    mono: false,
                },
                env.color,
            ),
            Node::Space(w) => LayoutBox::space(w * em),
            Node::Styled { style, body } => self.layout_list(body, env.with_style(*style), None),
            Node::Color { color, body } => self.layout_list(
                body,
                Env {
                    color: *color,
                    ..env
                },
                None,
            ),
            Node::Boxed(body) => self.layout_boxed(body, env),
            Node::Phantom(body) => {
                let mut b = self.layout_node(body, env);
                b.items.clear();
                b
            }
            Node::Error(text) => self.text_box(
                text,
                Font {
                    size: em * 0.85,
                    italic: false,
                    bold: false,
                    mono: true,
                },
                ERROR_COLOR,
            ),
        }
    }

    /// ノード列を横に並べる。クラスに応じた空白とイタリック補正を入れる。
    /// `lead` はリスト直前の仮想要素のクラス (aligned の右側セル用)。
    fn layout_list(&mut self, nodes: &[Node], env: Env, lead: Option<Class>) -> LayoutBox {
        let boxes: Vec<LayoutBox> = nodes.iter().map(|n| self.layout_node(n, env)).collect();
        self.assemble(nodes, boxes, env, lead)
    }

    /// レイアウト済みのボックス列を横に連結する。
    fn assemble(
        &mut self,
        nodes: &[Node],
        boxes: Vec<LayoutBox>,
        env: Env,
        lead: Option<Class>,
    ) -> LayoutBox {
        let em = self.em(env);
        let mut classes: Vec<Option<Class>> = nodes.iter().map(node_class).collect();
        resolve_classes(&mut classes, lead);
        let mut out = LayoutBox::default();
        let mut x = 0.0f32;
        let mut prev = lead;
        let mut prev_italic = 0.0;
        for ((node, class), b) in nodes.iter().zip(&classes).zip(boxes) {
            if let (Some(p), Some(c)) = (prev, *class) {
                let (mu, always) = spacing_mu(p, c);
                if always || !env.style.is_script() {
                    x += f32::from(mu) * em / 18.0;
                }
            }
            // 傾いた文字の直後に立体の記号が来るときだけイタリック補正を入れる
            let italic_next = matches!(node, Node::Atom(a) if a.italic);
            if !italic_next {
                x += prev_italic;
            }
            prev_italic = b.italic;
            let w = b.width;
            out.place(b, x, 0.0);
            x += w;
            if class.is_some() {
                prev = *class;
            }
        }
        out.width = x.max(0.0);
        out.italic = prev_italic;
        out
    }

    /// 上下添字を配置する (TeX の付録 G の規則を簡略化)。
    fn layout_scripts(
        &mut self,
        base: &Node,
        sup: Option<&Node>,
        sub: Option<&Node>,
        env: Env,
    ) -> LayoutBox {
        let em = self.em(env);
        let script_env = env.with_style(env.style.script());
        let script_em = self.em(script_env);

        // 大型演算子 (ディスプレイ形式) と \overbrace 等は上下に配置する
        let limits = match base {
            Node::BigOp(op) => match op.limits {
                Limits::Above => true,
                Limits::Side => false,
                Limits::Auto => env.style == MathStyle::Display && !op.integral,
            },
            Node::Brace { .. } => true,
            _ => false,
        };
        let b = self.layout_node(base, env);
        let sup_box = sup.map(|n| self.layout_node(n, script_env));
        let sub_box = sub.map(|n| self.layout_node(n, script_env));
        if limits {
            return self.stack_limits(b, sup_box, sub_box, em, 0.15 * em, 0.15 * em);
        }

        let th = self.rule(em);
        let xh = self.x_height * em;
        let is_char = matches!(base, Node::Atom(_));
        let (mut u, mut v) = if is_char {
            (0.0, 0.0)
        } else {
            (b.ascent - 0.386 * script_em, b.descent + 0.05 * script_em)
        };
        let integral = matches!(base, Node::BigOp(op) if op.integral && !op.named);
        let mut out = LayoutBox {
            width: b.width,
            ascent: b.ascent,
            descent: b.descent,
            ..Default::default()
        };
        let sup_x = b.width + b.italic + if integral { 0.05 * em } else { 0.0 };
        let sub_x = if integral {
            b.width - 0.3 * b.width.min(em)
        } else {
            b.width
        };
        let base_italic = b.italic;
        out.place(b, 0.0, 0.0);

        let min_sup = match env.style {
            MathStyle::Display => 0.413,
            _ => 0.363,
        } * em;
        match (sup_box, sub_box) {
            (Some(sp), None) => {
                u = u.max(min_sup).max(sp.descent + xh / 4.0);
                out.width = out.width.max(sup_x + sp.width);
                out.place(sp, sup_x, -u);
            }
            (None, Some(sb)) => {
                v = v.max(0.15 * em).max(sb.ascent - 0.8 * xh);
                out.width = out.width.max(sub_x + sb.width);
                out.place(sb, sub_x, v);
            }
            (Some(sp), Some(sb)) => {
                u = u.max(min_sup).max(sp.descent + xh / 4.0);
                v = v.max(0.247 * em);
                let gap = (u - sp.descent) - (sb.ascent - v);
                if gap < 4.0 * th {
                    v += 4.0 * th - gap;
                    let psi = 0.8 * xh - (u - sp.descent);
                    if psi > 0.0 {
                        u += psi;
                        v -= psi;
                    }
                }
                out.width = out.width.max(sup_x + sp.width).max(sub_x + sb.width);
                out.place(sp, sup_x, -u);
                out.place(sb, sub_x, v);
            }
            (None, None) => out.italic = base_italic,
        }
        // 添字の後の小さな空白 (\scriptspace)
        out.width += 0.05 * em;
        out
    }

    /// 本体の上下に添字を中央揃えで積む (`\sum` の上下限、`\overset` など)。
    fn stack_limits(
        &mut self,
        base: LayoutBox,
        over: Option<LayoutBox>,
        under: Option<LayoutBox>,
        em: f32,
        gap_over: f32,
        gap_under: f32,
    ) -> LayoutBox {
        let width = [
            Some(base.width),
            over.as_ref().map(|b| b.width),
            under.as_ref().map(|b| b.width),
        ]
        .into_iter()
        .flatten()
        .fold(0.0f32, f32::max);
        let mut out = LayoutBox {
            width,
            ..Default::default()
        };
        let base_asc = base.ascent;
        let base_desc = base.descent;
        let bx = (width - base.width) / 2.0;
        out.place(base, bx, 0.0);
        if let Some(o) = over {
            let shift = base_asc + gap_over.max(0.2 * em - o.descent) + o.descent;
            let ox = (width - o.width) / 2.0;
            out.place(o, ox, -shift);
            out.ascent += 0.1 * em;
        }
        if let Some(u) = under {
            let shift = base_desc + gap_under.max(0.6 * em - u.ascent).min(0.35 * em) + u.ascent;
            let ux = (width - u.width) / 2.0;
            out.place(u, ux, shift);
            out.descent += 0.1 * em;
        }
        out
    }

    /// 大型演算子・関数名をレイアウトする。
    fn layout_big_op(&mut self, op: &BigOp, env: Env) -> LayoutBox {
        let em = self.em(env);
        if op.named {
            return self.text_box(
                &op.text,
                Font {
                    size: em,
                    italic: false,
                    bold: false,
                    mono: false,
                },
                env.color,
            );
        }
        let display = env.style == MathStyle::Display;
        let target = match (display, op.integral) {
            (true, true) => 2.1,
            (true, false) => 1.45,
            (false, true) => 1.3,
            (false, false) => 1.0,
        } * em;
        let mut b = self.sized_glyph(&op.text, em, target, env.color);
        self.center_on_axis(&mut b, em);
        b.italic = 0.0;
        b
    }

    /// 分数・二項係数をレイアウトする。
    fn layout_frac(&mut self, f: &Frac, env: Env) -> LayoutBox {
        let style = match f.style {
            // 添字の中で \dfrac が出ても大きくしすぎない
            Some(s) if !env.style.is_script() => s,
            _ => env.style,
        };
        let env = env.with_style(style);
        let em = self.em(env);
        let inner = env.with_style(style.fraction());
        let num = self.layout_node(&f.num, inner);
        let den = self.layout_node(&f.den, inner);
        let th = if f.bar { self.rule(em) } else { 0.0 };
        let axis = self.axis * em;
        let (u0, v0, clr) = if style == MathStyle::Display {
            (0.68 * em, 0.69 * em, 3.0 * self.rule(em))
        } else {
            (0.40 * em, 0.35 * em, self.rule(em) * 1.5)
        };
        let (u, v) = if f.bar {
            (
                u0.max(axis + th / 2.0 + clr + num.descent),
                v0.max(den.ascent + clr + th / 2.0 - axis),
            )
        } else {
            // 線なし (二項係数): 分子と分母の間隔を確保
            let mut u = u0;
            let mut v = v0;
            let gap = (u - num.descent) - (den.ascent - v);
            let need = if style == MathStyle::Display {
                7.0
            } else {
                3.0
            } * self.rule(em);
            if gap < need {
                u += (need - gap) / 2.0;
                v += (need - gap) / 2.0;
            }
            (u, v)
        };
        let pad = 0.12 * em;
        let inner_w = num.width.max(den.width);
        let width = inner_w + 2.0 * pad;
        let mut out = LayoutBox {
            width,
            ..Default::default()
        };
        let nx = pad + (inner_w - num.width) / 2.0;
        let dx = pad + (inner_w - den.width) / 2.0;
        out.place(num, nx, -u);
        out.place(den, dx, v);
        if f.bar {
            let y = -axis;
            let rect = Rect::from_min_max(
                pos2(pad * 0.5, y - th / 2.0),
                pos2(width - pad * 0.5, y + th / 2.0),
            );
            out.add_item(
                Item::Rect {
                    rect,
                    color: env.color,
                },
                rect.top(),
                rect.bottom(),
            );
        }
        if f.left.is_some() || f.right.is_some() {
            return self.wrap_delims(out, f.left, f.right, env);
        }
        out
    }

    /// 根号をレイアウトする。根号記号は中身の高さに合わせてパスで描く。
    fn layout_sqrt(&mut self, index: Option<&Node>, body: &Node, env: Env) -> LayoutBox {
        let em = self.em(env);
        let th = self.rule(em);
        let b = self.layout_node(body, env);
        let phi = if env.style == MathStyle::Display {
            self.x_height * em
        } else {
            th
        };
        let clearance = th + phi / 4.0;
        let body_asc = b.ascent.max(self.x_height * em);
        let body_desc = b.descent.max(0.0);
        let total = body_asc + clearance + th + body_desc;

        // 根号の形: 左の短い跳ね → 太い下り線 → 細い上り線 → 上の横線
        let rw = (0.5 * em + 0.08 * total).min(1.1 * em);
        let top = -(body_asc + clearance + th / 2.0);
        let bottom = body_desc + 0.06 * em;
        let tick_h = (0.45 * em).min(total * 0.5);
        let p_tick0 = pos2(0.05 * em, bottom - tick_h + 0.08 * em);
        let p_tick1 = pos2(0.2 * rw, bottom - tick_h);
        let p_bottom = pos2(0.5 * rw, bottom);
        let p_top = pos2(rw, top);

        let mut out = LayoutBox::default();
        let mut index_shift = 0.0;
        let mut index_box = None;
        if let Some(idx) = index {
            let ie = env.with_style(MathStyle::ScriptScript);
            let ib = self.layout_node(idx, ie);
            // 指数は根号の左上、跳ねの上に載せる
            let anchor_x = 0.5 * rw;
            index_shift = (ib.width - anchor_x + 0.05 * em).max(0.0);
            index_box = Some(ib);
        }
        let shift = vec2(index_shift, 0.0);
        let color = env.color;
        out.add_item(
            Item::Line {
                points: vec![p_tick0 + shift, p_tick1 + shift],
                width: th,
                color,
            },
            p_tick1.y,
            p_tick0.y,
        );
        out.add_item(
            Item::Line {
                points: vec![p_tick1 + shift, p_bottom + shift],
                width: th * 1.9,
                color,
            },
            p_tick1.y,
            p_bottom.y + th,
        );
        let end_x = rw + b.width + 0.1 * em;
        out.add_item(
            Item::Line {
                points: vec![p_bottom + shift, p_top + shift, pos2(end_x, top) + shift],
                width: th,
                color,
            },
            top - th / 2.0,
            p_bottom.y,
        );
        out.place(b, rw + index_shift + 0.03 * em, 0.0);
        out.width = end_x + index_shift + 0.05 * em;
        out.ascent = out.ascent.max(-top + th);
        if let Some(ib) = index_box {
            let raise = 0.6 * (bottom - top) - bottom + ib.descent;
            let ix = index_shift + 0.5 * rw - ib.width - 0.02 * em;
            out.place(ib, ix.max(0.0), -raise);
        }
        out
    }

    /// `\left ... \right` をレイアウトする。`\middle` も同じ高さに伸ばす。
    fn layout_delimited(
        &mut self,
        left: Option<char>,
        right: Option<char>,
        body: &[Node],
        env: Env,
    ) -> LayoutBox {
        let em = self.em(env);
        let mut boxes: Vec<LayoutBox> = body.iter().map(|n| self.layout_node(n, env)).collect();
        let (asc, desc) = body
            .iter()
            .zip(&boxes)
            .filter(|(n, _)| !matches!(n, Node::Middle(_)))
            .fold((0.0f32, 0.0f32), |(a, d), (_, b)| {
                (a.max(b.ascent), d.max(b.descent))
            });
        let size = self.delim_size(asc, desc, em);
        for (n, b) in body.iter().zip(boxes.iter_mut()) {
            if let Node::Middle(d) = n {
                *b = self.delim_box(*d, size, env);
            }
        }
        let inner = self.assemble(body, boxes, env, None);
        self.wrap_delims(inner, left, right, env)
    }

    /// 中身の高さから伸縮括弧の必要な高さを求める (TeX の \delimitershortfall 規則)。
    fn delim_size(&self, ascent: f32, descent: f32, em: f32) -> f32 {
        let axis = self.axis * em;
        let delta = (ascent - axis).max(descent + axis).max(0.0);
        (2.0 * delta * 0.901).max(2.0 * delta - 0.5 * em)
    }

    /// ボックスを左右の伸縮括弧で囲む。
    fn wrap_delims(
        &mut self,
        inner: LayoutBox,
        left: Option<char>,
        right: Option<char>,
        env: Env,
    ) -> LayoutBox {
        let em = self.em(env);
        let size = self.delim_size(inner.ascent, inner.descent, em);
        let l = self.delim_box(left, size, env);
        let r = self.delim_box(right, size, env);
        let mut out = LayoutBox::default();
        let mut x = 0.0;
        for b in [l, inner, r] {
            let w = b.width;
            out.place(b, x, 0.0);
            x += w;
        }
        out.width = x;
        out
    }

    /// 高さ `height` の区切り記号を作る。1 行に収まる高さなら文字、それ以上はパスで描く。
    fn delim_box(&mut self, delim: Option<char>, height: f32, env: Env) -> LayoutBox {
        let em = self.em(env);
        let Some(c) = delim else {
            return LayoutBox::space(0.12 * em);
        };
        let font = Font {
            size: em,
            italic: false,
            bold: false,
            mono: false,
        };
        let glyph = self.text_box(&c.to_string(), font, env.color);
        if height <= glyph.height() * 1.05 || !height.is_finite() {
            return glyph;
        }
        let height = height.min(200.0 * em);
        match delim_shape(c, height, em, self.rule(em), env.color) {
            Some(mut b) => {
                b.raise(self.axis * em);
                b
            }
            None => {
                let mut b = self.sized_glyph(&c.to_string(), em, height, env.color);
                self.center_on_axis(&mut b, em);
                b
            }
        }
    }

    /// 表 (行列・cases・aligned 等) をレイアウトする。
    fn layout_matrix(&mut self, m: &Matrix, env: Env) -> LayoutBox {
        let cell_style = match m.style {
            _ if env.style.is_script() => env.style,
            CellStyle::Text => MathStyle::Text,
            CellStyle::Display => MathStyle::Display,
            CellStyle::Script => MathStyle::Script,
            CellStyle::Inherit => env.style,
        };
        let cell_env = env.with_style(cell_style);
        let em = self.em(cell_env);
        let ncols = m.columns().max(1);

        // セルを組む (aligned の右側セルは直前に仮想の Ord があるものとして空白を決める)
        let cells: Vec<Vec<LayoutBox>> = m
            .rows
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .map(|(j, cell)| {
                        let lead = (m.kind == TableKind::Aligned && !j.is_multiple_of(2))
                            .then_some(Class::Ord);
                        let mut b = self.layout_list(cell, cell_env, lead);
                        // 末尾の傾いた文字が隣のセルに食い込まないよう補正を幅に含める
                        b.width += b.italic;
                        b.italic = 0.0;
                        b
                    })
                    .collect()
            })
            .collect();

        // 列幅・行の高さ (支柱 strut で最低限の高さを確保)
        let mut col_w = vec![0.0f32; ncols];
        for row in &cells {
            for (j, c) in row.iter().enumerate() {
                col_w[j] = col_w[j].max(c.width);
            }
        }
        let (strut_asc, strut_desc, row_gap) = match m.kind {
            TableKind::Substack => (0.0, 0.0, 0.1 * em),
            TableKind::Matrix | TableKind::Array => (0.75 * em, 0.3 * em, 0.15 * em),
            _ => (0.75 * em, 0.3 * em, 0.35 * em),
        };
        let row_metrics: Vec<(f32, f32)> = cells
            .iter()
            .map(|row| {
                row.iter().fold((strut_asc, strut_desc), |(a, d), c| {
                    (a.max(c.ascent), d.max(c.descent))
                })
            })
            .collect();
        let total_h: f32 = row_metrics.iter().map(|(a, d)| a + d).sum::<f32>()
            + row_gap * row_metrics.len().saturating_sub(1) as f32;

        // 列の x 位置
        let outer_pad = if m.kind == TableKind::Array {
            0.5 * em
        } else {
            0.0
        };
        let mut col_x = Vec::with_capacity(ncols);
        let mut x = outer_pad;
        for (j, w) in col_w.iter().enumerate() {
            col_x.push(x);
            x += w;
            if j + 1 < ncols {
                x += m.col_gap(j) * em;
            }
        }
        let width = x + outer_pad;

        let axis = self.axis * em;
        let top = -(total_h / 2.0 + axis);
        let mut out = LayoutBox {
            width,
            ascent: total_h / 2.0 + axis,
            descent: (total_h / 2.0 - axis).max(0.0),
            ..Default::default()
        };
        let mut y = top;
        let mut row_bounds = vec![top];
        for (row, (asc, desc)) in cells.into_iter().zip(&row_metrics) {
            let baseline = y + asc;
            for (j, c) in row.into_iter().enumerate() {
                let slack = col_w[j] - c.width;
                let cx = col_x[j]
                    + match m.align(j) {
                        ColAlign::Left => 0.0,
                        ColAlign::Center => slack / 2.0,
                        ColAlign::Right => slack,
                    };
                out.place(c, cx, baseline);
            }
            y = baseline + desc + row_gap;
            row_bounds.push(baseline + desc);
        }

        // 罫線 (array の | と \hline)
        let th = self.rule(em);
        let bottom = top + total_h;
        for &h in &m.hlines {
            let yy = match h {
                0 => top - row_gap / 2.0,
                h if h >= row_metrics.len() => bottom + row_gap / 2.0,
                h => row_bounds.get(h).copied().unwrap_or(bottom) + row_gap / 2.0,
            };
            let rect = Rect::from_min_max(pos2(0.0, yy - th / 2.0), pos2(width, yy + th / 2.0));
            out.add_item(
                Item::Rect {
                    rect,
                    color: env.color,
                },
                rect.top(),
                rect.bottom(),
            );
        }
        for &v in &m.vlines {
            let xx = if v == 0 {
                0.0
            } else if v >= ncols {
                width
            } else {
                col_x[v] - m.col_gap(v - 1) * em / 2.0
            };
            let (t, b) = (top - row_gap / 2.0, bottom + row_gap / 2.0);
            let rect = Rect::from_min_max(pos2(xx - th / 2.0, t), pos2(xx + th / 2.0, b));
            out.add_item(
                Item::Rect {
                    rect,
                    color: env.color,
                },
                t,
                b,
            );
        }

        if m.left.is_some() || m.right.is_some() {
            // 括弧と中身の間の小さな余白
            let mut padded = LayoutBox::space(0.0);
            let w = out.width;
            padded.place(out, 0.1 * em, 0.0);
            padded.width = w + 0.2 * em;
            return self.wrap_delims(padded, m.left, m.right, env);
        }
        out
    }

    /// アクセント記号をパスで描いて本体の上 (下) に載せる。
    fn layout_accent(&mut self, kind: AccentKind, body: &Node, env: Env) -> LayoutBox {
        use AccentKind::*;
        let em = self.em(env);
        let th = self.rule(em);
        let b = self.layout_node(body, env);
        let is_char = matches!(body, Node::Atom(a) if a.text.chars().count() == 1);
        let skew = if is_char { b.italic * 0.6 } else { 0.0 };
        let cx = b.width / 2.0 + skew;
        let body_top = -b.ascent.max(self.x_height * em);
        let base_y = body_top - 0.1 * em; // アクセントの下端
        let color = env.color;
        let body_w = b.width;
        let narrow = |w: f32| if is_char { w * em } else { body_w.max(w * em) };
        let mut out = LayoutBox {
            width: b.width,
            ..Default::default()
        };
        let italic = b.italic;
        let body_desc = b.descent;
        out.place(b, 0.0, 0.0);
        let line = |points: std::vec::Vec<Pos2>, width: f32| Item::Line {
            points,
            width,
            color,
        };
        match kind {
            Overline | Underline => {
                let y = if kind == Overline {
                    body_top - 3.0 * th
                } else {
                    body_desc + 3.0 * th
                };
                let rect =
                    Rect::from_min_max(pos2(0.0, y - th / 2.0), pos2(out.width, y + th / 2.0));
                out.add_item(
                    Item::Rect { rect, color },
                    rect.top() - th,
                    rect.bottom() + th,
                );
            }
            Bar => {
                let w = narrow(0.42);
                let y = base_y - 0.04 * em;
                out.add_item(
                    line(vec![pos2(cx - w / 2.0, y), pos2(cx + w / 2.0, y)], th),
                    y - th,
                    y,
                );
            }
            Hat | WideHat | Check => {
                let w = if kind == WideHat || !is_char {
                    (out.width * 0.95).max(0.45 * em)
                } else {
                    0.42 * em
                };
                let h = (0.14 * em + w * 0.06).min(0.32 * em);
                let (y_edge, y_mid) = if kind == Check {
                    (base_y - h, base_y)
                } else {
                    (base_y, base_y - h)
                };
                out.add_item(
                    line(
                        vec![
                            pos2(cx - w / 2.0, y_edge),
                            pos2(cx, y_mid),
                            pos2(cx + w / 2.0, y_edge),
                        ],
                        th * 1.1,
                    ),
                    base_y - h - th,
                    base_y,
                );
            }
            Vec | RightArrow | LeftArrow | LeftRightArrow => {
                let w = if kind == Vec {
                    narrow(0.5)
                } else {
                    out.width.max(0.6 * em)
                };
                let y = base_y - 0.1 * em;
                let (x0, x1) = (cx - w / 2.0, cx + w / 2.0);
                let head = 0.12 * em;
                out.add_item(line(vec![pos2(x0, y), pos2(x1, y)], th), y - head, y + head);
                if kind != LeftArrow {
                    out.items.push(line(
                        vec![
                            pos2(x1 - head, y - head),
                            pos2(x1, y),
                            pos2(x1 - head, y + head),
                        ],
                        th,
                    ));
                }
                if matches!(kind, LeftArrow | LeftRightArrow) {
                    out.items.push(line(
                        vec![
                            pos2(x0 + head, y - head),
                            pos2(x0, y),
                            pos2(x0 + head, y + head),
                        ],
                        th,
                    ));
                }
            }
            Dot | Ddot | Dddot => {
                let r = (0.065 * em).max(1.0);
                let y = base_y - r;
                let offsets: &[f32] = match kind {
                    Dot => &[0.0],
                    Ddot => &[-0.12, 0.12],
                    _ => &[-0.2, 0.0, 0.2],
                };
                for dx in offsets {
                    out.add_item(
                        Item::Dot {
                            center: pos2(cx + dx * em, y),
                            radius: r,
                            color,
                        },
                        y - r,
                        y + r,
                    );
                }
            }
            Tilde | WideTilde => {
                let w = if kind == WideTilde || !is_char {
                    out.width.max(0.45 * em)
                } else {
                    0.45 * em
                };
                let amp = 0.05 * em + w * 0.02;
                let y = base_y - amp - 0.02 * em;
                let points = (0..=16)
                    .map(|i| {
                        let t = i as f32 / 16.0;
                        pos2(
                            cx - w / 2.0 + w * t,
                            y - amp * (t * std::f32::consts::TAU).sin(),
                        )
                    })
                    .collect();
                out.add_item(line(points, th * 1.1), y - amp - th, y + amp);
            }
            Acute | Grave => {
                let (dx0, dx1) = if kind == Acute {
                    (-0.05, 0.12)
                } else {
                    (0.05, -0.12)
                };
                let y = base_y;
                out.add_item(
                    line(
                        vec![pos2(cx + dx0 * em, y), pos2(cx + dx1 * em, y - 0.2 * em)],
                        th * 1.2,
                    ),
                    y - 0.2 * em,
                    y,
                );
            }
            Breve | Ring => {
                let r = 0.12 * em;
                let y = base_y - r;
                let (a0, a1) = if kind == Breve {
                    (0.0, std::f32::consts::PI)
                } else {
                    (0.0, std::f32::consts::TAU)
                };
                let points = (0..=16)
                    .map(|i| {
                        let a = a0 + (a1 - a0) * i as f32 / 16.0;
                        pos2(cx + r * a.cos(), y - r * 0.2 + r * a.sin() * 0.8)
                    })
                    .collect();
                out.add_item(line(points, th), y - r, y + r);
            }
        }
        out.italic = italic;
        out
    }

    /// `\overbrace` / `\underbrace` の横向き波括弧を描く。
    fn layout_brace(&mut self, over: bool, body: &Node, env: Env) -> LayoutBox {
        let em = self.em(env);
        let b = self.layout_node(body, env);
        let depth = (0.25 * em + b.width * 0.02).min(0.45 * em);
        let gap = 0.12 * em;
        let len = b.width.max(em);
        let (center, widths) = brace_centerline(len, depth, self.rule(em), em);
        let start_y = if over {
            -(b.ascent + gap + depth)
        } else {
            b.descent + gap + depth
        };
        // 横向き: 軸方向 = x、張り出し方向 = y (尖りが外側)
        let pts: Vec<Pos2> = center
            .iter()
            .map(|p| {
                let y = if over { start_y + p.x } else { start_y - p.x };
                pos2(p.y + (b.width - len) / 2.0, y)
            })
            .collect();
        let (outer, inner) = variable_stroke(&pts, &widths);
        let mut out = LayoutBox {
            width: b.width,
            ..Default::default()
        };
        out.place(b, 0.0, 0.0);
        let (top, bottom) = if over {
            (start_y - 1.0, start_y + depth)
        } else {
            (start_y - depth, start_y + 1.0)
        };
        out.add_item(
            Item::Band {
                outer,
                inner,
                color: env.color,
            },
            top,
            bottom,
        );
        out
    }

    /// `\boxed` の枠を描く。
    fn layout_boxed(&mut self, body: &Node, env: Env) -> LayoutBox {
        let em = self.em(env);
        let th = self.rule(em);
        let pad = 0.25 * em;
        let b = self.layout_node(body, env);
        let (w, asc, desc) = (b.width + 2.0 * pad, b.ascent + pad, b.descent + pad);
        let mut out = LayoutBox::space(w);
        out.place(b, pad, 0.0);
        let r = Rect::from_min_max(pos2(0.0, -asc), pos2(w, desc));
        out.add_item(
            Item::Line {
                points: vec![
                    r.left_top(),
                    r.right_top(),
                    r.right_bottom(),
                    r.left_bottom(),
                    r.left_top(),
                ],
                width: th,
                color: env.color,
            },
            r.top() - th,
            r.bottom() + th,
        );
        out
    }
}

// ============================================================================
// ==== 機能: 伸縮記号の形状生成 (括弧・波括弧) ====
// ============================================================================

/// 2 次ベジエ曲線を `n` 分割した点列 (始点を含まない)。
fn quad_bezier(p0: Pos2, c: Pos2, p1: Pos2, n: usize) -> impl Iterator<Item = Pos2> {
    (1..=n).map(move |i| {
        let t = i as f32 / n as f32;
        let a = (1.0 - t) * (1.0 - t);
        let b = 2.0 * (1.0 - t) * t;
        let d = t * t;
        pos2(a * p0.x + b * c.x + d * p1.x, a * p0.y + b * c.y + d * p1.y)
    })
}

/// 中心線と各点の太さから、帯の外側・内側の輪郭を求める。
fn variable_stroke(points: &[Pos2], widths: &[f32]) -> (Vec<Pos2>, Vec<Pos2>) {
    let n = points.len();
    let mut outer = Vec::with_capacity(n);
    let mut inner = Vec::with_capacity(n);
    let mut last_normal = vec2(1.0, 0.0);
    for i in 0..n {
        let prev = points[i.saturating_sub(1)];
        let next = points[(i + 1).min(n - 1)];
        let t = next - prev;
        let normal = if t.length() > 1e-4 {
            vec2(-t.y, t.x).normalized()
        } else {
            last_normal
        };
        last_normal = normal;
        let w = widths.get(i).copied().unwrap_or(1.0) / 2.0;
        outer.push(points[i] + normal * w);
        inner.push(points[i] - normal * w);
    }
    (outer, inner)
}

/// 縦向き左波括弧 `{` の中心線と太さ。
/// 返す点は (x = 張り出し方向 0..depth, y = 軸方向 0..len)。尖りは x = 0, y = len/2。
fn brace_centerline(len: f32, depth: f32, th: f32, em: f32) -> (Vec<Pos2>, Vec<f32>) {
    let thin = th * 0.7;
    let thick = (0.09 * em).max(th * 1.6);
    let r = (depth / 2.0).min(len / 4.0);
    let mid = depth / 2.0;
    let half = len / 2.0;
    let mut pts = vec![pos2(depth, 0.0)];
    let mut ws = vec![thin];
    // 上半分: 先端 → 柱 → 尖り
    for p in quad_bezier(pos2(depth, 0.0), pos2(mid, 0.0), pos2(mid, r), 8) {
        let t = p.y / r.max(1e-3);
        pts.push(p);
        ws.push(thin + (thick - thin) * t.clamp(0.0, 1.0));
    }
    pts.push(pos2(mid, half - r));
    ws.push(thick);
    for (i, p) in quad_bezier(pos2(mid, half - r), pos2(mid, half), pos2(0.0, half), 8).enumerate()
    {
        pts.push(p);
        ws.push(thick + (thin - thick) * (i + 1) as f32 / 8.0);
    }
    // 下半分は上半分を軸方向に反転
    let upper = pts.len();
    for i in (0..upper - 1).rev() {
        pts.push(pos2(pts[i].x, len - pts[i].y));
        ws.push(ws[i]);
    }
    (pts, ws)
}

/// 区切り記号をパスで描いたボックス (ベースライン = 記号の上下中央) を返す。
/// パスで描けない文字は `None` (呼び出し側で文字を拡大して代用)。
fn delim_shape(c: char, h: f32, em: f32, th: f32, color: Color32) -> Option<LayoutBox> {
    let top = -h / 2.0;
    let bottom = h / 2.0;
    let pad = 0.08 * em;
    let stroke = (0.075 * em).max(th * 1.3);
    let line = |points: Vec<Pos2>, width: f32| Item::Line {
        points,
        width,
        color,
    };
    // 左向きの形を作り、右向きは左右反転する
    let (mirror, base) = match c {
        '(' | '⟮' => (false, '('),
        ')' | '⟯' => (true, '('),
        '[' => (false, '['),
        ']' => (true, '['),
        '{' => (false, '{'),
        '}' => (true, '{'),
        '⟨' => (false, '⟨'),
        '⟩' => (true, '⟨'),
        '⌊' => (false, '⌊'),
        '⌋' => (true, '⌊'),
        '⌈' => (false, '⌈'),
        '⌉' => (true, '⌈'),
        '/' => (false, '/'),
        '\\' => (true, '/'),
        '|' | '∣' => (false, '|'),
        '‖' | '∥' => (false, '‖'),
        _ => return None,
    };
    let mut items = Vec::new();
    let width;
    match base {
        '(' => {
            let bulge = (0.2 * em + h * 0.05).min(0.5 * em);
            width = bulge + stroke + 2.0 * pad;
            let x1 = pad + bulge + stroke / 2.0;
            let x0 = pad + stroke / 2.0;
            let ctrl = pos2(2.0 * x0 - x1, 0.0);
            let start = pos2(x1, top);
            let mut pts = vec![start];
            pts.extend(quad_bezier(start, ctrl, pos2(x1, bottom), 32));
            let n = pts.len();
            let widths: Vec<f32> = (0..n)
                .map(|i| {
                    let t = i as f32 / (n - 1) as f32;
                    th * 0.6 + (stroke - th * 0.6) * (t * std::f32::consts::PI).sin()
                })
                .collect();
            let (outer, inner) = variable_stroke(&pts, &widths);
            items.push(Item::Band {
                outer,
                inner,
                color,
            });
        }
        '[' | '⌊' | '⌈' => {
            let arm = (0.25 * em).min(0.2 * em + h * 0.02);
            let w = stroke * 0.75;
            width = arm + w + 2.0 * pad;
            let x0 = pad + w / 2.0;
            let x1 = x0 + arm;
            let pts = match base {
                '[' => vec![
                    pos2(x1, top),
                    pos2(x0, top),
                    pos2(x0, bottom),
                    pos2(x1, bottom),
                ],
                '⌊' => vec![pos2(x0, top), pos2(x0, bottom), pos2(x1, bottom)],
                _ => vec![pos2(x1, top), pos2(x0, top), pos2(x0, bottom)],
            };
            items.push(line(pts, w));
        }
        '{' => {
            let depth = (0.3 * em + h * 0.03).min(0.55 * em);
            width = depth + 2.0 * pad;
            let (center, widths) = brace_centerline(h, depth, th, em);
            let pts: Vec<Pos2> = center.iter().map(|p| pos2(pad + p.x, top + p.y)).collect();
            let (outer, inner) = variable_stroke(&pts, &widths);
            items.push(Item::Band {
                outer,
                inner,
                color,
            });
        }
        '⟨' => {
            let w = (0.25 * em + 0.08 * h).min(0.6 * em);
            width = w + 2.0 * pad;
            items.push(line(
                vec![pos2(pad + w, top), pos2(pad, 0.0), pos2(pad + w, bottom)],
                th * 1.2,
            ));
        }
        '/' => {
            let w = (0.3 * h).min(1.2 * em);
            width = w + 2.0 * pad;
            items.push(line(vec![pos2(pad, bottom), pos2(pad + w, top)], th * 1.2));
        }
        '|' => {
            width = 2.0 * pad + stroke;
            let x = width / 2.0;
            items.push(line(vec![pos2(x, top), pos2(x, bottom)], stroke * 0.7));
        }
        _ => {
            // '‖'
            let gap = 0.12 * em;
            width = 2.0 * pad + gap + stroke;
            let x0 = pad + stroke / 2.0;
            let x1 = x0 + gap;
            items.push(line(vec![pos2(x0, top), pos2(x0, bottom)], stroke * 0.7));
            items.push(line(vec![pos2(x1, top), pos2(x1, bottom)], stroke * 0.7));
        }
    }
    if mirror {
        let flip = |p: &mut Pos2| p.x = width - p.x;
        for item in &mut items {
            match item {
                Item::Line { points, .. } => points.iter_mut().for_each(flip),
                Item::Band { outer, inner, .. } => {
                    outer.iter_mut().chain(inner.iter_mut()).for_each(flip);
                }
                _ => {}
            }
        }
    }
    Some(LayoutBox {
        width,
        ascent: h / 2.0,
        descent: h / 2.0,
        italic: 0.0,
        items,
    })
}

// ============================================================================
// ==== 機能: 描画 ====
// ============================================================================

/// ボックスの描画アイテムを `origin` (左端のベースライン位置) を基準に描く。
fn paint_box(painter: &Painter, b: &LayoutBox, origin: Pos2) {
    let o = origin.to_vec2();
    for item in &b.items {
        match item {
            Item::Text {
                pos,
                galley,
                color,
                bold,
            } => {
                let p = origin + *pos;
                painter.galley(p, galley.clone(), *color);
                if *bold > 0.0 {
                    painter.galley(p + vec2(*bold, 0.0), galley.clone(), *color);
                }
            }
            Item::Rect { rect, color } => {
                painter.rect_filled(rect.translate(o), 0.0, *color);
            }
            Item::Line {
                points,
                width,
                color,
            } => {
                let pts: Vec<Pos2> = points.iter().map(|p| *p + o).collect();
                painter.add(Shape::line(pts, Stroke::new(*width, *color)));
            }
            Item::Band {
                outer,
                inner,
                color,
            } => paint_band(painter, outer, inner, o, *color),
            Item::Dot {
                center,
                radius,
                color,
            } => {
                painter.circle_filled(*center + o, *radius, *color);
            }
        }
    }
}

/// 太さの変わる帯を三角形メッシュで塗り、輪郭を細線でなぞってアンチエイリアスする。
fn paint_band(painter: &Painter, outer: &[Pos2], inner: &[Pos2], o: Vec2, color: Color32) {
    let n = outer.len().min(inner.len());
    if n < 2 {
        return;
    }
    let mut mesh = Mesh::default();
    for i in 0..n {
        mesh.colored_vertex(outer[i] + o, color);
        mesh.colored_vertex(inner[i] + o, color);
    }
    for i in 0..(n - 1) as u32 {
        let k = i * 2;
        mesh.add_triangle(k, k + 1, k + 2);
        mesh.add_triangle(k + 1, k + 3, k + 2);
    }
    painter.add(Shape::mesh(mesh));
    let edge = Stroke::new(0.6, color);
    painter.add(Shape::line(
        outer[..n].iter().map(|p| *p + o).collect(),
        edge,
    ));
    painter.add(Shape::line(
        inner[..n].iter().map(|p| *p + o).collect(),
        edge,
    ));
}

// ============================================================================
// ==== 機能: Unicode 近似 (インライン数式) ====
// ============================================================================

/// 上付き文字への変換表。
fn superscript_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '−' | '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'i' => 'ⁱ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'n' => 'ⁿ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'A' => 'ᴬ',
        'B' => 'ᴮ',
        'D' => 'ᴰ',
        'E' => 'ᴱ',
        'G' => 'ᴳ',
        'H' => 'ᴴ',
        'I' => 'ᴵ',
        'J' => 'ᴶ',
        'K' => 'ᴷ',
        'L' => 'ᴸ',
        'M' => 'ᴹ',
        'N' => 'ᴺ',
        'O' => 'ᴼ',
        'P' => 'ᴾ',
        'R' => 'ᴿ',
        'T' => 'ᵀ',
        'U' => 'ᵁ',
        'V' => 'ⱽ',
        'W' => 'ᵂ',
        'α' => 'ᵅ',
        'β' => 'ᵝ',
        'γ' => 'ᵞ',
        'δ' => 'ᵟ',
        'φ' | 'ϕ' => 'ᵠ',
        'χ' => 'ᵡ',
        'θ' => 'ᶿ',
        '′' => '′',
        '″' => '″',
        '∗' | '*' => '*',
        _ => return None,
    })
}

/// 下付き文字への変換表。
fn subscript_char(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '−' | '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        'β' => 'ᵦ',
        'γ' => 'ᵧ',
        'ρ' => 'ᵨ',
        'φ' | 'ϕ' => 'ᵩ',
        'χ' => 'ᵪ',
        _ => return None,
    })
}

/// Unicode 出力用のクラス (括弧類は前後に空白を入れないよう Ord 扱い)。
fn uni_class(node: &Node) -> Option<Class> {
    match node {
        Node::Delimited { .. } | Node::Matrix(_) => Some(Class::Ord),
        other => node_class(other),
    }
}

/// 2 要素間に空白を入れるか。`compact` (添字・根号の中) では関数名の後だけ入れる。
fn uni_space(left: Class, right: Class, compact: bool) -> bool {
    use Class::*;
    match spacing_mu(left, right).0 {
        0 => false,
        3 => {
            let op = matches!(left, Op) && matches!(right, Ord | Inner | Op)
                || matches!(right, Op) && matches!(left, Ord | Close | Inner);
            op || (!compact && matches!(left, Punct | Inner) | matches!(right, Inner))
        }
        _ => !compact,
    }
}

/// 末尾が空白でなければ空白を 1 つ足す。
fn push_space(out: &mut String) {
    if !out.ends_with(' ') {
        out.push(' ');
    }
}

/// ノード列を Unicode 文字列にする。
fn uni_list(nodes: &[Node], compact: bool, lead: Option<Class>) -> String {
    let mut classes: Vec<Option<Class>> = nodes.iter().map(uni_class).collect();
    resolve_classes(&mut classes, lead);
    let mut out = String::new();
    let mut prev = lead;
    for (node, class) in nodes.iter().zip(&classes) {
        if let (Some(p), Some(c)) = (prev, *class)
            && uni_space(p, c, compact)
        {
            push_space(&mut out);
        }
        out.push_str(&uni_node(node, compact));
        if class.is_some() {
            prev = *class;
        }
    }
    out
}

/// 添字を上付き・下付き文字に変換する。変換できない文字があれば `^(...)` 形式にする。
fn uni_script(node: &Node, sup: bool) -> String {
    let text: String = uni_node(node, true).chars().filter(|c| *c != ' ').collect();
    if text.is_empty() {
        return String::new();
    }
    let map = if sup {
        superscript_char
    } else {
        subscript_char
    };
    if let Some(mapped) = text.chars().map(map).collect::<Option<String>>() {
        return mapped;
    }
    let mark = if sup { '^' } else { '_' };
    if text.chars().count() == 1 {
        format!("{mark}{text}")
    } else {
        format!("{mark}({text})")
    }
}

/// 分数・根号の中身として、必要なら括弧で囲む。
fn uni_paren(s: String) -> String {
    let simple = s.chars().count() <= 1
        || s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '′' | '∞'));
    if simple { s } else { format!("({s})") }
}

/// 文字ごとに結合文字を付ける。
fn combine_each(s: &str, mark: char) -> String {
    let mut out = String::new();
    for c in s.chars() {
        out.push(c);
        if c != ' ' {
            out.push(mark);
        }
    }
    out
}

/// ノード 1 つを Unicode 文字列にする。
fn uni_node(node: &Node, compact: bool) -> String {
    match node {
        Node::Atom(a) => a.text.clone(),
        Node::Group(nodes) | Node::Styled { body: nodes, .. } | Node::Color { body: nodes, .. } => {
            uni_list(nodes, compact, None)
        }
        Node::Scripts { base, sup, sub } => {
            let mut s = uni_node(base, compact);
            if let Some(sb) = sub {
                s.push_str(&uni_script(sb, false));
            }
            if let Some(sp) = sup {
                s.push_str(&uni_script(sp, true));
            }
            s
        }
        Node::OverUnder { base, over, under } => {
            let mut s = uni_node(base, compact);
            if let Some(u) = under {
                s.push_str(&uni_script(u, false));
            }
            if let Some(o) = over {
                s.push_str(&uni_script(o, true));
            }
            s
        }
        Node::Frac(f) => {
            let num = uni_node(&f.num, true);
            let den = uni_node(&f.den, true);
            if !f.bar && f.left == Some('(') {
                format!("C({num}, {den})")
            } else {
                format!("{}/{}", uni_paren(num), uni_paren(den))
            }
        }
        Node::Sqrt { index, body } => {
            let root = match index.as_deref().map(|i| uni_node(i, true)).as_deref() {
                None | Some("") | Some("2") => "√".to_owned(),
                Some("3") => "∛".to_owned(),
                Some("4") => "∜".to_owned(),
                Some(n) => {
                    let sup: Option<String> = n.chars().map(superscript_char).collect();
                    format!("{}√", sup.unwrap_or_else(|| format!("({n})")))
                }
            };
            format!("{root}{}", uni_paren(uni_node(body, true)))
        }
        Node::BigOp(op) => op.text.clone(),
        Node::Delimited { left, right, body } => {
            let mut s = String::new();
            s.extend(*left);
            s.push_str(&uni_list(body, compact, None));
            s.extend(*right);
            s
        }
        Node::Middle(d) | Node::BigDelim { delim: d, .. } => {
            d.map(String::from).unwrap_or_default()
        }
        Node::Matrix(m) => {
            let (cell_sep, row_sep) = match m.kind {
                TableKind::Aligned => ("", "; "),
                TableKind::Cases => (", ", "; "),
                _ => (" ", "; "),
            };
            let rows: Vec<String> = m
                .rows
                .iter()
                .map(|row| {
                    row.iter()
                        .enumerate()
                        .map(|(j, cell)| {
                            let lead = (m.kind == TableKind::Aligned && !j.is_multiple_of(2))
                                .then_some(Class::Ord);
                            uni_list(cell, compact, lead)
                        })
                        .collect::<Vec<_>>()
                        .join(cell_sep)
                })
                .collect();
            let mut s = String::new();
            s.extend(m.left);
            s.push_str(&rows.join(row_sep));
            s.extend(m.right);
            s
        }
        Node::Accent { kind, body } => {
            let s = uni_node(body, compact);
            let mark = match kind {
                AccentKind::Hat | AccentKind::WideHat => '\u{302}',
                AccentKind::Check => '\u{30C}',
                AccentKind::Bar => '\u{304}',
                AccentKind::Overline => return combine_each(&s, '\u{305}'),
                AccentKind::Underline => return combine_each(&s, '\u{332}'),
                AccentKind::Vec | AccentKind::RightArrow => '\u{20D7}',
                AccentKind::LeftArrow => '\u{20D6}',
                AccentKind::LeftRightArrow => '\u{20E1}',
                AccentKind::Dot => '\u{307}',
                AccentKind::Ddot => '\u{308}',
                AccentKind::Dddot => '\u{20DB}',
                AccentKind::Tilde | AccentKind::WideTilde => '\u{303}',
                AccentKind::Acute => '\u{301}',
                AccentKind::Grave => '\u{300}',
                AccentKind::Breve => '\u{306}',
                AccentKind::Ring => '\u{30A}',
            };
            format!("{s}{mark}")
        }
        Node::Brace { body, .. } | Node::Boxed(body) => uni_node(body, compact),
        Node::Text { text, .. } => text.clone(),
        Node::Space(w) => {
            if *w <= 0.0 {
                String::new()
            } else if *w < 0.9 {
                " ".to_owned()
            } else {
                "  ".repeat((*w).round().clamp(1.0, 4.0) as usize)
            }
        }
        Node::Phantom(_) => String::new(),
        Node::Error(text) => text.clone(),
    }
}

// ============================================================================
// ==== 機能: テスト ====
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- テスト用ヘルパ ----

    /// 既定フォントの egui::Context で 1 フレーム実行する (テクスチャ更新は破棄)。
    fn run_frame(ctx: &Context, f: impl FnMut(&mut Ui)) {
        let mut output = ctx.run_ui(egui::RawInput::default(), f);
        output.textures_delta.clear();
    }

    /// 既定フォントの egui::Context で数式をレイアウトする。
    fn layout_of(tex: &str) -> LayoutBox {
        let ctx = Context::default();
        let mut out = None;
        run_frame(&ctx, |ui| {
            out = Some(layout_tex(ui.ctx(), tex, 20.0, Color32::WHITE));
        });
        out.expect("layout ran")
    }

    /// 指定文字列を描く Text アイテムの位置 (Galley の左上) を探す。
    fn text_pos(b: &LayoutBox, text: &str) -> Option<Vec2> {
        b.items.iter().find_map(|it| match it {
            Item::Text { pos, galley, .. } if galley.text() == text => Some(*pos),
            _ => None,
        })
    }

    /// 指定文字列を描く Text アイテムの Galley の高さ (文字サイズの目安)。
    fn text_height(b: &LayoutBox, text: &str) -> Option<f32> {
        b.items.iter().find_map(|it| match it {
            Item::Text { galley, .. } if galley.text() == text => Some(galley.size().y),
            _ => None,
        })
    }

    /// トップレベルのノード列を取り出す。
    fn top_nodes(tex: &str) -> Vec<Node> {
        match parse(tex) {
            Node::Group(v) => v,
            other => vec![other],
        }
    }

    // ---- トークナイザ ----

    #[test]
    fn tokenize_basic() {
        let t = tokenize(r"\frac{a}{b}^2_x \, % comment");
        assert_eq!(t[0], Token::Command("frac".into()));
        assert_eq!(t[1], Token::Open);
        assert_eq!(t[2], Token::Char('a'));
        assert!(t.contains(&Token::Sup));
        assert!(t.contains(&Token::Sub));
        assert!(t.contains(&Token::Command(",".into())));
        assert!(!t.contains(&Token::Char('c')), "comment is dropped");
        assert_eq!(tokenize(r"\\"), vec![Token::Command("\\".into())]);
        assert_eq!(tokenize("\\"), vec![Token::Char('\\')]);
    }

    // ---- パーサ ----

    #[test]
    fn parse_frac_and_scripts() {
        let n = top_nodes(r"\frac{a+b}{c} + x_i^2");
        assert!(matches!(n[0], Node::Frac(_)));
        match &n[2] {
            Node::Scripts { base, sup, sub } => {
                assert!(matches!(&**base, Node::Atom(a) if a.text == "x" && a.italic));
                assert!(sup.is_some() && sub.is_some());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_numbers_and_operators() {
        let n = top_nodes("3.14 - x");
        assert!(matches!(&n[0], Node::Atom(a) if a.text == "3.14" && !a.italic));
        assert!(matches!(&n[1], Node::Atom(a) if a.text == "−" && a.class == Class::Bin));
        // 添字の数字は 1 文字だけ (TeX と同じ)
        let n = top_nodes("x^23");
        assert_eq!(n.len(), 2);
    }

    #[test]
    fn parse_matrix_rows_cols() {
        let n = parse(r"\begin{pmatrix} 1 & 2 & 3 \\ 4 & 5 & 6 \\ \end{pmatrix}");
        let Node::Group(v) = n else { panic!() };
        let Node::Matrix(m) = &v[0] else { panic!() };
        assert_eq!(m.rows.len(), 2);
        assert_eq!(m.columns(), 3);
        assert_eq!(m.left, Some('('));
        assert_eq!(m.kind, TableKind::Matrix);
    }

    #[test]
    fn parse_array_spec_and_hline() {
        let n = top_nodes(r"\begin{array}{l|cr} a & b & c \\ \hline d & e & f \end{array}");
        let Node::Matrix(m) = &n[0] else { panic!() };
        assert_eq!(
            m.cols,
            vec![ColAlign::Left, ColAlign::Center, ColAlign::Right]
        );
        assert_eq!(m.vlines, vec![1]);
        assert_eq!(m.hlines, vec![1]);
    }

    #[test]
    fn parse_top_level_lines() {
        let Node::Matrix(m) = parse(r"a = b \\ c = d") else {
            panic!()
        };
        assert_eq!(m.kind, TableKind::Gathered);
        assert_eq!(m.rows.len(), 2);
        let Node::Matrix(m) = parse(r"a &= b \\ c &= d \\") else {
            panic!()
        };
        assert_eq!(m.kind, TableKind::Aligned);
        assert_eq!(m.rows.len(), 2);
    }

    #[test]
    fn parse_recovers_from_unbalanced_input() {
        // 閉じ括弧の不足・過剰、\right の欠落、\end の欠落
        assert!(matches!(&top_nodes(r"\frac{a}{b")[0], Node::Frac(_)));
        assert_eq!(top_nodes("a}}b").len(), 2);
        assert!(matches!(
            &top_nodes(r"\left( x")[0],
            Node::Delimited { right: None, .. }
        ));
        assert!(matches!(
            &top_nodes(r"\begin{matrix} a & b")[0],
            Node::Matrix(_)
        ));
        assert!(matches!(&top_nodes(r"x \right)")[1], Node::Atom(a) if a.text == ")"));
        // \left 内のグループで \right が見つかっても正しく閉じる
        let n = top_nodes(r"\left( {a \right) b");
        assert!(matches!(
            &n[0],
            Node::Delimited {
                right: Some(')'),
                ..
            }
        ));
    }

    #[test]
    fn parse_unknown_command() {
        let n = top_nodes(r"\foo{x}");
        assert!(matches!(&n[0], Node::Error(s) if s == r"\foo"));
    }

    #[test]
    fn parse_big_ops_and_limits() {
        let n = top_nodes(r"\sum_{i=1}^n \int\limits_0^1 \lim_{x\to0}");
        let base_of = |n: &Node| match n {
            Node::Scripts { base, .. } => match &**base {
                Node::BigOp(op) => op.clone(),
                _ => panic!(),
            },
            _ => panic!(),
        };
        assert!(!base_of(&n[0]).integral);
        assert_eq!(base_of(&n[1]).limits, Limits::Above);
        assert!(base_of(&n[2]).named);
    }

    #[test]
    fn deep_nesting_does_not_overflow() {
        let deep = "{".repeat(5000) + "x" + &"}".repeat(5000);
        let _ = parse(&deep);
        let sqrt = r"\sqrt".repeat(5000) + "x";
        let _ = parse(&sqrt);
        let _ = to_unicode(&sqrt);
        let _ = parse(&r"\left(".repeat(3000));
        let _ = parse(&"x^".repeat(3000));
    }

    // ---- Unicode 変換 ----

    #[test]
    fn unicode_examples() {
        assert_eq!(to_unicode("E = mc^2"), "E = mc²");
        assert_eq!(to_unicode(r"\alpha + \beta"), "α + β");
        assert_eq!(to_unicode(r"\frac{a}{b}"), "a/b");
        assert_eq!(to_unicode(r"\frac{a+b}{2}"), "(a+b)/2");
        assert_eq!(to_unicode("x_i"), "xᵢ");
        assert_eq!(to_unicode("x_{ij}"), "xᵢⱼ");
        assert_eq!(to_unicode("x_{qz}"), "x_(qz)");
        assert_eq!(to_unicode("x_q"), "x_q");
        assert_eq!(to_unicode(r"\sqrt{x}"), "√x");
        assert_eq!(to_unicode(r"\sqrt{x+1}"), "√(x+1)");
        assert_eq!(to_unicode(r"\mathbb{R}"), "ℝ");
        assert_eq!(to_unicode(r"\text{if }"), "if ");
        assert_eq!(to_unicode(r"\foo"), r"\foo");
        assert_eq!(to_unicode(r"\sum_{i=1}^n i"), "∑ᵢ₌₁ⁿ i");
        assert_eq!(to_unicode(r"\sin x"), "sin x");
        assert_eq!(to_unicode(r"f'(x)"), "f′(x)");
        assert_eq!(to_unicode(r"a \leq b"), "a ≤ b");
        assert_eq!(to_unicode(r"-x"), "−x");
        assert_eq!(to_unicode(r"\hat{x}"), "x\u{302}");
        assert_eq!(to_unicode(r"\left( x \right)"), "(x)");
        assert_eq!(to_unicode(r"f(x, y)"), "f(x, y)");
        assert_eq!(to_unicode(r"\sqrt[3]{x}"), "∛x");
        assert_eq!(to_unicode(r"x \in \mathbb{N}"), "x ∈ ℕ");
    }

    // ---- レイアウト ----

    #[test]
    fn fraction_is_taller_than_symbol() {
        let a = layout_of("a");
        let f = layout_of(r"\frac{a}{b}");
        assert!(
            f.height() > a.height() * 1.8,
            "{} vs {}",
            f.height(),
            a.height()
        );
        assert!(
            f.items.iter().any(|i| matches!(i, Item::Rect { .. })),
            "has bar"
        );
        // 分子は分母より上
        let na = text_pos(&f, "a").unwrap();
        let nb = text_pos(&f, "b").unwrap();
        assert!(na.y < nb.y);
    }

    #[test]
    fn superscript_is_raised_and_smaller() {
        let b = layout_of("x^2");
        let x = text_pos(&b, "x").unwrap();
        let two = text_pos(&b, "2").unwrap();
        assert!(two.x > x.x);
        // Galley 上端ではなくベースラインで比較: 小さい添字の Galley は低いので補正
        let hx = text_height(&b, "x").unwrap();
        let h2 = text_height(&b, "2").unwrap();
        assert!(h2 < hx * 0.8, "script is smaller");
        assert!(two.y + h2 < x.y + hx, "baseline of 2 above baseline of x");
        // 下付きは下がる
        let s = layout_of("x_2");
        let x2 = text_pos(&s, "x").unwrap();
        let sub = text_pos(&s, "2").unwrap();
        assert!(sub.y + text_height(&s, "2").unwrap() > x2.y + hx);
    }

    #[test]
    fn combined_scripts_do_not_collide() {
        let b = layout_of("x_i^2");
        let i = text_pos(&b, "i").unwrap();
        let two = text_pos(&b, "2").unwrap();
        assert!(two.y < i.y);
        assert!((two.x - i.x).abs() < 20.0);
    }

    #[test]
    fn matrix_layout_rows_and_columns() {
        let b = layout_of(r"\begin{bmatrix} 1 & 2 \\ 3 & 4 \end{bmatrix}");
        let p1 = text_pos(&b, "1").unwrap();
        let p2 = text_pos(&b, "2").unwrap();
        let p3 = text_pos(&b, "3").unwrap();
        assert!((p1.y - p2.y).abs() < 0.5, "same row");
        assert!((p1.x - p3.x).abs() < 0.5, "same column");
        assert!(p3.y > p1.y + 10.0, "second row below");
        assert!(p2.x > p1.x + 10.0, "second column right");
    }

    #[test]
    fn display_sum_has_limits_below_and_integral_at_side() {
        let s = layout_of(r"\sum_{k}^{n} k");
        let sum = text_pos(&s, "∑").unwrap();
        let k = text_pos(&s, "k").unwrap();
        let sum_h = text_height(&s, "∑").unwrap();
        assert!(k.y > sum.y + sum_h * 0.5, "lower limit below the sum");
        let i = layout_of(r"\int_{a}^{b} f");
        let int = text_pos(&i, "∫").unwrap();
        let a = text_pos(&i, "a").unwrap();
        assert!(a.x > int.x + 2.0, "integral limit at the side");
    }

    #[test]
    fn sqrt_and_delimiters_draw_paths() {
        let s = layout_of(r"\sqrt{x+1}");
        assert!(s.items.iter().any(|i| matches!(i, Item::Line { .. })));
        let plain = layout_of("x+1");
        assert!(s.width > plain.width);
        let d = layout_of(r"\left( \frac{a}{b} \right)");
        assert!(
            d.items.iter().any(|i| matches!(i, Item::Band { .. })),
            "drawn parens"
        );
        let small = layout_of(r"\left( x \right)");
        assert!(
            text_pos(&small, "(").is_some(),
            "short parens use the glyph"
        );
    }

    #[test]
    fn unknown_command_is_rendered_in_error_color() {
        let b = layout_of(r"\nosuchcommand x");
        assert!(b.items.iter().any(
            |i| matches!(i, Item::Text { galley, color, .. } if galley.text() == r"\nosuchcommand" && *color == ERROR_COLOR)
        ));
    }

    #[test]
    fn show_display_allocates_layout_size() {
        let ctx = Context::default();
        let tex = r"x = \frac{-b \pm \sqrt{b^2-4ac}}{2a}";
        let mut size = None;
        run_frame(&ctx, |ui| {
            let r = show_display(ui, tex, 20.0, Color32::WHITE);
            size = Some(r.rect.size());
        });
        let b = layout_of(tex);
        let size = size.unwrap();
        assert_eq!(size.x, b.width.ceil());
        assert_eq!(size.y, b.height().ceil());
        assert!(size.y > 40.0);
    }

    // ---- 不正入力でパニックしないこと ----

    /// 簡易な線形合同法の乱数。
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> usize {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (self.0 >> 33) as usize
        }
    }

    #[test]
    fn fuzz_no_panic() {
        const PIECES: &[&str] = &[
            "{",
            "}",
            "{",
            "}",
            "^",
            "_",
            "&",
            r"\\",
            " ",
            "x",
            "1",
            "+",
            "=",
            "'",
            "[",
            "]",
            "(",
            ")",
            "|",
            ".",
            "-",
            "~",
            "%",
            "\\",
            r"\frac",
            r"\sqrt",
            r"\left",
            r"\right",
            r"\middle",
            r"\begin{matrix}",
            r"\end{matrix}",
            r"\begin{array}{c|l}",
            r"\end{array}",
            r"\begin{cases}",
            r"\end{cases}",
            r"\begin{aligned}",
            r"\end{foo}",
            r"\text{",
            r"\mathbb",
            r"\hat",
            r"\overbrace",
            r"\sum",
            r"\int",
            r"\lim",
            r"\limits",
            r"\over",
            r"\displaystyle",
            r"\color{red}",
            r"\not",
            r"\alpha",
            r"\unknown",
            r"\big(",
            r"\hline",
            r"\operatorname*{x}",
            r"\binom",
            r"\boxed",
            r"\substack{",
            r"\xrightarrow",
            r"\hspace{",
            r"\{",
            r"\|",
            "α",
            "日本",
            r"\,",
            r"\!",
            r"\quad",
            r"\tag{1}",
            r"\pmod",
            r"\mathcal",
            r"\phantom",
            r"\overset",
        ];
        let mut rng = Lcg(0x5eed);
        let mut inputs: Vec<String> = (0..1500)
            .map(|_| {
                let len = rng.next() % 30;
                (0..len)
                    .map(|_| PIECES[rng.next() % PIECES.len()])
                    .collect()
            })
            .collect();
        // 任意の文字列 (制御文字・サロゲート外の範囲) も混ぜる
        for _ in 0..300 {
            let len = rng.next() % 20;
            let s: String = (0..len)
                .filter_map(|_| char::from_u32((rng.next() % 0x3000) as u32))
                .collect();
            inputs.push(s);
        }
        for s in &inputs {
            let _ = to_unicode(s);
        }
        let ctx = Context::default();
        run_frame(&ctx, |ui| {
            for s in &inputs {
                let _ = show_display(ui, s, 18.0, Color32::WHITE);
            }
            let _ = show_display(ui, "x", f32::NAN, Color32::WHITE);
            let _ = show_display(ui, "x", 0.0, Color32::WHITE);
            let _ = show_display(ui, "", 1e9, Color32::WHITE);
        });
    }
}
