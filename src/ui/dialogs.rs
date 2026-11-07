//! # 機能: ダイアログと重ね表示
//! - ファイル検索（Ctrl+P）: あいまい検索、↑↓ で選択、Enter で開く、Ctrl+Enter で新しいタブ
//! - 設定ウィンドウ
//! - 通知（トースト）
//! - 画像の拡大表示（ライトボックス）

use std::path::PathBuf;

use egui::{
    Align, Align2, Button, Color32, FontId, Frame, Id, Key, Layout, Margin, Rect, RichText,
    ScrollArea, Sense, TextEdit, TextWrapMode, vec2,
};

use super::{fuzzy_label, rank_files, rel_display};
use crate::app::App;
use crate::i18n::Language;
use crate::settings::{MAX_FONT, MIN_FONT};
use crate::theme::ThemeMode;

impl App {
    // ==== 機能: ファイル検索（Ctrl+P） ====

    pub(super) fn quick_open(&mut self, ctx: &egui::Context) {
        let Some(mut q) = self.quick.take() else {
            return;
        };
        let s = self.s;
        let files: Vec<PathBuf> = self
            .tree
            .as_ref()
            .map(|t| t.files.clone())
            .unwrap_or_default();
        // 検索語が変わったときだけ並べ直す。空欄なら最近開いたファイルを先頭に。
        if q.results_for.as_deref() != Some(q.query.as_str()) {
            q.results = if q.query.trim().is_empty() {
                let mut order: Vec<usize> = self
                    .settings
                    .recent_files
                    .iter()
                    .filter_map(|p| p.strip_prefix(&self.root).ok())
                    .filter_map(|rel| files.iter().position(|f| f == rel))
                    .collect();
                for i in 0..files.len() {
                    if order.len() >= 200 {
                        break;
                    }
                    if !order.contains(&i) {
                        order.push(i);
                    }
                }
                order.into_iter().map(|i| (i, Vec::new())).collect()
            } else {
                rank_files(&files, &q.query, 200)
            };
            q.results_for = Some(q.query.clone());
            q.selected = 0;
        }

        let mut chosen: Option<(usize, bool)> = None;
        let pal = self.palette.clone();
        let modal = egui::Modal::new(Id::unique("quick_open"))
            .area(
                egui::Modal::default_area(Id::unique("quick_open_area"))
                    .anchor(Align2::CENTER_TOP, vec2(0.0, 80.0)),
            )
            .show(ctx, |ui| {
                ui.set_width(580.0);
                let resp = ui.add(
                    TextEdit::singleline(&mut q.query)
                        .hint_text(s.quick_hint)
                        .font(FontId::proportional(16.0))
                        .desired_width(f32::INFINITY),
                );
                if std::mem::take(&mut q.focus) {
                    resp.request_focus();
                }
                let n = q.results.len();
                let mut moved = false;
                ui.input_mut(|i| {
                    if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) && n > 0 {
                        q.selected = (q.selected + 1) % n;
                        moved = true;
                    }
                    if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) && n > 0 {
                        q.selected = (q.selected + n - 1) % n;
                        moved = true;
                    }
                    if i.consume_key(egui::Modifiers::COMMAND, Key::Enter) && n > 0 {
                        chosen = Some((q.selected, true));
                    }
                    if i.consume_key(egui::Modifiers::NONE, Key::Enter) && n > 0 {
                        chosen = Some((q.selected, false));
                    }
                });
                ui.add_space(6.0);
                if n == 0 {
                    ui.label(RichText::new(s.no_matching_files).color(pal.muted));
                }
                ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                    ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                        ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                        for (row, (idx, positions)) in q.results.iter().enumerate() {
                            let job =
                                fuzzy_label(&rel_display(&files[*idx]), positions, &pal, 14.0);
                            let resp = ui.add(Button::selectable(row == q.selected, job));
                            if row == q.selected && moved {
                                resp.scroll_to_me(None);
                            }
                            if let Some(new_tab) = super::clicked_with_tab(ui, &resp) {
                                chosen = Some((row, new_tab));
                            }
                        }
                    });
                });
                ui.add_space(4.0);
                ui.label(RichText::new(s.quick_help).size(11.0).color(pal.muted));
            });
        let mut close = modal.should_close();
        if let Some((row, new_tab)) = chosen
            && let Some((idx, _)) = q.results.get(row)
        {
            let path = self.root.join(&files[*idx]);
            self.open_with(ctx, &path, None, new_tab);
            close = true;
        }
        if !close {
            self.quick = Some(q);
        }
    }

    // ==== 機能: フォルダを開く（アプリ内のウィンドウ） ====

    /// OS のフォルダ選択ダイアログが使えない環境で、代わりに出すウィンドウ。
    pub(super) fn open_dialog_window(&mut self, ctx: &egui::Context) {
        if !self.open_dialog {
            return;
        }
        let modal = egui::Modal::new(Id::unique("open_folder_dialog")).show(ctx, |ui| {
            ui.set_width(460.0);
            self.open_folder_form(ui)
        });
        if modal.inner || modal.should_close() {
            self.open_dialog = false;
        }
    }

    // ==== 機能: 設定ウィンドウ ====

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        // フォント一覧はバックグラウンドで作る（全フォントのスキャンは時間がかかる）。
        self.request_font_families(ctx);
        let s = self.s;
        let mut open = true;
        let mut fonts_changed = false;
        let mut theme_changed = false;
        let mut lang_changed = false;
        egui::Window::new(s.settings)
            .id(Id::unique("settings_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .anchor(Align2::RIGHT_TOP, vec2(-16.0, 48.0))
            .show(ctx, |ui| {
                egui::Grid::new("settings_grid")
                    .num_columns(2)
                    .spacing([16.0, 10.0])
                    .show(ui, |ui| {
                        ui.label(s.theme);
                        ui.horizontal(|ui| {
                            for mode in [ThemeMode::System, ThemeMode::Light, ThemeMode::Dark] {
                                let label = self.theme_label(mode);
                                theme_changed |= ui
                                    .selectable_value(&mut self.settings.theme, mode, label)
                                    .clicked();
                            }
                        });
                        ui.end_row();

                        ui.label(s.language);
                        ui.horizontal(|ui| {
                            for (lang, label) in [
                                (Language::Auto, s.lang_auto),
                                (Language::English, "English"),
                                (Language::Japanese, "日本語"),
                            ] {
                                lang_changed |= ui
                                    .selectable_value(&mut self.settings.language, lang, label)
                                    .clicked();
                            }
                        });
                        ui.end_row();

                        ui.label(s.text_size);
                        ui.add(
                            egui::Slider::new(&mut self.settings.font_size, MIN_FONT..=MAX_FONT)
                                .step_by(1.0)
                                .suffix(" px"),
                        );
                        ui.end_row();

                        ui.label(s.line_width);
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.settings.readable_width, s.limit);
                            ui.add_enabled(
                                self.settings.readable_width,
                                egui::Slider::new(&mut self.settings.content_width, 480.0..=1600.0)
                                    .step_by(20.0)
                                    .suffix(" px"),
                            );
                        });
                        ui.end_row();

                        ui.label(s.text_font);
                        fonts_changed |= font_combo(
                            ui,
                            "body_font",
                            &mut self.settings.body_font,
                            &self.fonts.families,
                            s.automatic,
                        );
                        ui.end_row();

                        ui.label(s.code_font);
                        fonts_changed |= font_combo(
                            ui,
                            "mono_font",
                            &mut self.settings.mono_font,
                            &self.fonts.families,
                            s.automatic,
                        );
                        ui.end_row();

                        ui.label(s.editor);
                        ui.add(
                            TextEdit::singleline(&mut self.settings.editor_command)
                                .hint_text(s.editor_hint)
                                .desired_width(260.0),
                        );
                        ui.end_row();

                        ui.label(s.auto_reload);
                        ui.checkbox(&mut self.settings.auto_reload, s.auto_reload_label);
                        ui.end_row();
                    });
                ui.separator();
                ui.label(
                    RichText::new(format!("mdview v{}", env!("CARGO_PKG_VERSION")))
                        .size(11.0)
                        .color(self.palette.muted),
                );
                let r = &self.fonts.report;
                let none = || "—".to_string();
                ui.label(
                    RichText::new(s.fonts_in_use(
                        &r.body.clone().unwrap_or_else(none),
                        &r.cjk.clone().unwrap_or_else(none),
                        &r.mono.clone().unwrap_or_else(none),
                        r.bold,
                    ))
                    .size(11.0)
                    .color(self.palette.muted),
                );
            });
        if theme_changed {
            self.apply_theme(ctx);
        }
        if fonts_changed {
            self.apply_fonts(ctx);
        }
        if lang_changed {
            self.apply_language();
        }
        self.settings_open = open;
    }

    // ==== 機能: 通知（トースト） ====

    pub(super) fn toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((msg, t)) = &self.toast else { return };
        let age = ctx.input(|i| i.time) - t;
        if age > 2.5 {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        // 最後の 0.4 秒でフェードアウト
        let alpha = ((2.5 - age) / 0.4).clamp(0.0, 1.0) as f32;
        egui::Area::new(Id::unique("toast"))
            .anchor(Align2::RIGHT_BOTTOM, vec2(-16.0, -32.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                Frame::popup(ui.style())
                    .inner_margin(Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.label(msg.as_str());
                    });
            });
    }

    // ==== 機能: 画像の拡大表示（ライトボックス） ====

    /// 画面全体を暗くして画像を最大限大きく表示する。クリックか Esc で閉じる。
    pub(super) fn lightbox(&mut self, ctx: &egui::Context) {
        let Some(uri) = self.lightbox.clone() else {
            return;
        };
        let s = self.s;
        let screen = ctx.content_rect();
        let mut close = false;
        let mut open_external = false;
        egui::Area::new(Id::unique("lightbox"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(225));
                let bg = ui.allocate_rect(screen, Sense::click());
                let img_rect = Rect::from_min_max(
                    screen.min + vec2(32.0, 32.0),
                    screen.max - vec2(32.0, 72.0),
                );
                ui.put(
                    img_rect,
                    egui::Image::new(uri.clone())
                        .max_size(img_rect.size())
                        .maintain_aspect_ratio(true)
                        .fit_to_exact_size(img_rect.size()),
                );
                let bar = Rect::from_min_max(
                    egui::pos2(screen.left(), screen.bottom() - 56.0),
                    screen.max,
                );
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(24.0);
                        let name = uri.rsplit('/').next().unwrap_or(&uri);
                        ui.label(
                            RichText::new(crate::util::percent_decode(name)).color(Color32::WHITE),
                        );
                        ui.add_space(12.0);
                        if ui.button(s.open_externally).clicked() {
                            open_external = true;
                        }
                        ui.add_space(12.0);
                        ui.label(
                            RichText::new(s.lightbox_help)
                                .size(11.5)
                                .color(Color32::GRAY),
                        );
                    });
                });
                if bg.clicked() {
                    close = true;
                }
            });
        if open_external {
            if let Some(path) = uri.strip_prefix("file://") {
                let _ = open::that_detached(path);
            } else {
                ctx.open_url(egui::OpenUrl::new_tab(&uri));
            }
        }
        if close {
            self.lightbox = None;
        }
    }
}

/// フォントの選択（「自動」＋ システムのフォント一覧）。変わったら true。
fn font_combo(
    ui: &mut egui::Ui,
    id: &str,
    value: &mut String,
    families: &[String],
    automatic: &str,
) -> bool {
    let mut changed = false;
    let label = if value.is_empty() {
        automatic.to_string()
    } else {
        value.clone()
    };
    egui::ComboBox::from_id_salt(id)
        .selected_text(label)
        .width(220.0)
        .height(320.0)
        .show_ui(ui, |ui| {
            if ui.selectable_label(value.is_empty(), automatic).clicked() {
                value.clear();
                changed = true;
            }
            if families.is_empty() {
                // 一覧はバックグラウンドで作成中
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("…");
                });
            }
            for f in families {
                if ui.selectable_label(value == f, f).clicked() {
                    *value = f.clone();
                    changed = true;
                }
            }
        });
    changed
}
