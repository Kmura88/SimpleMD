//! 描画命令リスト（シーン）と egui への描画。
//!
//! レイアウト結果は色を「役割」（[`Role`]）で保持するため、テーマが変わっても
//! 再レイアウトせずに描き直せる。

use egui::epaint::Mesh;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};

use super::Style;

// ==== 機能: 色の役割 ====

/// 描画時に [`Style`] から実際の色へ解決される色の役割。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    /// 透明（描かない）。
    None,
    /// 文字色。
    Text,
    /// エッジ・矢印の色。
    Line,
    /// ライフラインやフレームなどの控えめな線。
    Muted,
    /// ノードの塗り。
    NodeFill,
    /// ノードの枠線。
    NodeStroke,
    /// 強調色。
    Accent,
    /// 背景色。
    Background,
    /// サブグラフ（クラスタ）の塗り。
    ClusterFill,
    /// サブグラフ（クラスタ）の枠線。
    ClusterStroke,
    /// ノートの塗り。
    NoteFill,
    /// ノートの枠線。
    NoteStroke,
    /// エッジラベルの背景。
    LabelBg,
    /// 系列色（円グラフ等）: `(番号, 総数)`。
    Series(u16, u16),
    /// ソースで指定された固定色。
    Custom(Color32),
}

impl Role {
    /// スタイルから実際の色を求める。
    pub fn resolve(self, s: &Style) -> Color32 {
        match self {
            Role::None => Color32::TRANSPARENT,
            Role::Text => s.text,
            Role::Line => s.line,
            Role::Muted => mix(s.background, s.line, 0.55),
            Role::NodeFill => s.node_fill,
            Role::NodeStroke => s.node_stroke,
            Role::Accent => s.accent,
            Role::Background => s.background,
            Role::ClusterFill => mix(s.background, s.accent, if s.dark { 0.12 } else { 0.07 }),
            Role::ClusterStroke => mix(s.background, s.node_stroke, 0.7),
            Role::NoteFill => mix(s.background, s.accent, if s.dark { 0.25 } else { 0.15 }),
            Role::NoteStroke => mix(s.background, s.accent, 0.7),
            Role::LabelBg => mix(s.background, s.node_fill, 0.35),
            Role::Series(i, n) => series_color(s, i, n),
            Role::Custom(c) => c,
        }
    }
}

/// 2 色を `t` (0..1) の割合で混ぜる（ガンマ空間の単純な線形補間）。
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// 強調色の色相を回転させて `n` 個中 `i` 番目の系列色を作る。
fn series_color(s: &Style, i: u16, n: u16) -> Color32 {
    let base = egui::ecolor::HsvaGamma::from(s.accent);
    let n = n.max(1) as f32;
    let h = (base.h + i as f32 / n).rem_euclid(1.0);
    let sat = base.s.clamp(0.45, 0.75);
    let v = if s.dark { 0.72 } else { 0.86 };
    let v = if i % 2 == 1 { v - 0.08 } else { v };
    egui::ecolor::HsvaGamma {
        h,
        s: sat,
        v,
        a: 1.0,
    }
    .into()
}

// ==== 機能: 描画命令 ====

/// 線種。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dash {
    /// 実線。
    Solid,
    /// 破線。
    Dashed,
    /// 点線。
    Dotted,
}

/// 1 つの描画命令。座標はシーン左上を原点とするポイント単位。
#[derive(Clone, Debug)]
pub enum Item {
    /// 角丸四角形。
    Rect {
        /// 位置と大きさ。
        rect: Rect,
        /// 角の半径。
        radius: f32,
        /// 塗り。
        fill: Role,
        /// 枠線色。
        stroke: Role,
        /// 枠線の太さ。
        width: f32,
    },
    /// 円。
    Circle {
        /// 中心。
        center: Pos2,
        /// 半径。
        radius: f32,
        /// 塗り。
        fill: Role,
        /// 枠線色。
        stroke: Role,
        /// 枠線の太さ。
        width: f32,
    },
    /// 多角形。`points[0]` から全頂点が見える（星型）ことを前提に塗る。
    Polygon {
        /// 頂点列。
        points: Vec<Pos2>,
        /// 塗り。
        fill: Role,
        /// 枠線色。
        stroke: Role,
        /// 枠線の太さ。
        width: f32,
    },
    /// 折れ線。
    Path {
        /// 点列。
        points: Vec<Pos2>,
        /// 線の色。
        color: Role,
        /// 線の太さ。
        width: f32,
        /// 線種。
        dash: Dash,
        /// 閉じた線かどうか。
        closed: bool,
    },
    /// 1 行のテキスト。
    Text {
        /// 基準位置。
        pos: Pos2,
        /// 文字列。
        text: String,
        /// フォントサイズ。
        size: f32,
        /// 文字色。
        color: Role,
        /// 基準位置に対する揃え。
        anchor: Align2,
    },
}

