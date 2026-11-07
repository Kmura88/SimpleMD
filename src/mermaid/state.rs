//! stateDiagram / stateDiagram-v2 のパーサ（→ [`Graph`]、描画は flowchart と共通）。

use super::graph::{Builder, Dir, Edge, Graph, Marker, NodeKind, Shape};
use super::text::{clean_label, unquote};

// ==== 機能: 状態参照の解決 ====

/// 状態図パーサの状態。
struct Parser {
    /// グラフ構築器。
    b: Builder,
    /// 開いている複合状態（クラスタ）のスタック。
    stack: Vec<usize>,
}

impl Parser {
    /// 現在の複合状態。
    fn current(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    /// 状態名からノードを得る。`[*]` は開始（左辺）/終了（右辺）ノードとしてスコープごとに作る。
    fn state_ref(&mut self, name: &str, is_source: bool) -> Result<usize, String> {
        let name = name.split(":::").next().unwrap_or("").trim();
        if name.is_empty() {
            return Err("状態名がありません".into());
        }
        if name == "[*]" {
            let scope = self
                .current()
                .map_or(String::new(), |c| self.b.g.clusters[c].id.clone());
            let (id, shape) = if is_source {
                (format!("[*]start@{scope}"), Shape::StateStart)
            } else {
                (format!("[*]end@{scope}"), Shape::StateEnd)
            };
            let n = self.b.node(&id, shape);
            let node = &mut self.b.g.nodes[n];
            node.kind = NodeKind::Shape(shape);
            node.label.clear();
            node.explicit = true;
            let cur = self.current();
            self.b.assign(n, cur);
            return Ok(n);
        }
        if name.contains(char::is_whitespace) {
            return Err(format!("状態名に空白は使えません: `{name}`"));
        }
        let n = self.b.node(name, Shape::Round);
        let cur = self.current();
        self.b.assign(n, cur);
        Ok(n)
    }

    /// `state ...` 文を処理する。
    fn state_decl(&mut self, rest: &str) -> Result<(), String> {
        let rest = rest.trim();
        let opens = rest.ends_with('{');
        let rest = rest.trim_end_matches('{').trim();
        let (id, desc, tail) = if let Some(after_q) = rest.strip_prefix('"') {
            // state "説明" as X
            let q = after_q.find('"').ok_or("閉じ引用符がありません")?;
            let desc = &after_q[..q];
            let after = after_q[q + 1..].trim();
            let id = after
                .strip_prefix("as")
                .map(str::trim)
                .ok_or("`as` が必要です")?;
            let (id, tail) = id.split_once(char::is_whitespace).unwrap_or((id, ""));
            (id.to_string(), Some(desc.to_string()), tail.to_string())
        } else {
            let (id, tail) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let tail = tail.trim();
            if let Some(d) = tail.strip_prefix("as ") {
                // state X as "説明"（非公式だが受け付ける）
                (id.to_string(), Some(unquote(d).to_string()), String::new())
            } else {
                (id.to_string(), None, tail.to_string())
            }
        };
        if id.is_empty() {
            return Err("状態名がありません".into());
        }
        if opens {
            let title = clean_label(desc.as_deref().unwrap_or(&id));
            let parent = self.current();
            let c = self.b.cluster(&id, title, parent);
            self.stack.push(c);
            return Ok(());
        }
        let n = self.state_ref(&id, true)?;
        let node = &mut self.b.g.nodes[n];
        let shape = match tail.trim() {
            "<<choice>>" => Some(Shape::Choice),
            "<<fork>>" | "<<join>>" => Some(Shape::ForkBar),
            _ => None,
        };
        if let Some(s) = shape {
            node.kind = NodeKind::Shape(s);
            node.explicit = true;
        }
        if let Some(d) = desc {
            node.label = clean_label(&d);
            node.explicit = true;
        }
        Ok(())
    }
}

// ==== 機能: 文の解析 ====

/// 状態図を解析する。`lines[0]` はヘッダ行。
pub fn parse(lines: &[(usize, String)]) -> Result<Graph, String> {
    let mut p = Parser {
        b: Builder::new(),
        stack: Vec::new(),
    };
    let mut in_note = false;
    for (no, line) in lines.iter().skip(1) {
        let l = line.trim();
        if in_note {
            if l == "end note" {
                in_note = false;
            }
            continue;
        }
        p.statement(l, &mut in_note)
            .map_err(|e| format!("{no} 行目: {e}"))?;
    }
    Ok(p.b.finish())
}

impl Parser {
    /// 1 行を処理する。
    fn statement(&mut self, l: &str, in_note: &mut bool) -> Result<(), String> {
        let first = l.split_whitespace().next().unwrap_or("");
        match first {
            "note" => {
                if !l.contains(':') {
                    *in_note = true;
                }
                return Ok(());
            }
            "}" => {
                self.stack.pop();
                return Ok(());
            }
            "--" | "hide" | "scale" | "classDef" | "class" | "style" | "accTitle" | "accDescr"
            | "accTitle:" | "accDescr:" | "title" | "click" => return Ok(()),
            "direction" => {
                if self.stack.is_empty()
                    && let Some(d) = l.split_whitespace().nth(1).and_then(Dir::parse)
                {
                    self.b.g.dir = d;
                }
                return Ok(());
            }
            "state" => return self.state_decl(&l["state".len()..]),
            _ => {}
        }
        if let Some(pos) = l.find("-->") {
            let lhs = &l[..pos];
            let rhs_all = &l[pos + 3..];
            let (rhs, label) = match rhs_all.split_once(':') {
                Some((r, lb)) => (r, clean_label(lb)),
                None => (rhs_all, Vec::new()),
            };
            let a = self.state_ref(lhs, true)?;
            let b = self.state_ref(rhs, false)?;
            self.b.edge(Edge {
                label,
                end: Marker::Arrow,
                ..Edge::new(a, b)
            });
            return Ok(());
        }
        if let Some((id, desc)) = l.split_once(':') {
            let n = self.state_ref(id, true)?;
            let lines = clean_label(desc);
            let node = &mut self.b.g.nodes[n];
            if node.explicit {
                node.label.extend(lines);
            } else {
                node.label = lines;
                node.explicit = true;
            }
            return Ok(());
        }
        self.state_ref(l, true).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::text::preprocess;

    #[test]
    fn parses_basic_state_diagram() {
        let src = "stateDiagram-v2\n  [*] --> Still\n  Still --> [*]\n  Still --> Moving : push\n  Moving --> Crash\n  Crash --> [*]\n  state \"Long description\" as LD\n  LD : extra";
        let g = parse(&preprocess(src)).unwrap();
        let start = g
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Shape(Shape::StateStart))
            .count();
        let end = g
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Shape(Shape::StateEnd))
            .count();
        assert_eq!((start, end), (1, 1));
        assert_eq!(g.edges.len(), 5);
        assert_eq!(g.edges[2].label, vec!["push"]);
        let ld = g.nodes.iter().find(|n| n.id == "LD").unwrap();
        assert_eq!(ld.label, vec!["Long description", "extra"]);
    }

    #[test]
    fn parses_composite_states_and_choice() {
        let src = "stateDiagram\n direction LR\n [*] --> First\n state First {\n [*] --> second\n second --> [*]\n }\n state c <<choice>>\n First --> c\n note right of c\n text\n end note";
        let g = parse(&preprocess(src)).unwrap();
        assert_eq!(g.dir, Dir::LR);
        assert_eq!(g.clusters.len(), 1);
        let second = g.nodes.iter().find(|n| n.id == "second").unwrap();
        assert_eq!(second.cluster, Some(0));
        let c = g.nodes.iter().find(|n| n.id == "c").unwrap();
        assert_eq!(c.kind, NodeKind::Shape(Shape::Choice));
        assert!(g.nodes.iter().all(|n| n.id != "First"));
    }
}
