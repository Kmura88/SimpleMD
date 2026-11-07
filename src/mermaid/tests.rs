//! 結合テスト: レイアウトの幾何的性質と、どんな入力でもパニックしないことの確認。

use egui::{Rect, Vec2, vec2};

use super::graph::Graph;
use super::graph_draw::{Computed, compute};
use super::text::{Measurer, preprocess};
use super::{build_scene, class, er, flowchart, state};

// ==== 機能: テスト補助 ====

/// 簡易計測関数（ASCII は 0.6 em、それ以外は 1 em）。
fn fake_measure(s: &str, size: f32) -> Vec2 {
    let w: f32 = s
        .chars()
        .map(|c| if c.is_ascii() { 0.6 } else { 1.0 })
        .sum::<f32>()
        * size;
    vec2(w, size * 1.25)
}

/// flowchart を解析してレイアウトする。
fn flow(src: &str) -> (Graph, Computed) {
    let g = flowchart::parse(&preprocess(src)).expect("parse");
    let mut f = fake_measure;
    let mut m = Measurer::new(&mut f, 14.0);
    let c = compute(&g, &mut m);
    (g, c)
}

/// ノード ID から矩形を引く。
fn rect_of(g: &Graph, c: &Computed, id: &str) -> Rect {
    let i = g.nodes.iter().position(|n| n.id == id).expect("node id");
    c.node_rects[i]
}

/// どの 2 ノードも重ならないことを確認する。
fn assert_no_overlap(g: &Graph, c: &Computed) {
    for i in 0..c.node_rects.len() {
        for j in i + 1..c.node_rects.len() {
            let (a, b) = (c.node_rects[i].shrink(0.5), c.node_rects[j].shrink(0.5));
            assert!(
                !a.intersects(b),
                "ノード {} と {} が重なっています: {a:?} {b:?}",
                g.nodes[i].id,
                g.nodes[j].id
            );
        }
    }
}

/// エッジラベルがノードと重ならないことを確認する。
fn assert_labels_clear(c: &Computed) {
    for (lc, sz) in c.label_centers.iter().zip(&c.label_sizes) {
        let Some(lc) = lc else { continue };
        let lr = Rect::from_center_size(*lc, *sz).shrink(0.5);
        for nr in &c.node_rects {
            assert!(
                !lr.intersects(*nr),
                "ラベル {lr:?} がノード {nr:?} と重なっています"
            );
        }
    }
}

/// 図全体がシーンの大きさに収まることを確認する。
fn assert_inside(c: &Computed) {
    let whole = Rect::from_min_size(egui::Pos2::ZERO, c.size).expand(0.5);
    for r in &c.node_rects {
        assert!(
            whole.contains_rect(*r),
            "{r:?} が図の外にあります {whole:?}"
        );
    }
}

const BIG_FLOW: &str = "flowchart TD
    A[Christmas] -->|Get money| B(Go shopping)
    B --> C{Let me think}
    C -->|One| D[Laptop]
    C -->|Two| E[iPhone]
    C -->|Three| F[fa:fa-car Car]
    D --> G[(Database)]
    E --> G
    F --> H((End))
    G --> H
    A --> H";

// ==== 機能: レイアウトの性質 ====

#[test]
fn td_ranks_increase_along_edges() {
    let (g, c) = flow(BIG_FLOW);
    assert_no_overlap(&g, &c);
    assert_labels_clear(&c);
    assert_inside(&c);
    for e in &g.edges {
        let (a, b) = (c.node_rects[e.from], c.node_rects[e.to]);
        assert!(
            a.center().y < b.center().y,
            "TD では下向き: {} -> {}",
            g.nodes[e.from].id,
            g.nodes[e.to].id
        );
        assert!(a.bottom() < b.top());
    }
}

