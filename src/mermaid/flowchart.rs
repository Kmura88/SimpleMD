//! flowchart / graph のパーサ（→ [`Graph`]）。

use std::collections::HashMap;

use super::graph::{
    Builder, Dir, Edge, EdgeStroke, Graph, Marker, NodeKind, NodeStyle, Shape, parse_style_props,
};
use super::text::{clean_label, split_statements};

// ==== 機能: 文字カーソル ====

/// 文字単位で読み進めるカーソル（UTF-8 を安全に扱うため char 配列で持つ）。
struct Cursor {
    /// 文字列。
    s: Vec<char>,
    /// 現在位置。
    i: usize,
}

impl Cursor {
    /// 文字列からカーソルを作る。
    fn new(s: &str) -> Self {
        Self {
            s: s.chars().collect(),
            i: 0,
        }
    }

    /// 現在位置から `k` 文字先の文字。
    fn peek_at(&self, k: usize) -> Option<char> {
        self.s.get(self.i + k).copied()
    }

    /// 現在の文字。
    fn peek(&self) -> Option<char> {
        self.peek_at(0)
    }

    /// 末尾に達したか。
    fn eof(&self) -> bool {
        self.i >= self.s.len()
    }

    /// 空白を読み飛ばす。
    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    /// 現在位置が文字列 `p` で始まるか。
    fn starts_with(&self, p: &str) -> bool {
        p.chars()
            .enumerate()
            .all(|(k, c)| self.peek_at(k) == Some(c))
    }

    /// 位置 `from` から `p` が最初に現れる位置を探す。
    fn find(&self, from: usize, p: &str) -> Option<usize> {
        let pat: Vec<char> = p.chars().collect();
        if pat.is_empty() {
            return None;
        }
        (from..self.s.len()).find(|&k| self.s[k..].starts_with(&pat))
    }

    /// 範囲の文字列を取り出す。
    fn slice(&self, a: usize, b: usize) -> String {
        let b = b.min(self.s.len());
        let a = a.min(b);
        self.s[a..b].iter().collect()
    }
}

// ==== 機能: リンク（エッジ）の解析 ====

/// 解析したリンクの情報。
#[derive(Clone, Debug)]
struct Link {
    /// 線種。
    stroke: EdgeStroke,
    /// 始点記号。
    start: Marker,
    /// 終点記号。
    end: Marker,
    /// 最小ランク差。
    len: usize,
    /// ラベル。
    label: Vec<String>,
}

/// ID に使える文字か（`-` は矢印でない場合だけ、呼び出し側で判定）。
fn is_id_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$' || c == '\''
}

/// 線部分（`-` `=` `.` `~` の連なり）から線種を判定する。
fn classify(run: &str) -> EdgeStroke {
    if run.contains('~') {
        EdgeStroke::Invisible
    } else if run.contains('=') {
        EdgeStroke::Thick
    } else if run.contains('.') {
        EdgeStroke::Dotted
    } else {
        EdgeStroke::Solid
    }
}

/// 線の長さから最小ランク差を求める（`-->` = 1, `--->` = 2 …）。
fn link_len(run: &str, has_end: bool) -> usize {
    let dots = run.chars().filter(|&c| c == '.').count();
    if dots > 0 {
        return dots.clamp(1, 8);
    }
    let n = run
        .chars()
        .filter(|&c| c == '-' || c == '=' || c == '~')
        .count();
    let base = if has_end { 1 } else { 2 };
    n.saturating_sub(base).clamp(1, 8)
}

/// 終点記号を読む（`>` `o` `x`。線の直後にある場合のみ呼ばれる）。
fn read_end_marker(c: &mut Cursor) -> Marker {
    match c.peek() {
        Some('>') => {
            c.i += 1;
            Marker::Arrow
        }
        Some(ch @ ('o' | 'x')) => {
            c.i += 1;
            if ch == 'o' {
                Marker::Circle
            } else {
                Marker::Cross
            }
        }
        _ => Marker::None,
    }
}

