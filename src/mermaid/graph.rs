//! グラフ系の図（flowchart / state / class / er）の共通モデルと構築補助。

use std::collections::HashMap;

use egui::Color32;

// ==== 機能: モデル定義 ====

/// 図の向き。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dir {
    /// 上から下。
    #[default]
    TD,
    /// 下から上。
    BT,
    /// 左から右。
    LR,
    /// 右から左。
    RL,
}

impl Dir {
    /// `TD` `TB` `BT` `LR` `RL` などの文字列から向きを得る。
    pub fn parse(s: &str) -> Option<Dir> {
        match s.trim().trim_end_matches(';').to_ascii_uppercase().as_str() {
            "TD" | "TB" | "V" => Some(Dir::TD),
            "BT" => Some(Dir::BT),
            "LR" | ">" | "H" => Some(Dir::LR),
            "RL" | "<" => Some(Dir::RL),
            "^" => Some(Dir::BT),
            _ => None,
        }
    }

    /// ランク軸が横向き（LR / RL）かどうか。
    pub fn horizontal(self) -> bool {
        matches!(self, Dir::LR | Dir::RL)
    }
}

/// ノードの形状。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `[ ]` 四角形。
    Rect,
    /// `( )` 角丸四角形。
    Round,
    /// `([ ])` スタジアム。
    Stadium,
    /// `[[ ]]` サブルーチン。
    Subroutine,
    /// `[( )]` 円柱（データベース）。
    Cylinder,
    /// `(( ))` 円。
    Circle,
    /// `((( )))` 二重円。
    DoubleCircle,
    /// `> ]` 旗形（左に切り込み）。
    Asymmetric,
    /// `{ }` ひし形。
    Diamond,
    /// `{{ }}` 六角形。
    Hexagon,
    /// `[/ /]` 平行四辺形。
    Parallelogram,
    /// `[\ \]` 逆向きの平行四辺形。
    ParallelogramAlt,
    /// `[/ \]` 台形（下が広い）。
    Trapezoid,
    /// `[\ /]` 逆台形（上が広い）。
    TrapezoidAlt,
    /// 状態図の開始点（塗りつぶし円）。
    StateStart,
    /// 状態図の終了点（二重丸）。
    StateEnd,
    /// 状態図の fork / join（太い棒）。
    ForkBar,
    /// 状態図の choice（小さなひし形）。
    Choice,
}

/// ノードの種類: 形状つきラベル、または区画を持つ表（クラス・エンティティ）。
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// 形状の中にラベルを描く。
    Shape(Shape),
    /// 見出しと区画（区画 → 行 → セル）からなる表。
    Table {
        /// 見出し行（注釈・名前）。
        header: Vec<String>,
        /// 区画の列。各区画は行の列、各行はセルの列。
        sections: Vec<Vec<Vec<String>>>,
    },
}

/// `style` / `classDef` で指定された色。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeStyle {
    /// 塗り。
    pub fill: Option<Color32>,
    /// 枠線。
    pub stroke: Option<Color32>,
    /// 文字色。
    pub text: Option<Color32>,
}

impl NodeStyle {
    /// `other` で指定された項目だけ上書きする。
    pub fn merge(&mut self, other: &NodeStyle) {
        self.fill = other.fill.or(self.fill);
        self.stroke = other.stroke.or(self.stroke);
        self.text = other.text.or(self.text);
    }
}

/// ノード。
#[derive(Clone, Debug)]
pub struct Node {
    /// ソース上の ID。
    pub id: String,
    /// 表示ラベル（行の列）。
    pub label: Vec<String>,
    /// 種類。
    pub kind: NodeKind,
    /// 直接所属するクラスタ。
    pub cluster: Option<usize>,
    /// 色指定。
    pub style: NodeStyle,
    /// 形状やラベルが明示されたか（エッジから暗黙に作られただけなら false）。
    pub explicit: bool,
}

/// エッジの線種。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeStroke {
    /// 実線。
    Solid,
    /// 点線。
    Dotted,
    /// 太線。
    Thick,
    /// 不可視（レイアウトにのみ影響）。
    Invisible,
}

/// エッジ端の記号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// なし。
    None,
    /// 塗りつぶし矢印。
    Arrow,
    /// 開いた矢印（V 字）。
    OpenArrow,
    /// 円 (`--o`)。
    Circle,
    /// バツ (`--x`)。
    Cross,
    /// 中抜き三角形（継承）。
    Triangle,
    /// 塗りつぶしひし形（コンポジション）。
    DiamondFilled,
    /// 中抜きひし形（集約）。
    DiamondHollow,
    /// ER 図のカーディナリティ（`zero`: 0 を含む、`many`: 多）。
    Er {
        /// 最小が 0 か。
        zero: bool,
        /// 最大が多か。
        many: bool,
    },
}

