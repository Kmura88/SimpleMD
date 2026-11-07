//! Mermaid 図レンダラ（純 Rust・egui の Painter で描画）
//!
//! Markdown 中の ```` ```mermaid ```` ブロックを解析し、egui 上に図として描く。
//! 外部プロセスや JavaScript には依存しない。
//!
//! # 対応している図と構文
//!
//! ## flowchart / graph
//! - 向き: `TD` `TB` `BT` `LR` `RL`（省略時は `TD`）
//! - ノード形状: `A[四角]` `A(角丸)` `A([スタジアム])` `A[[サブルーチン]]` `A[(円柱)]`
//!   `A((円))` `A(((二重円)))` `A>旗形]` `A{ひし形}` `A{{六角形}}` `A[/平行四辺形/]`
//!   `A[\逆平行四辺形\]` `A[/台形\]` `A[\逆台形/]`
//! - ラベル: `"..."` による引用、`<br>` 改行、簡単な HTML タグと `#quot;` 等の実体参照
//! - エッジ: `-->` `---` `-.->` `-.-` `==>` `===` `--o` `--x` `<-->` `~~~`（不可視）
//!   と長い矢印 `--->`、ラベル `-->|text|` `-- text -->` `-. text .->` `== text ==>`
//! - 連鎖 `A --> B --> C`、`A & B --> C & D`
//! - `subgraph id [タイトル] ... end`（入れ子可）、`direction`（無視）
//! - `classDef` / `class` / `style` / `:::cls` の fill・stroke・color、`linkStyle` の stroke
//! - `click` 等の未対応文は無視、`;` 区切り、`%%` コメント
//!
//! ## sequenceDiagram
//! - `participant A as Alice` / `actor B`、暗黙の参加者、`title`
//! - メッセージ `->` `-->` `->>` `-->>` `-x` `--x` `-)` `--)` `<<->>` `<<-->>`、自己メッセージ
//! - `+` / `-` による活性化、`activate` / `deactivate`
//! - `autonumber`、`Note left of / right of / over A,B: text`
//! - `loop` `alt`/`else` `opt` `par`/`and` `critical`/`option` `break` `rect` ブロック、`box`（無視）
//!
//! ## pie
//! - `pie [showData] [title ...]`、`title ...` 行、`"ラベル" : 値`
//!
//! ## stateDiagram / stateDiagram-v2
//! - `[*]` 開始/終了、`A --> B : label`、`state "説明" as X`、`X : 説明`
//! - `state X { ... }`（複合状態）、`<<choice>>` `<<fork>>` `<<join>>`、`direction`
//! - `note ... end note` と `--` 区切りは無視
//!
//! ## classDiagram
//! - `class Name { 属性 / メソッド() }`、`Name : +member`、`<<interface>>` 注釈、`~T~` ジェネリクス
//! - 関係 `<|--` `*--` `o--` `-->` `--` `..>` `..|>` `..`（両端の向きも可）、多重度 `"1"`、`: label`
//! - `namespace X { ... }`
//!
//! ## erDiagram
//! - `A ||--o{ B : label`（`|o` `||` `}o` `}|` / `o|` `||` `o{` `|{`、`--` と `..`）
//! - 属性ブロック `A { type name PK "comment" }`
//!
//! # 構成
//! - `text`: ラベル整形・前処理・計測
//! - `scene`: 描画命令リスト（[`Scene`]）と egui への描画
//! - `graph` / `layered` / `graph_draw`: グラフ系の図の共通モデル・階層レイアウト・描画
//! - `flowchart` / `state` / `class` / `er`: 各図のパーサ（→ グラフモデル）
//! - `sequence` / `pie`: 独自レイアウトの図

mod class;
mod er;
mod flowchart;
mod graph;
mod graph_draw;
mod layered;
mod pie;
mod scene;
mod sequence;
mod state;
mod text;

#[cfg(test)]
mod tests;

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{Color32, FontId, Sense, Vec2};

pub use scene::{Dash, Item, Role, Scene};

// ==== 機能: 公開型（スタイル） ====

/// ホストアプリのテーマから渡される色とサイズ。
#[derive(Clone, Debug)]
pub struct Style {
    /// 基本フォントサイズ（ポイント）。
    pub font_size: f32,
    /// 文字色。
    pub text: egui::Color32,
    /// エッジ・ライフラインの色。
    pub line: egui::Color32,
    /// ノードの塗り色。
    pub node_fill: egui::Color32,
    /// ノードの枠線色。
    pub node_stroke: egui::Color32,
    /// 強調色（ハイライト・円グラフの基準色・ノート）。
    pub accent: egui::Color32,
    /// 背景色。
    pub background: egui::Color32,
    /// ダークテーマかどうか。
    pub dark: bool,
}

