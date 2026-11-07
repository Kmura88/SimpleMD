//! # 機能: キーボードショートカット・マウスのサイドボタン・ドラッグ＆ドロップ
//! キー入力を `Action` に変換してから実行する（どのキーで何が起きるかをここで一覧できる）。
//! スライド表示中・画像の拡大表示中は、それぞれ専用のキー操作を優先する。

use egui::{Key, Modifiers, PointerButton};

use super::{App, LeftTab, ScrollTarget};

/// ユーザー操作。
enum Action {
    QuickOpen,
    Find,
    FindNext,
    FindPrev,
    SearchFiles,
    ToggleSidebar,
    ToggleOutline,
    FontBigger,
    FontSmaller,
    FontReset,
    Reload,
    Settings,
    Back,
    Forward,
    Escape,
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,
    OpenEditor,
    ExportHtml,
    Present,
    OpenFolder,
    ScrollTo(f32),
    ScrollBy(f32),
}

impl App {
    /// 毎フレーム、入力を見て操作を実行する。
    pub fn handle_input(&mut self, ctx: &egui::Context) {
        // ---- ドラッグ＆ドロップ: フォルダなら開き直し、ファイルなら開く ----
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(p) = dropped {
            if p.is_dir() {
                self.set_root(p, true);
            } else {
                self.open(ctx, &p, None, true);
            }
        }

        // ---- スライド表示中はスライド用のキーだけ ----
        if self.slides.is_some() {
            self.slide_input(ctx);
            return;
        }
        // ---- 画像の拡大表示中は Esc で閉じるだけ ----
        if self.lightbox.is_some() {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                self.lightbox = None;
            }
            return;
        }

        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let typing = ctx.text_edit_focused();
        let page = self.viewport_h * 0.9;
        let mut actions: Vec<Action> = Vec::new();
        ctx.input_mut(|i| {
            let mut on = |pressed: bool, a: Action| {
                if pressed {
                    actions.push(a);
                }
            };
            // 修飾キーが多い組み合わせから先に判定する（Ctrl+Shift+F を Ctrl+F と取り違えないように）。
            on(i.consume_key(cmd_shift, Key::F), Action::SearchFiles);
            on(i.consume_key(cmd_shift, Key::O), Action::ToggleOutline);
            on(i.consume_key(cmd_shift, Key::E), Action::OpenEditor);
            on(i.consume_key(cmd_shift, Key::N), Action::OpenFolder);
            on(i.consume_key(cmd_shift, Key::Tab), Action::PrevTab);
            on(i.consume_key(cmd, Key::Tab), Action::NextTab);
            on(i.consume_key(cmd, Key::PageDown), Action::NextTab);
            on(i.consume_key(cmd, Key::PageUp), Action::PrevTab);
            on(i.consume_key(cmd, Key::T), Action::NewTab);
            on(i.consume_key(cmd, Key::W), Action::CloseTab);
            on(i.consume_key(cmd, Key::E), Action::ExportHtml);
            on(i.consume_key(cmd, Key::P), Action::QuickOpen);
            on(i.consume_key(cmd, Key::O), Action::QuickOpen);
            on(i.consume_key(cmd, Key::F), Action::Find);
            on(i.consume_key(Modifiers::SHIFT, Key::F3), Action::FindPrev);
            on(i.consume_key(Modifiers::NONE, Key::F3), Action::FindNext);
            on(i.consume_key(cmd, Key::G), Action::FindNext);
            on(i.consume_key(cmd, Key::B), Action::ToggleSidebar);
            on(i.consume_key(cmd, Key::Plus), Action::FontBigger);
            on(i.consume_key(cmd, Key::Equals), Action::FontBigger);
            on(i.consume_key(cmd, Key::Minus), Action::FontSmaller);
            on(i.consume_key(cmd, Key::Num0), Action::FontReset);
            on(i.consume_key(cmd, Key::R), Action::Reload);
            on(i.consume_key(Modifiers::NONE, Key::F5), Action::Reload);
            on(i.consume_key(Modifiers::NONE, Key::F11), Action::Present);
            on(i.consume_key(cmd, Key::Comma), Action::Settings);
            on(i.consume_key(Modifiers::ALT, Key::ArrowLeft), Action::Back);
            on(
                i.consume_key(Modifiers::ALT, Key::ArrowRight),
                Action::Forward,
            );
            on(
                i.pointer.button_pressed(PointerButton::Extra1),
                Action::Back,
            );
            on(
                i.pointer.button_pressed(PointerButton::Extra2),
                Action::Forward,
            );
            on(i.consume_key(Modifiers::NONE, Key::Escape), Action::Escape);
            // 文字入力中でなければ、矢印キーなどで本文をスクロールする。
            if !typing {
                on(
                    i.consume_key(Modifiers::NONE, Key::Home),
                    Action::ScrollTo(0.0),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::End),
                    Action::ScrollTo(1.0e7),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::PageDown),
                    Action::ScrollBy(page),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::Space),
                    Action::ScrollBy(page),
                );
                on(
                    i.consume_key(Modifiers::SHIFT, Key::Space),
                    Action::ScrollBy(-page),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::PageUp),
                    Action::ScrollBy(-page),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    Action::ScrollBy(48.0),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    Action::ScrollBy(-48.0),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::J),
                    Action::ScrollBy(48.0),
                );
                on(
                    i.consume_key(Modifiers::NONE, Key::K),
                    Action::ScrollBy(-48.0),
                );
            }
        });
        for a in actions {
            self.run(ctx, a);
        }
    }

    /// 操作を実行する。
    fn run(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::QuickOpen => self.open_quick(),
            Action::Find => self.open_find(),
            Action::FindNext => {
                if self.find.open {
                    self.find_step(1)
                } else {
                    self.open_find()
                }
            }
            Action::FindPrev => self.find_step(-1),
            Action::SearchFiles => {
                self.settings.show_sidebar = true;
                self.left_tab = LeftTab::Search;
                self.search.focus = true;
            }
            Action::ToggleSidebar => self.settings.show_sidebar ^= true,
            Action::ToggleOutline => self.settings.show_outline ^= true,
            Action::FontBigger => self.set_font_size(self.settings.font_size + 1.0),
            Action::FontSmaller => self.set_font_size(self.settings.font_size - 1.0),
            Action::FontReset => self.set_font_size(16.0),
            Action::Reload => {
                self.check_reload(ctx, true);
                self.rescan();
            }
            Action::Settings => self.settings_open ^= true,
            Action::Back => self.go(ctx, -1),
            Action::Forward => self.go(ctx, 1),
            Action::Escape => {
                if self.quick.is_some() {
                    self.quick = None;
                } else if self.find.open {
                    self.close_find();
                } else if self.open_dialog {
                    self.open_dialog = false;
                } else if self.settings_open {
                    self.settings_open = false;
                }
            }
            Action::NewTab => self.new_tab(ctx),
            Action::CloseTab => self.close_tab(ctx, self.active),
            Action::NextTab => self.cycle_tab(ctx, 1),
            Action::PrevTab => self.cycle_tab(ctx, -1),
            Action::OpenEditor => self.open_current_in_editor(ctx),
            Action::ExportHtml => self.export_html(ctx),
            Action::Present => self.start_slides(ctx),
            Action::OpenFolder => self.pick_folder(),
            Action::ScrollTo(y) => self.scroll_to(ScrollTarget::Y(y)),
            Action::ScrollBy(dy) => {
                let y = self.tab().scroll_offset + dy;
                self.scroll_to(ScrollTarget::Y(y))
            }
        }
    }
}
