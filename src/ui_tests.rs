//! # UI 操作のテスト（egui_kittest）
//! GPU なしで実際のアプリを動かし、クリック・キー入力・マウスオーバーで各機能を確認する。
//! 表示言語は英語に固定する（実行環境のロケールに左右されないように）。

use std::path::{Path, PathBuf};

use egui::{Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::app::App;
use crate::i18n::Language;

// ==== テスト用の準備 ====

/// テスト用のノートフォルダを一時ディレクトリに作る。
fn sample_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mdview_ui_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("guide")).unwrap();
    std::fs::write(
        dir.join("README.md"),
        "---\ntags: [home, demo]\n---\n# Home\n\n[the guide](guide/intro.md#setup)\n\n[top](#home)\n\n[[intro|Intro page]]\n\n## Second\n\nfind me, find me again\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("guide/intro.md"),
        "# Intro\n\nhello\n\n## Setup\n\nsetup text\n",
    )
    .unwrap();
    dir
}

/// アプリを起動し、フォルダのスキャンが終わるまで進める。
fn harness(start: &Path) -> Harness<'static, App> {
    let start = start.to_path_buf();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1100.0, 760.0))
        .allow_missing_glyphs()
        .build_eframe(move |cc| App::new(cc, Some(start), None));
    h.state_mut().settings.language = Language::English;
    h.state_mut().apply_language();
    for _ in 0..100 {
        h.step();
        if h.state().tree.is_some() && h.state().doc().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    settle(&mut h);
    h
}

/// アニメーションやバックグラウンド処理のために何フレームか進める。
fn settle(h: &mut Harness<'_, App>) {
    for _ in 0..40 {
        h.step();
    }
}

/// フォーカスのある入力欄に文字を入れる。
fn type_text(h: &Harness<'_, App>, text: &str) {
    h.event(egui::Event::Text(text.to_string()));
}

/// アクティブなタブのファイル名。
fn current_file(h: &Harness<'_, App>) -> String {
    h.state()
        .doc()
        .map(|d| d.path.file_name().unwrap().to_string_lossy().into_owned())
        .unwrap_or_default()
}

// ==== ナビゲーション ====

#[test]
fn opens_index_and_follows_links() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    assert_eq!(current_file(&h), "README.md");
    assert!(h.query_all_by_label("Home").count() >= 2); // 見出し＋目次

    // アンカー付きの相対リンクで別のノートへ
    h.get_by_label("the guide").click();
    settle(&mut h);
    assert_eq!(current_file(&h), "intro.md");

    // 戻る / 進む
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowLeft);
    settle(&mut h);
    assert_eq!(current_file(&h), "README.md");
    h.key_press_modifiers(Modifiers::ALT, Key::ArrowRight);
    settle(&mut h);
    assert_eq!(current_file(&h), "intro.md");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn quick_open_with_keyboard() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    h.key_press_modifiers(Modifiers::COMMAND, Key::P);
    settle(&mut h);
    assert!(h.state().quick.is_some(), "quick open should be visible");
    type_text(&h, "intr");
    settle(&mut h);
    h.key_press(Key::Enter);
    settle(&mut h);
    assert!(h.state().quick.is_none());
    assert_eq!(current_file(&h), "intro.md");
    std::fs::remove_dir_all(dir).ok();
}

// ==== 検索 ====

#[test]
fn find_in_page_counts_matches() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    settle(&mut h);
    type_text(&h, "find me");
    settle(&mut h);
    assert_eq!(h.state().tab().view.match_count, 2);
    h.get_by_label("1 / 2");
    h.key_press(Key::F3);
    settle(&mut h);
    h.get_by_label("2 / 2");
    h.key_press(Key::Escape);
    settle(&mut h);
    assert!(!h.state().find.open);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn full_text_search() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    settle(&mut h);
    type_text(&h, "setup");
    for _ in 0..100 {
        h.step(); // 検索中はスピナーが再描画を続ける
        if h.state().search.results.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let results = h.state().search.results.as_ref().expect("search finished");
    assert_eq!(results.hits.len(), 3); // README のリンク＋intro の見出しと本文
    assert_eq!(results.files_with_hits, 2);
    std::fs::remove_dir_all(dir).ok();
}

// ==== スクロール ====

