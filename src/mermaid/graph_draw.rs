//! グラフ系の図の描画: ノード寸法の計算 → 階層レイアウト → 描画命令の生成。

use egui::{Align2, Pos2, Rect, Vec2, pos2, vec2};

use super::graph::{Dir, EdgeStroke, Graph, Marker, NodeKind, Shape};
use super::layered::{self, LCluster, LEdge, LNode, Params};
use super::scene::{Dash, Item, Role, Scene};
use super::text::{Measurer, TextBlock};

// ==== 機能: ノードの寸法 ====

/// 表（クラス・エンティティ）ノードの寸法情報。
struct TableGeom {
    /// 見出し。
    header: TextBlock,
    /// 見出し部の高さ。
    header_h: f32,
    /// 区画 → 行 → セル（文字列, 大きさ）。
    sections: Vec<Vec<Vec<(String, Vec2)>>>,
    /// 列の幅。
    col_w: Vec<f32>,
    /// 1 行の高さ。
    row_h: f32,
}

/// ノードの描画寸法（画面座標系）。
struct NodeGeom {
    /// 外形の大きさ。
    size: Vec2,
    /// ラベル。
    text: TextBlock,
    /// 表ノードの場合の詳細。
    table: Option<TableGeom>,
}

/// ラベルの大きさから形状ごとの外形サイズを求める。
fn shape_size(shape: Shape, t: Vec2, f: f32, horizontal: bool) -> Vec2 {
    let (px, py) = (f * 1.0, f * 0.6);
    let h = t.y + 2.0 * py;
    match shape {
        Shape::Rect | Shape::Round => vec2(t.x + 2.0 * px, h),
        Shape::Subroutine => vec2(t.x + 3.0 * px, h),
        Shape::Stadium => vec2(t.x + h + px * 0.5, h),
        Shape::Cylinder => {
            let w = t.x + 2.0 * px;
            vec2(w, h + 2.0 * cylinder_ry(w, f))
        }
        Shape::Circle => {
            let d = (t.x * t.x + t.y * t.y).sqrt() + py;
            vec2(d, d)
        }
        Shape::DoubleCircle => {
            let d = (t.x * t.x + t.y * t.y).sqrt() + py + f * 0.6;
            vec2(d, d)
        }
        Shape::Diamond => {
            let (iw, ih) = (t.x + px, t.y + py);
            let w = iw + ih * 1.6;
            let h = ih * w / (w - iw).max(1.0);
            vec2(w, h.max(ih * 1.6))
        }
        Shape::Hexagon => vec2(t.x + 2.0 * px + h / 2.0, h),
        Shape::Parallelogram | Shape::ParallelogramAlt | Shape::Trapezoid | Shape::TrapezoidAlt => {
            vec2(t.x + 2.0 * px + h * 0.5, h)
        }
        Shape::Asymmetric => vec2(t.x + 2.0 * px + h * 0.3, h),
        Shape::StateStart => vec2(f * 1.1, f * 1.1),
        Shape::StateEnd => vec2(f * 1.3, f * 1.3),
        Shape::ForkBar if horizontal => vec2(f * 0.5, f * 4.5),
        Shape::ForkBar => vec2(f * 4.5, f * 0.5),
        Shape::Choice => vec2(f * 1.8, f * 1.8),
    }
}

/// 円柱の上下楕円の縦半径。
fn cylinder_ry(w: f32, f: f32) -> f32 {
    (w * 0.08).min(f * 0.6).max(1.0)
}

/// ラベルを描かない形状か。
fn is_marker_shape(shape: Shape) -> bool {
    matches!(
        shape,
        Shape::StateStart | Shape::StateEnd | Shape::ForkBar | Shape::Choice
    )
}

