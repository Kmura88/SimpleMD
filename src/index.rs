//! # 機能: ノートの索引
//!
//! フォルダ内の全ノートを読み、次の情報をまとめる（フォルダのスキャンと同じバックグラウンドスレッドで作る）。
//! - タイトル（フロントマターの `title:`、なければ最初の `# 見出し`）
//! - タグ（フロントマターの `tags:` / `tag:` と、本文中の `#タグ`）
//! - リンク先のノート（`[文字](相対パス.md)` と `[[Wiki リンク]]`）
//! - バックリンク（そのノートへリンクしているノート）
//!
//! Wiki リンク `[[名前]]` の解決（ファイル名 → パス）もここで行う。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use std::time::SystemTime;

use regex::Regex;

use crate::util;

/// 1 ノートの情報。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NoteInfo {
    pub title: Option<String>,
    pub tags: Vec<String>,
    /// リンク先ノートの番号（`Tree::files` の添字）。重複なし。
    pub links_to: Vec<usize>,
}

/// 1 ファイルを読んで取り出した情報（リンクはまだノート番号に解決していない）。
/// ファイルの更新時刻と一緒に覚えておき、変わっていなければ次のスキャンで使い回す。
#[derive(Debug, Clone)]
struct Analyzed {
    modified: Option<SystemTime>,
    title: Option<String>,
    tags: Vec<String>,
    /// ルートからの相対パスに解決済みの通常リンク。
    rel_links: Vec<PathBuf>,
    /// Wiki リンクの名前。
    wiki_links: Vec<String>,
}

/// フォルダ全体の索引。添字はすべて `Tree::files` と同じ並び。
#[derive(Debug, Clone, Default)]
pub struct NoteIndex {
    pub notes: Vec<NoteInfo>,
    /// ノート番号 → そのノートへリンクしているノート番号の一覧。
    pub backlinks: Vec<Vec<usize>>,
    /// タグ → ノート番号の一覧（タグ名順）。
    pub tags: BTreeMap<String, Vec<usize>>,
    /// 今回のスキャンで実際に読んだファイル数（差分更新の確認用）。
    pub files_read: usize,
    /// 小文字にしたファイル名（拡張子なし）→ ノート番号。Wiki リンクの解決用。
    by_stem: HashMap<String, Vec<usize>>,
    /// ルートからの相対パス → ノート番号。
    by_rel: HashMap<PathBuf, usize>,
    /// 機能: 差分更新 — 相対パス → 前回読んだ結果。
    cache: Arc<HashMap<PathBuf, Analyzed>>,
}

impl NoteIndex {
    /// `files`（ルートからの相対パス）の索引を作る。
    /// `prev`（前回の索引）があれば、更新時刻が変わっていないファイルは読み直さない。
    pub fn build(root: &Path, files: &[PathBuf], prev: Option<&NoteIndex>) -> Self {
        let mut idx = NoteIndex {
            by_rel: files
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, f)| (f, i))
                .collect(),
            ..Default::default()
        };
        for (i, rel) in files.iter().enumerate() {
            let stem = rel
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            idx.by_stem.entry(stem).or_default().push(i);
        }

        // 1 周目: 各ファイルを読む（変わっていなければ前回の結果を使う）。
        let mut cache: HashMap<PathBuf, Analyzed> = HashMap::with_capacity(files.len());
        for rel in files {
            let path = root.join(rel);
            let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let reused = prev
                .and_then(|p| p.cache.get(rel))
                .filter(|a| modified.is_some() && a.modified == modified)
                .cloned();
            let analyzed = reused.unwrap_or_else(|| {
                idx.files_read += 1;
                // Shift_JIS のノートもタイトル・タグが文字化けしないよう、表示と同じ判定で読む。
                let text = std::fs::read(&path)
                    .map(|b| crate::app::decode(&b).0)
                    .unwrap_or_default();
                analyze(root, rel, &text, modified)
            });
            cache.insert(rel.clone(), analyzed);
        }

        // 2 周目: リンクをノート番号に解決し、バックリンクとタグの逆引きを作る。
        idx.backlinks = vec![Vec::new(); files.len()];
        let mut notes = Vec::with_capacity(files.len());
        for (i, rel) in files.iter().enumerate() {
            let a = &cache[rel];
            let mut links_to: Vec<usize> = a
                .rel_links
                .iter()
                .filter_map(|r| idx.by_rel.get(r).copied())
                .chain(
                    a.wiki_links
                        .iter()
                        .filter_map(|n| idx.resolve_wiki(files, n)),
                )
                .collect();
            links_to.sort_unstable();
            links_to.dedup();
            for &t in &links_to {
                if t != i {
                    idx.backlinks[t].push(i);
                }
            }
            for tag in &a.tags {
                idx.tags.entry(tag.clone()).or_default().push(i);
            }
            notes.push(NoteInfo {
                title: a.title.clone(),
                tags: a.tags.clone(),
                links_to,
            });
        }
        idx.notes = notes;
        idx.cache = Arc::new(cache);
        idx
    }

    /// 相対パスからノート番号を引く。
    pub fn by_rel(&self, rel: &Path) -> Option<usize> {
        self.by_rel.get(rel).copied()
    }

    /// Wiki リンクの名前（`名前`, `フォルダ/名前`, `名前#見出し`, `名前|表示名`）からノートを探す。
    /// 同名が複数あるときは、パスが短い（浅い）ものを優先する。
    pub fn resolve_wiki(&self, files: &[PathBuf], name: &str) -> Option<usize> {
        let name = name.split(['#', '|']).next().unwrap_or("").trim();
        let name = name.replace('\\', "/");
        let name = name
            .strip_suffix(".md")
            .or_else(|| name.strip_suffix(".markdown"))
            .unwrap_or(&name)
            .to_lowercase();
        if name.is_empty() {
            return None;
        }
        let pick = |cands: &mut Vec<usize>| {
            cands.sort_by_key(|&i| (files[i].components().count(), files[i].clone()));
            cands.first().copied()
        };
        if name.contains('/') {
            let mut cands: Vec<usize> = files
                .iter()
                .enumerate()
                .filter(|(_, f)| {
                    let s = f
                        .with_extension("")
                        .to_string_lossy()
                        .replace('\\', "/")
                        .to_lowercase();
                    s == name || s.ends_with(&format!("/{name}"))
                })
                .map(|(i, _)| i)
                .collect();
            return pick(&mut cands);
        }
        let mut cands = self.by_stem.get(&name)?.clone();
        pick(&mut cands)
    }
}

