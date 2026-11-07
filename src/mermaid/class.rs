//! classDiagram のパーサ（→ [`Graph`]、クラスは表ノードとして描く）。

use std::collections::HashMap;

use super::graph::{Builder, Dir, Edge, EdgeStroke, Graph, Marker, NodeKind, Shape};
use super::text::unquote;

// ==== 機能: クラス情報の収集 ====

/// 1 つのクラスの内容。
#[derive(Default)]
struct ClassData {
    /// 表示名（ジェネリクス展開済み）。
    name: String,
    /// `<<interface>>` などの注釈。
    annotation: Option<String>,
    /// 属性。
    attrs: Vec<String>,
    /// メソッド。
    methods: Vec<String>,
}

/// クラス図パーサの状態。
struct Parser {
    /// グラフ構築器。
    b: Builder,
    /// ノード番号 → クラス内容。
    data: HashMap<usize, ClassData>,
    /// 開いている namespace のスタック。
    ns: Vec<usize>,
}

/// `~T~` を `<T>` に置き換える。
fn generics(s: &str) -> String {
    let mut out = String::new();
    let mut open = true;
    for c in s.chars() {
        if c == '~' {
            out.push(if open { '<' } else { '>' });
            open = !open;
        } else {
            out.push(c);
        }
    }
    out
}

impl Parser {
    /// クラス名からノードを得る（`Name~T~` `Name["label"]` `Name:::cls` に対応）。
    fn class(&mut self, raw: &str) -> Result<usize, String> {
        let raw = raw.split(":::").next().unwrap_or("").trim();
        let (id_part, label) = match raw.find('[') {
            Some(k) => (
                &raw[..k],
                Some(unquote(raw[k + 1..].trim_end_matches(']')).to_string()),
            ),
            None => (raw, None),
        };
        let id = id_part.split('~').next().unwrap_or("").trim();
        if id.is_empty() || id.contains(char::is_whitespace) {
            return Err(format!("クラス名が不正です: `{raw}`"));
        }
        let n = self.b.node(id, Shape::Rect);
        let cur = self.ns.last().copied();
        self.b.assign(n, cur);
        let d = self.data.entry(n).or_default();
        if d.name.is_empty() || id_part.contains('~') || label.is_some() {
            d.name = label.unwrap_or_else(|| generics(id_part.trim()));
        }
        Ok(n)
    }

    /// メンバー行を追加する（`(` を含めばメソッド）。
    fn member(&mut self, n: usize, m: &str) {
        let m = m.trim();
        if m.is_empty() {
            return;
        }
        let d = self.data.entry(n).or_default();
        if let Some(a) = m.strip_prefix("<<").and_then(|x| x.strip_suffix(">>")) {
            d.annotation = Some(a.trim().to_string());
            return;
        }
        let text = generics(m.trim_end_matches(['$', '*']));
        if m.contains('(') {
            d.methods.push(text);
        } else {
            d.attrs.push(text);
        }
    }
}

// ==== 機能: 関係の解析 ====

/// 関係記述の解析結果。
struct Relation {
    /// 左辺のクラス。
    lhs: String,
    /// 右辺のクラス。
    rhs: String,
    /// 左辺側の多重度。
    lcard: Option<String>,
    /// 右辺側の多重度。
    rcard: Option<String>,
    /// 左端記号。
    lmark: Marker,
    /// 右端記号。
    rmark: Marker,
    /// 点線か。
    dotted: bool,
    /// ラベル。
    label: Vec<String>,
}