/// ノード 1 つの寸法を計算する。
fn node_geom(kind: &NodeKind, label: &[String], m: &mut Measurer, horizontal: bool) -> NodeGeom {
    let f = m.font;
    match kind {
        NodeKind::Shape(shape) => {
            let text = if is_marker_shape(*shape) {
                TextBlock::default()
            } else {
                m.text(label)
            };
            let t = if text.is_empty() {
                vec2(f, f * 1.2)
            } else {
                text.size
            };
            NodeGeom {
                size: shape_size(*shape, t, f, horizontal),
                text,
                table: None,
            }
        }
        NodeKind::Table { header, sections } => {
            let header = m.text(header);
            let row_h = m.line("Ag", f).y;
            let mut col_w: Vec<f32> = Vec::new();
            let sections: Vec<Vec<Vec<(String, Vec2)>>> = sections
                .iter()
                .map(|rows| {
                    rows.iter()
                        .map(|cells| {
                            cells
                                .iter()
                                .enumerate()
                                .map(|(ci, c)| {
                                    let s = m.line(c, f);
                                    if col_w.len() <= ci {
                                        col_w.resize(ci + 1, 0.0);
                                    }
                                    col_w[ci] = col_w[ci].max(s.x);
                                    (c.clone(), s)
                                })
                                .collect()
                        })
                        .collect()
                })
                .collect();
            let gap = f * 0.9;
            let cols_w: f32 =
                col_w.iter().sum::<f32>() + gap * col_w.len().saturating_sub(1) as f32;
            let w = header.size.x.max(cols_w).max(f * 4.0) + 2.0 * f * 0.8;
            let header_h = header.size.y + f * 0.9;
            let body_h: f32 = sections
                .iter()
                .map(|rows| rows.len().max(1) as f32 * row_h + f * 0.6)
                .sum();
            NodeGeom {
                size: vec2(w, header_h + body_h),
                text: TextBlock::default(),
                table: Some(TableGeom {
                    header,
                    header_h,
                    sections,
                    col_w,
                    row_h,
                }),
            }
        }
    }
}

// ==== 機能: 座標変換（抽象空間 → 画面） ====

/// レイアウトの抽象空間（x: 交差軸, y: ランク軸）から画面座標への変換。
#[derive(Clone, Copy)]
struct Transform {
    /// 図の向き。
    dir: Dir,
    /// 抽象空間での全体サイズ。
    size: Vec2,
}

impl Transform {
    /// 点を変換する。
    fn pos(&self, p: Pos2) -> Pos2 {
        match self.dir {
            Dir::TD => p,
            Dir::BT => pos2(p.x, self.size.y - p.y),
            Dir::LR => pos2(p.y, p.x),
            Dir::RL => pos2(self.size.y - p.y, p.x),
        }
    }

    /// ランク軸の正方向の単位ベクトル（画面座標）。
    fn axis(&self) -> Vec2 {
        match self.dir {
            Dir::TD => vec2(0.0, 1.0),
            Dir::BT => vec2(0.0, -1.0),
            Dir::LR => vec2(1.0, 0.0),
            Dir::RL => vec2(-1.0, 0.0),
        }
    }

    /// 矩形を変換する。
    fn rect(&self, r: Rect) -> Rect {
        Rect::from_two_pos(self.pos(r.min), self.pos(r.max))
    }

    /// 画面サイズ。
    fn screen_size(&self) -> Vec2 {
        if self.dir.horizontal() {
            vec2(self.size.y, self.size.x)
        } else {
            self.size
        }
    }

    /// 画面サイズを抽象空間のサイズへ変換する。
    fn to_layout(dir: Dir, v: Vec2) -> Vec2 {
        if dir.horizontal() { vec2(v.y, v.x) } else { v }
    }
}

// ==== 機能: 図全体の描画 ====

/// レイアウト済みのグラフ（画面座標の矩形を含む）。
pub struct Computed {
    /// 抽象空間 → 画面の変換。
    tf: Transform,
    /// 抽象空間でのレイアウト結果。
    lay: layered::Layout,
    /// ノード寸法。
    geoms: Vec<NodeGeom>,
    /// エッジラベル。
    edge_labels: Vec<TextBlock>,
    /// クラスタのタイトル。
    titles: Vec<TextBlock>,
    /// ノードの画面上の矩形。
    pub node_rects: Vec<Rect>,
    /// クラスタの画面上の矩形。
    pub cluster_rects: Vec<Option<Rect>>,
    /// エッジラベル中心の画面座標。
    pub label_centers: Vec<Option<Pos2>>,
    /// エッジラベルの背景を含む大きさ。
    pub label_sizes: Vec<Vec2>,
    /// 図全体の画面サイズ。
    pub size: Vec2,
}

/// エッジラベル背景の内側余白。
fn label_pad(f: f32) -> Vec2 {
    vec2(f * 0.4, f * 0.2)
}

