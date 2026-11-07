//! # 機能: 開いているフォルダ（ワークスペース）
//!
//! - フォルダのスキャン（ファイルツリーと Markdown ファイル一覧、ノートの索引）
//! - フォルダの変更監視（ファイルの追加・削除・保存でツリーと索引を自動更新）
//! - 自然順ソート、ファイル名のあいまい検索、全文検索（バックグラウンド）

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::index::NoteIndex;
use crate::textsearch::{self, Needle};

// ==== 機能: フォルダのスキャン（ファイルツリー） ====

/// 中に入らないフォルダ名。
const IGNORED_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "__pycache__",
    "venv",
    ".venv",
    "dist",
    "build",
];
/// たどるフォルダの最大の深さ（シンボリックリンクの循環対策も兼ねる）。
const MAX_DEPTH: usize = 16;
/// 集めるファイル数の上限（巨大なフォルダを開いたときの保険）。
const MAX_FILES: usize = 50_000;

/// 拡張子が Markdown（`.md` / `.markdown` / `.mdx` など）なら true。
pub fn is_markdown(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "md" | "markdown" | "mdown" | "mkd" | "mkdn" | "mdx"
        )
    })
}

#[derive(Debug, Clone, PartialEq)]
/// ファイルツリーの 1 項目（ファイルまたはフォルダ）。
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    /// ファイルなら `None`。
    pub children: Option<Vec<Node>>,
}

/// フォルダをスキャンした結果。
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub root: PathBuf,
    /// 最上位の項目（フォルダが先、次にファイル。それぞれ自然順）。
    pub nodes: Vec<Node>,
    /// すべての Markdown ファイル（root からの相対パス、ツリーの並び順）。
    pub files: Vec<PathBuf>,
    /// 機能: ノートの索引（タイトル・タグ・リンク・バックリンク）
    pub index: NoteIndex,
}

impl Tree {
    /// 絶対パスからノート番号を引く。
    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.index.by_rel(path.strip_prefix(&self.root).ok()?)
    }
}

/// `root` 以下を再帰的にたどって Markdown ファイルを集める。Markdown を含まないフォルダは省く。
pub fn scan(root: &Path, prev: Option<&NoteIndex>) -> Tree {
    let mut files = Vec::new();
    let nodes = scan_dir(root, root, 0, &mut files);
    // 前回の索引があれば、変わっていないファイルは読み直さない（差分更新）。
    let index = NoteIndex::build(root, &files, prev);
    Tree {
        root: root.to_path_buf(),
        nodes,
        files,
        index,
    }
}

/// 1 つのフォルダを読み、子フォルダへ再帰する。見つけたファイルは `files` に追加する。
/// 隠しファイル（`.` 始まり）と [`IGNORED_DIRS`] は飛ばす。
fn scan_dir(root: &Path, dir: &Path, depth: usize, files: &mut Vec<PathBuf>) -> Vec<Node> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<(String, PathBuf, bool)> = read
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = e.path();
            // シンボリックリンクはたどるが、循環は MAX_DEPTH で止める。
            let meta = fs::metadata(&path).ok()?;
            Some((name, path, meta.is_dir()))
        })
        .collect();
    entries.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| natural_cmp(&a.0, &b.0)));

    let mut nodes = Vec::new();
    for (name, path, is_dir) in entries {
        if files.len() >= MAX_FILES {
            break;
        }
        if is_dir {
            if depth >= MAX_DEPTH || IGNORED_DIRS.contains(&name.as_str()) {
                continue;
            }
            let before = files.len();
            let children = scan_dir(root, &path, depth + 1, files);
            if files.len() > before {
                nodes.push(Node {
                    name,
                    path,
                    children: Some(children),
                });
            }
        } else if is_markdown(&path) {
            files.push(path.strip_prefix(root).unwrap_or(&path).to_path_buf());
            nodes.push(Node {
                name,
                path,
                children: None,
            });
        }
    }
    nodes
}

// ==== 機能: 自然順ソート ====