#[test]
fn keyboard_scrolling_outline_jump_and_position_memory() {
    let dir = sample_dir();
    let body: String = (1..=80)
        .map(|i| format!("## Part {i}\n\nparagraph {i}\n\n"))
        .collect();
    std::fs::write(dir.join("long.md"), format!("# Long\n\n{body}")).unwrap();
    let mut h = harness(&dir.join("long.md"));
    assert_eq!(current_file(&h), "long.md");
    h.key_press(Key::End);
    settle(&mut h);
    let bottom = h.state().tab().scroll_offset;
    assert!(
        bottom > 2000.0,
        "End should scroll to the bottom, got {bottom}"
    );
    h.key_press(Key::Home);
    settle(&mut h);
    assert_eq!(h.state().tab().scroll_offset, 0.0);

    // 目次から見出しへ
    h.get_by_label("Part 20").click();
    settle(&mut h);
    let y = h.state().tab().view.heading_ys[20];
    let off = h.state().tab().scroll_offset;
    assert!(
        (off - (y - 12.0)).abs() < 2.0,
        "offset {off} should be at heading {y}"
    );

    // 機能: ファイルごとのスクロール位置の記憶 — 他のファイルを開いて戻ると位置が戻る
    let ctx = h.ctx.clone();
    h.state_mut().open(&ctx, &dir.join("README.md"), None, true);
    settle(&mut h);
    h.state_mut().open(&ctx, &dir.join("long.md"), None, true);
    settle(&mut h);
    let restored = h.state().tab().scroll_offset;
    assert!((restored - off).abs() < 2.0, "restored {restored} vs {off}");
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: タブ ====

#[test]
fn tabs_ctrl_click_and_close() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    // Ctrl+クリックで新しいタブ
    h.get_by_label("the guide")
        .click_modifiers(Modifiers::COMMAND);
    settle(&mut h);
    assert_eq!(h.state().tabs.len(), 2);
    assert_eq!(h.state().active, 1);
    assert_eq!(current_file(&h), "intro.md");
    // タブバーからタブを切り替え
    h.get_by_label("README").click();
    settle(&mut h);
    assert_eq!(current_file(&h), "README.md");
    // Ctrl+T で新しいタブ（ようこそ画面）、Ctrl+W で閉じる
    h.key_press_modifiers(Modifiers::COMMAND, Key::T);
    settle(&mut h);
    assert_eq!(h.state().tabs.len(), 3);
    assert!(h.state().doc().is_none());
    h.key_press_modifiers(Modifiers::COMMAND, Key::W);
    settle(&mut h);
    assert_eq!(h.state().tabs.len(), 2);
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: Wiki リンク・バックリンク・タグ ====

#[test]
fn wiki_links_backlinks_and_tags() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    // [[intro]] をクリック
    h.get_by_label("Intro page").click();
    settle(&mut h);
    assert_eq!(current_file(&h), "intro.md");
    // intro のバックリンクに README（タイトル "Home"）が出る
    h.get_by_label("BACKLINKS (1)");
    assert!(h.query_by_label_contains("Home").is_some());

    // タグ一覧
    h.get_by_label("Tags").click();
    settle(&mut h);
    h.get_by_label("#demo  1").click();
    settle(&mut h);
    assert_eq!(h.state().selected_tag.as_deref(), Some("demo"));
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: <details> 折りたたみ ====

#[test]
fn details_toggle() {
    let dir = sample_dir();
    std::fs::write(
        dir.join("d.md"),
        "# D\n\n<details>\n<summary>More info</summary>\n\nhidden body\n\n</details>\n",
    )
    .unwrap();
    let mut h = harness(&dir.join("d.md"));
    assert!(h.query_by_label("hidden body").is_none());
    h.get_by_label("More info").click();
    settle(&mut h);
    assert!(h.query_by_label("hidden body").is_some());
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: 画像の拡大表示・見出しリンクのコピー ====

#[test]
fn lightbox_and_heading_link() {
    let dir = sample_dir();
    let img = image::RgbaImage::from_pixel(40, 30, image::Rgba([200, 30, 30, 255]));
    img.save(dir.join("pic.png")).unwrap();
    std::fs::write(dir.join("i.md"), "# Pictures\n\n![a red box](pic.png)\n").unwrap();
    let mut h = harness(&dir.join("i.md"));
    // 画像の読み込み（バックグラウンド）を待つ
    for _ in 0..50 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    h.get_by_label("a red box").click();
    settle(&mut h);
    assert!(
        h.state()
            .lightbox
            .as_deref()
            .is_some_and(|u| u.ends_with("pic.png"))
    );
    h.key_press(Key::Escape);
    settle(&mut h);
    assert!(h.state().lightbox.is_none());

    // 見出しにマウスを乗せると 🔗 が出て、押すとリンクがコピーされる
    // 本文の見出し（目次の項目ではなく中央のもの）にマウスを乗せる
    h.get_all_by_label("Pictures")
        .find(|n| n.rect().left() > 300.0 && n.rect().left() < 800.0)
        .unwrap()
        .hover();
    settle(&mut h);
    h.get_by_label("🔗").click();
    // トーストは数秒で消えるので、すぐに確認する
    h.step();
    h.step();
    let toast = h.state().toast.as_ref().map(|(m, _)| m.clone());
    assert_eq!(toast.as_deref(), Some("Link copied"));
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: リンク先プレビュー ====

#[test]
fn link_preview_popup() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    assert!(h.query_by_label("setup text").is_none());
    h.get_by_label("the guide").hover();
    for _ in 0..60 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    // リンク先（guide/intro.md の Setup 節）の本文がポップアップに出る
    assert!(h.query_by_label("setup text").is_some());
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: スライド表示 ====

#[test]
fn presentation_mode() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    h.key_press(Key::F11);
    settle(&mut h);
    let total = h.state().slides.as_ref().unwrap().slides.len();
    assert_eq!(total, 2); // # Home / ## Second
    h.get_by_label("1 / 2");
    h.key_press(Key::ArrowRight);
    settle(&mut h);
    h.get_by_label("2 / 2");
    h.key_press(Key::Escape);
    settle(&mut h);
    assert!(h.state().slides.is_none());
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: 自動再読み込み・フォルダの変更監視・文字コード ====

#[test]
fn reloads_and_watches_folder() {
    let dir = sample_dir();
    let file = dir.join("README.md");
    let mut h = harness(&file);
    assert_eq!(h.state().doc().unwrap().doc.headings[0].text, "Home");
    std::fs::write(&file, "# Changed\n\nnew body\n").unwrap();
    std::fs::write(dir.join("guide/new-note.md"), "# New\n").unwrap();
    let mut ok = false;
    for _ in 0..300 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let changed = h.state().doc().unwrap().doc.headings[0].text == "Changed";
        let found = h
            .state()
            .tree
            .as_ref()
            .is_some_and(|t| t.files.iter().any(|f| f.ends_with("new-note.md")));
        if changed && found {
            ok = true;
            break;
        }
    }
    assert!(ok, "file reload and new file detection");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn shift_jis_file_is_decoded() {
    let dir = sample_dir();
    let (bytes, _, _) = encoding_rs::SHIFT_JIS.encode("# 見出し\n\n本文です\n");
    std::fs::write(dir.join("sjis.md"), &bytes).unwrap();
    let h = harness(&dir.join("sjis.md"));
    let doc = h.state().doc().unwrap();
    assert!(doc.sjis);
    assert_eq!(doc.doc.headings[0].text, "見出し");
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: UI の言語 ====

#[test]
fn japanese_ui() {
    let dir = sample_dir();
    let mut h = harness(&dir);
    h.state_mut().settings.language = Language::Japanese;
    h.state_mut().apply_language();
    settle(&mut h);
    h.get_by_label("ファイル");
    h.get_by_label("タグ");
    std::fs::remove_dir_all(dir).ok();
}

// ==== 不具合の再発防止 ====

/// コードブロックにカーソルを合わせても、後続の要素の位置が変わらない
/// （以前はコピーボタンを `ui.put` で置いていたため、後続のブロックがコードに重なった）。
#[test]
fn hovering_code_does_not_shift_layout() {
    let dir = sample_dir();
    std::fs::write(
        dir.join("c.md"),
        "# Code\n\n```rust\nfn a() {}\nfn b() {}\nfn c() {}\n```\n\n## After code\n\ntext below\n",
    )
    .unwrap();
    let mut h = harness(&dir.join("c.md"));
    let doc_node = |h: &Harness<'_, App>, label: &str| {
        h.get_all_by_label(label)
            .map(|n| n.rect())
            .find(|r| r.left() > 300.0 && r.left() < 800.0)
            .unwrap()
    };
    let before = doc_node(&h, "After code");
    let code = h
        .get_all_by_label_contains("fn a()")
        .map(|n| n.rect())
        .next()
        .unwrap();
    h.hover_at(code.center());
    settle(&mut h);
    assert!(
        h.query_by_label("📋 Copy").is_some(),
        "copy button appears on hover"
    );
    let after = doc_node(&h, "After code");
    assert_eq!(before, after, "following blocks must not move");
    std::fs::remove_dir_all(dir).ok();
}

// ==== 機能: ドラッグ＆ドロップ ====

/// テスト用のドロップされたファイル。
#[derive(Debug)]
struct Dropped(PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &Path {
        &self.0
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

#[test]
fn drop_file_and_folder() {
    let dir = sample_dir();
    let other = sample_dir();
    std::fs::write(other.join("dropped.md"), "# Dropped\n").unwrap();
    let mut h = harness(&dir);
    // ファイルをドロップ → そのファイルを開く
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(dir.join("guide/intro.md"))));
    settle(&mut h);
    assert_eq!(current_file(&h), "intro.md");
    // フォルダをドロップ → そのフォルダを開いて README を表示
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(other.clone())));
    for _ in 0..100 {
        h.step();
        if current_file(&h) == "README.md" && h.state().root.ends_with(other.file_name().unwrap()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.state().root.ends_with(other.file_name().unwrap()));
    std::fs::remove_dir_all(dir).ok();
    std::fs::remove_dir_all(other).ok();
}

// ==== 機能: 日本語の検索 ====

#[test]
fn japanese_find_in_page_and_full_text_search() {
    let dir = sample_dir();
    std::fs::write(
        dir.join("jp.md"),
        "# 会議\n\n今日のメモ。昨日のﾒﾓ。全角のＡＢＣ。\n",
    )
    .unwrap();
    // Shift_JIS のファイルも全文検索の対象になる
    let (sjis, _, _) = encoding_rs::SHIFT_JIS.encode("# 古いノート\n\n手書きのメモを清書した。\n");
    std::fs::write(dir.join("old.md"), &sjis).unwrap();
    let mut h = harness(&dir.join("jp.md"));

    // ページ内検索: ひらがなで、カタカナ・半角カタカナに当たる
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    settle(&mut h);
    type_text(&h, "めも");
    settle(&mut h);
    assert_eq!(h.state().tab().view.match_count, 2);
    // 全角英字も半角で当たる
    h.state_mut().find.query = "abc".into();
    settle(&mut h);
    assert_eq!(h.state().tab().view.match_count, 1);
    h.key_press(Key::Escape);
    settle(&mut h);

    // 全文検索: ひらがなで、UTF-8 と Shift_JIS の両方のファイルのカタカナに当たる
    h.key_press_modifiers(Modifiers::COMMAND | Modifiers::SHIFT, Key::F);
    settle(&mut h);
    type_text(&h, "めも");
    for _ in 0..100 {
        h.step();
        if h.state().search.results.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let results = h.state().search.results.as_ref().expect("search finished");
    let files: Vec<String> = results
        .hits
        .iter()
        .map(|hit| hit.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(files.contains(&"jp.md".to_string()), "{files:?}");
    assert!(
        files.contains(&"old.md".to_string()),
        "Shift_JIS: {files:?}"
    );
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn quick_open_with_romaji_and_kana() {
    let dir = sample_dir();
    std::fs::write(dir.join("ガイド.md"), "# ガイド\n").unwrap();
    let files = vec![
        PathBuf::from("README.md"),
        PathBuf::from("guide/intro.md"),
        PathBuf::from("ガイド.md"),
    ];
    // ローマ字・ひらがな・半角カタカナのどれでも「ガイド.md」が一番上に来る
    for q in ["gaido", "がいど", "ｶﾞｲﾄﾞ"] {
        let ranked = crate::ui::rank_files_for_test(&files, q);
        assert_eq!(ranked.first().copied(), Some(2), "query {q}");
    }
    std::fs::remove_dir_all(dir).ok();
}

/// IME（fcitx5 / mozc など）で確定した日本語が検索欄に入る。
#[test]
fn ime_commit_reaches_search_fields() {
    let dir = sample_dir();
    std::fs::write(dir.join("jp.md"), "# 会議\n\nメモを取る。\n").unwrap();
    let mut h = harness(&dir.join("jp.md"));
    h.key_press_modifiers(Modifiers::COMMAND, Key::F);
    settle(&mut h);
    // 変換中の文字 → 確定
    h.event(egui::Event::Ime(egui::ImeEvent::Preedit {
        text: "めも".into(),
        active_range_chars: None,
    }));
    h.step();
    h.event(egui::Event::Ime(egui::ImeEvent::Commit("めも".into())));
    settle(&mut h);
    assert_eq!(h.state().find.query, "めも");
    assert_eq!(h.state().tab().view.match_count, 1);
    std::fs::remove_dir_all(dir).ok();
}