// ==== 解析の部品 ====

/// 1 ノートを解析する（タイトル・タグ・リンク先）。
fn analyze(root: &Path, rel: &Path, text: &str, modified: Option<SystemTime>) -> Analyzed {
    let (front, body) = split_front_matter(text);
    let body = strip_code(body);
    let mut tags = Vec::new();
    if let Some(f) = front {
        tags.extend(front_tags(f));
    }
    tags.extend(body_tags(&body));
    tags.sort();
    tags.dedup();

    // 通常のリンク: ノートのフォルダからの相対パスを、ルートからの相対パスにする。
    let dir = root
        .join(rel)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let rel_links = MD_LINK
        .captures_iter(&body)
        .filter_map(|c| {
            let target = c[1].split('#').next().unwrap_or("");
            if target.is_empty() || util::has_scheme(target) {
                return None;
            }
            let abs = util::resolve_path(target, &dir);
            abs.strip_prefix(root).ok().map(Path::to_path_buf)
        })
        .collect();
    let wiki_links = WIKI_LINK
        .captures_iter(&body)
        .map(|c| c[1].to_string())
        .collect();
    Analyzed {
        modified,
        title: front
            .and_then(|f| front_value(f, "title"))
            .or_else(|| first_h1(&body)),
        tags,
        rel_links,
        wiki_links,
    }
}

static MD_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\]\(\s*<?([^)\s>]+)>?(?:\s+"[^"]*")?\s*\)"#).unwrap());
static WIKI_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\[\]\n]+)\]\]").unwrap());
static HASHTAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[\s(（])#([\p{L}\p{N}_/\-]+)").unwrap());
static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?ms)^\s*(```|~~~).*?^\s*(```|~~~)\s*$").unwrap());
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]*`").unwrap());

/// 先頭の `---` で囲まれたフロントマターと本文に分ける。
fn split_front_matter(text: &str) -> (Option<&str>, &str) {
    let text = text.trim_start_matches('\u{feff}');
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (None, text);
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        let t = line.trim_end();
        if t == "---" || t == "..." {
            return (Some(&rest[..offset]), &rest[offset + line.len()..]);
        }
        offset += line.len();
    }
    (None, text)
}

/// コードブロックとインラインコードを除く（その中の `#` や リンクを拾わないため）。
fn strip_code(body: &str) -> String {
    let no_fence = FENCE.replace_all(body, "");
    INLINE_CODE.replace_all(&no_fence, "").into_owned()
}

/// フロントマターの `key: value` を取り出す（引用符は外す）。
fn front_value(front: &str, key: &str) -> Option<String> {
    front.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        (k.trim() == key)
            .then(|| unquote(v.trim()).to_string())
            .filter(|v| !v.is_empty())
    })
}

