# mdview の構成（機能 → ファイル対応表）

リファクタリングや機能追加のときに「どこを見ればよいか」をまとめた表です。
各ファイルの先頭（`//!`）にもそのファイルが担当する機能の一覧があり、
ファイル内は `// ==== 機能: ○○ ==== ` という見出しコメントで区切ってあります。
エディタで `機能:` を検索すると、機能ごとの入口に飛べます。

## 全体の流れ

```
main.rs ── 起動（フォント・構文定義の先読み開始 → wgpu、失敗したら OpenGL。Linux は X11 優先）
   └─ app::App（状態）
        ├─ 毎フレーム: poll() → handle_input() → draw()
        │     poll        : バックグラウンド処理の受け取り・自動再読み込み     app/mod.rs
        │     handle_input: キー操作・ドロップ                                 app/input.rs
        │     draw        : 画面の描画                                         ui/mod.rs
        └─ 終了時: save() → 設定・タブ・スクロール位置を保存                   app/mod.rs, settings.rs
```

Markdown の表示は 2 段階です。

1. **解析** `markdown::parse()` — pulldown-cmark のイベント列を、描画しやすい文書モデル（`markdown/ast.rs` の `Document`）に変換する。ファイルを開いたときに 1 回だけ。
2. **描画** `render::show()` — 文書モデルを egui で描く。毎フレーム。画面外のブロックは高さだけ確保して省略する。

## 機能 → ファイル

### 表示

| 機能 | ファイル | 主な関数・型 |
|---|---|---|
| Markdown の解析（GFM・脚注・フロントマター・数式） | `markdown/parse.rs` | `parse`, `Builder` |
| 文書モデル | `markdown/ast.rs` | `Document`, `Block`, `Run`, `Style` |
| 見出し（＋見出しリンクのコピー 🔗） | `render/blocks.rs` | `Renderer::heading` |
| 段落・行内の文字装飾 | `render/blocks.rs`, `render/text.rs` | `paragraph`, `inline_flow`, `format`, `build_job` |
| リンクのホバー・クリック（Ctrl+クリックで新しいタブ） | `render/text.rs` | `show_job`, `char_at` |
| 画像（クリックで拡大） | `render/blocks.rs`, `ui/dialogs.rs` | `Renderer::image`, `App::lightbox` |
| 引用・注記（`> [!NOTE]`） | `render/blocks.rs` | `Renderer::quote` |
| リスト・タスクリスト | `render/blocks.rs` | `Renderer::list`, `list_marker` |
| 表 | `render/table.rs` | `Renderer::table`, `fit_columns` |
| コードブロック（構文ハイライト・コピー） | `render/code.rs`, `highlight.rs` | `code_block`, `highlight_async`（バックグラウンドで色付け） |
| 重ねて置くボタン（コピー・🔗。レイアウトを崩さない） | `render/mod.rs` | `Renderer::overlay_button` |
| 数式（行内・ブロック） | `math.rs`, `render/code.rs`, `render/text.rs` | `math::to_unicode`, `math::show_display`, `Renderer::math` |
| Mermaid 図 | `mermaid/`, `render/code.rs` | `mermaid::show`, `Renderer::mermaid` |
| `<details>` 折りたたみ | `markdown/parse.rs`, `render/blocks.rs` | `Builder::html_block`, `Renderer::details` |
| HTML ブロック（画像・文字のみ） | `markdown/parse.rs`, `render/blocks.rs` | `parse_html_block`, `Renderer::html` |
| フロントマター・脚注 | `render/blocks.rs` | `front_matter`, `footnotes` |
| 画面外の描画省略（長い文書を軽く） | `render/mod.rs` | `show`, `ViewState::heights` |
| フォント（日本語・太字・等幅）の選択と読み込み | `fonts.rs` | `Scanner::choose`, `build`, `preload`, `FontChoice`（キャッシュ） |
| フォントのバックグラウンド処理（設定画面の一覧・読み直し） | `app/fonts.rs` | `install_fonts`, `apply_fonts`, `request_font_families` |
| 配色（ライト / ダーク） | `theme.rs` | `Palette` |

### ナビゲーション

| 機能 | ファイル | 主な関数・型 |
|---|---|---|
| ファイルを開く・前回位置から再開 | `app/navigation.rs` | `App::open`, `open_with` |
| リンクをたどる（相対パス・アンカー・Wiki・外部 URL） | `app/navigation.rs` | `follow_link`, `resolve_relative`, `resolve_wiki` |
| 戻る / 進む | `app/navigation.rs` | `go`, `record_history` |
| スクロール位置の予約（見出し・検索一致へ） | `app/navigation.rs` | `ScrollTarget`, `take_scroll` |
| タブ | `app/tabs.rs`, `ui/document.rs` | `Tab`, `new_tab`, `close_tab`, `tab_bar` |
| ファイルごとのスクロール位置の記憶 | `settings.rs`, `app/tabs.rs` | `remember_position`, `position_of` |
| ファイルツリー・絞り込み・すべて閉じる | `ui/sidebar.rs`, `workspace.rs` | `files_tab`, `tree_nodes`, `collapse_all`, `folder_id`, `scan` |
| フォルダ移動モード（1 フォルダずつ表示） | `ui/sidebar.rs`, `settings.rs` | `drill_nodes`, `find_children`, `TreeStyle`, `App::browse_dir` |
| 目次・バックリンク | `ui/outline.rs`, `index.rs` | `outline`, `backlinks`, `NoteIndex` |
| タグ | `ui/sidebar.rs`, `index.rs` | `tags_tab`, `front_tags`, `body_tags` |
| リンク先プレビュー | `ui/preview.rs` | `preview_popup`, `section_blocks` |
| スライド表示 | `ui/slides.rs` | `start_slides`, `split_slides`, `draw_slides` |