/// `A "1" <|-- "*" B : label` 形式の関係を解析する。関係でなければ `None`。
fn parse_relation(l: &str) -> Option<Relation> {
    let (body, label) = match l.split_once(" : ").or_else(|| l.split_once(':')) {
        Some((b, lb)) if b.contains("--") || b.contains("..") => (b, lb.trim()),
        _ => (l, ""),
    };
    let k = [body.find("--"), body.find("..")]
        .into_iter()
        .flatten()
        .min()?;
    let dotted = body[k..].starts_with("..");
    let before = &body[..k];
    let after = &body[k + 2..];
    let (lmark, lcut) = if before.ends_with("<|") {
        (Marker::Triangle, 2)
    } else if before.ends_with('*') {
        (Marker::DiamondFilled, 1)
    } else if before.ends_with('<') {
        (Marker::OpenArrow, 1)
    } else if before.ends_with('o') && before[..before.len() - 1].ends_with(char::is_whitespace) {
        (Marker::DiamondHollow, 1)
    } else {
        (Marker::None, 0)
    };
    let (rmark, rcut) = if after.starts_with("|>") {
        (Marker::Triangle, 2)
    } else if after.starts_with('*') {
        (Marker::DiamondFilled, 1)
    } else if after.starts_with('>') {
        (Marker::OpenArrow, 1)
    } else if after.starts_with('o') && after[1..].starts_with(char::is_whitespace) {
        (Marker::DiamondHollow, 1)
    } else {
        (Marker::None, 0)
    };
    let lhs_part = before[..before.len() - lcut].trim();
    let rhs_part = after[rcut..].trim();
    // 多重度（引用符）を切り出す
    let (lhs, lcard) = match lhs_part
        .strip_suffix('"')
        .and_then(|x| x.rfind('"').map(|q| (x, q)))
    {
        Some((x, q)) => (x[..q].trim().to_string(), Some(x[q + 1..].to_string())),
        None => (lhs_part.to_string(), None),
    };
    let (rhs, rcard) = match rhs_part
        .strip_prefix('"')
        .and_then(|x| x.find('"').map(|q| (x, q)))
    {
        Some((x, q)) => (x[q + 1..].trim().to_string(), Some(x[..q].to_string())),
        None => (rhs_part.to_string(), None),
    };
    if lhs.is_empty() || rhs.is_empty() {
        return None;
    }
    Some(Relation {
        lhs,
        rhs,
        lcard,
        rcard,
        lmark,
        rmark,
        dotted,
        label: super::text::clean_label(label),
    })
}

// ==== 機能: 文の解析 ====

/// クラス図を解析する。`lines[0]` はヘッダ行。
pub fn parse(lines: &[(usize, String)]) -> Result<Graph, String> {
    let mut p = Parser {
        b: Builder::new(),
        data: HashMap::new(),
        ns: Vec::new(),
    };
    let mut body: Option<usize> = None;
    for (no, line) in lines.iter().skip(1) {
        let l = line.trim();
        if let Some(n) = body {
            match l.strip_suffix('}') {
                Some(last) => {
                    p.member(n, last);
                    body = None;
                }
                None => p.member(n, l),
            }
            continue;
        }
        body = p.statement(l).map_err(|e| format!("{no} 行目: {e}"))?;
    }
    // 表ノードを組み立てる
    for (n, d) in p.data {
        let mut header = Vec::new();
        if let Some(a) = d.annotation {
            header.push(format!("«{a}»"));
        }
        header.push(d.name);
        let to_rows = |v: Vec<String>| v.into_iter().map(|s| vec![s]).collect::<Vec<_>>();
        if let Some(node) = p.b.g.nodes.get_mut(n) {
            node.kind = NodeKind::Table {
                header,
                sections: vec![to_rows(d.attrs), to_rows(d.methods)],
            };
            node.explicit = true;
        }
    }
    Ok(p.b.finish())
}