/// エッジ。
#[derive(Clone, Debug)]
pub struct Edge {
    /// 始点ノード。
    pub from: usize,
    /// 終点ノード。
    pub to: usize,
    /// 中央ラベル。
    pub label: Vec<String>,
    /// 線種。
    pub stroke: EdgeStroke,
    /// 始点側の記号。
    pub start: Marker,
    /// 終点側の記号。
    pub end: Marker,
    /// 最小ランク差（長い矢印 `--->` で増える）。
    pub min_len: usize,
    /// 始点付近のラベル（多重度など）。
    pub start_label: Option<String>,
    /// 終点付近のラベル（多重度など）。
    pub end_label: Option<String>,
    /// `linkStyle` で指定された色。
    pub color: Option<Color32>,
}

impl Edge {
    /// 既定値（実線・終点矢印なし）でエッジを作る。
    pub fn new(from: usize, to: usize) -> Self {
        Self {
            from,
            to,
            label: Vec::new(),
            stroke: EdgeStroke::Solid,
            start: Marker::None,
            end: Marker::None,
            min_len: 1,
            start_label: None,
            end_label: None,
            color: None,
        }
    }
}

/// サブグラフ（クラスタ）。
#[derive(Clone, Debug)]
pub struct Cluster {
    /// ソース上の ID。
    pub id: String,
    /// タイトル（行の列）。
    pub title: Vec<String>,
    /// 親クラスタ。
    pub parent: Option<usize>,
}

/// グラフ系の図の共通モデル。
#[derive(Clone, Debug, Default)]
pub struct Graph {
    /// 向き。
    pub dir: Dir,
    /// ノード。
    pub nodes: Vec<Node>,
    /// エッジ。
    pub edges: Vec<Edge>,
    /// クラスタ。
    pub clusters: Vec<Cluster>,
}

impl Graph {
    /// `anc` が `c` 自身またはその祖先かどうか。
    pub fn is_ancestor(&self, anc: usize, c: usize) -> bool {
        let mut cur = Some(c);
        let mut guard = 0;
        while let Some(x) = cur {
            if x == anc {
                return true;
            }
            cur = self.clusters.get(x).and_then(|c| c.parent);
            guard += 1;
            if guard > self.clusters.len() {
                break;
            }
        }
        false
    }
}

// ==== 機能: グラフ構築補助 ====

/// パーサが使うグラフ構築器（ID → 番号の対応を管理）。
#[derive(Default)]
pub struct Builder {
    /// 構築中のグラフ。
    pub g: Graph,
    /// ノード ID → 番号。
    index: HashMap<String, usize>,
    /// クラスタ ID → 番号。
    cluster_index: HashMap<String, usize>,
}

impl Builder {
    /// 空の構築器を作る。
    pub fn new() -> Self {
        Self::default()
    }

    /// ID のノードを取得し、なければ既定の形状で作る。
    pub fn node(&mut self, id: &str, default_shape: Shape) -> usize {
        if let Some(&i) = self.index.get(id) {
            return i;
        }
        let i = self.g.nodes.len();
        self.g.nodes.push(Node {
            id: id.to_string(),
            label: vec![id.to_string()],
            kind: NodeKind::Shape(default_shape),
            cluster: None,
            style: NodeStyle::default(),
            explicit: false,
        });
        self.index.insert(id.to_string(), i);
        i
    }

