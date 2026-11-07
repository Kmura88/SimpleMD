//! # 機能: システムフォントの検出と読み込み
//!
//! egui 同梱のフォントには日本語（CJK）の字形も太字もないため、システムから
//! 本文用・太字用・日本語用・等幅用・記号用のフォントを選んで組み込む。
//!
//! ## 起動を速くする工夫
//! システムの全フォントを調べる（[`Scanner`]）のは数百 ms〜1 秒かかる。そこで
//! - 選んだフォントのファイルの場所を [`FontChoice`] としてキャッシュファイルに保存し、
//!   次回からは全スキャンせずに直接ファイルを読む
//! - 読み込みはウィンドウの準備と並行して別スレッドで行う（[`preload`]）
//! - 設定画面のフォント一覧は、画面を開いたときにバックグラウンドで作る
//!
//! ## フォントの選び方
//! - 設定でフォント名が指定されていればそれ、なければ候補リストの上から最初に見つかったもの
//! - WSL では Windows 側のフォントフォルダも探す
//! - 実際に使ったフォントは [`FontReport`] で設定画面に表示する

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

// ==== 機能: 太字用フォントファミリー ====

/// 太字テキストに使うフォントファミリー名。
pub const BOLD: &str = "bold";

/// 太字用の [`FontFamily`]。
pub fn bold() -> FontFamily {
    FontFamily::Name(BOLD.into())
}

// ==== 機能: フォント候補リスト（上から順に優先） ====

/// 本文用の欧文ゴシック体の候補。
const LATIN_SANS: &[&str] = &[
    "Segoe UI",
    "SF Pro Text",
    "Helvetica Neue",
    "Noto Sans",
    "Inter",
    "Cantarell",
    "Ubuntu",
    "DejaVu Sans",
    "Liberation Sans",
    "Arial",
];

/// 日本語（CJK）ゴシック体の候補。
/// MS Gothic / MS UI Gothic はあえて入れていない（埋め込みビットマップが egui で描けないため）。
const CJK_SANS: &[&str] = &[
    "Noto Sans CJK JP",
    "Source Han Sans JP",
    "Yu Gothic UI",
    "Meiryo",
    "Yu Gothic",
    "Hiragino Sans",
    "Hiragino Kaku Gothic ProN",
    "IPAexGothic",
    "IPAGothic",
    "VL Gothic",
    "TakaoGothic",
    "Noto Sans CJK SC",
    "Microsoft YaHei",
    "Malgun Gothic",
    "Droid Sans Fallback",
    // 可変フォントは既定の（細い）太さで描かれてしまうので最後に回す。
    "Noto Sans JP",
];

/// 等幅フォント（コード用）の候補。
const MONO: &[&str] = &[
    "Cascadia Mono",
    "Cascadia Code",
    "JetBrains Mono",
    "SF Mono",
    "Menlo",
    "Consolas",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "Liberation Mono",
    "Ubuntu Mono",
];

/// 記号用フォントの候補（最初に見つかった 1 つだけを、全ファミリーの最後の代替にする）。
const SYMBOLS: &[&str] = &[
    "Segoe UI Symbol",
    "Apple Symbols",
    "Noto Sans Symbols 2",
    "DejaVu Sans",
];

/// 追加で探すフォルダ（WSL では Windows 側のフォントを使える）。
const EXTRA_DIRS: &[&str] = &["/mnt/c/Windows/Fonts"];

// ==== 機能: 選んだフォント（キャッシュできる形） ====

/// フォントファイル内の 1 書体（.ttc は複数の書体を含むので番号も持つ）。
#[derive(Debug, Clone, PartialEq)]
pub struct FaceRef {
    pub family: String,
    pub path: PathBuf,
    pub index: u32,
}

/// 用途ごとに選んだ書体。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontChoice {
    /// 選んだときの設定（本文フォント名, コードフォント名）。設定が変わったら選び直す。
    pub key: (String, String),
    pub body: Option<FaceRef>,
    pub body_bold: Option<FaceRef>,
    pub cjk: Option<FaceRef>,
    pub cjk_bold: Option<FaceRef>,
    pub mono: Option<FaceRef>,
    pub symbol: Option<FaceRef>,
}