#[test]
fn lr_swaps_axes() {
    let src = BIG_FLOW.replacen("flowchart TD", "flowchart LR", 1);
    let (g, c) = flow(&src);
    assert_no_overlap(&g, &c);
    assert_labels_clear(&c);
    assert_inside(&c);
    for e in &g.edges {
        let (a, b) = (c.node_rects[e.from], c.node_rects[e.to]);
        assert!(a.right() < b.left(), "LR では右向き");
    }
}

#[test]
fn bt_and_rl_reverse() {
    let (g, c) = flow("graph BT\nA-->B-->C");
    assert!(rect_of(&g, &c, "A").center().y > rect_of(&g, &c, "C").center().y);
    let (g, c) = flow("graph RL\nA-->B-->C");
    assert!(rect_of(&g, &c, "A").center().x > rect_of(&g, &c, "C").center().x);
}

#[test]
fn subgraph_box_contains_members() {
    let src = "flowchart TB
    c1-->a2
    subgraph one
    a1-->a2
    end
    subgraph two
    b1-->b2
    end
    subgraph three
    c1-->c2
    subgraph inner [Inner box]
      c3 --> c4
    end
    end
    one --> two
    three --> two
    two --> c2
    c2 --> c3";
    let (g, c) = flow(src);
    assert_no_overlap(&g, &c);
    assert_inside(&c);
    for (ni, n) in g.nodes.iter().enumerate() {
        let mut cur = n.cluster;
        while let Some(ci) = cur {
            let cr = c.cluster_rects[ci].expect("cluster rect");
            assert!(
                cr.contains_rect(c.node_rects[ni]),
                "{} がクラスタ {} の外にあります",
                n.id,
                g.clusters[ci].id
            );
            cur = g.clusters[ci].parent;
        }
    }
    // 非メンバーは枠と重ならない（入れ子でない兄弟クラスタ同士も重ならない）
    for (ci, cl) in g.clusters.iter().enumerate() {
        let cr = c.cluster_rects[ci].unwrap().shrink(0.5);
        for (ni, n) in g.nodes.iter().enumerate() {
            let member = n.cluster.is_some_and(|nc| g.is_ancestor(ci, nc));
            if !member {
                assert!(
                    !cr.intersects(c.node_rects[ni]),
                    "{} が {} の枠と重なっています",
                    n.id,
                    cl.id
                );
            }
        }
        for (cj, _) in g.clusters.iter().enumerate() {
            if ci != cj && !g.is_ancestor(ci, cj) && !g.is_ancestor(cj, ci) {
                let other = c.cluster_rects[cj].unwrap().shrink(0.5);
                assert!(
                    !cr.intersects(other),
                    "クラスタ {ci} と {cj} が重なっています"
                );
            }
        }
    }
}

#[test]
fn lr_subgraphs_and_cycles_do_not_overlap() {
    let src = "graph LR
    subgraph s1 [Frontend]
      ui[UI] --> api{{API}}
    end
    subgraph s2 [Backend]
      api --> svc[/Service/] --> db[(DB)]
      svc --> cache[(Cache)]
    end
    db --> ui
    cache -.->|miss| db
    ui --> ui";
    let (g, c) = flow(src);
    assert_no_overlap(&g, &c);
    assert_labels_clear(&c);
    assert_inside(&c);
}

#[test]
fn wide_fanout_and_cjk_labels() {
    let mut src = String::from("graph TD\nroot[ルートノード]\n");
    for i in 0..12 {
        src.push_str(&format!(
            "root --> n{i}[子ノード {i}]\nn{i} --> leaf{}\n",
            i % 4
        ));
    }
    let (g, c) = flow(&src);
    assert_no_overlap(&g, &c);
    assert_inside(&c);
}