impl Default for Style {
    /// ライトテーマ向けの既定値。
    fn default() -> Self {
        Self {
            font_size: 14.0,
            text: Color32::from_rgb(0x22, 0x22, 0x22),
            line: Color32::from_rgb(0x55, 0x5a, 0x66),
            node_fill: Color32::from_rgb(0xec, 0xee, 0xff),
            node_stroke: Color32::from_rgb(0x93, 0x70, 0xdb),
            accent: Color32::from_rgb(0x4c, 0x7d, 0xd8),
            background: Color32::WHITE,
            dark: false,
        }
    }
}

// ==== 機能: 公開 API（表示・レイアウト） ====

/// Mermaid 図を解析し、自然な大きさで描画する（横スクロールは呼び出し側で包む）。
///
/// 未対応の図種や構文エラーは `Err(メッセージ)` を返す。呼び出し側はソース表示に戻す想定。
/// 解析・レイアウト結果は `ui.ctx()` の一時データにキャッシュされる。
pub fn show(ui: &mut egui::Ui, src: &str, style: &Style) -> Result<egui::Response, String> {
    let scene = layout(ui.ctx(), src, style.font_size)?;
    let (rect, response) = ui.allocate_exact_size(scene.size, Sense::hover());
    if ui.is_rect_visible(rect) {
        scene.paint(ui.painter(), rect.min, style);
    }
    Ok(response)
}

/// 解析とレイアウトを行い、結果の [`Scene`] を返す（`ctx` の一時データにキャッシュ）。
///
/// キーはソース・フォントサイズ・ピクセル密度のハッシュ。色はキャッシュに含まれないため、
/// テーマ切り替えでは再レイアウトされない。
pub fn layout(ctx: &egui::Context, src: &str, font_size: f32) -> Result<Arc<Scene>, String> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut hasher);
    font_size.to_bits().hash(&mut hasher);
    ctx.pixels_per_point().to_bits().hash(&mut hasher);
    let id = egui::Id::unique(("mermaid_scene", hasher.finish()));

    type Cached = Arc<Result<Arc<Scene>, String>>;
    if let Some(hit) = ctx.data(|d| d.get_temp::<Cached>(id)) {
        return (*hit).clone();
    }
    let mut measure = |s: &str, size: f32| -> Vec2 {
        let s = if s.is_empty() { " " } else { s };
        ctx.fonts_mut(|f| {
            f.layout_no_wrap(s.to_owned(), FontId::proportional(size), Color32::WHITE)
                .size()
        })
    };
    let result = build_scene(src, font_size, &mut measure).map(Arc::new);
    ctx.data_mut(|d| d.insert_temp::<Cached>(id, Arc::new(result.clone())));
    result
}

/// 解析とレイアウトを行う純粋関数（キャッシュなし）。
///
/// `measure(text, font_size)` は 1 行のテキストの大きさを返す関数。テストでは
/// 簡易な計測関数を渡せる。
pub fn build_scene(
    src: &str,
    font_size: f32,
    measure: &mut dyn FnMut(&str, f32) -> Vec2,
) -> Result<Scene, String> {
    let font_size = if font_size.is_finite() {
        font_size.clamp(4.0, 200.0)
    } else {
        14.0
    };
    let lines = text::preprocess(src);
    let Some((_, header)) = lines.first() else {
        return Err("空の Mermaid 図です".into());
    };
    let kind = header.split_whitespace().next().unwrap_or("");
    let kind = kind.trim_end_matches(';');
    let mut m = text::Measurer::new(measure, font_size);
    match kind {
        "flowchart" | "graph" | "flowchart-elk" => {
            let g = flowchart::parse(&lines)?;
            Ok(graph_draw::draw(&g, &mut m))
        }
        "stateDiagram" | "stateDiagram-v2" => {
            let g = state::parse(&lines)?;
            Ok(graph_draw::draw(&g, &mut m))
        }
        "classDiagram" | "classDiagram-v2" => {
            let g = class::parse(&lines)?;
            Ok(graph_draw::draw(&g, &mut m))
        }
        "erDiagram" => {
            let g = er::parse(&lines)?;
            Ok(graph_draw::draw(&g, &mut m))
        }
        "sequenceDiagram" => {
            let d = sequence::parse(&lines)?;
            Ok(sequence::draw(&d, &mut m))
        }
        "pie" => {
            let d = pie::parse(&lines)?;
            Ok(pie::draw(&d, &mut m))
        }
        other => Err(format!("未対応の Mermaid 図の種類です: {other}")),
    }
}

/// ソースの図種が対応済みかどうかを（解析せずに）判定する。
pub fn is_supported(src: &str) -> bool {
    let lines = text::preprocess(src);
    let kind = lines
        .first()
        .and_then(|(_, l)| l.split_whitespace().next())
        .unwrap_or("")
        .trim_end_matches(';');
    matches!(
        kind,
        "flowchart"
            | "graph"
            | "flowchart-elk"
            | "stateDiagram"
            | "stateDiagram-v2"
            | "classDiagram"
            | "classDiagram-v2"
            | "erDiagram"
            | "sequenceDiagram"
            | "pie"
    )
}
