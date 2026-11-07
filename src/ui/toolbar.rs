//! # 機能: ツールバーとステータスバー

use std::time::SystemTime;

use egui::text::LayoutJob;
use egui::{Align, Button, FontId, Layout, RichText, TextFormat, TextWrapMode, Ui, vec2};

use super::{display_name, icon_button, icon_toggle, thousands};
use crate::app::App;
use crate::theme::ThemeMode;

impl App {
    // ==== 機能: ツールバー ====

    /// 左: サイドバー切替・戻る/進む・パンくず、右: ファイル検索・各種ボタン・⋯ メニュー。
    pub(super) fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.s;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icon_toggle(ui, "📁", self.settings.show_sidebar, s.tip_sidebar).clicked() {
                self.settings.show_sidebar ^= true;
            }
            ui.add_space(6.0);
            if icon_button(ui, "⬅", self.can_go(-1), s.tip_back).clicked() {
                self.go(&ctx, -1);
            }
            if icon_button(ui, "➡", self.can_go(1), s.tip_forward).clicked() {
                self.go(&ctx, 1);
            }
            ui.add_space(10.0);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.menu_button(RichText::new("⋯").size(16.0), |ui| self.more_menu(ui))
                    .response
                    .on_hover_text(s.tip_more);
                if icon_button(ui, "⚙", true, s.tip_settings).clicked() {
                    self.settings_open ^= true;
                }
                let theme = self.settings.theme;
                if icon_button(ui, theme.icon(), true, self.theme_label(theme)).clicked() {
                    self.settings.theme = theme.next();
                    self.apply_theme(&ctx);
                }
                if icon_toggle(ui, "📑", self.settings.show_outline, s.tip_outline).clicked() {
                    self.settings.show_outline ^= true;
                }
                if icon_toggle(ui, "🔍", self.find.open, s.tip_find).clicked() {
                    if self.find.open {
                        self.close_find();
                    } else {
                        self.open_find();
                    }
                }
                ui.add_space(6.0);
                let quick = Button::new(
                    RichText::new(format!("🔎  {}   Ctrl+P", s.go_to_file))
                        .color(self.palette.muted)
                        .size(12.5),
                )
                .fill(self.palette.bg)
                .stroke(egui::Stroke::new(1.0, self.palette.border))
                .min_size(vec2(200.0, 24.0));
                if ui.add(quick).clicked() {
                    self.open_quick();
                }
                ui.add_space(8.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    self.breadcrumb(ui);
                });
            });
        });
    }

    /// テーマ名（表示言語に合わせる）。
    pub(super) fn theme_label(&self, t: ThemeMode) -> &'static str {
        match t {
            ThemeMode::System => self.s.theme_system,
            ThemeMode::Light => self.s.theme_light,
            ThemeMode::Dark => self.s.theme_dark,
        }
    }

    /// ⋯ メニュー: 外部エディタ・フォルダで表示・パスのコピー・書き出し・印刷・スライド・フォルダを開く。
    fn more_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let s = self.s;
        let has_doc = self.doc().is_some();
        ui.set_min_width(260.0);
        let item = |ui: &mut Ui, label: &str, enabled: bool| {
            ui.add_enabled(enabled, Button::new(label).frame_when_inactive(false))
                .clicked()
        };
        if item(ui, &format!("✏  {}", s.menu_open_editor), has_doc) {
            self.open_current_in_editor(&ctx);
            ui.close();
        }
        if item(ui, &format!("📂  {}", s.menu_reveal), has_doc) {
            self.reveal_current(&ctx);
            ui.close();
        }
        if item(ui, &format!("📋  {}", s.menu_copy_path), has_doc) {
            self.copy_current_path(&ctx);
            ui.close();
        }
        ui.separator();
        if item(ui, &format!("🌐  {}", s.menu_export_html), has_doc) {
            ui.close();
            self.export_html(&ctx);
        }
        if item(ui, &format!("🖨  {}", s.menu_print), has_doc) {
            ui.close();
            self.print(&ctx);
        }
        if item(ui, &format!("▶  {}", s.menu_present), has_doc) {
            ui.close();
            self.start_slides(&ctx);
        }
        ui.separator();
        if item(ui, &format!("🗁  {}", s.menu_open_folder), true) {
            ui.close();
            self.pick_folder();
        }
    }

    /// パンくずリスト（フォルダ名 › サブフォルダ › ファイル名）。
    fn breadcrumb(&mut self, ui: &mut Ui) {
        ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
        let root_name = display_name(&self.root);
        let Some(doc) = self.doc() else {
            ui.label(RichText::new(root_name).strong());
            return;
        };
        let rel = doc.path.strip_prefix(&self.root).unwrap_or(&doc.path);
        let mut job = LayoutJob::default();
        let muted = TextFormat::simple(FontId::proportional(13.0), self.palette.muted);
        job.append(&root_name, 0.0, muted.clone());
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        for (i, part) in parts.iter().enumerate() {
            job.append("  ›  ", 0.0, muted.clone());
            let last = i + 1 == parts.len();
            let fmt = if last {
                TextFormat::simple(FontId::new(13.0, crate::fonts::bold()), self.palette.text)
            } else {
                muted.clone()
            };
            job.append(part, 0.0, fmt);
        }
        let full = doc.path.display().to_string();
        ui.add(egui::Label::new(job).truncate()).on_hover_text(full);
    }

    // ==== 機能: ステータスバー ====

    /// 左: 相対パス（Shift_JIS なら表示）、右: ファイル数・語数/文字数・更新時刻・文字サイズ。
    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
        let muted = self.palette.muted;
        let s = self.s;
        ui.horizontal(|ui| {
            let small = |t: String| RichText::new(t).size(11.5).color(muted);
            match self.doc() {
                Some(doc) => {
                    let rel = doc.path.strip_prefix(&self.root).unwrap_or(&doc.path);
                    ui.label(small(rel.display().to_string()));
                    if doc.sjis {
                        ui.label(small("· Shift_JIS".into()));
                    }
                }
                None => {
                    ui.label(small(self.root.display().to_string()));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(small(format!("{:.0}px", self.settings.font_size)));
                if let Some(doc) = self.doc() {
                    if let Some(m) = doc.modified {
                        let secs = SystemTime::now()
                            .duration_since(m)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        ui.label(small(format!("{} ·", s.modified_ago(secs))));
                    }
                    ui.label(small(format!(
                        "{} ·",
                        s.words_chars(&thousands(doc.words), &thousands(doc.chars))
                    )));
                }
                if let Some(tree) = &self.tree {
                    ui.label(small(format!(
                        "{} ·",
                        s.file_count(&thousands(tree.files.len()))
                    )));
                }
            });
        });
    }
}