### 検索

| 機能 | ファイル | 主な関数・型 |
|---|---|---|
| 日本語に強い照合（かな・全半角・大小・半角カナの濁点、ローマ字→かな） | `textsearch.rs` | `Needle`, `fold_char`, `romaji_to_hiragana` |
| ファイル名のあいまい検索（Ctrl+P。ローマ字でも） | `ui/dialogs.rs`, `ui/mod.rs`, `workspace.rs` | `quick_open`, `rank_files`, `fuzzy_match` |
| ページ内検索（Ctrl+F） | `ui/document.rs`, `render/text.rs`, `app/navigation.rs` | `find_bar`, `FindState::needle`, `highlight_matches`, `find_step` |
| 全文検索（Ctrl+Shift+F。Shift_JIS も対象） | `ui/sidebar.rs`, `workspace.rs` | `search_tab`, `Searcher`, `search_files`, `highlight_snippet` |

### ファイル・外部連携

| 機能 | ファイル | 主な関数・型 |
|---|---|---|
| 読み込み（UTF-8 / Shift_JIS） | `app/document.rs` | `load_doc`, `decode` |
| 自動再読み込み | `app/mod.rs` | `check_reload` |
| フォルダの変更監視（ツリー・索引の自動更新） | `workspace.rs`, `app/mod.rs` | `FolderWatcher`, `notifications_unreliable`, `poll` |
| 索引の差分更新（変わったファイルだけ読み直す） | `index.rs` | `NoteIndex::build(.., prev)`, `Analyzed` |
| 外部エディタで開く・フォルダで表示・パスのコピー | `app/external.rs` | `open_in_editor`, `reveal_in_file_manager` |
| フォルダ選択ダイアログ（使えない環境ではアプリ内ウィンドウ） | `app/external.rs`, `ui/sidebar.rs`, `ui/dialogs.rs` | `pick_folder`, `open_folder_form`, `open_dialog_window` |
| HTML 書き出し・印刷 | `app/export.rs` | `to_html`, `export_html`, `print` |
| ドラッグ＆ドロップ（Linux は X11 で起動して対応） | `app/input.rs`, `main.rs`, `ui/mod.rs` | `handle_input`, `prefer_x11`, `drop_overlay` |

### 画面・設定

| 機能 | ファイル | 主な関数・型 |
|---|---|---|
| 画面全体のレイアウト | `ui/mod.rs` | `App::draw` |
| ツールバー・⋯ メニュー・ステータスバー | `ui/toolbar.rs` | `toolbar`, `more_menu`, `status_bar` |
| ようこそ画面 | `ui/document.rs` | `welcome` |
| 設定ウィンドウ | `ui/dialogs.rs` | `settings_window` |
| 設定の保存・復元 | `settings.rs`, `app/mod.rs` | `Settings`, `start`, `restore_tabs` |
| UI の言語（英語 / 日本語） | `i18n.rs` | `Language`, `Strings`, `EN`, `JA` |
| キーボードショートカット | `app/input.rs` | `Action`, `handle_input`, `run` |

### 共通の仕組み

| 機能 | ファイル | 主な関数・型 |
|---|---|---|
| バックグラウンド処理（スレッドで実行 → 終わったら再描画） | `bg.rs` | `BgTask::spawn`, `BgTask::poll` |
| 簡易プロファイラ（`MDVIEW_PROFILE=1`） | `app/profile.rs` | `record`, `phase`, `slow` |

## 動作を軽く保つための決まり

- **重い処理は UI スレッドでしない。** ファイルの読み込み・フォルダのスキャン・フォント・構文ハイライト・全文検索は、すべてバックグラウンド（`BgTask` など）で行い、終わったら再描画する。
- **画面外は描かない。** `render::show` はトップレベルのブロックの高さを覚え、画面から離れたブロックは空白だけ確保する。
- **レイアウトを動かす置き方をしない。** 本文の上に重ねるボタンは `ui.put` / `ui.add` ではなく `overlay_button`（当たり判定と描画だけ）を使う。`ui.put` はカーソルを動かすので、後続のブロックがずれたり重なったりする。
- **計測してから直す。** `MDVIEW_PROFILE=1 cargo run --release -- <フォルダ>` で、起動の内訳と 1 フレームの処理時間、20 ms を超えたブロックが表示される。

## テスト

| 種類 | 場所 |
|---|---|
| 解析・索引・検索・書き出しなどの単体テスト | 各ファイル末尾の `mod tests` |
| UI 操作のテスト（GPU なしで実際のアプリを操作。レイアウト崩れ・ドロップの再発防止を含む） | `ui_tests.rs` |

```bash
cargo test
cargo clippy --all-targets
```

## よくある変更の手順

- **画面の文言を変える・追加する**: `i18n.rs` の `Strings` にフィールドを足し、`EN` と `JA` の両方に書く（片方を忘れるとコンパイルエラーになる）。
- **ショートカットを追加する**: `app/input.rs` の `Action` に項目を足し、`handle_input` でキーを割り当て、`run` で処理する。ようこそ画面の一覧（`i18n.rs` の `shortcut_list`）と README も更新する。
- **新しいブロック要素を表示する**: `markdown/ast.rs` の `Block` に追加 → `markdown/parse.rs` で作る → `render/mod.rs` の `block()` で振り分け → `render/blocks.rs` などに描画関数を書く。
- **設定項目を追加する**: `settings.rs` の `Settings` にフィールドと既定値を足す（`#[serde(default)]` なので古い保存データも読める）→ `ui/dialogs.rs` の `settings_window` に UI を足す。
