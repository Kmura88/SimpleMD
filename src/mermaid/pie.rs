//! pie（円グラフ）の解析・レイアウト・描画。

use egui::{Align2, Pos2, Rect, pos2, vec2};

use super::scene::{Item, Role, Scene};
use super::text::{Measurer, clean_label, unquote};

// ==== 機能: モデルと解析 ====

/// 円グラフのデータ。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pie {
    /// タイトル。
    pub title: Option<String>,
    /// 凡例に値を表示するか（`showData`）。
    pub show_data: bool,
    /// (ラベル, 値) の列。
    pub slices: Vec<(String, f64)>,
}

/// 円グラフを解析する。`lines[0]` はヘッダ行。
pub fn parse(lines: &[(usize, String)]) -> Result<Pie, String> {
    let mut pie = Pie::default();
    let header = lines.first().map(|(_, l)| l.as_str()).unwrap_or("");
    let mut rest = header.trim_start_matches("pie").trim();
    if let Some(r) = rest.strip_prefix("showData") {
        pie.show_data = true;
        rest = r.trim();
    }
    if let Some(t) = rest.strip_prefix("title") {
        pie.title = Some(t.trim().to_string());
    }
    for (no, line) in lines.iter().skip(1) {
        let l = line.trim();
        if l == "showData" {
            pie.show_data = true;
            continue;
        }
        if let Some(t) = l.strip_prefix("title") {
            pie.title = Some(t.trim().to_string());
            continue;
        }
        if l.starts_with("accTitle") || l.starts_with("accDescr") {
            continue;
        }
        let (label, value) = l
            .rsplit_once(':')
            .ok_or_else(|| format!("{no} 行目: `\"ラベル\" : 値` の形式ではありません"))?;
        let value: f64 = value
            .trim()
            .parse()
            .map_err(|_| format!("{no} 行目: 数値ではありません: `{}`", value.trim()))?;
        if !value.is_finite() || value < 0.0 {
            return Err(format!("{no} 行目: 値は 0 以上の数値が必要です"));
        }
        let label = clean_label(unquote(label)).join(" ");
        pie.slices.push((label, value));
    }
    let total: f64 = pie.slices.iter().map(|s| s.1).sum();
    if pie.slices.is_empty() || total <= 0.0 {
        return Err("円グラフのデータがありません".into());
    }
    Ok(pie)
}

// ==== 機能: レイアウトと描画 ====

/// 値を表示用に整形する（整数なら小数点なし）。
fn fmt_value(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 && v.abs() < 1e15 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.2}")
    }
}

/// 円グラフのシーンを作る。
pub fn draw(pie: &Pie, m: &mut Measurer) -> Scene {
    let f = m.font;
    let mut items = Vec::new();
    let margin = f;
    let mut top = margin;
    let mut width: f32 = 0.0;
    if let Some(t) = &pie.title {
        let tb = m.block(std::slice::from_ref(t), f * 1.25);
        tb.emit(&mut items, pos2(margin, top), Align2::LEFT_TOP, Role::Text);
        width = width.max(tb.size.x + margin * 2.0);
        top += tb.size.y + f * 0.8;
    }
    let total: f64 = pie.slices.iter().map(|s| s.1).sum();
    let n = pie.slices.len().min(u16::MAX as usize) as u16;
    let r = f * 8.0;
    let center = pos2(margin + r, top + r);
    let tau = std::f32::consts::TAU;
    let mut angle = -tau / 4.0;
    let mut pct_labels = Vec::new();
    for (i, (_, v)) in pie.slices.iter().enumerate() {
        let frac = if total > 0.0 {
            (*v / total) as f32
        } else {
            0.0
        };
        let sweep = frac * tau;
        if sweep > 0.0 {
            let steps = ((sweep / tau) * 96.0).ceil().max(2.0) as usize;
            let mut pts: Vec<Pos2> = vec![center];
            for k in 0..=steps {
                let a = angle + sweep * k as f32 / steps as f32;
                pts.push(center + vec2(a.cos(), a.sin()) * r);
            }
            items.push(Item::Polygon {
                points: pts,
                fill: Role::Series(i as u16, n),
                stroke: Role::Background,
                width: 1.5,
            });
            if frac >= 0.04 {
                let mid = angle + sweep / 2.0;
                pct_labels.push((
                    center + vec2(mid.cos(), mid.sin()) * r * 0.68,
                    format!("{:.0}%", frac * 100.0),
                ));
            }
        }
        angle += sweep;
    }
    items.push(Item::Circle {
        center,
        radius: r,
        fill: Role::None,
        stroke: Role::Muted,
        width: 1.0,
    });
    for (p, t) in pct_labels {
        items.push(Item::Text {
            pos: p,
            text: t,
            size: f * 0.9,
            color: Role::Custom(egui::Color32::from_rgb(0x20, 0x20, 0x20)),
            anchor: Align2::CENTER_CENTER,
        });
    }

    // 凡例
    let lx = center.x + r + f * 1.5;
    let row_h = m.line("Ag", f).y.max(f * 1.2) + f * 0.35;
    let legend_h = row_h * pie.slices.len() as f32;
    let mut y = center.y - legend_h / 2.0;
    let mut legend_w: f32 = 0.0;
    for (i, (label, v)) in pie.slices.iter().enumerate() {
        let text = if pie.show_data {
            format!("{label} [{}]", fmt_value(*v))
        } else {
            label.clone()
        };
        let sz = f * 0.9;
        items.push(Item::Rect {
            rect: Rect::from_min_size(pos2(lx, y + (row_h - sz) / 2.0), vec2(sz, sz)),
            radius: 2.0,
            fill: Role::Series(i as u16, n),
            stroke: Role::None,
            width: 0.0,
        });
        let w = m.line(&text, f).x;
        legend_w = legend_w.max(w + sz + f * 0.5);
        items.push(Item::Text {
            pos: pos2(lx + sz + f * 0.5, y + row_h / 2.0),
            text,
            size: f,
            color: Role::Text,
            anchor: Align2::LEFT_CENTER,
        });
        y += row_h;
    }
    width = width.max(lx + legend_w + margin);
    let bottom = (center.y + r).max(center.y + legend_h / 2.0) + margin;
    Scene {
        size: vec2(width, bottom),
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::text::preprocess;

    #[test]
    fn parses_pie() {
        let src = "pie showData title Pets\n \"Dogs\" : 386\n \"Cats\" : 85.5\n \"Rats\" : 15";
        let p = parse(&preprocess(src)).unwrap();
        assert!(p.show_data);
        assert_eq!(p.title.as_deref(), Some("Pets"));
        assert_eq!(p.slices.len(), 3);
        assert_eq!(p.slices[1], ("Cats".to_string(), 85.5));
    }

    #[test]
    fn parses_title_line_and_rejects_bad_values() {
        let p = parse(&preprocess("pie\n title Key elements\n \"A\" : 1")).unwrap();
        assert_eq!(p.title.as_deref(), Some("Key elements"));
        assert!(parse(&preprocess("pie\n \"A\" : abc")).is_err());
        assert!(parse(&preprocess("pie\n \"A\" : -1")).is_err());
        assert!(parse(&preprocess("pie title x")).is_err());
    }
}
