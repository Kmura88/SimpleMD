//! 階層型（Sugiyama 方式）グラフレイアウト。
//!
//! 座標は向きに依存しない抽象空間で計算する: `x` が交差軸（同じランク内の並び）、
//! `y` がランク軸（エッジの流れる向き）。LR などへの変換は呼び出し側で行う。
//!
//! 手順:
//! 1. 閉路の除去（DFS で後退辺を反転）
//! 2. ランク付け（最長路 + 始点の引き寄せ）。ラベル付きエッジがあればランクを 2 倍にし、
//!    中間ランクにラベル用ダミーを置く
//! 3. 長いエッジをダミーノードで分割
//! 4. 重心法による交差削減（クラスタの連続性を保つ階層ソート）
//! 5. 座標決定（隣接ランクの重心へ寄せる等張回帰 + クラスタ同士の重なり解消）

use egui::{Pos2, Rect, Vec2, pos2, vec2};

// ==== 機能: 入出力の型 ====

/// レイアウト対象のノード。
#[derive(Clone, Debug)]
pub struct LNode {
    /// 大きさ（x: 交差軸方向、y: ランク軸方向）。
    pub size: Vec2,
    /// 直接所属するクラスタ。
    pub cluster: Option<usize>,
}

/// レイアウト対象のエッジ。
#[derive(Clone, Debug)]
pub struct LEdge {
    /// 始点ノード。
    pub from: usize,
    /// 終点ノード。
    pub to: usize,
    /// 最小ランク差（1 以上）。
    pub min_len: usize,
    /// ラベルの大きさ（交差軸, ランク軸）。
    pub label: Option<Vec2>,
}

/// レイアウト対象のクラスタ（サブグラフ）。
#[derive(Clone, Debug, Default)]
pub struct LCluster {
    /// 親クラスタ。
    pub parent: Option<usize>,
    /// 基本余白に加える、座標の小さい側の余白（タイトル用）。
    pub pad_lo: Vec2,
    /// 基本余白に加える、座標の大きい側の余白。
    pub pad_hi: Vec2,
    /// 最小の大きさ（タイトル幅など）。
    pub min_size: Vec2,
}

/// 間隔などのパラメータ。
#[derive(Clone, Debug)]
pub struct Params {
    /// 同じランク内のノード間隔。
    pub node_sep: f32,
    /// ランク間隔。
    pub rank_sep: f32,
    /// クラスタ枠の内側余白。
    pub cluster_pad: f32,
    /// 図全体の外側余白。
    pub margin: f32,
}

/// エッジの経路。
#[derive(Clone, Debug, Default)]
pub struct EdgeRoute {
    /// 始点→終点の順に並んだ中間点（ダミーノードの中心）。
    pub points: Vec<Pos2>,
    /// ラベルの中心。
    pub label: Option<Pos2>,
    /// 自己ループかどうか（経路は描画側で作る）。
    pub self_loop: bool,
}

/// レイアウト結果。
#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// 各ノードの中心。
    pub nodes: Vec<Pos2>,
    /// 各エッジの経路（入力と同じ順）。
    pub edges: Vec<EdgeRoute>,
    /// 各クラスタの枠（メンバーがなければ `None`）。
    pub clusters: Vec<Option<Rect>>,
    /// 全体の大きさ。
    pub size: Vec2,
}

// ==== 機能: 作業用データ ====

/// 非巡回化後のエッジ（作業用）。
struct DagEdge {
    /// 上流ノード。
    u: usize,
    /// 下流ノード。
    v: usize,
    /// 最小ランク差。
    len: usize,
    /// 元のエッジ番号。
    orig: usize,
    /// 元の向きから反転したか。
    reversed: bool,
}