/// レイアウト済みの図。描画命令の列と全体の大きさを持つ。
#[derive(Clone, Debug, Default)]
pub struct Scene {
    /// 図全体の大きさ（ポイント）。
    pub size: Vec2,
    /// 描画命令（先頭から順に描く）。
    pub items: Vec<Item>,
}

// ==== 機能: egui への描画 ====

impl Scene {
    /// `origin` を左上として egui の Painter に描画する。
    pub fn paint(&self, painter: &Painter, origin: Pos2, style: &Style) {
        let off = origin.to_vec2();
        for item in &self.items {
            paint_item(painter, item, off, style);
        }
    }

    /// すべての描画命令を囲む矩形（テスト・デバッグ用）。
    pub fn bounds(&self) -> Rect {
        let mut r = Rect::NOTHING;
        for item in &self.items {
            match item {
                Item::Rect { rect, .. } => r = r.union(*rect),
                Item::Circle { center, radius, .. } => {
                    r = r.union(Rect::from_center_size(*center, Vec2::splat(radius * 2.0)))
                }
                Item::Polygon { points, .. } | Item::Path { points, .. } => {
                    for p in points {
                        r.extend_with(*p);
                    }
                }
                Item::Text { pos, .. } => r.extend_with(*pos),
            }
        }
        r
    }
}

/// 描画命令列をまとめて平行移動する。
pub fn translate_items(items: &mut [Item], d: Vec2) {
    for item in items {
        match item {
            Item::Rect { rect, .. } => *rect = rect.translate(d),
            Item::Circle { center, .. } => *center += d,
            Item::Polygon { points, .. } | Item::Path { points, .. } => {
                points.iter_mut().for_each(|p| *p += d)
            }
            Item::Text { pos, .. } => *pos += d,
        }
    }
}

/// 描画命令 1 つを描く。
fn paint_item(painter: &Painter, item: &Item, off: Vec2, style: &Style) {
    match item {
        Item::Rect {
            rect,
            radius,
            fill,
            stroke,
            width,
        } => {
            painter.rect(
                rect.translate(off),
                *radius,
                fill.resolve(style),
                Stroke::new(*width, stroke.resolve(style)),
                StrokeKind::Middle,
            );
        }
        Item::Circle {
            center,
            radius,
            fill,
            stroke,
            width,
        } => {
            painter.circle(
                *center + off,
                *radius,
                fill.resolve(style),
                Stroke::new(*width, stroke.resolve(style)),
            );
        }
        Item::Polygon {
            points,
            fill,
            stroke,
            width,
        } => {
            if points.len() < 3 {
                return;
            }
            let pts: Vec<Pos2> = points.iter().map(|p| *p + off).collect();
            let fill = fill.resolve(style);
            let stroke = Stroke::new(*width, stroke.resolve(style));
            if is_convex(&pts) {
                painter.add(Shape::convex_polygon(pts, fill, stroke));
            } else {
                let mut mesh = Mesh::default();
                for p in &pts {
                    mesh.colored_vertex(*p, fill);
                }
                for i in 1..pts.len() as u32 - 1 {
                    mesh.add_triangle(0, i, i + 1);
                }
                painter.add(Shape::mesh(mesh));
                painter.add(Shape::closed_line(pts, stroke));
            }
        }
        Item::Path {
            points,
            color,
            width,
            dash,
            closed,
        } => {
            if points.len() < 2 {
                return;
            }
            let mut pts: Vec<Pos2> = points.iter().map(|p| *p + off).collect();
            let stroke = Stroke::new(*width, color.resolve(style));
            match dash {
                Dash::Solid if *closed => {
                    painter.add(Shape::closed_line(pts, stroke));
                }
                Dash::Solid => {
                    painter.add(Shape::line(pts, stroke));
                }
                Dash::Dashed | Dash::Dotted => {
                    if *closed {
                        pts.push(pts[0]);
                    }
                    let (d, g) = if *dash == Dash::Dashed {
                        (5.0, 3.5)
                    } else {
                        (2.0, 2.5)
                    };
                    painter.extend(Shape::dashed_line(&pts, stroke, d, g));
                }
            }
        }
        Item::Text {
            pos,
            text,
            size,
            color,
            anchor,
        } => {
            painter.text(
                *pos + off,
                *anchor,
                text,
                FontId::proportional(*size),
                color.resolve(style),
            );
        }
    }
}

/// 多角形が凸かどうか（向きはどちらでもよい）。
fn is_convex(pts: &[Pos2]) -> bool {
    let n = pts.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let c = pts[(i + 2) % n];
        let cross = (b - a).x * (c - b).y - (b - a).y * (c - b).x;
        if cross.abs() < 1e-4 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}