impl FontChoice {
    /// 用途名と書体の組（保存・読み込み・ファイル存在確認に使う）。
    fn slots(&self) -> [(&'static str, &Option<FaceRef>); 6] {
        [
            ("body", &self.body),
            ("body_bold", &self.body_bold),
            ("cjk", &self.cjk),
            ("cjk_bold", &self.cjk_bold),
            ("mono", &self.mono),
            ("symbol", &self.symbol),
        ]
    }

    fn slot_mut(&mut self, name: &str) -> Option<&mut Option<FaceRef>> {
        Some(match name {
            "body" => &mut self.body,
            "body_bold" => &mut self.body_bold,
            "cjk" => &mut self.cjk,
            "cjk_bold" => &mut self.cjk_bold,
            "mono" => &mut self.mono,
            "symbol" => &mut self.symbol,
            _ => return None,
        })
    }

    /// 記録されたファイルがすべて残っているか（フォントのアンインストール対策）。
    fn files_exist(&self) -> bool {
        self.slots()
            .iter()
            .all(|(_, f)| f.as_ref().is_none_or(|f| f.path.is_file()))
    }

    /// キャッシュファイルの文字列にする（1 行 1 書体、タブ区切り）。
    pub fn to_cache_string(&self) -> String {
        let mut s = format!("key\t{}\t{}\n", self.key.0, self.key.1);
        for (name, face) in self.slots() {
            if let Some(f) = face {
                s.push_str(&format!(
                    "{name}\t{}\t{}\t{}\n",
                    f.index,
                    f.family,
                    f.path.display()
                ));
            }
        }
        s
    }

    /// キャッシュファイルの文字列から読む。壊れていれば `None`。
    pub fn from_cache_string(s: &str) -> Option<Self> {
        let mut choice = FontChoice::default();
        let mut has_key = false;
        for line in s.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            match parts.as_slice() {
                ["key", body, mono] => {
                    choice.key = (body.to_string(), mono.to_string());
                    has_key = true;
                }
                [name, index, family, path] => {
                    *choice.slot_mut(name)? = Some(FaceRef {
                        family: family.to_string(),
                        path: PathBuf::from(path),
                        index: index.parse().ok()?,
                    });
                }
                _ => return None,
            }
        }
        has_key.then_some(choice)
    }
}

// ==== 機能: キャッシュファイル ====

/// キャッシュファイルの場所（eframe の設定と同じフォルダ）。
fn cache_path() -> Option<PathBuf> {
    eframe::storage_dir("mdview").map(|d| d.join("font_cache.tsv"))
}

fn read_cache() -> Option<FontChoice> {
    let text = std::fs::read_to_string(cache_path()?).ok()?;
    FontChoice::from_cache_string(&text).filter(FontChoice::files_exist)
}

fn write_cache(choice: &FontChoice) {
    if let Some(path) = cache_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, choice.to_cache_string());
    }
}

// ==== 機能: 起動時の先読み ====

/// 読み込み済みのフォント（egui の定義・報告・選んだ書体）。
pub struct LoadedFonts {
    pub defs: FontDefinitions,
    pub report: FontReport,
    pub choice: FontChoice,
}

impl LoadedFonts {
    /// 読み込みに失敗したとき用（egui 同梱のフォントだけ）。
    pub fn fallback() -> Self {
        let choice = FontChoice::default();
        let (defs, report) = build(&choice);
        Self {
            defs,
            report,
            choice,
        }
    }
}

/// 起動直後に呼び、別スレッドでフォントを読み込み始める。
/// キャッシュがあればそれを、なければ全スキャンして選ぶ（結果はキャッシュに保存）。
pub fn preload() -> std::thread::JoinHandle<LoadedFonts> {
    std::thread::spawn(|| {
        let choice = read_cache().unwrap_or_else(|| {
            let choice = Scanner::new().choose("", "");
            write_cache(&choice);
            choice
        });
        let (defs, report) = build(&choice);
        LoadedFonts {
            defs,
            report,
            choice,
        }
    })
}