/// 線の文字列を読む。
fn read_run(c: &mut Cursor) -> String {
    let a = c.i;
    while c
        .peek()
        .is_some_and(|ch| matches!(ch, '-' | '=' | '.' | '~'))
    {
        c.i += 1;
    }
    c.slice(a, c.i)
}

/// 現在位置からリンクを読む。リンクでなければ位置を戻して `None`。
fn parse_link(c: &mut Cursor) -> Option<Link> {
    let save = c.i;
    c.skip_ws();
    let mut start = Marker::None;
    match c.peek() {
        Some('<') => {
            start = Marker::Arrow;
            c.i += 1;
        }
        Some(ch @ ('o' | 'x'))
            if c.peek_at(1).is_some_and(|n| n == '-' || n == '=')
                && c.peek_at(2).is_some_and(|n| matches!(n, '-' | '=' | '.')) =>
        {
            start = if ch == 'o' {
                Marker::Circle
            } else {
                Marker::Cross
            };
            c.i += 1;
        }
        _ => {}
    }
    let mut run = read_run(c);
    if run.is_empty() {
        c.i = save;
        return None;
    }
    let mut end = read_end_marker(c);
    let mut label = Vec::new();
    // `-- text -->` 形式: 開き記号の直後が空白でラベルが続く
    if end == Marker::None
        && matches!(run.as_str(), "--" | "==" | "-.")
        && c.peek().is_some_and(char::is_whitespace)
    {
        let closer = match run.as_str() {
            "--" => "--",
            "==" => "==",
            _ => ".-",
        };
        if let Some(k) = c.find(c.i, closer) {
            label = clean_label(&c.slice(c.i, k));
            c.i = k;
            // 長さと線種は閉じ側の線で決める
            run = read_run(c);
            end = read_end_marker(c);
        }
    }
    let stroke = classify(&run);
    let len = link_len(&run, end != Marker::None);
    c.skip_ws();
    if c.peek() == Some('|')
        && let Some(k) = c.find(c.i + 1, "|")
    {
        label = clean_label(&c.slice(c.i + 1, k));
        c.i = k + 1;
    }
    Some(Link {
        stroke,
        start,
        end,
        len,
        label,
    })
}

// ==== 機能: ノードの解析 ====

/// 形状の開き記号・閉じ記号の表（長いものから照合）。
const SHAPES: &[(&str, &[(&str, Shape)])] = &[
    ("(((", &[(")))", Shape::DoubleCircle)]),
    ("((", &[("))", Shape::Circle)]),
    ("([", &[("])", Shape::Stadium)]),
    ("[[", &[("]]", Shape::Subroutine)]),
    ("[(", &[(")]", Shape::Cylinder)]),
    ("{{", &[("}}", Shape::Hexagon)]),
    (
        "[/",
        &[("/]", Shape::Parallelogram), ("\\]", Shape::Trapezoid)],
    ),
    (
        "[\\",
        &[
            ("\\]", Shape::ParallelogramAlt),
            ("/]", Shape::TrapezoidAlt),
        ],
    ),
    ("(", &[(")", Shape::Round)]),
    ("[", &[("]", Shape::Rect)]),
    ("{", &[("}", Shape::Diamond)]),
    (">", &[("]", Shape::Asymmetric)]),
];

/// ノード宣言の解析結果。
struct NodeRef {
    /// ID。
    id: String,
    /// 明示された形状とラベル。
    shape: Option<(Shape, Vec<String>)>,
    /// `:::class` で指定されたクラス名。
    class: Option<String>,
}