/// ノード寸法の計算とレイアウトを行う。
pub fn compute(g: &Graph, m: &mut Measurer) -> Computed {
    let f = m.font;
    let horizontal = g.dir.horizontal();
    let geoms: Vec<NodeGeom> = g
        .nodes
        .iter()
        .map(|n| node_geom(&n.kind, &n.label, m, horizontal))
        .collect();
    let label_pad = label_pad(f);
    let edge_labels: Vec<TextBlock> = g.edges.iter().map(|e| m.text(&e.label)).collect();
    let loop_w = f * 2.2;

    // ---- レイアウト入力の構築 ----
    let mut lnodes: Vec<LNode> = geoms
        .iter()
        .zip(&g.nodes)
        .map(|(geom, n)| LNode {
            size: Transform::to_layout(g.dir, geom.size),
            cluster: n.cluster,
        })
        .collect();
    for (e, lbl) in g.edges.iter().zip(&edge_labels) {
        if e.from == e.to && e.from < lnodes.len() {
            // 自己ループの分だけ交差軸方向に余白を確保する
            let extra = loop_w + if horizontal { lbl.size.y } else { lbl.size.x } + f * 0.5;
            lnodes[e.from].size.x += extra * 2.0;
        }
    }
    let ledges: Vec<LEdge> = g
        .edges
        .iter()
        .zip(&edge_labels)
        .map(|(e, lbl)| LEdge {
            from: e.from,
            to: e.to,
            min_len: e.min_len,
            label: (!lbl.is_empty())
                .then(|| Transform::to_layout(g.dir, lbl.size + label_pad * 2.0)),
        })
        .collect();
    let titles: Vec<TextBlock> = g.clusters.iter().map(|c| m.text(&c.title)).collect();
    let lclusters: Vec<LCluster> = g
        .clusters
        .iter()
        .zip(&titles)
        .map(|(c, t)| {
            let th = if t.is_empty() {
                0.0
            } else {
                t.size.y + f * 0.3
            };
            let tw = t.size.x + f * 1.5;
            let mut lc = LCluster {
                parent: c.parent,
                ..Default::default()
            };
            match g.dir {
                Dir::TD => lc.pad_lo.y = th,
                Dir::BT => lc.pad_hi.y = th,
                Dir::LR | Dir::RL => lc.pad_lo.x = th,
            }
            lc.min_size = if horizontal {
                vec2(0.0, tw)
            } else {
                vec2(tw, 0.0)
            };
            lc
        })
        .collect();
    let params = Params {
        node_sep: f * 2.4,
        rank_sep: f * 3.0,
        cluster_pad: f * 0.9,
        margin: f * 0.6,
    };
    let lay = layered::layout(&lnodes, &ledges, &lclusters, &params);
    let tf = Transform {
        dir: g.dir,
        size: lay.size,
    };
    let node_rects: Vec<Rect> = lay
        .nodes
        .iter()
        .zip(&geoms)
        .map(|(p, geom)| Rect::from_center_size(tf.pos(*p), geom.size))
        .collect();
    let cluster_rects = lay.clusters.iter().map(|r| r.map(|r| tf.rect(r))).collect();
    let label_centers = lay
        .edges
        .iter()
        .map(|e| e.label.map(|p| tf.pos(p)))
        .collect();
    let label_sizes = edge_labels
        .iter()
        .map(|l| l.size + label_pad * 2.0)
        .collect();
    Computed {
        size: tf.screen_size(),
        tf,
        lay,
        geoms,
        edge_labels,
        titles,
        node_rects,
        cluster_rects,
        label_centers,
        label_sizes,
    }
}