/// "file2" が "file10" より前に来るように比べる（大文字小文字は区別しない）。
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek(), b.peek()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take_num = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut s = String::new();
                    while let Some(c) = it.peek().filter(|c| c.is_ascii_digit()) {
                        s.push(*c);
                        it.next();
                    }
                    s
                };
                let (na, nb) = (take_num(&mut a), take_num(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

// ==== 機能: フォルダの変更監視 ====

/// フォルダを再帰的に監視し、Markdown やフォルダが変わったら知らせる。
/// OS の通知が届かない環境（WSL から Windows 側のドライブを見ている場合など）のために、
/// 一定時間ごとの再スキャンも合わせて行う。
pub struct FolderWatcher {
    _watcher: Option<notify::RecommendedWatcher>,
    rx: mpsc::Receiver<()>,
    /// 最後に変更を受け取った時刻（まとめて処理するための待ち時間に使う）。
    dirty_since: Option<Instant>,
    last_rescan: Instant,
    /// 一定時間ごとの再スキャンをするか。
    periodic: bool,
}

/// OS の変更通知が届かないことがある場所か。
/// WSL から Windows 側のドライブ（`/mnt/c` など）を見ている場合は inotify が働かない。
fn notifications_unreliable(root: &Path) -> bool {
    let wsl = std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::fs::read_to_string("/proc/version")
            .is_ok_and(|v| v.to_ascii_lowercase().contains("microsoft"));
    wsl && root.starts_with("/mnt/")
}

/// 変更をまとめるための待ち時間。
const DEBOUNCE: Duration = Duration::from_millis(700);
/// 通知が来なくても再スキャンする間隔。
const PERIODIC: Duration = Duration::from_secs(30);

impl FolderWatcher {
    /// `root` の監視を始める。変更があると画面の再描画を予約する。
    pub fn new(root: &Path, ctx: &egui::Context) -> Self {
        use notify::Watcher as _;
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if matches!(ev.kind, notify::EventKind::Access(_)) {
                return;
            }
            // Markdown ファイルかフォルダに関係する変更だけを拾う。
            let relevant = ev
                .paths
                .iter()
                .any(|p| is_markdown(p) || p.extension().is_none());
            if relevant {
                let _ = tx.send(());
                ctx.request_repaint_after(DEBOUNCE);
            }
        })
        .ok()
        .and_then(|mut w| {
            w.watch(root, notify::RecursiveMode::Recursive).ok()?;
            Some(w)
        });
        // 通知が届かない環境でだけ、定期的に読み直す（全ノートを読むので無駄に回さない）。
        let periodic = watcher.is_none() || notifications_unreliable(root);
        Self {
            periodic,
            _watcher: watcher,
            rx,
            dirty_since: None,
            last_rescan: Instant::now(),
        }
    }

    /// 再スキャンすべきなら true（変更後しばらく静かになったとき、または定期時刻）。
    pub fn should_rescan(&mut self) -> bool {
        while self.rx.try_recv().is_ok() {
            self.dirty_since = Some(Instant::now());
        }
        let now = Instant::now();
        let due = match self.dirty_since {
            Some(t) => now - t >= DEBOUNCE,
            None => self.periodic && now - self.last_rescan >= PERIODIC,
        };
        if due {
            self.dirty_since = None;
            self.last_rescan = now;
        }
        due
    }
}

// ==== 機能: ファイル名のあいまい検索（クイックオープン用） ====

/// `query` の文字が `candidate` に順番どおり含まれるかで点数をつける（大文字小文字は区別しない）。
/// 一致しなければ `None`。点数は高いほど良い。一致した文字の位置も返す。
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<(i32, Vec<usize>)> {
    // 照合はひらがな/カタカナ・全角/半角・大文字/小文字を区別しない。
    let q: Vec<char> = textsearch::normalize(query)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if q.is_empty() {
        return Some((0, Vec::new()));
    }
    let chars: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| textsearch::fold_char(*c)).collect();
    // ファイル名の部分（最後の区切り以降）での一致は加点する。
    let name_start = chars
        .iter()
        .rposition(|c| *c == '/' || *c == '\\')
        .map_or(0, |i| i + 1);

    let mut score = 0;
    let mut positions = Vec::with_capacity(q.len());
    let mut qi = 0;
    let mut prev: Option<usize> = None;
    for (i, c) in lower.iter().enumerate() {
        if qi < q.len() && *c == q[qi] {
            let mut s = 1;
            if prev.is_some_and(|p| p + 1 == i) {
                s += 5; // 連続して一致
            }
            let boundary = i == 0
                || matches!(chars[i - 1], '/' | '\\' | '_' | '-' | ' ' | '.')
                || (chars[i].is_uppercase() && chars[i - 1].is_lowercase());
            // 単語の区切り（先頭・記号の直後・camelCase の大文字）で一致したら加点。
            if boundary {
                s += 4;
            }
            if i >= name_start {
                s += 2;
            }
            score += s;
            positions.push(i);
            prev = Some(i);
            qi += 1;
        }
    }
    if qi < q.len() {
        return None;
    }
    // 短い候補と、前のほうで一致したものを優先する。
    score -= (chars.len() as i32) / 8;
    score -= positions.first().map_or(0, |p| (*p as i32) / 16);
    Some((score, positions))
}

// ==== 機能: 全文検索（バックグラウンドのスレッドで実行） ====

/// 全文検索で一致した 1 行。
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub path: PathBuf,
    /// 行番号（1 始まり）。
    pub line: usize,
    pub text: String,
}

/// 1 回の検索の結果一式。`generation` で古い検索の結果を見分ける。
pub struct SearchResults {
    pub generation: u64,
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub files_with_hits: usize,
    pub truncated: bool,
}

/// 検索結果の最大件数（超えたら打ち切って `truncated` にする）。
const MAX_HITS: usize = 1000;