/// ID を読む（`-` は矢印の始まりでない場合だけ ID に含める）。
fn read_id(c: &mut Cursor) -> String {
    let a = c.i;
    while let Some(ch) = c.peek() {
        let next = c.peek_at(1);
        let ok = is_id_char(ch)
            || (ch == '-'
                && c.i > a
                && next.is_some_and(|n| is_id_char(n) && n != 'o' && n != 'x'))
            || (ch == '.' && c.i > a && next.is_some_and(char::is_alphanumeric));
        if !ok {
            break;
        }
        c.i += 1;
    }
    c.slice(a, c.i)
}

/// 形状つきラベルを読む。
fn read_shape(c: &mut Cursor) -> Result<Option<(Shape, Vec<String>)>, String> {
    for (open, closers) in SHAPES {
        if !c.starts_with(open) {
            continue;
        }
        let body_start = c.i + open.chars().count();
        let mut k = body_start;
        while c.s.get(k).is_some_and(|ch| ch.is_whitespace()) {
            k += 1;
        }
        // 引用符つきラベルなら閉じ引用符の後から閉じ記号を探す
        let search_from = if c.s.get(k) == Some(&'"') {
            c.find(k + 1, "\"").map_or(k, |q| q + 1)
        } else {
            body_start
        };
        let found = closers
            .iter()
            .filter_map(|(close, shape)| c.find(search_from, close).map(|p| (p, *close, *shape)))
            .min_by_key(|(p, _, _)| *p);
        let Some((p, close, shape)) = found else {
            return Err(format!("閉じ記号 `{}` が見つかりません", closers[0].0));
        };
        let label = clean_label(&c.slice(body_start, p));
        c.i = p + close.chars().count();
        return Ok(Some((shape, label)));
    }
    Ok(None)
}

/// ノード参照（ID + 形状 + `:::class`）を読む。
fn parse_node(c: &mut Cursor) -> Result<NodeRef, String> {
    c.skip_ws();
    let id = read_id(c);
    if id.is_empty() {
        return Err(format!(
            "ノード ID が必要です（位置 {}: `{}`）",
            c.i,
            c.slice(c.i, c.i + 12)
        ));
    }
    let shape = read_shape(c)?;
    // 新構文 `A@{ ... }` は中身を読み飛ばす
    if c.peek() == Some('@')
        && c.peek_at(1) == Some('{')
        && let Some(k) = c.find(c.i, "}")
    {
        c.i = k + 1;
    }
    let mut class = None;
    if c.starts_with(":::") {
        c.i += 3;
        let cls = read_id(c);
        if !cls.is_empty() {
            class = Some(cls);
        }
    }
    Ok(NodeRef { id, shape, class })
}

// ==== 機能: 文の解析 ====

/// flowchart パーサの状態。
struct Parser {
    /// グラフ構築器。
    b: Builder,
    /// 開いているサブグラフのスタック。
    stack: Vec<usize>,
    /// `classDef` の定義。
    class_defs: HashMap<String, NodeStyle>,
    /// ノードへのクラス割り当て。
    node_classes: Vec<(usize, String)>,
    /// `style` による直接指定。
    node_styles: Vec<(usize, NodeStyle)>,
    /// `linkStyle` の指定（None = default）。
    link_styles: Vec<(Option<usize>, NodeStyle)>,
    /// 自動採番用カウンタ。
    anon: usize,
}

/// flowchart を解析する。`lines[0]` はヘッダ行。
pub fn parse(lines: &[(usize, String)]) -> Result<Graph, String> {
    let mut p = Parser {
        b: Builder::new(),
        stack: Vec::new(),
        class_defs: HashMap::new(),
        node_classes: Vec::new(),
        node_styles: Vec::new(),
        link_styles: Vec::new(),
        anon: 0,
    };
    let Some((_, header)) = lines.first() else {
        return Err("空の flowchart です".into());
    };
    // ヘッダ: `flowchart LR` の後ろに文が続く場合もある（`graph TD; A-->B`）
    let rest_of_header = {
        let mut it = header.splitn(2, |c: char| c.is_whitespace() || c == ';');
        it.next();
        let rest = it.next().unwrap_or("").trim();
        let (dir_tok, after) = rest
            .split_once(|c: char| c.is_whitespace() || c == ';')
            .unwrap_or((rest, ""));
        match Dir::parse(dir_tok) {
            Some(d) => {
                p.b.g.dir = d;
                after.to_string()
            }
            None => rest.to_string(),
        }
    };
    let mut all: Vec<(usize, String)> = Vec::new();
    if !rest_of_header.trim().is_empty() {
        all.push((lines[0].0, rest_of_header));
    }
    all.extend(lines[1..].iter().cloned());
    for (no, line) in &all {
        for stmt in split_statements(line) {
            p.statement(&stmt).map_err(|e| format!("{no} 行目: {e}"))?;
        }
    }
    p.apply_styles();
    Ok(p.b.finish())
}