/// グラフをレイアウトしてシーンを作る。
pub fn draw(g: &Graph, m: &mut Measurer) -> Scene {
    let f = m.font;
    let horizontal = g.dir.horizontal();
    let c = compute(g, m);
    let Computed {
        tf,
        lay,
        geoms,
        edge_labels,
        titles,
        node_rects,
        cluster_rects,
        label_centers,
        label_sizes,
        size,
    } = c;

    let mut items = Vec::new();

    // ---- クラスタ（外側から） ----
    let mut corder: Vec<usize> = (0..g.clusters.len()).collect();
    let depth = |mut c: usize| {
        let mut d = 0;
        while let Some(p) = g.clusters.get(c).and_then(|c| c.parent) {
            d += 1;
            c = p;
            if d > g.clusters.len() {
                break;
            }
        }
        d
    };
    corder.sort_by_key(|&c| depth(c));
    for c in corder {
        let Some(Some(r)) = cluster_rects.get(c).copied() else {
            continue;
        };
        items.push(Item::Rect {
            rect: r,
            radius: f * 0.3,
            fill: Role::ClusterFill,
            stroke: Role::ClusterStroke,
            width: 1.0,
        });
        let t = &titles[c];
        if !t.is_empty() {
            let y = if g.dir == Dir::BT {
                r.bottom() - f * 0.3 - t.size.y
            } else {
                r.top() + f * 0.3
            };
            t.emit(
                &mut items,
                pos2(r.center().x, y),
                Align2::CENTER_TOP,
                Role::Text,
            );
        }
    }

    // ---- エッジ ----
    let offsets = port_offsets(g, &lay, &geoms, horizontal, f);
    let mut labels = Vec::new();
    for (i, e) in g.edges.iter().enumerate() {
        if e.stroke == EdgeStroke::Invisible || e.from >= g.nodes.len() || e.to >= g.nodes.len() {
            continue;
        }
        let route = &lay.edges[i];
        let color = e.color.map_or(Role::Line, Role::Custom);
        let lbl = &edge_labels[i];
        if route.self_loop {
            let pos = draw_self_loop(&mut items, node_rects[e.from], e, color, horizontal, f);
            if !lbl.is_empty() {
                let anchor = if horizontal {
                    Align2::CENTER_TOP
                } else {
                    Align2::LEFT_CENTER
                };
                labels.push((lbl, label_sizes[i], pos, anchor));
            }
            continue;
        }
        let ends = EdgeEnds {
            from: (
                lay.nodes[e.from],
                node_kind_shape(&g.nodes[e.from].kind),
                geoms[e.from].size,
            ),
            to: (
                lay.nodes[e.to],
                node_kind_shape(&g.nodes[e.to].kind),
                geoms[e.to].size,
            ),
            offsets: offsets[i],
        };
        draw_edge(&mut items, &tf, e, &route.points, &ends, color, f);
        if let Some(lp) = label_centers[i]
            && !lbl.is_empty()
        {
            labels.push((lbl, label_sizes[i], lp, Align2::CENTER_CENTER));
        }
    }
    for (lbl, lsize, pos, anchor) in labels {
        let r = anchor.anchor_size(pos, lsize);
        items.push(Item::Rect {
            rect: r,
            radius: f * 0.2,
            fill: Role::LabelBg,
            stroke: Role::None,
            width: 0.0,
        });
        lbl.emit_centered(&mut items, r.center(), Role::Text);
    }

    // ---- ノード ----
    for (i, n) in g.nodes.iter().enumerate() {
        let r = node_rects[i];
        let fill = n.style.fill.map_or(Role::NodeFill, Role::Custom);
        let stroke = n.style.stroke.map_or(Role::NodeStroke, Role::Custom);
        let text = n.style.text.map_or(Role::Text, Role::Custom);
        match &n.kind {
            NodeKind::Shape(shape) => {
                draw_shape(&mut items, *shape, r, fill, stroke, f);
                let mut c = r.center();
                if *shape == Shape::Cylinder {
                    c.y += cylinder_ry(r.width(), f) * 0.5;
                }
                geoms[i].text.emit_centered(&mut items, c, text);
            }
            NodeKind::Table { .. } => {
                if let Some(t) = &geoms[i].table {
                    draw_table(&mut items, t, r, fill, stroke, text, f);
                }
            }
        }
    }

    Scene { size, items }
}

/// ノード種別から形状を得る（表は四角形扱い）。
fn node_kind_shape(k: &NodeKind) -> Shape {
    match k {
        NodeKind::Shape(s) => *s,
        NodeKind::Table { .. } => Shape::Rect,
    }
}

// ==== 機能: エッジの描画 ====

/// エッジ両端のノード情報（抽象空間の中心, 形状, 画面サイズ）。
struct EdgeEnds {
    /// 始点側。
    from: (Pos2, Shape, Vec2),
    /// 終点側。
    to: (Pos2, Shape, Vec2),
    /// 始点・終点のポートの交差軸方向のずれ。
    offsets: (f32, f32),
}

/// ポートをずらしてよい（辺が平らな）形状か。
fn spreadable(shape: Shape) -> bool {
    matches!(
        shape,
        Shape::Rect | Shape::Round | Shape::Subroutine | Shape::Cylinder | Shape::Stadium
    )
}