/// 設定で指定されたフォントに合わせて読み込む（設定画面で変えたとき）。全スキャンする。
pub fn load_with(scanner: &Scanner, body: &str, mono: &str) -> LoadedFonts {
    let choice = scanner.choose(body, mono);
    write_cache(&choice);
    let (defs, report) = build(&choice);
    LoadedFonts {
        defs,
        report,
        choice,
    }
}

// ==== 機能: 全フォントのスキャン（遅い。必要なときだけ） ====

/// システムの全フォントのデータベース。
pub struct Scanner {
    db: fontdb::Database,
}

impl Scanner {
    /// システムフォント（Linux では追加フォルダも）を調べる。
    pub fn new() -> Self {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        if cfg!(target_os = "linux") {
            for dir in EXTRA_DIRS {
                if Path::new(dir).is_dir() {
                    db.load_fonts_dir(dir);
                }
            }
        }
        Self { db }
    }

    /// 重複のないファミリー名の一覧（名前順）。設定画面の選択肢に使う。
    pub fn families(&self) -> Vec<String> {
        let mut set: Vec<String> = self
            .db
            .faces()
            .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        set.sort_by_key(|s| s.to_lowercase());
        set
    }

    /// ファミリー名（大文字小文字を区別しない）で書体を探す。
    /// `bold` なら太さ 600 以上、そうでなければ 600 未満から、700 / 400 に最も近い
    /// 斜体でないものを選ぶ。
    fn find(&self, family: &str, bold: bool) -> Option<FaceRef> {
        let target = if bold { 700 } else { 400 };
        let face = self
            .db
            .faces()
            .filter(|f| {
                f.families
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case(family))
            })
            .filter(|f| matches!(f.source, fontdb::Source::File(_)))
            .filter(|f| !bold || f.weight.0 >= 600)
            .filter(|f| bold || f.weight.0 < 600)
            .min_by_key(|f| {
                let italic = if f.style == fontdb::Style::Normal {
                    0
                } else {
                    10_000
                };
                (f.weight.0 as i32 - target).abs() + italic
            })?;
        let fontdb::Source::File(path) = &face.source else {
            return None;
        };
        Some(FaceRef {
            family: family.to_string(),
            path: path.clone(),
            index: face.index,
        })
    }

    /// 用途ごとの書体を選ぶ。`body` / `mono` が空でなければ候補リストより優先する。
    pub fn choose(&self, body: &str, mono: &str) -> FontChoice {
        let pick = |names: &[&str], over: &str| -> Option<FaceRef> {
            std::iter::once(over)
                .filter(|s| !s.trim().is_empty())
                .chain(names.iter().copied())
                .find_map(|n| self.find(n, false))
        };
        let body_face = pick(LATIN_SANS, body);
        let cjk_face = pick(CJK_SANS, "");
        FontChoice {
            key: (body.to_string(), mono.to_string()),
            body_bold: body_face.as_ref().and_then(|f| self.find(&f.family, true)),
            cjk_bold: cjk_face.as_ref().and_then(|f| self.find(&f.family, true)),
            body: body_face,
            cjk: cjk_face,
            mono: pick(MONO, mono),
            symbol: SYMBOLS.iter().find_map(|n| self.find(n, false)),
        }
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

// ==== 機能: egui のフォント定義を作る ====

/// 実際に使われたフォント（設定画面に表示する）。
#[derive(Debug, Clone, Default)]
pub struct FontReport {
    pub body: Option<String>,
    pub cjk: Option<String>,
    pub mono: Option<String>,
    pub bold: bool,
}

/// 選んだ書体のファイルを読み、egui のフォント定義を作る。
/// 同じファイル（.ttc）は 1 回だけ読み、ファイルの読み込みは並列に行う。
pub fn build(choice: &FontChoice) -> (FontDefinitions, FontReport) {
    // 1. 必要なファイルを並列に読む
    let paths: Vec<PathBuf> = choice
        .slots()
        .iter()
        .filter_map(|(_, f)| f.as_ref().map(|f| f.path.clone()))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let handles: Vec<_> = paths
        .into_iter()
        .map(|p| std::thread::spawn(move || (p.clone(), std::fs::read(&p).ok())))
        .collect();
    let blobs: HashMap<PathBuf, egui::epaint::text::Blob> = handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .filter_map(|(p, data)| Some((p, Arc::new(data?) as egui::epaint::text::Blob)))
        .collect();

    // 2. 用途ごとに egui に登録する（キーは「ファミリー名#用途」）
    let mut defs = FontDefinitions::default();
    let mut add = |slot: &str, face: &Option<FaceRef>| -> Option<String> {
        let f = face.as_ref()?;
        let blob = blobs.get(&f.path)?.clone();
        let key = format!("{}#{slot}", f.family);
        defs.font_data
            .insert(key.clone(), Arc::new(FontData::from_blob(blob, f.index)));
        Some(key)
    };
    let body = add("body", &choice.body);
    let body_bold = add("body_bold", &choice.body_bold);
    let cjk = add("cjk", &choice.cjk);
    let cjk_bold = add("cjk_bold", &choice.cjk_bold);
    let mono = add("mono", &choice.mono);
    let symbol = add("symbol", &choice.symbol);

    let report = FontReport {
        body: body
            .as_ref()
            .and(choice.body.as_ref().map(|f| f.family.clone())),
        cjk: cjk
            .as_ref()
            .and(choice.cjk.as_ref().map(|f| f.family.clone())),
        mono: mono
            .as_ref()
            .and(choice.mono.as_ref().map(|f| f.family.clone())),
        bold: body_bold.is_some() || cjk_bold.is_some(),
    };

    // 3. ファミリーごとのフォールバック順（指定 → egui 既定 → 記号、重複なし）
    let base_prop = defs.families[&FontFamily::Proportional].clone();
    let base_mono = defs.families[&FontFamily::Monospace].clone();
    let chain = |parts: Vec<Option<String>>, tail: &[String]| -> Vec<String> {
        let mut seen = HashSet::new();
        parts
            .into_iter()
            .flatten()
            .chain(tail.iter().cloned())
            .chain(symbol.iter().cloned())
            .filter(|k| seen.insert(k.clone()))
            .collect()
    };
    let prop = chain(vec![body.clone(), cjk.clone()], &base_prop);
    // 太字の書体が無ければ通常の書体で代用する。
    let bold_list = chain(
        vec![body_bold.or(body.clone()), cjk_bold.or(cjk.clone())],
        &base_prop,
    );
    let monospace = chain(vec![mono, cjk], &base_mono);
    defs.families.insert(FontFamily::Proportional, prop);
    defs.families.insert(FontFamily::Monospace, monospace);
    defs.families.insert(bold(), bold_list);
    (defs, report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_round_trip() {
        let choice = FontChoice {
            key: ("My Font".into(), String::new()),
            body: Some(FaceRef {
                family: "Segoe UI".into(),
                path: PathBuf::from("/fonts/segoe ui.ttf"),
                index: 0,
            }),
            cjk: Some(FaceRef {
                family: "Meiryo".into(),
                path: PathBuf::from("/fonts/meiryo.ttc"),
                index: 2,
            }),
            ..Default::default()
        };
        let s = choice.to_cache_string();
        assert_eq!(FontChoice::from_cache_string(&s), Some(choice));
        assert_eq!(FontChoice::from_cache_string("garbage"), None);
        assert_eq!(FontChoice::from_cache_string(""), None);
    }

    #[test]
    fn build_without_fonts_still_has_families() {
        let (defs, report) = build(&FontChoice::default());
        assert!(defs.families.contains_key(&bold()));
        assert!(report.body.is_none());
    }
}