impl Parser {
    /// 現在のサブグラフ。
    fn current(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    /// 1 文を処理する。
    fn statement(&mut self, s: &str) -> Result<(), String> {
        let first = s.split_whitespace().next().unwrap_or("");
        match first {
            "end" if s.trim() == "end" => {
                self.stack.pop();
                Ok(())
            }
            "subgraph" => {
                self.subgraph(s["subgraph".len()..].trim());
                Ok(())
            }
            "direction" | "click" | "accTitle" | "accDescr" | "accTitle:" | "accDescr:"
            | "title" => Ok(()),
            "classDef" => {
                let rest = s["classDef".len()..].trim();
                let (names, props) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                let st = parse_style_props(props);
                for n in names.split(',') {
                    self.class_defs.insert(n.trim().to_string(), st);
                }
                Ok(())
            }
            "class" => {
                let rest = s["class".len()..].trim();
                if let Some((ids, cls)) = rest.rsplit_once(char::is_whitespace) {
                    for id in ids.split(',') {
                        let id = id.trim();
                        if !id.is_empty() {
                            let n = self.b.node(id, Shape::Rect);
                            self.node_classes.push((n, cls.trim().to_string()));
                        }
                    }
                }
                Ok(())
            }
            "style" => {
                let rest = s["style".len()..].trim();
                if let Some((id, props)) = rest.split_once(char::is_whitespace)
                    && let Some(n) = self.b.find(id.trim())
                {
                    self.node_styles.push((n, parse_style_props(props)));
                }
                Ok(())
            }
            "linkStyle" => {
                let rest = s["linkStyle".len()..].trim();
                if let Some((ids, props)) = rest.split_once(char::is_whitespace) {
                    let st = parse_style_props(props);
                    for id in ids.split(',') {
                        let id = id.trim();
                        if id == "default" {
                            self.link_styles.push((None, st));
                        } else if let Ok(k) = id.parse::<usize>() {
                            self.link_styles.push((Some(k), st));
                        }
                    }
                }
                Ok(())
            }
            _ => self.chain(s),
        }
    }

    /// `subgraph` 見出しを処理する。
    fn subgraph(&mut self, rest: &str) {
        let (id, title) = if let Some(open) = rest.find('[') {
            let id = rest[..open].trim().to_string();
            let inner = rest[open + 1..].trim_end();
            let inner = inner.strip_suffix(']').unwrap_or(inner);
            (id, clean_label(inner))
        } else if rest.starts_with('"') {
            (String::new(), clean_label(rest))
        } else {
            (rest.to_string(), clean_label(rest))
        };
        let id = if id.is_empty() {
            self.anon += 1;
            format!("__subgraph{}", self.anon)
        } else {
            id
        };
        let parent = self.current();
        let c = self.b.cluster(&id, title, parent);
        self.stack.push(c);
    }

    /// ノード参照を登録してノード番号を返す。
    fn add_node(&mut self, r: NodeRef) -> usize {
        let n = self.b.node(&r.id, Shape::Rect);
        if let Some((shape, label)) = r.shape {
            let node = &mut self.b.g.nodes[n];
            node.kind = NodeKind::Shape(shape);
            node.label = label;
            node.explicit = true;
        }
        if let Some(cls) = r.class {
            self.node_classes.push((n, cls));
        }
        let cur = self.current();
        self.b.assign(n, cur);
        n
    }

    /// `A & B --> C --> D` のような連鎖文を処理する。
    fn chain(&mut self, s: &str) -> Result<(), String> {
        let mut c = Cursor::new(s);
        let mut prev: Option<(Vec<usize>, Link)> = None;
        loop {
            // ノードグループ（& 区切り）
            let mut group = Vec::new();
            loop {
                let r = parse_node(&mut c)?;
                group.push(self.add_node(r));
                c.skip_ws();
                if c.peek() == Some('&') {
                    c.i += 1;
                    continue;
                }
                break;
            }
            if let Some((from, link)) = prev.take() {
                for &a in &from {
                    for &b in &group {
                        self.b.edge(Edge {
                            label: link.label.clone(),
                            stroke: link.stroke,
                            start: link.start,
                            end: link.end,
                            min_len: link.len,
                            ..Edge::new(a, b)
                        });
                    }
                }
            }
            c.skip_ws();
            if c.eof() {
                return Ok(());
            }
            // エッジ ID `e1@-->` は読み飛ばせないので未対応
            let Some(link) = parse_link(&mut c) else {
                return Err(format!(
                    "解釈できない記述です: `{}`",
                    c.slice(c.i, c.i + 20)
                ));
            };
            c.skip_ws();
            if c.eof() {
                return Err("エッジの終点がありません".into());
            }
            prev = Some((group, link));
        }
    }

    /// `classDef` / `class` / `style` / `linkStyle` を適用する。
    fn apply_styles(&mut self) {
        if let Some(def) = self.class_defs.get("default").copied() {
            for n in &mut self.b.g.nodes {
                n.style.merge(&def);
            }
        }
        for (n, cls) in &self.node_classes {
            if let (Some(st), Some(node)) = (self.class_defs.get(cls), self.b.g.nodes.get_mut(*n)) {
                node.style.merge(st);
            }
        }
        for (n, st) in &self.node_styles {
            if let Some(node) = self.b.g.nodes.get_mut(*n) {
                node.style.merge(st);
            }
        }
        for (k, st) in &self.link_styles {
            let Some(color) = st.stroke else { continue };
            match k {
                None => self
                    .b
                    .g
                    .edges
                    .iter_mut()
                    .for_each(|e| e.color = Some(color)),
                Some(k) => {
                    if let Some(e) = self.b.g.edges.get_mut(*k) {
                        e.color = Some(color);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::text::preprocess;

    fn p(src: &str) -> Graph {
        parse(&preprocess(src)).expect("parse")
    }

    fn shape_of(g: &Graph, id: &str) -> Shape {
        let n = g.nodes.iter().find(|n| n.id == id).expect("node");
        match n.kind {
            NodeKind::Shape(s) => s,
            _ => panic!("not a shape"),
        }
    }

    #[test]
    fn parses_all_shapes() {
        let g = p(
            "flowchart LR\nA[rect] --> B(round) --> C([stadium]) --> D[[sub]]\n\
                   E[(db)] --> F((circle)) --> G>asym] --> H{diamond}\n\
                   I{{hex}} --> J[/para/] --> K[\\alt\\] --> L[/trap\\] --> M[\\inv/]\n\
                   N(((dbl)))",
        );
        assert_eq!(g.dir, Dir::LR);
        let expect = [
            ("A", Shape::Rect),
            ("B", Shape::Round),
            ("C", Shape::Stadium),
            ("D", Shape::Subroutine),
            ("E", Shape::Cylinder),
            ("F", Shape::Circle),
            ("G", Shape::Asymmetric),
            ("H", Shape::Diamond),
            ("I", Shape::Hexagon),
            ("J", Shape::Parallelogram),
            ("K", Shape::ParallelogramAlt),
            ("L", Shape::Trapezoid),
            ("M", Shape::TrapezoidAlt),
            ("N", Shape::DoubleCircle),
        ];
        for (id, s) in expect {
            assert_eq!(shape_of(&g, id), s, "{id}");
        }
        assert_eq!(g.nodes[0].label, vec!["rect"]);
    }

    #[test]
    fn parses_edge_kinds_and_labels() {
        let g = p(
            "graph TD\nA-->B\nA---C\nA-.->D\nA==>E\nA--oF\nA--xG\nA<-->H\n\
                   A-->|yes|I\nA-- no -->J\nA-. maybe .->K\nA== big ==>L\nA--->M\nA~~~N",
        );
        let e = &g.edges;
        assert_eq!(e.len(), 13);
        assert_eq!(e[0].end, Marker::Arrow);
        assert_eq!(e[1].end, Marker::None);
        assert_eq!(e[2].stroke, EdgeStroke::Dotted);
        assert_eq!(e[3].stroke, EdgeStroke::Thick);
        assert_eq!(e[4].end, Marker::Circle);
        assert_eq!(e[5].end, Marker::Cross);
        assert_eq!((e[6].start, e[6].end), (Marker::Arrow, Marker::Arrow));
        assert_eq!(e[7].label, vec!["yes"]);
        assert_eq!(e[8].label, vec!["no"]);
        assert_eq!(e[8].end, Marker::Arrow);
        assert_eq!(e[9].label, vec!["maybe"]);
        assert_eq!(e[9].stroke, EdgeStroke::Dotted);
        assert_eq!(e[10].label, vec!["big"]);
        assert_eq!(e[10].stroke, EdgeStroke::Thick);
        assert_eq!(e[11].min_len, 2);
        assert_eq!(e[12].stroke, EdgeStroke::Invisible);
    }

    #[test]
    fn parses_chains_ampersand_and_semicolons() {
        let g = p("graph LR; A --> B --> C; D & E --> F & G");
        assert_eq!(g.nodes.len(), 7);
        assert_eq!(g.edges.len(), 2 + 4);
    }

    #[test]
    fn parses_quoted_labels_and_ids_with_dash() {
        let g = p("flowchart TD\n  my-node[\"Hello <br> 世界\"] --> other_node");
        assert_eq!(g.nodes[0].id, "my-node");
        assert_eq!(g.nodes[0].label, vec!["Hello", "世界"]);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn parses_subgraphs_nested() {
        let g = p(
            "flowchart TB\n c1-->a2\n subgraph one [First]\n a1-->a2\n subgraph inner\n x\n end\n end\n subgraph two\n b1-->b2\n end\n one --> two",
        );
        assert_eq!(g.clusters.len(), 3);
        assert_eq!(g.clusters[0].title, vec!["First"]);
        assert_eq!(g.clusters[1].parent, Some(0));
        let a2 = g.nodes.iter().find(|n| n.id == "a2").unwrap();
        assert_eq!(a2.cluster, Some(0));
        let x = g.nodes.iter().find(|n| n.id == "x").unwrap();
        assert_eq!(x.cluster, Some(1));
        // one --> two はクラスタ内のノード間エッジに付け替えられる
        assert!(g.nodes.iter().all(|n| n.id != "one" && n.id != "two"));
        assert_eq!(g.edges.len(), 4);
    }

    #[test]
    fn applies_styles() {
        let g = p(
            "graph TD\nA:::hot --> B\nclassDef hot fill:#f00\nstyle B stroke:#00f\nlinkStyle 0 stroke:#0f0",
        );
        assert!(g.nodes[0].style.fill.is_some());
        assert!(g.nodes[1].style.stroke.is_some());
        assert!(g.edges[0].color.is_some());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(&preprocess("graph TD\nA -->")).is_err());
        assert!(parse(&preprocess("graph TD\nA[unclosed")).is_err());
    }
}