/// 同じノードの同じ辺に複数のエッジが接続する場合、ポートを辺に沿って分散させる。
/// 戻り値はエッジごとの (始点側, 終点側) の交差軸方向のずれ（抽象空間）。
fn port_offsets(
    g: &Graph,
    lay: &layered::Layout,
    geoms: &[NodeGeom],
    horizontal: bool,
    f: f32,
) -> Vec<(f32, f32)> {
    use std::collections::HashMap;
    let mut out = vec![(0.0f32, 0.0f32); g.edges.len()];
    // (ノード, 辺の向き) → [(相手側の交差軸座標, エッジ番号, 始点側か)]
    /// (相手側の交差軸座標, エッジ番号, 始点側か)
    type PortList = Vec<(f32, usize, bool)>;
    let mut groups: HashMap<(usize, bool), PortList> = HashMap::new();
    for (i, e) in g.edges.iter().enumerate() {
        if e.from == e.to || e.from >= lay.nodes.len() || e.to >= lay.nodes.len() {
            continue;
        }
        let Some(route) = lay.edges.get(i) else {
            continue;
        };
        let (fc, tc) = (lay.nodes[e.from], lay.nodes[e.to]);
        let first = route.points.first().copied().unwrap_or(tc);
        let last = route.points.last().copied().unwrap_or(fc);
        groups
            .entry((e.from, first.y > fc.y))
            .or_default()
            .push((first.x, i, true));
        groups
            .entry((e.to, last.y > tc.y))
            .or_default()
            .push((last.x, i, false));
    }
    for ((node, _), mut list) in groups {
        let shape = node_kind_shape(&g.nodes[node].kind);
        let is_table = matches!(g.nodes[node].kind, NodeKind::Table { .. });
        if list.len() < 2 || !(spreadable(shape) || is_table) {
            continue;
        }
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        let size = geoms[node].size;
        let side = if horizontal { size.y } else { size.x };
        let n = list.len() as f32;
        let step = (side * 0.7 / (n - 1.0)).min(f * 1.2);
        for (k, (_, i, is_start)) in list.into_iter().enumerate() {
            let off = (k as f32 - (n - 1.0) / 2.0) * step;
            if is_start {
                out[i].0 = off;
            } else {
                out[i].1 = off;
            }
        }
    }
    out
}

/// 画面上でノード境界がポート位置（外接矩形の辺の中点）から内側にある量。
fn port_inset(shape: Shape, size: Vec2, horizontal: bool, toward_positive: bool) -> f32 {
    if !horizontal {
        return 0.0;
    }
    let h = size.y;
    match shape {
        Shape::Parallelogram | Shape::ParallelogramAlt | Shape::Trapezoid | Shape::TrapezoidAlt => {
            h * 0.25
        }
        Shape::Asymmetric if !toward_positive => h * 0.3,
        _ => 0.0,
    }
}

/// 3 次ベジェ曲線上の点。
fn cubic(p0: Pos2, c1: Pos2, c2: Pos2, p1: Pos2, t: f32) -> Pos2 {
    let u = 1.0 - t;
    let v = p0.to_vec2() * (u * u * u)
        + c1.to_vec2() * (3.0 * u * u * t)
        + c2.to_vec2() * (3.0 * u * t * t)
        + p1.to_vec2() * (t * t * t);
    v.to_pos2()
}

/// 点列を滑らかな曲線（エルミート補間による 3 次ベジェ）で結んだ折れ線を返す。
///
/// 両端の接線はランク軸方向（ノードに垂直に出入りする）、中間点の接線は
/// 前後の点を結ぶ向き（Catmull-Rom）とする。
fn smooth_path(pts: &[Pos2]) -> Vec<Pos2> {
    let n = pts.len();
    if n < 2 {
        return pts.to_vec();
    }
    let tangent = |i: usize| -> Vec2 {
        if i == 0 {
            vec2(0.0, pts[1].y - pts[0].y)
        } else if i == n - 1 {
            vec2(0.0, pts[n - 1].y - pts[n - 2].y)
        } else {
            (pts[i + 1] - pts[i - 1]) * 0.5
        }
    };
    let mut out = vec![pts[0]];
    for i in 0..n - 1 {
        let (p0, p1) = (pts[i], pts[i + 1]);
        let c1 = p0 + tangent(i) / 3.0 * if i == 0 { 1.5 } else { 1.0 };
        let c2 = p1 - tangent(i + 1) / 3.0 * if i + 1 == n - 1 { 1.5 } else { 1.0 };
        let steps = 12;
        for k in 1..=steps {
            out.push(cubic(p0, c1, c2, p1, k as f32 / steps as f32));
        }
    }
    out
}

