//! erDiagram のパーサ（→ [`Graph`]、エンティティは表ノードとして描く）。

use std::collections::HashMap;

use super::graph::{Builder, Dir, Edge, EdgeStroke, Graph, Marker, NodeKind, Shape};
use super::text::{clean_label, unquote};

// ==== 機能: カーディナリティ記号 ====

/// 左側の記号（`|o` `||` `}o` `}|`）を解釈する。
fn left_marker(s: &str) -> Option<Marker> {
    Some(match s {
        "|o" => Marker::Er {
            zero: true,
            many: false,
        },
        "||" => Marker::Er {
            zero: false,
            many: false,
        },
        "}o" => Marker::Er {
            zero: true,
            many: true,
        },
        "}|" => Marker::Er {
            zero: false,
            many: true,
        },
        _ => return None,
    })
}

/// 右側の記号（`o|` `||` `o{` `|{`）を解釈する。
fn right_marker(s: &str) -> Option<Marker> {
    let mirrored: String = s
        .chars()
        .rev()
        .map(|c| if c == '{' { '}' } else { c })
        .collect();
    left_marker(&mirrored)
}

/// `||--o{` のような関係記号を (左記号, 右記号, 点線か) に分解する。
fn parse_rel(tok: &str) -> Option<(Marker, Marker, bool)> {
    if tok.chars().count() != 6 || !tok.is_ascii() {
        return None;
    }
    let (l, line, r) = (&tok[0..2], &tok[2..4], &tok[4..6]);
    let dotted = match line {
        "--" => false,
        ".." => true,
        _ => return None,
    };
    Some((left_marker(l)?, right_marker(r)?, dotted))
}

// ==== 機能: 文の解析 ====

/// ER 図を解析する。`lines[0]` はヘッダ行。
pub fn parse(lines: &[(usize, String)]) -> Result<Graph, String> {
    let mut b = Builder::new();
    let mut attrs: HashMap<usize, Vec<Vec<String>>> = HashMap::new();
    let mut body: Option<usize> = None;
    for (no, line) in lines.iter().skip(1) {
        let l = line.trim();
        let err = |e: &str| format!("{no} 行目: {e}");
        if let Some(n) = body {
            if l.starts_with('}') {
                body = None;
            } else {
                let row = parse_attr(l).ok_or_else(|| err("属性を解釈できません"))?;
                attrs.entry(n).or_default().push(row);
            }
            continue;
        }
        let first = l.split_whitespace().next().unwrap_or("");
        match first {
            "direction" => {
                if let Some(d) = l.split_whitespace().nth(1).and_then(Dir::parse) {
                    b.g.dir = d;
                }
                continue;
            }
            "title" | "accTitle" | "accDescr" | "accTitle:" | "accDescr:" | "classDef"
            | "class" | "style" => continue,
            _ => {}
        }
        if let Some(head) = l.strip_suffix('{') {
            let n = entity(&mut b, head).ok_or_else(|| err("エンティティ名が不正です"))?;
            attrs.entry(n).or_default();
            body = Some(n);
            continue;
        }
        let (rel_part, label) = match l.split_once(':') {
            Some((a, lb)) => (a, clean_label(lb)),
            None => (l, Vec::new()),
        };
        let toks: Vec<&str> = rel_part.split_whitespace().collect();
        match toks.as_slice() {
            [a, rel, c] => {
                let (lm, rm, dotted) =
                    parse_rel(rel).ok_or_else(|| err("関係記号を解釈できません"))?;
                let a = entity(&mut b, a).ok_or_else(|| err("エンティティ名が不正です"))?;
                let c = entity(&mut b, c).ok_or_else(|| err("エンティティ名が不正です"))?;
                b.edge(Edge {
                    label,
                    stroke: if dotted {
                        EdgeStroke::Dotted
                    } else {
                        EdgeStroke::Solid
                    },
                    start: lm,
                    end: rm,
                    ..Edge::new(a, c)
                });
            }
            [a] => {
                entity(&mut b, a).ok_or_else(|| err("エンティティ名が不正です"))?;
            }
            _ => return Err(err("解釈できない記述です")),
        }
    }
    for (i, node) in b.g.nodes.iter_mut().enumerate() {
        let rows = attrs.remove(&i).unwrap_or_default();
        let sections = if rows.is_empty() {
            Vec::new()
        } else {
            vec![rows]
        };
        node.kind = NodeKind::Table {
            header: node.label.clone(),
            sections,
        };
    }
    Ok(b.finish())
}

/// エンティティ名（`NAME` / `NAME["別名"]` / `"NAME"`）からノードを得る。
fn entity(b: &mut Builder, raw: &str) -> Option<usize> {
    let raw = raw.trim();
    let (id, alias) = match raw.find('[') {
        Some(k) => (&raw[..k], Some(unquote(raw[k + 1..].trim_end_matches(']')))),
        None => (raw, None),
    };
    let id = unquote(id).trim();
    if id.is_empty() {
        return None;
    }
    let n = b.node(id, Shape::Rect);
    if let Some(a) = alias {
        b.g.nodes[n].label = vec![a.to_string()];
    }
    Some(n)
}

/// 属性行 `type name [PK, FK] ["comment"]` を [型, 名前, キーと注釈] に分解する。
fn parse_attr(l: &str) -> Option<Vec<String>> {
    let (main, comment) = match l.find('"') {
        Some(q) => (&l[..q], Some(unquote(&l[q..]).to_string())),
        None => (l, None),
    };
    let mut toks = main.split_whitespace();
    let ty = toks.next()?;
    let name = toks.next()?;
    let keys: Vec<&str> = toks
        .flat_map(|t| t.split(','))
        .filter(|t| !t.is_empty())
        .collect();
    let mut extra = keys.join(", ");
    if let Some(c) = comment {
        if !extra.is_empty() {
            extra.push(' ');
        }
        extra.push_str(&c);
    }
    Some(vec![ty.to_string(), name.to_string(), extra])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::text::preprocess;

    #[test]
    fn parses_relations_and_attributes() {
        let src = "erDiagram\n  CUSTOMER ||--o{ ORDER : places\n  ORDER ||--|{ LINE-ITEM : contains\n  CUSTOMER }|..|{ DELIVERY-ADDRESS : uses\n  CUSTOMER {\n    string name PK \"the name\"\n    int age\n  }";
        let g = parse(&preprocess(src)).unwrap();
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.edges.len(), 3);
        assert_eq!(
            g.edges[0].start,
            Marker::Er {
                zero: false,
                many: false
            }
        );
        assert_eq!(
            g.edges[0].end,
            Marker::Er {
                zero: true,
                many: true
            }
        );
        assert_eq!(
            g.edges[1].end,
            Marker::Er {
                zero: false,
                many: true
            }
        );
        assert_eq!(g.edges[2].stroke, EdgeStroke::Dotted);
        assert_eq!(g.edges[0].label, vec!["places"]);
        match &g.nodes[0].kind {
            NodeKind::Table { sections, .. } => {
                assert_eq!(sections[0][0], vec!["string", "name", "PK the name"]);
                assert_eq!(sections[0][1], vec!["int", "age", ""]);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn rejects_bad_relation() {
        assert!(parse(&preprocess("erDiagram\n A ??--?? B : x")).is_err());
    }
}
