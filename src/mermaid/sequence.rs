//! sequenceDiagram（シーケンス図）の解析・レイアウト・描画。

use egui::{Align2, Pos2, Rect, Vec2, pos2, vec2};

use super::scene::{Dash, Item, Role, Scene, translate_items};
use super::text::{Measurer, TextBlock, clean_label};

// ==== 機能: モデル定義 ====

/// 参加者。
#[derive(Clone, Debug, PartialEq)]
pub struct Participant {
    /// ID。
    pub id: String,
    /// 表示ラベル。
    pub label: Vec<String>,
    /// `actor`（人型）かどうか。
    pub actor: bool,
}

/// 矢印の先端。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Head {
    /// なし（`->` `-->`）。
    None,
    /// 塗りつぶし矢印（`->>`）。
    Filled,
    /// バツ（`-x`）。
    Cross,
    /// 開いた矢印（`-)` 非同期）。
    Open,
}

/// ノートの位置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePos {
    /// 参加者の左。
    Left(usize),
    /// 参加者の右。
    Right(usize),
    /// 参加者（範囲）の上。
    Over(usize, usize),
}

/// 時系列のイベント。
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// メッセージ。
    Message {
        /// 送信者。
        from: usize,
        /// 受信者。
        to: usize,
        /// 本文。
        text: Vec<String>,
        /// 点線か。
        dotted: bool,
        /// 先端。
        head: Head,
        /// 双方向か（`<<->>`）。
        bidir: bool,
        /// 受信者を活性化するか（`+`）。
        activate: bool,
        /// 送信者を非活性化するか（`-`）。
        deactivate: bool,
        /// 自動採番の番号。
        number: Option<u32>,
    },
    /// ノート。
    Note {
        /// 位置。
        pos: NotePos,
        /// 本文。
        text: Vec<String>,
    },
    /// ブロック開始（`loop` など）。
    Start {
        /// 種類（`loop` `alt` …）。
        kind: String,
        /// 条件ラベル。
        label: String,
    },
    /// ブロック内の区切り（`else` `and` `option`）。
    Else {
        /// 条件ラベル。
        label: String,
    },
    /// ブロック終了。
    End,
    /// 活性化。
    Activate(usize),
    /// 非活性化。
    Deactivate(usize),
}

/// シーケンス図。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sequence {
    /// タイトル。
    pub title: Option<String>,
    /// 参加者（登場順）。
    pub participants: Vec<Participant>,
    /// イベント列。
    pub events: Vec<Event>,
}

// ==== 機能: 解析 ====

/// 矢印記号の表（同じ位置では長いものを優先）。
const ARROWS: &[(&str, bool, Head, bool)] = &[
    // (記号, 点線, 先端, 双方向)
    ("<<-->>", true, Head::Filled, true),
    ("<<->>", false, Head::Filled, true),
    ("-->>", true, Head::Filled, false),
    ("->>", false, Head::Filled, false),
    ("--x", true, Head::Cross, false),
    ("-x", false, Head::Cross, false),
    ("--)", true, Head::Open, false),
    ("-)", false, Head::Open, false),
    ("-->", true, Head::None, false),
    ("->", false, Head::None, false),
];

/// パーサの状態。
struct Parser {
    /// 構築中の図。
    d: Sequence,
    /// 自動採番の (次の番号, 増分)。
    autonumber: Option<(u32, u32)>,
    /// ブロックのスタック（`box` は描画しないので区別する）。
    blocks: Vec<bool>,
}

impl Parser {
    /// 参加者を取得し、なければ作る。
    fn participant(&mut self, id: &str) -> Result<usize, String> {
        let id = id.trim();
        if id.is_empty() {
            return Err("参加者名がありません".into());
        }
        if let Some(i) = self.d.participants.iter().position(|p| p.id == id) {
            return Ok(i);
        }
        self.d.participants.push(Participant {
            id: id.to_string(),
            label: clean_label(id),
            actor: false,
        });
        Ok(self.d.participants.len() - 1)
    }

    /// `participant A as Alice` / `actor B` を処理する。
    fn declare(&mut self, rest: &str, actor: bool) -> Result<(), String> {
        let rest = rest.trim();
        let (id, alias) = match rest.split_once(" as ") {
            Some((i, a)) => (i.trim(), Some(a.trim())),
            None => (rest, None),
        };
        let id = id.trim_matches('"');
        let i = self.participant(id)?;
        let p = &mut self.d.participants[i];
        p.actor = actor;
        if let Some(a) = alias {
            p.label = clean_label(a);
        }
        Ok(())
    }