/// エッジを、ランク軸方向に接線を持つ滑らかな曲線として描く（両端に記号）。
fn draw_edge(
    items: &mut Vec<Item>,
    tf: &Transform,
    e: &super::graph::Edge,
    mids: &[Pos2],
    ends: &EdgeEnds,
    color: Role,
    f: f32,
) {
    let horizontal = tf.dir.horizontal();
    let half_rank = |s: Vec2| if horizontal { s.x / 2.0 } else { s.y / 2.0 };
    let (fc, fshape, fsize) = ends.from;
    let (tc, tshape, tsize) = ends.to;
    let first = mids.first().copied().unwrap_or(tc);
    let last = mids.last().copied().unwrap_or(fc);
    let s_out = (first.y - fc.y).signum();
    let s_in = (tc.y - last.y).signum();
    let s_out = if s_out == 0.0 { 1.0 } else { s_out };
    let s_in = if s_in == 0.0 { 1.0 } else { s_in };
    // 画面上で正方向（右/下）へ出るかどうかでインセットを決める
    let screen_pos = |s: f32| (tf.axis().x + tf.axis().y) * s > 0.0;
    let start = fc
        + vec2(
            ends.offsets.0,
            s_out * (half_rank(fsize) - port_inset(fshape, fsize, horizontal, screen_pos(s_out))),
        );
    let end = tc
        - vec2(
            -ends.offsets.1,
            s_in * (half_rank(tsize) - port_inset(tshape, tsize, horizontal, !screen_pos(s_in))),
        );
    let start_cut = marker_shorten(e.start, f);
    let end_cut = marker_shorten(e.end, f);
    let mut pts = Vec::with_capacity(mids.len() + 2);
    pts.push(start + vec2(0.0, s_out * start_cut));
    pts.extend_from_slice(mids);
    pts.push(end - vec2(0.0, s_in * end_cut));

    let line: Vec<Pos2> = smooth_path(&pts).into_iter().map(|p| tf.pos(p)).collect();
    let (width, dash) = match e.stroke {
        EdgeStroke::Thick => (f * 0.22, Dash::Solid),
        EdgeStroke::Dotted => (f * 0.1, Dash::Dotted),
        _ => (f * 0.1, Dash::Solid),
    };
    let width = width.max(1.0);
    items.push(Item::Path {
        points: line,
        color,
        width,
        dash,
        closed: false,
    });
    let axis = tf.axis();
    draw_marker(items, e.end, tf.pos(end), axis * s_in, color, f);
    draw_marker(items, e.start, tf.pos(start), -axis * s_out, color, f);

    // 多重度ラベル（ノード付近）
    let perp = vec2(-axis.y, axis.x).abs();
    let cardinal = |items: &mut Vec<Item>, text: &Option<String>, tip: Pos2, into: Vec2| {
        if let Some(t) = text {
            let p = tip - into * (f * 1.1) + perp * (f * 0.7);
            items.push(Item::Text {
                pos: p,
                text: t.clone(),
                size: f * 0.85,
                color: Role::Text,
                anchor: Align2::LEFT_CENTER,
            });
        }
    };
    cardinal(items, &e.start_label, tf.pos(start), -axis * s_out);
    cardinal(items, &e.end_label, tf.pos(end), axis * s_in);
}

/// 自己ループを描き、ラベル位置を返す。
fn draw_self_loop(
    items: &mut Vec<Item>,
    r: Rect,
    e: &super::graph::Edge,
    color: Role,
    horizontal: bool,
    f: f32,
) -> Pos2 {
    let lw = f * 2.2;
    let (a, b, c1, c2, label) = if horizontal {
        let a = pos2(r.center().x - r.width() * 0.25, r.bottom());
        let b = pos2(r.center().x + r.width() * 0.25, r.bottom());
        (
            a,
            b,
            a + vec2(-f, lw),
            b + vec2(f, lw),
            pos2(r.center().x, r.bottom() + lw * 0.8),
        )
    } else {
        let a = pos2(r.right(), r.center().y - r.height() * 0.25);
        let b = pos2(r.right(), r.center().y + r.height() * 0.25);
        (
            a,
            b,
            a + vec2(lw, -f),
            b + vec2(lw, f),
            pos2(r.right() + lw * 0.8, r.center().y),
        )
    };
    let pts: Vec<Pos2> = (0..=16)
        .map(|k| cubic(a, c1, c2, b, k as f32 / 16.0))
        .collect();
    let dir = (b - c2).normalized();
    items.push(Item::Path {
        points: pts,
        color,
        width: (f * 0.1).max(1.0),
        dash: if e.stroke == EdgeStroke::Dotted {
            Dash::Dotted
        } else {
            Dash::Solid
        },
        closed: false,
    });
    draw_marker(items, e.end, b, dir, color, f);
    label
}

/// 端の記号の分だけ線を短くする量。
fn marker_shorten(m: Marker, f: f32) -> f32 {
    match m {
        Marker::Arrow => f * 0.6,
        Marker::Circle => f * 0.6,
        Marker::Triangle => f * 0.9,
        Marker::DiamondFilled | Marker::DiamondHollow => f * 1.2,
        _ => 0.0,
    }
}