    /// 既存ノードの番号を引く。
    pub fn find(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// クラスタを作る（ID 重複時は既存を返す）。
    pub fn cluster(&mut self, id: &str, title: Vec<String>, parent: Option<usize>) -> usize {
        if let Some(&i) = self.cluster_index.get(id) {
            return i;
        }
        let i = self.g.clusters.len();
        self.g.clusters.push(Cluster {
            id: id.to_string(),
            title,
            parent,
        });
        self.cluster_index.insert(id.to_string(), i);
        i
    }

    /// ノードをクラスタに所属させる（より深いクラスタへの移動のみ許す）。
    pub fn assign(&mut self, node: usize, cluster: Option<usize>) {
        let Some(c) = cluster else { return };
        let Some(n) = self.g.nodes.get(node) else {
            return;
        };
        let ok = match n.cluster {
            None => true,
            Some(cur) => cur != c && self.g.is_ancestor(cur, c),
        };
        if ok {
            self.g.nodes[node].cluster = Some(c);
        }
    }

    /// エッジを追加する。
    pub fn edge(&mut self, e: Edge) {
        self.g.edges.push(e);
    }

    /// クラスタ ID を指す暗黙ノードへのエッジを、クラスタ内の代表ノードへ付け替え、
    /// その暗黙ノードを取り除いてグラフを返す。
    ///
    /// 入ってくるエッジはクラスタ内の最初のメンバーへ、出ていくエッジは最後のメンバーから
    /// 出るように付け替える。
    pub fn finish(mut self) -> Graph {
        let g = &mut self.g;
        // 暗黙ノード番号 → (入口の代表, 出口の代表)
        let mut redirect: HashMap<usize, (usize, usize)> = HashMap::new();
        for (ni, n) in g.nodes.iter().enumerate() {
            if n.explicit {
                continue;
            }
            let Some(&ci) = self.cluster_index.get(&n.id) else {
                continue;
            };
            let members: Vec<usize> = g
                .nodes
                .iter()
                .enumerate()
                .filter(|(j, m)| *j != ni && m.cluster.is_some_and(|mc| g.is_ancestor(ci, mc)))
                .map(|(j, _)| j)
                .collect();
            if let (Some(&first), Some(&last)) = (members.first(), members.last()) {
                redirect.insert(ni, (first, last));
            }
        }
        if redirect.is_empty() {
            return self.g;
        }
        for e in &mut g.edges {
            if let Some(&(_, out)) = redirect.get(&e.from) {
                e.from = out;
            }
            if let Some(&(inn, _)) = redirect.get(&e.to) {
                e.to = inn;
            }
        }
        // 番号を詰め直す
        let mut remap = vec![usize::MAX; g.nodes.len()];
        let mut nodes = Vec::new();
        for (i, n) in std::mem::take(&mut g.nodes).into_iter().enumerate() {
            if !redirect.contains_key(&i) {
                remap[i] = nodes.len();
                nodes.push(n);
            }
        }
        g.nodes = nodes;
        for e in &mut g.edges {
            e.from = remap.get(e.from).copied().unwrap_or(usize::MAX);
            e.to = remap.get(e.to).copied().unwrap_or(usize::MAX);
        }
        g.edges
            .retain(|e| e.from != usize::MAX && e.to != usize::MAX);
        self.g
    }
}

// ==== 機能: 色指定の解析 ====

/// `#rgb` `#rrggbb` `#rrggbbaa` `rgb(r,g,b)` と基本的な色名を解析する。
pub fn parse_color(s: &str) -> Option<Color32> {
    let s = s.trim().trim_end_matches(';').trim();
    if let Some(hex) = s.strip_prefix('#') {
        let v: Vec<u8> = hex
            .chars()
            .map(|c| c.to_digit(16).map(|d| d as u8))
            .collect::<Option<_>>()?;
        return match v.len() {
            3 => Some(Color32::from_rgb(v[0] * 17, v[1] * 17, v[2] * 17)),
            6 | 8 => {
                let b = |i: usize| v[i] * 16 + v[i + 1];
                let a = if v.len() == 8 { b(6) } else { 255 };
                Some(Color32::from_rgba_unmultiplied(b(0), b(2), b(4), a))
            }
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    if let Some(inner) = lower
        .strip_prefix("rgba(")
        .or_else(|| lower.strip_prefix("rgb("))
        .and_then(|x| x.strip_suffix(')'))
    {
        let parts: Vec<f32> = inner
            .split(',')
            .map(|p| p.trim().parse::<f32>().ok())
            .collect::<Option<_>>()?;
        if parts.len() < 3 {
            return None;
        }
        let c = |x: f32| x.clamp(0.0, 255.0) as u8;
        let a = parts.get(3).map_or(255, |a| {
            if *a <= 1.0 {
                (a.clamp(0.0, 1.0) * 255.0) as u8
            } else {
                c(*a)
            }
        });
        return Some(Color32::from_rgba_unmultiplied(
            c(parts[0]),
            c(parts[1]),
            c(parts[2]),
            a,
        ));
    }
    let named = match lower.as_str() {
        "white" => (255, 255, 255),
        "black" => (0, 0, 0),
        "red" => (255, 0, 0),
        "green" => (0, 128, 0),
        "blue" => (0, 0, 255),
        "yellow" => (255, 255, 0),
        "orange" => (255, 165, 0),
        "purple" => (128, 0, 128),
        "pink" => (255, 192, 203),
        "gray" | "grey" => (128, 128, 128),
        "lightgray" | "lightgrey" => (211, 211, 211),
        "lightblue" => (173, 216, 230),
        "lightgreen" => (144, 238, 144),
        "aqua" | "cyan" => (0, 255, 255),
        _ => return None,
    };
    Some(Color32::from_rgb(named.0, named.1, named.2))
}

/// `fill:#f9f,stroke:#333,color:#fff` 形式のスタイル指定を解析する。
pub fn parse_style_props(s: &str) -> NodeStyle {
    let mut st = NodeStyle::default();
    for part in s.split(',') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        let c = parse_color(v);
        match k.trim() {
            "fill" | "background" => st.fill = c.or(st.fill),
            "stroke" => st.stroke = c.or(st.stroke),
            "color" => st.text = c.or(st.text),
            _ => {}
        }
    }
    st
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_colors() {
        assert_eq!(parse_color("#fff"), Some(Color32::WHITE));
        assert_eq!(parse_color("#ff0000"), Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(
            parse_color("rgb(0, 255, 0)"),
            Some(Color32::from_rgb(0, 255, 0))
        );
        assert_eq!(parse_color("#zz"), None);
        let st = parse_style_props("fill:#f9f,stroke:#333,stroke-width:4px");
        assert!(st.fill.is_some() && st.stroke.is_some() && st.text.is_none());
    }
}