/// フロントマターのタグ。`tags: [a, b]` / `tags: a, b` / 次行以降の `- a` に対応。
fn front_tags(front: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut in_list = false;
    for line in front.lines() {
        if in_list {
            let t = line.trim();
            if let Some(item) = t.strip_prefix("- ") {
                tags.push(clean_tag(item));
                continue;
            }
            if t.is_empty() {
                continue;
            }
            in_list = false;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        if !matches!(k.trim(), "tags" | "tag" | "keywords") {
            continue;
        }
        let v = v.trim();
        if v.is_empty() {
            in_list = true;
            continue;
        }
        let v = v.trim_start_matches('[').trim_end_matches(']');
        tags.extend(v.split([',', ' ']).map(clean_tag).filter(|t| !t.is_empty()));
    }
    tags.retain(|t| !t.is_empty());
    tags
}

/// 本文中の `#タグ`（見出しの `# ` や数字だけの `#123` は除く）。
fn body_tags(body: &str) -> Vec<String> {
    HASHTAG
        .captures_iter(body)
        .map(|c| c[1].trim_end_matches(['/', '-']).to_string())
        .filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit()))
        .collect()
}

fn clean_tag(s: &str) -> String {
    unquote(s.trim()).trim_start_matches('#').trim().to_string()
}

fn unquote(s: &str) -> &str {
    s.trim_matches(|c| c == '"' || c == '\'')
}

/// 最初の `# 見出し` の文字列。
fn first_h1(body: &str) -> Option<String> {
    body.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().trim_end_matches('#').trim().to_string())
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (PathBuf, Vec<PathBuf>) {
        let dir = std::env::temp_dir().join(format!(
            "mdview_index_{}_{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a/deep")).unwrap();
        std::fs::write(
            dir.join("home.md"),
            "---\ntitle: \"Home page\"\ntags: [start, 'top']\n---\n# Ignored\nSee [x](a/one.md#sec) and [[Two]] and [[a/deep/two|alias]].\n#inline-tag but not #123 and `#code`\n```\n[[One]]\n```\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("a/one.md"),
            "# One\ntags:\nback to [home](../home.md)\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("a/deep/two.md"),
            "---\ntags:\n  - deep\n  - start\n---\nno title heading\n",
        )
        .unwrap();
        let files = vec![
            PathBuf::from("a/deep/two.md"),
            PathBuf::from("a/one.md"),
            PathBuf::from("home.md"),
        ];
        (dir, files)
    }

    #[test]
    fn titles_tags_links_backlinks() {
        let (dir, files) = setup();
        let idx = NoteIndex::build(&dir, &files, None);
        let home = &idx.notes[2];
        assert_eq!(home.title.as_deref(), Some("Home page"));
        assert_eq!(home.tags, vec!["inline-tag", "start", "top"]);
        // a/one.md と a/deep/two.md（[[Two]] と [[a/deep/two]] は同じノート）
        assert_eq!(home.links_to, vec![0, 1]);
        assert_eq!(idx.notes[1].title.as_deref(), Some("One"));
        assert_eq!(idx.notes[0].tags, vec!["deep", "start"]);
        assert_eq!(idx.backlinks[2], vec![1]); // one → home
        assert_eq!(idx.backlinks[0], vec![2]);
        assert_eq!(idx.tags["start"], vec![0, 2]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn incremental_rebuild_reads_only_changed_files() {
        let (dir, files) = setup();
        let first = NoteIndex::build(&dir, &files, None);
        assert_eq!(first.files_read, 3);
        // 何も変えなければ 1 つも読み直さない
        let again = NoteIndex::build(&dir, &files, Some(&first));
        assert_eq!(again.files_read, 0);
        assert_eq!(again.notes, first.notes);
        // 1 つ書き換えると、そのファイルだけ読み直して結果に反映される
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("a/one.md"), "# Renamed\n#newtag\n").unwrap();
        let third = NoteIndex::build(&dir, &files, Some(&again));
        assert_eq!(third.files_read, 1);
        assert_eq!(third.notes[1].title.as_deref(), Some("Renamed"));
        assert!(third.tags.contains_key("newtag"));
        assert!(third.backlinks[2].is_empty()); // one → home のリンクが消えた
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn wiki_resolution() {
        let (dir, files) = setup();
        let idx = NoteIndex::build(&dir, &files, None);
        assert_eq!(idx.resolve_wiki(&files, "ONE"), Some(1));
        assert_eq!(idx.resolve_wiki(&files, "two#Heading|x"), Some(0));
        assert_eq!(idx.resolve_wiki(&files, "deep/two.md"), Some(0));
        assert_eq!(idx.resolve_wiki(&files, "missing"), None);
        assert_eq!(idx.resolve_wiki(&files, ""), None);
        std::fs::remove_dir_all(dir).ok();
    }
}