/// 端の記号を描く。`tip` はノード境界上の点、`d` はノードへ向かう単位ベクトル。
fn draw_marker(items: &mut Vec<Item>, m: Marker, tip: Pos2, d: Vec2, color: Role, f: f32) {
    let n = vec2(-d.y, d.x);
    let lw = (f * 0.1).max(1.0);
    let line = |items: &mut Vec<Item>, a: Pos2, b: Pos2| {
        items.push(Item::Path {
            points: vec![a, b],
            color,
            width: lw,
            dash: Dash::Solid,
            closed: false,
        });
    };
    match m {
        Marker::None => {}
        Marker::Arrow => {
            let l = f * 0.65;
            let hw = l * 0.42;
            items.push(Item::Polygon {
                points: vec![tip, tip - d * l + n * hw, tip - d * l - n * hw],
                fill: color,
                stroke: color,
                width: 1.0,
            });
        }
        Marker::OpenArrow => {
            let l = f * 0.65;
            let hw = l * 0.45;
            items.push(Item::Path {
                points: vec![tip - d * l + n * hw, tip, tip - d * l - n * hw],
                color,
                width: lw,
                dash: Dash::Solid,
                closed: false,
            });
        }
        Marker::Circle => {
            let r = f * 0.3;
            items.push(Item::Circle {
                center: tip - d * r,
                radius: r,
                fill: Role::Background,
                stroke: color,
                width: lw,
            });
        }
        Marker::Cross => {
            let s = f * 0.3;
            let c = tip - d * (s * 1.4);
            line(items, c + (d + n) * s, c - (d + n) * s);
            line(items, c + (d - n) * s, c - (d - n) * s);
        }
        Marker::Triangle => {
            let l = f * 0.9;
            let hw = l * 0.5;
            items.push(Item::Polygon {
                points: vec![tip, tip - d * l + n * hw, tip - d * l - n * hw],
                fill: Role::Background,
                stroke: color,
                width: lw,
            });
        }
        Marker::DiamondFilled | Marker::DiamondHollow => {
            let l = f * 1.2;
            let hw = f * 0.38;
            let fill = if m == Marker::DiamondFilled {
                color
            } else {
                Role::Background
            };
            items.push(Item::Polygon {
                points: vec![
                    tip,
                    tip - d * (l / 2.0) + n * hw,
                    tip - d * l,
                    tip - d * (l / 2.0) - n * hw,
                ],
                fill,
                stroke: color,
                width: lw,
            });
        }
        Marker::Er { zero, many } => {
            let hw = f * 0.45;
            let second = if many {
                let a = tip - d * (f * 0.8);
                line(items, a, tip + n * hw);
                line(items, a, tip - n * hw);
                f * 1.05
            } else {
                let b = tip - d * (f * 0.45);
                line(items, b + n * hw, b - n * hw);
                f * 0.8
            };
            if zero {
                let r = f * 0.28;
                items.push(Item::Circle {
                    center: tip - d * (second + r),
                    radius: r,
                    fill: Role::Background,
                    stroke: color,
                    width: lw,
                });
            } else {
                let b = tip - d * second;
                line(items, b + n * hw, b - n * hw);
            }
        }
    }
}

// ==== 機能: ノード形状の描画 ====

/// 楕円弧の点列（角度はラジアン、画面座標で y 下向き）。
fn ellipse_arc(c: Pos2, rx: f32, ry: f32, a0: f32, a1: f32, steps: usize) -> Vec<Pos2> {
    (0..=steps)
        .map(|k| {
            let a = a0 + (a1 - a0) * k as f32 / steps as f32;
            pos2(c.x + rx * a.cos(), c.y + ry * a.sin())
        })
        .collect()
}