/// すべてのファイルを行単位で検索する（大文字小文字は区別しない）。終わるまで戻らない。
/// 戻り値は（一致した行, 一致したファイル数, 打ち切ったか）。
pub fn search_files(root: &Path, files: &[PathBuf], query: &str) -> (Vec<SearchHit>, usize, bool) {
    let mut hits = Vec::new();
    let mut files_with_hits = 0;
    // 日本語の表記ゆれ（ひらがな/カタカナ・全角/半角・大文字/小文字）を吸収して探す。
    let Some(needle) = Needle::new(query.trim(), false) else {
        return (hits, 0, false);
    };
    for rel in files {
        let path = root.join(rel);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        // Shift_JIS のファイルも読めるように、表示と同じ文字コード判定を使う。
        let (text, _) = crate::app::decode(&bytes);
        let mut any = false;
        for (i, line) in text.lines().enumerate() {
            if needle.is_in(line) {
                any = true;
                let trimmed = line.trim();
                let text: String = trimmed.chars().take(200).collect();
                hits.push(SearchHit {
                    path: path.clone(),
                    line: i + 1,
                    text,
                });
                if hits.len() >= MAX_HITS {
                    return (hits, files_with_hits + 1, true);
                }
            }
        }
        if any {
            files_with_hits += 1;
        }
    }
    (hits, files_with_hits, false)
}

/// バックグラウンドで検索する係。入力が落ち着くまで少し待ち、新しい依頼が来たら古い依頼は捨てる。
pub struct Searcher {
    generation: Arc<AtomicU64>,
    tx: mpsc::Sender<SearchResults>,
    rx: mpsc::Receiver<SearchResults>,
}

impl Default for Searcher {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            tx,
            rx,
        }
    }
}

impl Searcher {
    /// 検索を開始する（世代番号を進め、別スレッドで実行）。
    pub fn start(&self, root: PathBuf, files: Vec<PathBuf>, query: String, ctx: egui::Context) {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let current = self.generation.clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            // 入力のたびに全件スキャンしないよう、少し待ってから始める。
            std::thread::sleep(std::time::Duration::from_millis(120));
            if current.load(Ordering::SeqCst) != generation {
                return;
            }
            let (hits, files_with_hits, truncated) = search_files(&root, &files, &query);
            if current.load(Ordering::SeqCst) == generation {
                let _ = tx.send(SearchResults {
                    generation,
                    query,
                    hits,
                    files_with_hits,
                    truncated,
                });
                ctx.request_repaint();
            }
        });
    }

    /// 前回の確認以降に届いた最新の検索結果（最新の世代のものだけ）。
    pub fn poll(&self) -> Option<SearchResults> {
        let mut latest = None;
        while let Ok(r) = self.rx.try_recv() {
            if r.generation == self.generation.load(Ordering::SeqCst) {
                latest = Some(r);
            }
        }
        latest
    }
}

// ==== 機能: テスト ====

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v = vec!["note10.md", "Note2.md", "note1.md", "a.md"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a.md", "note1.md", "Note2.md", "note10.md"]);
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_match("xyz", "readme.md").is_none());
        let (s1, pos) = fuzzy_match("rdm", "docs/README.md").unwrap();
        assert_eq!(pos, vec![5, 8, 9]);
        let (s2, _) = fuzzy_match("rdm", "random/dir/more.md").unwrap();
        assert!(s1 > 0 && s2 > 0);
        let (exact, _) = fuzzy_match("read", "readme.md").unwrap();
        let (scattered, _) = fuzzy_match("read", "r_e_a_d.md").unwrap();
        assert!(exact > scattered);
        assert_eq!(fuzzy_match("", "x").unwrap().0, 0);
    }

    #[test]
    fn scan_and_search() {
        let dir = std::env::temp_dir().join(format!("mdview_ws_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub/deep")).unwrap();
        fs::create_dir_all(dir.join("empty")).unwrap();
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::create_dir_all(dir.join(".git")).unwrap();
        fs::write(dir.join("b.md"), "Hello\nworld").unwrap();
        fs::write(dir.join("a.markdown"), "nothing").unwrap();
        fs::write(dir.join("c.txt"), "hello").unwrap();
        fs::write(dir.join("sub/deep/x.md"), "say HELLO there").unwrap();
        fs::write(dir.join("node_modules/y.md"), "hello").unwrap();
        fs::write(dir.join(".git/z.md"), "hello").unwrap();

        let tree = scan(&dir, None);
        let names: Vec<_> = tree.nodes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["sub", "a.markdown", "b.md"]);
        assert_eq!(tree.files.len(), 3);
        assert_eq!(tree.files[0], PathBuf::from("sub/deep/x.md"));

        let (hits, nfiles, truncated) = search_files(&dir, &tree.files, "hello");
        assert_eq!(nfiles, 2);
        assert!(!truncated);
        assert_eq!(hits[0].text, "say HELLO there");
        assert_eq!(hits[1].line, 1);
        fs::remove_dir_all(&dir).unwrap();
    }
}