    /// ノート文を処理する。
    fn note(&mut self, rest: &str) -> Result<(), String> {
        let (place, text) = rest.split_once(':').ok_or("ノートに `:` がありません")?;
        let place = place.trim();
        let lower = place.to_ascii_lowercase();
        let pos = if let Some(p) = lower.strip_prefix("left of") {
            NotePos::Left(self.participant(&place[place.len() - p.len()..])?)
        } else if let Some(p) = lower.strip_prefix("right of") {
            NotePos::Right(self.participant(&place[place.len() - p.len()..])?)
        } else if let Some(p) = lower.strip_prefix("over") {
            let names = &place[place.len() - p.len()..];
            let mut it = names.split(',');
            let a = self.participant(it.next().unwrap_or(""))?;
            let b = match it.next() {
                Some(n) => self.participant(n)?,
                None => a,
            };
            NotePos::Over(a.min(b), a.max(b))
        } else {
            return Err(format!("ノートの位置を解釈できません: `{place}`"));
        };
        self.d.events.push(Event::Note {
            pos,
            text: clean_label(text),
        });
        Ok(())
    }

    /// メッセージ文を処理する。メッセージでなければ `Ok(false)`。
    fn message(&mut self, l: &str) -> Result<bool, String> {
        let (head_part, text) = match l.split_once(':') {
            Some((h, t)) => (h, t),
            None => (l, ""),
        };
        let found = head_part.char_indices().find_map(|(i, _)| {
            ARROWS
                .iter()
                .find(|(a, ..)| head_part[i..].starts_with(a))
                .map(|a| (i, *a))
        });
        let Some((i, (arrow, dotted, head, bidir))) = found else {
            return Ok(false);
        };
        let from = head_part[..i].trim();
        let mut rest = head_part[i + arrow.len()..].trim();
        let mut activate = false;
        let mut deactivate = false;
        if let Some(r) = rest.strip_prefix('+') {
            activate = true;
            rest = r.trim();
        } else if let Some(r) = rest.strip_prefix('-') {
            deactivate = true;
            rest = r.trim();
        }
        let from = self.participant(from)?;
        let to = self.participant(rest)?;
        let number = self.autonumber.map(|(n, step)| {
            self.autonumber = Some((n.saturating_add(step), step));
            n
        });
        self.d.events.push(Event::Message {
            from,
            to,
            text: clean_label(text),
            dotted,
            head,
            bidir,
            activate,
            deactivate,
            number,
        });
        Ok(true)
    }

    /// 1 行を処理する。
    fn statement(&mut self, l: &str) -> Result<(), String> {
        let (first, rest) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
        let rest = rest.trim();
        match first {
            "title" | "title:" => {
                self.d.title = Some(rest.trim_start_matches(':').trim().to_string());
            }
            "autonumber" => {
                let nums: Vec<u32> = rest
                    .split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                self.autonumber = if rest == "off" {
                    None
                } else {
                    Some((
                        nums.first().copied().unwrap_or(1),
                        nums.get(1).copied().unwrap_or(1),
                    ))
                };
            }
            "participant" => self.declare(rest, false)?,
            "actor" => self.declare(rest, true)?,
            "create" => {
                let (kind, r) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
                self.declare(r, kind == "actor")?;
            }
            "destroy" | "links" | "link" | "properties" | "details" | "accTitle" | "accDescr"
            | "accTitle:" | "accDescr:" => {}
            "box" => self.blocks.push(false),
            "loop" | "alt" | "opt" | "par" | "critical" | "break" | "rect" | "par_over" => {
                self.blocks.push(true);
                self.d.events.push(Event::Start {
                    kind: first.to_string(),
                    label: rest.to_string(),
                });
            }
            "else" | "and" | "option" => {
                if self.blocks.last() == Some(&true) {
                    self.d.events.push(Event::Else {
                        label: rest.to_string(),
                    });
                }
            }
            "end" => match self.blocks.pop() {
                Some(true) => self.d.events.push(Event::End),
                Some(false) => {}
                None => return Err("対応するブロックのない `end` です".into()),
            },
            "activate" => {
                let p = self.participant(rest)?;
                self.d.events.push(Event::Activate(p));
            }
            "deactivate" => {
                let p = self.participant(rest)?;
                self.d.events.push(Event::Deactivate(p));
            }
            _ if first.eq_ignore_ascii_case("note") => self.note(rest)?,
            _ => {
                if !self.message(l)? {
                    return Err(format!("解釈できない記述です: `{l}`"));
                }
            }
        }
        Ok(())
    }
}