impl Parser {
    /// 1 行を処理する。クラス本体が始まった場合はそのノードを返す。
    fn statement(&mut self, l: &str) -> Result<Option<usize>, String> {
        let first = l.split_whitespace().next().unwrap_or("");
        match first {
            "}" => {
                self.ns.pop();
                return Ok(None);
            }
            "direction" => {
                if let Some(d) = l.split_whitespace().nth(1).and_then(Dir::parse) {
                    self.b.g.dir = d;
                }
                return Ok(None);
            }
            "note" | "click" | "link" | "callback" | "cssClass" | "style" | "classDef"
            | "accTitle" | "accDescr" | "accTitle:" | "accDescr:" | "title" => return Ok(None),
            "namespace" => {
                let name = l["namespace".len()..].trim().trim_end_matches('{').trim();
                let parent = self.ns.last().copied();
                let c = self.b.cluster(name, vec![name.to_string()], parent);
                self.ns.push(c);
                return Ok(None);
            }
            "class" => {
                let rest = l["class".len()..].trim();
                let (head, inline) = match rest.split_once('{') {
                    Some((h, b)) => (h.trim(), Some(b)),
                    None => (rest, None),
                };
                let n = self.class(head)?;
                if let Some(b) = inline {
                    match b.trim().strip_suffix('}') {
                        Some(inner) => {
                            for m in inner.split(';') {
                                self.member(n, m);
                            }
                        }
                        None => {
                            self.member(n, b);
                            return Ok(Some(n));
                        }
                    }
                }
                return Ok(None);
            }
            _ => {}
        }
        if let Some(rest) = l.strip_prefix("<<") {
            // <<interface>> Name
            if let Some((a, name)) = rest.split_once(">>") {
                let n = self.class(name)?;
                self.data.entry(n).or_default().annotation = Some(a.trim().to_string());
            }
            return Ok(None);
        }
        if let Some(r) = parse_relation(l) {
            let a = self.class(&r.lhs)?;
            let b = self.class(&r.rhs)?;
            self.b.edge(Edge {
                label: r.label,
                stroke: if r.dotted {
                    EdgeStroke::Dotted
                } else {
                    EdgeStroke::Solid
                },
                start: r.lmark,
                end: r.rmark,
                start_label: r.lcard,
                end_label: r.rcard,
                ..Edge::new(a, b)
            });
            return Ok(None);
        }
        if let Some((name, member)) = l.split_once(':') {
            let n = self.class(name)?;
            self.member(n, member);
            return Ok(None);
        }
        self.class(l)?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::text::preprocess;

    #[test]
    fn parses_classes_members_and_relations() {
        let src = "classDiagram\n  Animal <|-- Duck\n  Animal : +int age\n  Animal: +isMammal()\n  class Duck{\n    +String beakColor\n    +swim()\n  }\n  Customer \"1\" --> \"*\" Ticket : buys\n  class Shape~T~\n  <<interface>> Shape\n  A *-- B\n  C o-- D\n  E ..|> F\n";
        let g = parse(&preprocess(src)).unwrap();
        let animal = g.nodes.iter().find(|n| n.id == "Animal").unwrap();
        match &animal.kind {
            NodeKind::Table { header, sections } => {
                assert_eq!(header, &vec!["Animal".to_string()]);
                assert_eq!(sections[0], vec![vec!["+int age".to_string()]]);
                assert_eq!(sections[1], vec![vec!["+isMammal()".to_string()]]);
            }
            _ => panic!(),
        }
        let shape = g.nodes.iter().find(|n| n.id == "Shape").unwrap();
        match &shape.kind {
            NodeKind::Table { header, .. } => {
                assert_eq!(
                    header,
                    &vec!["«interface»".to_string(), "Shape<T>".to_string()]
                )
            }
            _ => panic!(),
        }
        assert_eq!(g.edges[0].start, Marker::Triangle);
        let buys = &g.edges[1];
        assert_eq!(buys.start_label.as_deref(), Some("1"));
        assert_eq!(buys.end_label.as_deref(), Some("*"));
        assert_eq!(buys.label, vec!["buys"]);
        assert_eq!(g.edges[2].start, Marker::DiamondFilled);
        assert_eq!(g.edges[3].start, Marker::DiamondHollow);
        assert_eq!(g.edges[4].end, Marker::Triangle);
        assert_eq!(g.edges[4].stroke, EdgeStroke::Dotted);
    }
}