#[test]
fn state_class_er_layouts_do_not_overlap() {
    let mut fm = fake_measure;
    let srcs_state = "stateDiagram-v2\n[*] --> Still\nStill --> [*]\nStill --> Moving\nMoving --> Still\nMoving --> Crash\nCrash --> [*]\nstate Moving {\n[*] --> Slow\nSlow --> Fast\n}";
    let g = state::parse(&preprocess(srcs_state)).unwrap();
    let mut m = Measurer::new(&mut fm, 14.0);
    let c = compute(&g, &mut m);
    assert_no_overlap(&g, &c);
    let g = class::parse(&preprocess(
        "classDiagram\nAnimal <|-- Duck\nAnimal <|-- Fish\nAnimal : +int age\nclass Duck{\n+swim()\n}",
    ))
    .unwrap();
    let c = compute(&g, &mut m);
    assert_no_overlap(&g, &c);
    let g = er::parse(&preprocess(
        "erDiagram\nCUSTOMER ||--o{ ORDER : places\nORDER ||--|{ ITEM : contains\nCUSTOMER {\nstring name\n}",
    ))
    .unwrap();
    let c = compute(&g, &mut m);
    assert_no_overlap(&g, &c);
    assert_labels_clear(&c);
}

// ==== 機能: 全図種の描画とパニック耐性 ====

/// 各図種の代表的なサンプル。
const SAMPLES: &[&str] = &[
    BIG_FLOW,
    "graph LR; A-->B; B-->C; C-->A",
    "flowchart TB\nA & B --> C & D\nsubgraph S [Title]\nC\nend",
    "sequenceDiagram\nautonumber\nparticipant A as Alice\nactor B as Bob\nA->>+B: Hello Bob<br>how are you?\nB-->>-A: Fine\nA->A: think\nNote over A,B: 注意\nloop every minute\nA-)B: ping\nend\nalt ok\nA->>B: yes\nelse no\nA-xB: no\nend\npar\nA->>B: p1\nand\nB->>A: p2\nend\nrect rgb(200,220,255)\nA->>B: in rect\nend",
    "pie showData title Pets\n\"Dogs\" : 386\n\"Cats\" : 85\n\"Rats\" : 15",
    "stateDiagram-v2\n[*] --> A\nA --> B : go\nB --> [*]\nstate F <<fork>>\nA --> F\nF --> B",
    "classDiagram\nclass A{\n+x int\n+f() void\n}\nA <|-- B\nA \"1\" *-- \"many\" C : has",
    "erDiagram\nA ||--o{ B : r\nB }|..|| C : s",
];

#[test]
fn all_samples_build() {
    let mut fm = fake_measure;
    for s in SAMPLES {
        let scene = build_scene(s, 14.0, &mut fm).unwrap_or_else(|e| panic!("{e}\n{s}"));
        assert!(scene.size.x > 10.0 && scene.size.y > 10.0);
        assert!(!scene.items.is_empty());
        let b = scene.bounds();
        assert!(b.min.x >= -1.0 && b.min.y >= -1.0, "{b:?}\n{s}");
        assert!(
            b.max.x <= scene.size.x + 1.0 && b.max.y <= scene.size.y + 1.0,
            "{b:?} {:?}\n{s}",
            scene.size
        );
    }
}

#[test]
fn unsupported_types_are_errors() {
    let mut fm = fake_measure;
    assert!(build_scene("gantt\ntitle x", 14.0, &mut fm).is_err());
    assert!(build_scene("", 14.0, &mut fm).is_err());
    assert!(build_scene("%% only comment", 14.0, &mut fm).is_err());
    assert!(!super::is_supported("journey"));
    assert!(super::is_supported("graph TD"));
}

/// 決定的な擬似乱数（線形合同法）。
struct Lcg(u64);