/// シーケンス図を解析する。`lines[0]` はヘッダ行。
pub fn parse(lines: &[(usize, String)]) -> Result<Sequence, String> {
    let mut p = Parser {
        d: Sequence::default(),
        autonumber: None,
        blocks: Vec::new(),
    };
    for (no, line) in lines.iter().skip(1) {
        p.statement(line.trim())
            .map_err(|e| format!("{no} 行目: {e}"))?;
    }
    // 閉じられていないブロックは末尾で閉じる
    while let Some(drawn) = p.blocks.pop() {
        if drawn {
            p.d.events.push(Event::End);
        }
    }
    if p.d.participants.is_empty() {
        return Err("参加者がいません".into());
    }
    Ok(p.d)
}

// ==== 機能: レイアウト（横方向） ====

/// 参加者の見出しの寸法。
struct Head2 {
    /// ラベル。
    text: TextBlock,
    /// 見出しの幅。
    w: f32,
}

/// 区間 [lo, hi) の間隔合計が `need` 以上になるよう不足分を均等に配る。
fn ensure_span(gaps: &mut [f32], lo: usize, hi: usize, need: f32) {
    if hi <= lo || hi > gaps.len() {
        return;
    }
    let sum: f32 = gaps[lo..hi].iter().sum();
    if sum < need {
        let add = (need - sum) / (hi - lo) as f32;
        for g in &mut gaps[lo..hi] {
            *g += add;
        }
    }
}

// ==== 機能: レイアウトと描画 ====

/// ブロック（フレーム）の作業状態。
struct Frame {
    /// 種類。
    kind: String,
    /// 条件ラベル。
    label: String,
    /// 上端。
    top: f32,
    /// 区切り（y, ラベル）。
    sections: Vec<(f32, String)>,
    /// 中身の横範囲。
    min_x: f32,
    /// 中身の横範囲。
    max_x: f32,
}

impl Frame {
    /// 中身の横範囲を広げる。
    fn include(&mut self, a: f32, b: f32) {
        self.min_x = self.min_x.min(a.min(b));
        self.max_x = self.max_x.max(a.max(b));
    }
}

/// 開いているすべてのフレームの横範囲を広げる。
fn include_all(frames: &mut [Frame], a: f32, b: f32) {
    for fr in frames {
        fr.include(a, b);
    }
}

