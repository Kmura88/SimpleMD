//! # 機能: Markdown の解析（描画用の文書モデルへの変換）
//!
//! - `ast.rs`: 描画にそのまま使える文書モデル（ブロック・行内要素・見出し・脚注）
//! - `parse.rs`: pulldown-cmark のイベント列から文書モデルを組み立てる解析処理

pub mod ast;
pub mod parse;

pub use ast::*;
pub use parse::{WIKI_PREFIX, parse, slugify};