impl Lcg {
    /// 次の値。
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn never_panics_on_malformed_input() {
    let mut fm = fake_measure;
    let mut inputs: Vec<String> = vec![
        "graph".into(),
        "graph TD\n-->".into(),
        "graph TD\nA-->".into(),
        "graph TD\nA[".into(),
        "graph TD\nA[\"unterminated".into(),
        "graph TD\nA((((x))))".into(),
        "graph TD\nsubgraph\nend\nend\nend".into(),
        "graph TD\nA --> A --> A".into(),
        "graph TD\nA -- text without end".into(),
        "graph TD\nA-. .->B\nC==  ==>D".into(),
        "graph TD\n&&&".into(),
        "graph TD\nA & --> B".into(),
        "graph TD\nA-->|unterminated label B".into(),
        "graph XX\nA-->B".into(),
        "flowchart LR\nsubgraph a\nsubgraph b\nx-->a\nend\nend\na-->b".into(),
        "sequenceDiagram".into(),
        "sequenceDiagram\nA->>".into(),
        "sequenceDiagram\n->>B: x".into(),
        "sequenceDiagram\nNote over : x".into(),
        "sequenceDiagram\nloop\nloop\nA->>B:x".into(),
        "sequenceDiagram\nA->>-B: x\ndeactivate B\ndeactivate B".into(),
        "sequenceDiagram\nelse\nend".into(),
        "pie".into(),
        "pie\n\"a\":".into(),
        "pie\n\"a\": 1e999".into(),
        "pie\n\"a\": 0".into(),
        "stateDiagram\n[*] -->".into(),
        "stateDiagram\nstate {\n}\n}".into(),
        "stateDiagram\nstate \"x".into(),
        "classDiagram\nclass {".into(),
        "classDiagram\n<|--".into(),
        "classDiagram\nA <|-- \"1".into(),
        "erDiagram\nA ||--o{".into(),
        "erDiagram\nA {\n".into(),
        "erDiagram\nA ||--o{ B".into(),
        "\u{feff}graph TD\n\u{0}-->\u{7f}".into(),
        "graph TD\n日本語-->中文[\"全角「」\"]".into(),
        "---\n---\n---".into(),
    ];
    // サンプルのあらゆる位置での切り詰めと、ランダムな文字の挿入・削除
    let noise = [
        '[', ']', '(', ')', '{', '}', '"', '|', '-', '>', '<', '.', '=', ':', ';', '\n', ' ', '&',
        'o', 'x', '%', '#', 'あ',
    ];
    let mut rng = Lcg(42);
    for s in SAMPLES {
        let chars: Vec<char> = s.chars().collect();
        for cut in 0..chars.len() {
            inputs.push(chars[..cut].iter().collect());
        }
        for _ in 0..150 {
            let mut c = chars.clone();
            for _ in 0..(1 + rng.next() % 4) {
                let pos = (rng.next() as usize) % (c.len() + 1);
                if rng.next().is_multiple_of(3) && pos < c.len() {
                    c.remove(pos);
                } else {
                    c.insert(pos, noise[(rng.next() as usize) % noise.len()]);
                }
            }
            inputs.push(c.into_iter().collect());
        }
    }
    for input in &inputs {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = build_scene(input, 14.0, &mut fm);
        }));
        assert!(r.is_ok(), "パニックしました: {input:?}");
    }
    // 異常なフォントサイズ
    for size in [0.0, -5.0, f32::NAN, f32::INFINITY, 1000.0] {
        let _ = build_scene(BIG_FLOW, size, &mut fm);
    }
    // 異常な計測結果
    let mut bad = |_: &str, _: f32| vec2(f32::NAN, -1.0);
    for s in SAMPLES {
        let _ = build_scene(s, 14.0, &mut bad);
    }
}

#[test]
fn large_graph_is_fast_enough() {
    let mut src = String::from("graph TD\n");
    for i in 0..150 {
        src.push_str(&format!("n{i} --> n{}\n", (i * 7 + 3) % 150));
        src.push_str(&format!("n{i} --> n{}\n", (i * 13 + 1) % 150));
    }
    let mut fm = fake_measure;
    let t = std::time::Instant::now();
    let _ = build_scene(&src, 14.0, &mut fm).unwrap();
    assert!(t.elapsed().as_secs_f32() < 5.0, "{:?}", t.elapsed());
}