/// ダミーを含む拡張グラフの作業状態。
struct Work<'a> {
    /// パラメータ。
    p: &'a Params,
    /// クラスタ定義。
    clusters: &'a [LCluster],
    /// 実ノード数（これ以降はダミー）。
    n_real: usize,
    /// 各ノードのランク。
    rank: Vec<usize>,
    /// 各ノードの大きさ。
    size: Vec<Vec2>,
    /// ラベルを持たない細いダミーかどうか。
    thin: Vec<bool>,
    /// 各ノードの所属クラスタ列（外側→内側）。
    chain: Vec<Vec<usize>>,
    /// 上流隣接（ノード, 重み）。
    preds: Vec<Vec<(usize, f32)>>,
    /// 下流隣接（ノード, 重み）。
    succs: Vec<Vec<(usize, f32)>>,
    /// ランクごとの並び。
    layers: Vec<Vec<usize>>,
    /// ランク内の位置。
    pos: Vec<usize>,
    /// 交差軸座標。
    x: Vec<f32>,
    /// ランクごとのランク軸座標。
    rank_y: Vec<f32>,
    /// クラスタの子クラスタ一覧。
    children: Vec<Vec<usize>>,
    /// クラスタの深さ（最上位 = 0）。
    depth: Vec<usize>,
    /// ラベルのためにランクを 2 倍にしたか（ランク間隔を半分にする）。
    label_ranks: bool,
}

// ==== 機能: レイアウト本体 ====

/// 階層型レイアウトを実行する。どんな入力でもパニックしない。
pub fn layout(nodes: &[LNode], edges: &[LEdge], clusters: &[LCluster], p: &Params) -> Layout {
    let n = nodes.len();
    let cluster_chain = cluster_chains(clusters);
    let mut routes = vec![EdgeRoute::default(); edges.len()];

    // ---- 1. 閉路除去 ----
    let has_labels = edges
        .iter()
        .any(|e| e.label.is_some() && e.from != e.to && e.from < n && e.to < n);
    let mult = if has_labels { 2 } else { 1 };
    let mut dag: Vec<DagEdge> = Vec::new();
    for (i, e) in edges.iter().enumerate() {
        if e.from >= n || e.to >= n {
            continue;
        }
        if e.from == e.to {
            routes[i].self_loop = true;
            continue;
        }
        dag.push(DagEdge {
            u: e.from,
            v: e.to,
            len: e.min_len.clamp(1, 16) * mult,
            orig: i,
            reversed: false,
        });
    }
    break_cycles(n, &mut dag);

    // ---- 2. ランク付け ----
    let rank = assign_ranks(n, &dag);

    // ---- 3. ダミー挿入と作業状態の構築 ----
    let mut w = Work {
        p,
        clusters,
        n_real: n,
        rank,
        size: nodes.iter().map(|nd| sanitize_size(nd.size)).collect(),
        thin: vec![false; n],
        chain: nodes
            .iter()
            .map(|nd| {
                nd.cluster
                    .and_then(|c| cluster_chain.get(c).cloned())
                    .unwrap_or_default()
            })
            .collect(),
        preds: vec![Vec::new(); n],
        succs: vec![Vec::new(); n],
        layers: Vec::new(),
        pos: Vec::new(),
        x: Vec::new(),
        rank_y: Vec::new(),
        children: vec![Vec::new(); clusters.len()],
        depth: cluster_chain
            .iter()
            .map(|c| c.len().saturating_sub(1))
            .collect(),
        label_ranks: has_labels,
    };
    for (c, chain) in cluster_chain.iter().enumerate() {
        if chain.len() >= 2 {
            let parent = chain[chain.len() - 2];
            w.children[parent].push(c);
        }
    }
    let ranges = w.cluster_rank_ranges();
    let mut dummy_lists: Vec<(usize, Vec<usize>, Option<usize>, bool)> = Vec::new();
    for e in &dag {
        let label = edges[e.orig].label.map(sanitize_size);
        let (list, label_node) = w.insert_chain(e.u, e.v, label, &ranges);
        dummy_lists.push((e.orig, list, label_node, e.reversed));
    }

    // ---- 4. 並び順（交差削減） ----
    w.init_order();
    w.reduce_crossings();

    // ---- 5. 座標決定 ----
    w.assign_y(&ranges);
    w.assign_x();
    w.fix_cluster_overlaps();

    // ---- 6. 正規化と出力 ----
    let mut cluster_rects = w.cluster_rects();
    let mut bounds = Rect::NOTHING;
    for v in 0..w.rank.len() {
        bounds = bounds.union(w.node_rect(v));
    }
    for r in cluster_rects.iter().flatten() {
        bounds = bounds.union(*r);
    }
    if !bounds.is_positive() {
        bounds = Rect::from_min_size(Pos2::ZERO, Vec2::ZERO);
    }
    let shift = vec2(p.margin, p.margin) - bounds.min.to_vec2();
    let center = |w: &Work, v: usize| w.center(v) + shift;

    for (orig, list, label_node, reversed) in &dummy_lists {
        let mut pts: Vec<Pos2> = list.iter().map(|&d| center(&w, d)).collect();
        if *reversed {
            pts.reverse();
        }
        routes[*orig].points = pts;
        routes[*orig].label = label_node.map(|d| center(&w, d));
    }
    for r in cluster_rects.iter_mut().flatten() {
        *r = r.translate(shift);
    }
    Layout {
        nodes: (0..n).map(|v| center(&w, v)).collect(),
        edges: routes,
        clusters: cluster_rects,
        size: bounds.size() + vec2(p.margin, p.margin) * 2.0,
    }
}