/// 形状ノードの外形を描く。
fn draw_shape(items: &mut Vec<Item>, shape: Shape, r: Rect, fill: Role, stroke: Role, f: f32) {
    let width = 1.2;
    let poly = |items: &mut Vec<Item>, points: Vec<Pos2>| {
        items.push(Item::Polygon {
            points,
            fill,
            stroke,
            width,
        });
    };
    let rect = |items: &mut Vec<Item>, radius: f32| {
        items.push(Item::Rect {
            rect: r,
            radius,
            fill,
            stroke,
            width,
        });
    };
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    let c = r.center();
    let h = r.height();
    match shape {
        Shape::Rect => rect(items, 0.0),
        Shape::Round => rect(items, f * 0.4),
        Shape::Stadium => rect(items, h / 2.0),
        Shape::Subroutine => {
            rect(items, 0.0);
            let inset = f * 0.5;
            for x in [l + inset, rt - inset] {
                items.push(Item::Path {
                    points: vec![pos2(x, t), pos2(x, b)],
                    color: stroke,
                    width,
                    dash: Dash::Solid,
                    closed: false,
                });
            }
        }
        Shape::Cylinder => {
            let ry = cylinder_ry(r.width(), f);
            let rx = r.width() / 2.0;
            let top_c = pos2(c.x, t + ry);
            let bot_c = pos2(c.x, b - ry);
            let pi = std::f32::consts::PI;
            let mut body = ellipse_arc(top_c, rx, ry, pi, 2.0 * pi, 16);
            body.extend(ellipse_arc(bot_c, rx, ry, 0.0, pi, 16));
            poly(items, body);
            items.push(Item::Path {
                points: ellipse_arc(top_c, rx, ry, 0.0, pi, 16),
                color: stroke,
                width,
                dash: Dash::Solid,
                closed: false,
            });
        }
        Shape::Circle => items.push(Item::Circle {
            center: c,
            radius: r.width() / 2.0,
            fill,
            stroke,
            width,
        }),
        Shape::DoubleCircle => {
            items.push(Item::Circle {
                center: c,
                radius: r.width() / 2.0,
                fill,
                stroke,
                width,
            });
            items.push(Item::Circle {
                center: c,
                radius: r.width() / 2.0 - f * 0.3,
                fill,
                stroke,
                width,
            });
        }
        Shape::Asymmetric => {
            let notch = h * 0.3;
            poly(
                items,
                vec![
                    pos2(l + notch, c.y),
                    pos2(l, t),
                    pos2(rt, t),
                    pos2(rt, b),
                    pos2(l, b),
                ],
            );
        }
        Shape::Diamond | Shape::Choice => poly(
            items,
            vec![pos2(c.x, t), pos2(rt, c.y), pos2(c.x, b), pos2(l, c.y)],
        ),
        Shape::Hexagon => {
            let k = h / 4.0;
            poly(
                items,
                vec![
                    pos2(l, c.y),
                    pos2(l + k, t),
                    pos2(rt - k, t),
                    pos2(rt, c.y),
                    pos2(rt - k, b),
                    pos2(l + k, b),
                ],
            );
        }
        Shape::Parallelogram | Shape::ParallelogramAlt | Shape::Trapezoid | Shape::TrapezoidAlt => {
            let k = h * 0.5;
            let pts = match shape {
                Shape::Parallelogram => [(l + k, t), (rt, t), (rt - k, b), (l, b)],
                Shape::ParallelogramAlt => [(l, t), (rt - k, t), (rt, b), (l + k, b)],
                Shape::Trapezoid => [(l + k, t), (rt - k, t), (rt, b), (l, b)],
                _ => [(l, t), (rt, t), (rt - k, b), (l + k, b)],
            };
            poly(items, pts.iter().map(|&(x, y)| pos2(x, y)).collect());
        }
        Shape::StateStart => items.push(Item::Circle {
            center: c,
            radius: r.width() / 2.0,
            fill: Role::Text,
            stroke: Role::Text,
            width: 1.0,
        }),
        Shape::StateEnd => {
            items.push(Item::Circle {
                center: c,
                radius: r.width() / 2.0,
                fill: Role::Background,
                stroke: Role::Text,
                width: 1.2,
            });
            items.push(Item::Circle {
                center: c,
                radius: r.width() * 0.3,
                fill: Role::Text,
                stroke: Role::Text,
                width: 1.0,
            });
        }
        Shape::ForkBar => items.push(Item::Rect {
            rect: r,
            radius: 1.5,
            fill: Role::Text,
            stroke: Role::Text,
            width: 1.0,
        }),
    }
}

/// 表ノード（クラス・エンティティ）を描く。
fn draw_table(
    items: &mut Vec<Item>,
    t: &TableGeom,
    r: Rect,
    fill: Role,
    stroke: Role,
    text: Role,
    f: f32,
) {
    items.push(Item::Rect {
        rect: r,
        radius: 0.0,
        fill,
        stroke,
        width: 1.2,
    });
    t.header
        .emit_centered(items, pos2(r.center().x, r.top() + t.header_h / 2.0), text);
    let mut y = r.top() + t.header_h;
    let gap = f * 0.9;
    for rows in &t.sections {
        items.push(Item::Path {
            points: vec![pos2(r.left(), y), pos2(r.right(), y)],
            color: stroke,
            width: 1.0,
            dash: Dash::Solid,
            closed: false,
        });
        y += f * 0.3;
        for row in rows {
            let mut x = r.left() + f * 0.8;
            for (ci, (cell, _)) in row.iter().enumerate() {
                items.push(Item::Text {
                    pos: pos2(x, y + t.row_h / 2.0),
                    text: cell.clone(),
                    size: f,
                    color: text,
                    anchor: Align2::LEFT_CENTER,
                });
                x += t.col_w.get(ci).copied().unwrap_or(0.0) + gap;
            }
            y += t.row_h;
        }
        if rows.is_empty() {
            y += t.row_h;
        }
        y += f * 0.3;
    }
}