/// シーケンス図のシーンを作る。
pub fn draw(d: &Sequence, m: &mut Measurer) -> Scene {
    let f = m.font;
    let np = d.participants.len();
    let heads: Vec<Head2> = d
        .participants
        .iter()
        .map(|p| {
            let text = m.text(&p.label);
            let w = (text.size.x + 2.0 * f).max(f * 5.5);
            Head2 { text, w }
        })
        .collect();
    let box_h = heads
        .iter()
        .zip(&d.participants)
        .map(|(h, p)| {
            if p.actor {
                f * 2.6 + h.text.size.y + f * 0.3
            } else {
                h.text.size.y + f * 1.3
            }
        })
        .fold(f * 2.4, f32::max);

    // ---- 参加者間隔の決定 ----
    let mut gaps: Vec<f32> = heads
        .windows(2)
        .map(|w| (w[0].w + w[1].w) / 2.0 + f * 1.5)
        .collect();
    let mut left_extra: f32 = 0.0;
    let mut right_extra: f32 = 0.0;
    let texts: Vec<TextBlock> = d
        .events
        .iter()
        .map(|e| match e {
            Event::Message { text, .. } | Event::Note { text, .. } => m.text(text),
            Event::Start { label, .. } | Event::Else { label } if !label.is_empty() => {
                m.block(&[format!("[{label}]")], f * 0.9)
            }
            _ => TextBlock::default(),
        })
        .collect();
    let loop_w = f * 2.5;
    for (e, tb) in d.events.iter().zip(&texts) {
        match e {
            Event::Message {
                from, to, number, ..
            } if from != to => {
                let (lo, hi) = (*from.min(to), *from.max(to));
                let extra = if number.is_some() { f * 1.5 } else { 0.0 };
                ensure_span(&mut gaps, lo, hi, tb.size.x + f * 2.0 + extra);
            }
            Event::Message { from, .. } => {
                let need = loop_w.max(tb.size.x + f * 1.2) + f * 1.5;
                if *from + 1 < np {
                    ensure_span(&mut gaps, *from, from + 1, need);
                } else {
                    right_extra = right_extra.max(need - heads[*from].w / 2.0);
                }
            }
            Event::Note { pos, .. } => {
                let nw = tb.size.x + f * 1.2;
                match *pos {
                    NotePos::Right(a) => {
                        if a + 1 < np {
                            ensure_span(&mut gaps, a, a + 1, nw + f * 1.2);
                        } else {
                            right_extra = right_extra.max(nw + f * 0.6 - heads[a].w / 2.0);
                        }
                    }
                    NotePos::Left(a) => {
                        if a > 0 {
                            ensure_span(&mut gaps, a - 1, a, nw + f * 1.2);
                        } else {
                            left_extra = left_extra.max(nw + f * 0.6 - heads[a].w / 2.0);
                        }
                    }
                    NotePos::Over(a, b) => {
                        let nw = nw.max(f * 5.0);
                        if a == b {
                            let half = nw / 2.0 + f * 0.6;
                            if a > 0 {
                                ensure_span(&mut gaps, a - 1, a, half + f * 0.6);
                            } else {
                                left_extra = left_extra.max(half - heads[a].w / 2.0);
                            }
                            if a + 1 < np {
                                ensure_span(&mut gaps, a, a + 1, half + f * 0.6);
                            } else {
                                right_extra = right_extra.max(half - heads[a].w / 2.0);
                            }
                        } else {
                            ensure_span(&mut gaps, a, b, nw - f * 1.6);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let margin = f * 0.8;
    let mut cx = Vec::with_capacity(np);
    let mut x = margin + left_extra.max(0.0) + heads.first().map_or(0.0, |h| h.w / 2.0);
    for i in 0..np {
        if i > 0 {
            x += gaps[i - 1];
        }
        cx.push(x);
    }

    // ---- 縦方向の配置と描画命令 ----
    let mut bg: Vec<Item> = Vec::new();
    let mut bars: Vec<Item> = Vec::new();
    let mut fg: Vec<Item> = Vec::new();
    let mut ext = Rect::NOTHING;
    let title_tb = d
        .title
        .as_ref()
        .map(|t| m.block(std::slice::from_ref(t), f * 1.2));
    let mut y = margin + title_tb.as_ref().map_or(0.0, |t| t.size.y + f * 0.8);
    let head_top = y;
    for (i, h) in heads.iter().enumerate() {
        let r = draw_participant(&mut fg, &d.participants[i], h, cx[i], head_top, box_h, f);
        ext = ext.union(r);
    }
    y += box_h + f * 1.2;
    let bw = f * 0.35;
    let mut active: Vec<Vec<f32>> = vec![Vec::new(); np];
    let mut frames: Vec<Frame> = Vec::new();
    let mut last_y = y;
    let label_h = m.line("Ag", f * 0.9).y;

    for (e, tb) in d.events.iter().zip(&texts) {
        match e {
            Event::Message {
                from,
                to,
                dotted,
                head,
                bidir,
                activate,
                deactivate,
                number,
                ..
            } => {
                let (a, b) = (*from, *to);
                if a >= np || b >= np {
                    continue;
                }
                let dash = if *dotted { Dash::Dashed } else { Dash::Solid };
                if a != b {
                    let sign = if cx[b] >= cx[a] { 1.0 } else { -1.0 };
                    let depth_a = active[a].len();
                    let depth_b = active[b].len() + usize::from(*activate);
                    let x1 = cx[a] + if depth_a > 0 { sign * bw } else { 0.0 };
                    let x2 = cx[b] - if depth_b > 0 { sign * bw } else { 0.0 };
                    y += tb.size.y;
                    let mid = (x1 + x2) / 2.0;
                    tb.emit(&mut fg, pos2(mid, y), Align2::CENTER_BOTTOM, Role::Text);
                    ext = ext.union(Align2::CENTER_BOTTOM.anchor_size(pos2(mid, y), tb.size));
                    let ly = y + f * 0.45;
                    fg.push(Item::Path {
                        points: vec![pos2(x1, ly), pos2(x2, ly)],
                        color: Role::Line,
                        width: 1.3,
                        dash,
                        closed: false,
                    });
                    arrow_head(&mut fg, *head, pos2(x2, ly), vec2(sign, 0.0), f);
                    if *bidir {
                        arrow_head(&mut fg, *head, pos2(x1, ly), vec2(-sign, 0.0), f);
                    }
                    if let Some(n) = number {
                        number_badge(&mut fg, pos2(x1, ly), *n, f);
                    }
                    include_all(&mut frames, x1, x2);
                    include_all(&mut frames, mid - tb.size.x / 2.0, mid + tb.size.x / 2.0);
                    last_y = ly;
                    y = ly + f * 0.9;
                } else {
                    let depth = active[a].len();
                    let x0 = cx[a] + if depth > 0 { bw } else { 0.0 };
                    y += tb.size.y;
                    let badge = if number.is_some() { f * 0.7 } else { 0.0 };
                    let tx = x0 + f * 0.5 + badge;
                    tb.emit(&mut fg, pos2(tx, y), Align2::LEFT_BOTTOM, Role::Text);
                    ext = ext.union(Align2::LEFT_BOTTOM.anchor_size(pos2(tx, y), tb.size));
                    let y1 = y + f * 0.4;
                    let y2 = y1 + f * 1.5;
                    let x_end = cx[a]
                        + if depth + usize::from(*activate) > 0 {
                            bw
                        } else {
                            0.0
                        };
                    fg.push(Item::Path {
                        points: vec![
                            pos2(x0, y1),
                            pos2(cx[a] + loop_w, y1),
                            pos2(cx[a] + loop_w, y2),
                            pos2(x_end, y2),
                        ],
                        color: Role::Line,
                        width: 1.3,
                        dash,
                        closed: false,
                    });
                    arrow_head(&mut fg, *head, pos2(x_end, y2), vec2(-1.0, 0.0), f);
                    if let Some(n) = number {
                        number_badge(&mut fg, pos2(x0, y1), *n, f);
                    }
                    include_all(&mut frames, cx[a], (cx[a] + loop_w).max(tx + tb.size.x));
                    ext = ext.union(Rect::from_min_max(
                        pos2(cx[a], y1),
                        pos2(cx[a] + loop_w, y2),
                    ));
                    last_y = y2;
                    y = y2 + f * 0.9;
                }
                if *activate {
                    active[b].push(last_y);
                }
                if *deactivate && let Some(start) = active[a].pop() {
                    push_bar(&mut bars, cx[a], active[a].len(), start, last_y, bw, f);
                }
            }
            Event::Note { pos, .. } => {
                let nw = tb.size.x + f * 1.2;
                let nh = tb.size.y + f * 0.8;
                let (l, r) = match *pos {
                    NotePos::Right(a) if a < np => (cx[a] + f * 0.6, cx[a] + f * 0.6 + nw),
                    NotePos::Left(a) if a < np => (cx[a] - f * 0.6 - nw, cx[a] - f * 0.6),
                    NotePos::Over(a, b) if a < np && b < np => {
                        let (l, r) = (cx[a].min(cx[b]), cx[a].max(cx[b]));
                        let nw = nw.max(f * 5.0).max(r - l + f * 2.0);
                        let c = (l + r) / 2.0;
                        (c - nw / 2.0, c + nw / 2.0)
                    }
                    _ => continue,
                };
                y += f * 0.2;
                let rect = Rect::from_min_max(pos2(l, y), pos2(r, y + nh));
                fg.push(Item::Rect {
                    rect,
                    radius: 2.0,
                    fill: Role::NoteFill,
                    stroke: Role::NoteStroke,
                    width: 1.0,
                });
                tb.emit_centered(&mut fg, rect.center(), Role::Text);
                ext = ext.union(rect);
                include_all(&mut frames, l, r);
                y += nh + f * 0.8;
            }
            Event::Start { kind, label } => {
                y += f * 0.2;
                let fr = Frame {
                    kind: kind.clone(),
                    label: label.clone(),
                    top: y,
                    sections: Vec::new(),
                    min_x: f32::INFINITY,
                    max_x: f32::NEG_INFINITY,
                };
                frames.push(fr);
                y += label_h.max(tb.size.y) + f * 1.0;
            }
            Event::Else { label } => {
                if let Some(fr) = frames.last_mut() {
                    y += f * 0.2;
                    fr.sections.push((y, label.clone()));
                    y += label_h + f * 0.9;
                }
            }
            Event::End => {
                if let Some(fr) = frames.pop() {
                    y += f * 0.1;
                    let r = finish_frame(&mut bg, &mut fg, fr, y, &cx, m, f);
                    ext = ext.union(r);
                    include_all(&mut frames, r.left() - f * 0.1, r.right() + f * 0.1);
                    y += f * 0.8;
                }
            }
            Event::Activate(p) => {
                if let Some(st) = active.get_mut(*p) {
                    st.push(last_y);
                }
            }
            Event::Deactivate(p) => {
                if let Some(start) = active.get_mut(*p).and_then(|s| s.pop()) {
                    push_bar(
                        &mut bars,
                        cx[*p],
                        active[*p].len(),
                        start,
                        last_y.max(start + f),
                        bw,
                        f,
                    );
                }
            }
        }
    }
    y += f * 0.3;
    while let Some(fr) = frames.pop() {
        let r = finish_frame(&mut bg, &mut fg, fr, y, &cx, m, f);
        ext = ext.union(r);
        y += f * 0.6;
    }
    for (p, st) in active.iter_mut().enumerate() {
        while let Some(start) = st.pop() {
            push_bar(&mut bars, cx[p], st.len(), start, y, bw, f);
        }
    }
    y += f * 0.4;

    // ---- ライフラインと下側の参加者 ----
    let mut lines = Vec::new();
    for (i, h) in heads.iter().enumerate() {
        lines.push(Item::Path {
            points: vec![pos2(cx[i], head_top + box_h), pos2(cx[i], y)],
            color: Role::Muted,
            width: 1.0,
            dash: Dash::Solid,
            closed: false,
        });
        let r = draw_participant(&mut fg, &d.participants[i], h, cx[i], y, box_h, f);
        ext = ext.union(r);
    }
    if let Some(&last) = cx.last() {
        ext.extend_with(pos2(last + right_extra.max(0.0) + heads[np - 1].w / 2.0, y));
    }
    let mut items = bg;
    items.extend(lines);
    items.extend(bars);
    items.extend(fg);

    // 左端を余白位置にそろえる
    let dx = margin - ext.min.x.min(margin);
    translate_items(&mut items, vec2(dx, 0.0));
    let mut width = ext.max.x + dx + margin;
    if let Some(t) = title_tb {
        t.emit(
            &mut items,
            pos2(margin, margin),
            Align2::LEFT_TOP,
            Role::Text,
        );
        width = width.max(t.size.x + margin * 2.0);
    }
    Scene {
        size: vec2(width, y + box_h + margin),
        items,
    }
}

/// 参加者の見出し（四角または人型）を描き、占有矩形を返す。
fn draw_participant(
    items: &mut Vec<Item>,
    p: &Participant,
    h: &Head2,
    cx: f32,
    top: f32,
    box_h: f32,
    f: f32,
) -> Rect {
    if p.actor {
        let head_r = f * 0.45;
        let hc = pos2(cx, top + head_r + 1.0);
        items.push(Item::Circle {
            center: hc,
            radius: head_r,
            fill: Role::NodeFill,
            stroke: Role::NodeStroke,
            width: 1.3,
        });
        let neck = hc.y + head_r;
        let hip = neck + f * 1.0;
        let foot = hip + f * 0.9;
        let stick = |a: Pos2, b: Pos2| Item::Path {
            points: vec![a, b],
            color: Role::NodeStroke,
            width: 1.3,
            dash: Dash::Solid,
            closed: false,
        };
        items.push(stick(pos2(cx, neck), pos2(cx, hip)));
        items.push(stick(
            pos2(cx - f * 0.8, neck + f * 0.4),
            pos2(cx + f * 0.8, neck + f * 0.4),
        ));
        items.push(stick(pos2(cx, hip), pos2(cx - f * 0.6, foot)));
        items.push(stick(pos2(cx, hip), pos2(cx + f * 0.6, foot)));
        h.text.emit(
            items,
            pos2(cx, top + box_h),
            Align2::CENTER_BOTTOM,
            Role::Text,
        );
        Rect::from_center_size(
            pos2(cx, top + box_h / 2.0),
            vec2(h.text.size.x.max(f * 1.6), box_h),
        )
    } else {
        let r = Rect::from_center_size(pos2(cx, top + box_h / 2.0), vec2(h.w, box_h));
        items.push(Item::Rect {
            rect: r,
            radius: f * 0.25,
            fill: Role::NodeFill,
            stroke: Role::NodeStroke,
            width: 1.3,
        });
        h.text.emit_centered(items, r.center(), Role::Text);
        r
    }
}

/// メッセージの矢印の先端を描く（`d` は進行方向の単位ベクトル）。
fn arrow_head(items: &mut Vec<Item>, head: Head, tip: Pos2, d: Vec2, f: f32) {
    let n = vec2(-d.y, d.x);
    let l = f * 0.7;
    let hw = l * 0.42;
    match head {
        Head::None => {}
        Head::Filled => items.push(Item::Polygon {
            points: vec![tip, tip - d * l + n * hw, tip - d * l - n * hw],
            fill: Role::Line,
            stroke: Role::Line,
            width: 1.0,
        }),
        Head::Open => items.push(Item::Path {
            points: vec![tip - d * l + n * hw, tip, tip - d * l - n * hw],
            color: Role::Line,
            width: 1.3,
            dash: Dash::Solid,
            closed: false,
        }),
        Head::Cross => {
            let s = f * 0.32;
            let c = tip - d * s;
            for (a, b) in [(d + n, -(d + n)), (d - n, -(d - n))] {
                items.push(Item::Path {
                    points: vec![c + a * s, c + b * s],
                    color: Role::Line,
                    width: 1.5,
                    dash: Dash::Solid,
                    closed: false,
                });
            }
        }
    }
}

/// 自動採番の番号バッジを描く。
fn number_badge(items: &mut Vec<Item>, p: Pos2, n: u32, f: f32) {
    items.push(Item::Circle {
        center: p,
        radius: f * 0.6,
        fill: Role::Line,
        stroke: Role::Line,
        width: 1.0,
    });
    items.push(Item::Text {
        pos: p,
        text: n.to_string(),
        size: f * 0.72,
        color: Role::Background,
        anchor: Align2::CENTER_CENTER,
    });
}

/// 活性化バーを追加する。
fn push_bar(items: &mut Vec<Item>, cx: f32, depth: usize, y0: f32, y1: f32, bw: f32, f: f32) {
    let x = cx + depth as f32 * bw * 0.7;
    let y1 = y1.max(y0 + f * 0.6);
    items.push(Item::Rect {
        rect: Rect::from_min_max(pos2(x - bw, y0), pos2(x + bw, y1)),
        radius: 0.0,
        fill: Role::NodeFill,
        stroke: Role::NodeStroke,
        width: 1.0,
    });
}

/// ブロック（フレーム）の枠・見出しタブ・区切り線を描き、枠の矩形を返す。
fn finish_frame(
    bg: &mut Vec<Item>,
    fg: &mut Vec<Item>,
    fr: Frame,
    bottom: f32,
    cx: &[f32],
    m: &mut Measurer,
    f: f32,
) -> Rect {
    let (mut l, mut r) = if fr.min_x.is_finite() {
        (fr.min_x, fr.max_x)
    } else {
        let c = cx.first().copied().unwrap_or(0.0);
        (c, c + f * 6.0)
    };
    let kind = m.block(std::slice::from_ref(&fr.kind), f * 0.85);
    let cond = (!fr.label.is_empty()).then(|| m.block(&[format!("[{}]", fr.label)], f * 0.9));
    let tab_w = kind.size.x + f * 1.0;
    let tab_h = kind.size.y + f * 0.35;
    let need = tab_w + cond.as_ref().map_or(0.0, |c| c.size.x + f * 1.2);
    l -= f * 0.9;
    r += f * 0.9;
    if r - l < need {
        r = l + need;
    }
    let rect = Rect::from_min_max(pos2(l, fr.top), pos2(r, bottom));
    let is_rect_block = fr.kind == "rect";
    if is_rect_block {
        let fill = super::graph::parse_color(&fr.label).map_or(Role::ClusterFill, Role::Custom);
        bg.push(Item::Rect {
            rect,
            radius: 0.0,
            fill,
            stroke: Role::None,
            width: 0.0,
        });
        return rect;
    }
    bg.push(Item::Rect {
        rect,
        radius: 0.0,
        fill: Role::None,
        stroke: Role::Muted,
        width: 1.2,
    });
    let cut = f * 0.4;
    let (x0, y0) = (l, fr.top);
    fg.push(Item::Polygon {
        points: vec![
            pos2(x0, y0),
            pos2(x0 + tab_w, y0),
            pos2(x0 + tab_w, y0 + tab_h - cut),
            pos2(x0 + tab_w - cut, y0 + tab_h),
            pos2(x0, y0 + tab_h),
        ],
        fill: Role::NodeFill,
        stroke: Role::Muted,
        width: 1.0,
    });
    kind.emit(
        fg,
        pos2(x0 + f * 0.4, y0 + tab_h / 2.0),
        Align2::LEFT_CENTER,
        Role::Text,
    );
    if let Some(c) = cond {
        c.emit(
            fg,
            pos2(x0 + tab_w + f * 0.6, y0 + tab_h / 2.0),
            Align2::LEFT_CENTER,
            Role::Text,
        );
    }
    for (sy, label) in fr.sections {
        bg.push(Item::Path {
            points: vec![pos2(l, sy), pos2(r, sy)],
            color: Role::Muted,
            width: 1.0,
            dash: Dash::Dashed,
            closed: false,
        });
        if !label.is_empty() {
            let tb = m.block(&[format!("[{label}]")], f * 0.9);
            tb.emit(
                fg,
                pos2(rect.center().x, sy + f * 0.2),
                Align2::CENTER_TOP,
                Role::Text,
            );
        }
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::text::preprocess;

    fn p(src: &str) -> Sequence {
        parse(&preprocess(src)).expect("parse")
    }

    #[test]
    fn parses_participants_and_messages() {
        let d = p(
            "sequenceDiagram\n participant A as Alice\n actor B as Bob\n A->>B: Hello\n B-->>A: Hi\n A-xB: lost\n A-)B: async\n A->A: self\n A->>+B: act\n B-->>-A: done\n web-server->>db: q",
        );
        assert_eq!(d.participants.len(), 4);
        assert_eq!(d.participants[0].label, vec!["Alice"]);
        assert!(d.participants[1].actor);
        assert_eq!(d.participants[2].id, "web-server");
        let msgs: Vec<_> = d
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message {
                    from,
                    to,
                    head,
                    dotted,
                    activate,
                    deactivate,
                    ..
                } => Some((*from, *to, *head, *dotted, *activate, *deactivate)),
                _ => None,
            })
            .collect();
        assert_eq!(msgs[0], (0, 1, Head::Filled, false, false, false));
        assert_eq!(msgs[1], (1, 0, Head::Filled, true, false, false));
        assert_eq!(msgs[2].2, Head::Cross);
        assert_eq!(msgs[3].2, Head::Open);
        assert_eq!((msgs[4].0, msgs[4].1), (0, 0));
        assert!(msgs[5].4);
        assert!(msgs[6].5);
    }

    #[test]
    fn parses_notes_blocks_and_autonumber() {
        let d = p(
            "sequenceDiagram\n autonumber\n Alice->>John: Hello\n Note right of John: Text\n Note over Alice,John: Both\n loop Every minute\n John-->>Alice: Great!\n end\n alt ok\n A->B: x\n else bad\n A->B: y\n end\n box Aqua G\n participant Z\n end",
        );
        assert!(d.events.iter().any(|e| matches!(
            e,
            Event::Note {
                pos: NotePos::Right(1),
                ..
            }
        )));
        assert!(d.events.iter().any(|e| matches!(
            e,
            Event::Note {
                pos: NotePos::Over(0, 1),
                ..
            }
        )));
        let starts = d
            .events
            .iter()
            .filter(|e| matches!(e, Event::Start { .. }))
            .count();
        let ends = d.events.iter().filter(|e| matches!(e, Event::End)).count();
        assert_eq!((starts, ends), (2, 2));
        assert!(
            d.events
                .iter()
                .any(|e| matches!(e, Event::Else { label } if label == "bad"))
        );
        assert!(matches!(
            d.events[0],
            Event::Message {
                number: Some(1),
                ..
            }
        ));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(&preprocess("sequenceDiagram\n hello world")).is_err());
        assert!(parse(&preprocess("sequenceDiagram\n end")).is_err());
    }
}