/// 大きさの異常値（NaN・負値・巨大値）を補正する。
fn sanitize_size(s: Vec2) -> Vec2 {
    let f = |v: f32| {
        if v.is_finite() {
            v.clamp(0.0, 1.0e5)
        } else {
            0.0
        }
    };
    vec2(f(s.x), f(s.y))
}

/// 各クラスタの所属列（外側→自身）を求める。親の循環や範囲外は打ち切る。
fn cluster_chains(clusters: &[LCluster]) -> Vec<Vec<usize>> {
    (0..clusters.len())
        .map(|c| {
            let mut chain = vec![c];
            let mut cur = clusters[c].parent;
            while let Some(pc) = cur {
                if pc >= clusters.len() || chain.contains(&pc) {
                    break;
                }
                chain.push(pc);
                cur = clusters[pc].parent;
            }
            chain.reverse();
            chain
        })
        .collect()
}

// ==== 機能: 閉路除去 ====

/// DFS で後退辺を見つけて反転し、グラフを非巡回にする。
/// 入次数 0 のノードから先に探索するので、自然な向きが保たれやすい。
fn break_cycles(n: usize, dag: &mut [DagEdge]) {
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for (i, e) in dag.iter().enumerate() {
        out[e.u].push(i);
        indeg[e.v] += 1;
    }
    let mut starts: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    starts.extend((0..n).filter(|&v| indeg[v] != 0));
    let mut state = vec![0u8; n]; // 0: 未訪問, 1: 探索中, 2: 完了
    for s in starts {
        if state[s] != 0 {
            continue;
        }
        state[s] = 1;
        let mut stack = vec![(s, 0usize)];
        while let Some(&(v, k)) = stack.last() {
            if let Some(&ei) = out[v].get(k) {
                if let Some(top) = stack.last_mut() {
                    top.1 += 1;
                }
                let t = dag[ei].v;
                match state[t] {
                    0 => {
                        state[t] = 1;
                        stack.push((t, 0));
                    }
                    1 => dag[ei].reversed = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    for e in dag.iter_mut().filter(|e| e.reversed) {
        std::mem::swap(&mut e.u, &mut e.v);
    }
}

// ==== 機能: ランク付け ====

/// 最長路法でランクを付け、始点ノードを後続の直前まで引き寄せる。
fn assign_ranks(n: usize, dag: &[DagEdge]) -> Vec<usize> {
    let mut indeg = vec![0usize; n];
    let mut out: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for e in dag {
        indeg[e.v] += 1;
        out[e.u].push((e.v, e.len));
    }
    let has_in: Vec<bool> = indeg.iter().map(|&d| d > 0).collect();
    let mut queue: std::collections::VecDeque<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut topo = Vec::with_capacity(n);
    let mut rank = vec![0i64; n];
    while let Some(v) = queue.pop_front() {
        topo.push(v);
        for &(t, len) in &out[v] {
            rank[t] = rank[t].max(rank[v] + len as i64);
            indeg[t] -= 1;
            if indeg[t] == 0 {
                queue.push_back(t);
            }
        }
    }
    // 始点（入辺なし）を後続の直前へ寄せる
    for &v in topo.iter().rev() {
        if !has_in[v] && !out[v].is_empty() {
            let r = out[v].iter().map(|&(t, len)| rank[t] - len as i64).min();
            if let Some(r) = r {
                rank[v] = r.max(0);
            }
        }
    }
    let min = rank.iter().copied().min().unwrap_or(0);
    rank.iter().map(|&r| (r - min).max(0) as usize).collect()
}

impl Work<'_> {
    // ==== 機能: ダミーノードの挿入 ====

    /// 各クラスタのランク範囲（実ノードから計算、メンバーなしは `None`）。
    fn cluster_rank_ranges(&self) -> Vec<Option<(usize, usize)>> {
        let mut ranges: Vec<Option<(usize, usize)>> = vec![None; self.clusters.len()];
        for v in 0..self.n_real {
            let r = self.rank[v];
            for &c in &self.chain[v] {
                ranges[c] = Some(match ranges[c] {
                    None => (r, r),
                    Some((a, b)) => (a.min(r), b.max(r)),
                });
            }
        }
        ranges
    }

    /// ノードを追加して番号を返す。
    fn add_node(&mut self, rank: usize, size: Vec2, thin: bool, chain: Vec<usize>) -> usize {
        self.rank.push(rank);
        self.size.push(size);
        self.thin.push(thin);
        self.chain.push(chain);
        self.preds.push(Vec::new());
        self.succs.push(Vec::new());
        self.rank.len() - 1
    }

    /// 隣接ランク間の区間を追加する（ダミー同士ほど重く、まっすぐにしたい）。
    fn add_segment(&mut self, a: usize, b: usize) {
        let da = a >= self.n_real;
        let db = b >= self.n_real;
        let wgt = match (da, db) {
            (false, false) => 1.0,
            (true, true) => 8.0,
            _ => 2.0,
        };
        self.succs[a].push((b, wgt));
        self.preds[b].push((a, wgt));
    }

    /// エッジ u→v をダミーで分割し、(ダミー列, ラベル用ダミー) を返す。
    fn insert_chain(
        &mut self,
        u: usize,
        v: usize,
        label: Option<Vec2>,
        ranges: &[Option<(usize, usize)>],
    ) -> (Vec<usize>, Option<usize>) {
        let (ru, rv) = (self.rank[u], self.rank[v]);
        let span = rv.saturating_sub(ru);
        let mut list = Vec::new();
        let mut label_node = None;
        let mut prev = u;
        // 端点が属するクラスタの候補（深い順に選ぶ）
        let mut cands: Vec<usize> = self.chain[u].clone();
        for c in &self.chain[v] {
            if !cands.contains(c) {
                cands.push(*c);
            }
        }
        for k in 1..span {
            let r = ru + k;
            let chain = cands
                .iter()
                .copied()
                .filter(|&c| ranges[c].is_some_and(|(a, b)| a <= r && r <= b))
                .max_by_key(|&c| self.depth[c])
                .map(|c| self.full_chain(c))
                .unwrap_or_default();
            let is_label = label.is_some() && k == span / 2;
            let size = if is_label {
                label.unwrap_or(Vec2::ZERO)
            } else {
                Vec2::ZERO
            };
            let d = self.add_node(r, size, !is_label, chain);
            if is_label {
                label_node = Some(d);
            }
            self.add_segment(prev, d);
            list.push(d);
            prev = d;
        }
        self.add_segment(prev, v);
        (list, label_node)
    }

    /// クラスタ c の所属列（外側→c）。
    fn full_chain(&self, c: usize) -> Vec<usize> {
        let mut chain = vec![c];
        let mut cur = self.clusters[c].parent;
        while let Some(pc) = cur {
            if pc >= self.clusters.len() || chain.contains(&pc) {
                break;
            }
            chain.push(pc);
            cur = self.clusters[pc].parent;
        }
        chain.reverse();
        chain
    }

    // ==== 機能: 交差削減 ====

    /// DFS 順で初期の並びを作る。
    fn init_order(&mut self) {
        let total = self.rank.len();
        let max_rank = self.rank.iter().copied().max().unwrap_or(0);
        self.layers = vec![Vec::new(); max_rank + 1];
        let mut visited = vec![false; total];
        let mut starts: Vec<usize> = (0..self.n_real).collect();
        starts.sort_by_key(|&v| self.rank[v]);
        for s in starts {
            if visited[s] {
                continue;
            }
            let mut stack = vec![s];
            while let Some(v) = stack.pop() {
                if visited[v] {
                    continue;
                }
                visited[v] = true;
                self.layers[self.rank[v]].push(v);
                for &(t, _) in self.succs[v].iter().rev() {
                    if !visited[t] {
                        stack.push(t);
                    }
                }
            }
        }
        for (v, seen) in visited.iter().enumerate() {
            if !seen {
                self.layers[self.rank[v]].push(v);
            }
        }
        self.pos = vec![0; total];
        for layer in &mut self.layers {
            group_sort(layer, &self.chain, &|_| 0.0, 0);
        }
        self.update_pos();
    }

    /// ランク内の位置表を更新する。
    fn update_pos(&mut self) {
        for layer in &self.layers {
            for (i, &v) in layer.iter().enumerate() {
                self.pos[v] = i;
            }
        }
    }

    /// 重心法で上下に交互スイープし、交差数が最小の並びを採用する。
    fn reduce_crossings(&mut self) {
        let mut best = self.layers.clone();
        let mut best_cross = self.count_crossings();
        for it in 0..16 {
            if best_cross == 0 {
                break;
            }
            let down = it % 2 == 0;
            let order: Vec<usize> = if down {
                (1..self.layers.len()).collect()
            } else {
                (0..self.layers.len().saturating_sub(1)).rev().collect()
            };
            for r in order {
                let keys: Vec<(usize, f32)> = self.layers[r]
                    .iter()
                    .map(|&v| {
                        let nb = if down { &self.preds[v] } else { &self.succs[v] };
                        let key = if nb.is_empty() {
                            self.pos[v] as f32
                        } else {
                            nb.iter().map(|&(t, _)| self.pos[t] as f32).sum::<f32>()
                                / nb.len() as f32
                        };
                        (v, key)
                    })
                    .collect();
                let key_of = |v: usize| {
                    keys.iter()
                        .find(|(k, _)| *k == v)
                        .map_or(0.0, |(_, key)| *key)
                };
                let mut layer = std::mem::take(&mut self.layers[r]);
                group_sort(&mut layer, &self.chain, &key_of, 0);
                self.layers[r] = layer;
                self.update_pos();
            }
            let c = self.count_crossings();
            if c < best_cross {
                best_cross = c;
                best = self.layers.clone();
            }
        }
        self.layers = best;
        self.update_pos();
    }

    /// 全ランク間の辺交差数を数える。
    fn count_crossings(&self) -> usize {
        let mut total = 0;
        for layer in &self.layers {
            let mut segs: Vec<(usize, usize)> = Vec::new();
            for &u in layer {
                for &(v, _) in &self.succs[u] {
                    segs.push((self.pos[u], self.pos[v]));
                }
            }
            for i in 0..segs.len() {
                for j in i + 1..segs.len() {
                    let (a, b) = (segs[i], segs[j]);
                    if (a.0 < b.0 && a.1 > b.1) || (a.0 > b.0 && a.1 < b.1) {
                        total += 1;
                    }
                }
            }
        }
        total
    }

    // ==== 機能: 座標決定（ランク軸） ====

    /// ランク軸座標を決める。クラスタ境界のあるランク間には余白とタイトル分を足す。
    fn assign_y(&mut self, ranges: &[Option<(usize, usize)>]) {
        let nr = self.layers.len();
        let mut rank_h = vec![0.0f32; nr];
        for v in 0..self.rank.len() {
            let r = self.rank[v];
            rank_h[r] = rank_h[r].max(self.size[v].y);
        }
        let pad = self.p.cluster_pad;
        let mut top_extra = vec![0.0f32; nr];
        let mut bottom_extra = vec![0.0f32; nr];
        for c in 0..self.clusters.len() {
            let Some((lo, hi)) = ranges[c] else { continue };
            let chain = self.full_chain(c);
            let top: f32 = chain
                .iter()
                .filter(|&&a| ranges[a].is_some_and(|(l, _)| l == lo))
                .map(|&a| pad + self.clusters[a].pad_lo.y)
                .sum();
            let bottom: f32 = chain
                .iter()
                .filter(|&&a| ranges[a].is_some_and(|(_, h)| h == hi))
                .map(|&a| pad + self.clusters[a].pad_hi.y)
                .sum();
            top_extra[lo] = top_extra[lo].max(top);
            bottom_extra[hi] = bottom_extra[hi].max(bottom);
        }
        let mult_gap = if self.label_ranks {
            self.p.rank_sep / 2.0
        } else {
            self.p.rank_sep
        };
        self.rank_y = vec![0.0; nr];
        let mut y = top_extra.first().copied().unwrap_or(0.0)
            + rank_h.first().copied().unwrap_or(0.0) / 2.0;
        for r in 0..nr {
            if r > 0 {
                y += rank_h[r - 1] / 2.0
                    + mult_gap
                    + bottom_extra[r - 1]
                    + top_extra[r]
                    + rank_h[r] / 2.0;
            }
            self.rank_y[r] = y;
        }
    }

    // ==== 機能: 座標決定（交差軸） ====

    /// 隣り合う a（左）と b（右）の中心間の最小距離。
    fn sep(&self, a: usize, b: usize) -> f32 {
        let base = match (self.thin[a], self.thin[b]) {
            (false, false) => self.p.node_sep,
            (true, true) => self.p.node_sep * 0.3,
            _ => self.p.node_sep * 0.5,
        };
        let (ca, cb) = (&self.chain[a], &self.chain[b]);
        let k = ca.iter().zip(cb.iter()).take_while(|(x, y)| x == y).count();
        let pad = self.p.cluster_pad;
        let exits: f32 = ca[k..]
            .iter()
            .map(|&c| pad + self.clusters[c].pad_hi.x)
            .sum();
        let enters: f32 = cb[k..]
            .iter()
            .map(|&c| pad + self.clusters[c].pad_lo.x)
            .sum();
        self.size[a].x / 2.0 + self.size[b].x / 2.0 + base + exits + enters
    }

    /// 交差軸座標を決める: 左詰めの初期配置から、隣接ランクの重心へ寄せる
    /// 等張回帰（順序と最小間隔を保つ最小二乗配置）を繰り返す。
    fn assign_x(&mut self) {
        self.x = vec![0.0; self.rank.len()];
        for r in 0..self.layers.len() {
            let layer = &self.layers[r];
            let mut cur = 0.0;
            for i in 0..layer.len() {
                if i > 0 {
                    cur += self.sep(layer[i - 1], layer[i]);
                }
                self.x[layer[i]] = cur;
            }
        }
        let iters = 12;
        let nl = self.layers.len();
        for it in 0..iters {
            let both = it >= iters - 4;
            let down = it % 2 == 0;
            let order: Vec<usize> = if both || down {
                (0..nl).collect()
            } else {
                (0..nl).rev().collect()
            };
            for r in order {
                self.place_layer(r, down || both, !down || both);
            }
        }
    }

    /// 1 ランクを、隣接ノードの重み付き平均へできるだけ近づけて配置する。
    fn place_layer(&mut self, r: usize, use_preds: bool, use_succs: bool) {
        let layer = self.layers[r].clone();
        if layer.is_empty() {
            return;
        }
        let mut desired = Vec::with_capacity(layer.len());
        let mut weight = Vec::with_capacity(layer.len());
        for &v in &layer {
            let mut sw = 0.0;
            let mut sx = 0.0;
            if use_preds {
                for &(t, wt) in &self.preds[v] {
                    sw += wt;
                    sx += wt * self.x[t];
                }
            }
            if use_succs {
                for &(t, wt) in &self.succs[v] {
                    sw += wt;
                    sx += wt * self.x[t];
                }
            }
            if sw > 0.0 {
                desired.push(sx / sw);
                weight.push(sw);
            } else {
                desired.push(self.x[v]);
                weight.push(0.05);
            }
        }
        let seps: Vec<f32> = layer.windows(2).map(|w| self.sep(w[0], w[1])).collect();
        let xs = isotonic_place(&desired, &weight, &seps);
        for (&v, x) in layer.iter().zip(xs) {
            self.x[v] = x;
        }
    }

    // ==== 機能: クラスタの重なり解消 ====

    /// ノードの矩形（ダミーは幅 1 の細い矩形）。
    fn node_rect(&self, v: usize) -> Rect {
        Rect::from_center_size(self.center(v), self.size[v].max(vec2(1.0, 1.0)))
    }

    /// ノードの中心。
    fn center(&self, v: usize) -> Pos2 {
        let y = self.rank_y.get(self.rank[v]).copied().unwrap_or(0.0);
        pos2(self.x.get(v).copied().unwrap_or(0.0), y)
    }

    /// 各クラスタの枠を、内側のクラスタから順に計算する。
    fn cluster_rects(&self) -> Vec<Option<Rect>> {
        let nc = self.clusters.len();
        let mut rects: Vec<Option<Rect>> = vec![None; nc];
        let mut order: Vec<usize> = (0..nc).collect();
        order.sort_by_key(|&c| std::cmp::Reverse(self.depth[c]));
        let pad = self.p.cluster_pad;
        for c in order {
            let mut r = Rect::NOTHING;
            for v in 0..self.rank.len() {
                if self.chain[v].last() == Some(&c) {
                    r = r.union(self.node_rect(v));
                }
            }
            for &ch in &self.children[c] {
                if let Some(cr) = rects[ch] {
                    r = r.union(cr);
                }
            }
            if !r.is_positive() {
                continue;
            }
            let cl = &self.clusters[c];
            r.min -= vec2(pad, pad) + cl.pad_lo;
            r.max += vec2(pad, pad) + cl.pad_hi;
            let grow = (sanitize_size(cl.min_size) - r.size()).max(Vec2::ZERO);
            r = r.expand2(grow / 2.0);
            rects[c] = Some(r);
        }
        rects
    }

    /// 兄弟関係にあるクラスタ・ノード同士の重なりを、内側の階層から順に右へずらして解消する。
    fn fix_cluster_overlaps(&mut self) {
        if self.clusters.is_empty() {
            return;
        }
        let mut parents: Vec<Option<usize>> = (0..self.clusters.len()).map(Some).collect();
        parents.sort_by_key(|c| std::cmp::Reverse(c.map_or(0, |c| self.depth[c])));
        parents.push(None);
        for parent in parents {
            let rects = self.cluster_rects();
            // (枠, クラスタなら Some(c), ノードなら None と番号)
            let mut groups: Vec<(Rect, Option<usize>, usize)> = Vec::new();
            let child_clusters: Vec<usize> = match parent {
                Some(pc) => self.children[pc].clone(),
                None => (0..self.clusters.len())
                    .filter(|&c| self.depth[c] == 0)
                    .collect(),
            };
            for c in child_clusters {
                if let Some(r) = rects[c] {
                    groups.push((r, Some(c), c));
                }
            }
            for v in 0..self.rank.len() {
                if self.chain[v].last().copied() == parent {
                    groups.push((self.node_rect(v), None, v));
                }
            }
            groups.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x));
            for i in 0..groups.len() {
                let mut delta = 0.0f32;
                for j in 0..i {
                    let (a, b) = (groups[j].0, groups[i].0);
                    let overlap_y = a.min.y < b.max.y - 0.5 && b.min.y < a.max.y - 0.5;
                    if !overlap_y {
                        continue;
                    }
                    let thin = |g: &(Rect, Option<usize>, usize)| g.1.is_none() && self.thin[g.2];
                    let gap = if thin(&groups[i]) || thin(&groups[j]) {
                        self.p.node_sep * 0.4
                    } else {
                        self.p.node_sep * 0.6
                    };
                    delta = delta.max(a.max.x + gap - b.min.x);
                }
                if delta > 0.0 {
                    match groups[i].1 {
                        Some(c) => {
                            for v in 0..self.rank.len() {
                                if self.chain[v].contains(&c) {
                                    self.x[v] += delta;
                                }
                            }
                        }
                        None => self.x[groups[i].2] += delta,
                    }
                    groups[i].0 = groups[i].0.translate(vec2(delta, 0.0));
                }
            }
        }
    }
}

// ==== 機能: 補助アルゴリズム ====

/// ランク内の並びを、クラスタごとに連続するよう階層的に安定ソートする。
/// グループのキーはメンバーのキーの平均。
fn group_sort(items: &mut [usize], chain: &[Vec<usize>], key: &dyn Fn(usize) -> f32, depth: usize) {
    if items.len() <= 1 {
        return;
    }
    // (クラスタ, メンバー)
    let mut groups: Vec<(Option<usize>, Vec<usize>)> = Vec::new();
    for &v in items.iter() {
        match chain[v].get(depth).copied() {
            Some(c) => match groups.iter_mut().find(|(g, _)| *g == Some(c)) {
                Some((_, m)) => m.push(v),
                None => groups.push((Some(c), vec![v])),
            },
            None => groups.push((None, vec![v])),
        }
    }
    let mut keyed: Vec<(f32, Option<usize>, Vec<usize>)> = groups
        .into_iter()
        .map(|(g, m)| {
            let k = m.iter().map(|&v| key(v)).sum::<f32>() / m.len() as f32;
            (k, g, m)
        })
        .collect();
    keyed.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut i = 0;
    for (_, g, mut members) in keyed {
        if g.is_some() {
            group_sort(&mut members, chain, key, depth + 1);
        }
        for v in members {
            items[i] = v;
            i += 1;
        }
    }
}

/// 順序と最小間隔 `seps[i]`（i と i+1 の間）を保ちつつ、重み付き二乗誤差で
/// `desired` に最も近い座標列を求める（Pool Adjacent Violators 法）。
fn isotonic_place(desired: &[f32], weight: &[f32], seps: &[f32]) -> Vec<f32> {
    let n = desired.len();
    let mut off = vec![0.0f32; n];
    for i in 1..n {
        off[i] = off[i - 1] + seps.get(i - 1).copied().unwrap_or(0.0);
    }
    // ブロック: (重み合計, 重み付き目標合計, 要素数)
    let mut blocks: Vec<(f32, f32, usize)> = Vec::new();
    for i in 0..n {
        let w = weight[i].max(1e-3);
        let t = desired[i] - off[i];
        blocks.push((w, w * t, 1));
        while blocks.len() >= 2 {
            let b = blocks[blocks.len() - 1];
            let a = blocks[blocks.len() - 2];
            if a.1 / a.0 > b.1 / b.0 {
                blocks.pop();
                if let Some(last) = blocks.last_mut() {
                    *last = (a.0 + b.0, a.1 + b.1, a.2 + b.2);
                }
            } else {
                break;
            }
        }
    }
    let mut out = Vec::with_capacity(n);
    for (w, wt, cnt) in blocks {
        let y = wt / w;
        for _ in 0..cnt {
            let i = out.len();
            out.push(y + off[i]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Params {
        Params {
            node_sep: 20.0,
            rank_sep: 30.0,
            cluster_pad: 8.0,
            margin: 4.0,
        }
    }

    fn node(w: f32, h: f32) -> LNode {
        LNode {
            size: vec2(w, h),
            cluster: None,
        }
    }

    fn edge(a: usize, b: usize) -> LEdge {
        LEdge {
            from: a,
            to: b,
            min_len: 1,
            label: None,
        }
    }

    #[test]
    fn isotonic_respects_separation() {
        let xs = isotonic_place(&[0.0, 0.0, 0.0], &[1.0, 1.0, 1.0], &[10.0, 10.0]);
        assert!((xs[1] - xs[0] - 10.0).abs() < 1e-3);
        assert!((xs[0] + 10.0).abs() < 1e-3);
    }

    #[test]
    fn cycle_is_broken_and_ranks_valid() {
        let nodes = vec![node(10.0, 10.0); 3];
        let edges = vec![edge(0, 1), edge(1, 2), edge(2, 0)];
        let l = layout(&nodes, &edges, &[], &params());
        assert!(l.nodes[0].y < l.nodes[1].y && l.nodes[1].y < l.nodes[2].y);
        assert_eq!(l.edges[2].points.len(), 1);
    }

    #[test]
    fn long_edges_get_dummies() {
        let nodes = vec![node(10.0, 10.0); 3];
        let edges = vec![edge(0, 1), edge(1, 2), edge(0, 2)];
        let l = layout(&nodes, &edges, &[], &params());
        assert_eq!(l.edges[2].points.len(), 1);
        assert!(l.edges[0].points.is_empty());
    }

    #[test]
    fn garbage_input_does_not_panic() {
        let nodes = vec![node(f32::NAN, -1.0), node(f32::INFINITY, 5.0)];
        let edges = vec![edge(0, 5), edge(1, 1), edge(0, 1)];
        let clusters = vec![LCluster {
            parent: Some(0),
            ..Default::default()
        }];
        let _ = layout(&nodes, &edges, &clusters, &params());
        let _ = layout(&[], &[], &[], &params());
    }
}
