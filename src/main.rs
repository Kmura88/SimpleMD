//! # mdview — Markdown ノートビューア（起動処理）
//! - コマンドライン引数（開くフォルダ / ファイル）
//! - 機能: 描画方式のフォールバック（wgpu が使えなければ OpenGL で起動し直す）
//! - 機能: ウィンドウアイコン（プログラムで描画）
//!
//! 機能とファイルの対応は `docs/ARCHITECTURE.md` を参照。

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod bg;
mod fonts;
mod highlight;
mod i18n;
mod index;
mod markdown;
mod math;
// Mermaid モジュールは単体でも使える API（レイアウトだけ取り出す等）を公開しているため、
// mdview で使っていない部分の警告は出さない。
#[allow(dead_code, unused_imports)]
mod mermaid;
mod render;
mod settings;
mod textsearch;
mod theme;
mod ui;
mod util;
mod workspace;

#[cfg(test)]
mod ui_tests;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "mdview",
    version,
    about = "A fast, native Markdown notebook viewer"
)]
struct Args {
    /// Folder or Markdown file to open (default: the last session, or the current folder)
    path: Option<PathBuf>,
}

/// 起動直後に始めたフォント読み込み（wgpu → OpenGL と起動し直す場合もあるので共有する）。
type FontJob = Arc<Mutex<Option<JoinHandle<fonts::LoadedFonts>>>>;

fn main() -> eframe::Result {
    let args = Args::parse();
    // 機能: 起動を速く — フォントと構文定義の読み込みを、ウィンドウの準備と並行して始める。
    let fonts: FontJob = Arc::new(Mutex::new(Some(fonts::preload())));
    highlight::warm_up();

    // 機能: 描画方式のフォールバック — まず wgpu（Vulkan/DX12/Metal）で起動し、
    // 使えるアダプタが無ければ OpenGL で起動し直す。
    match run(eframe::Renderer::Wgpu, args.path.clone(), fonts.clone()) {
        Err(e @ eframe::Error::Wgpu(_)) => {
            eprintln!("[mdview] wgpu unavailable ({e}); falling back to OpenGL");
            run(eframe::Renderer::Glow, args.path, fonts)
        }
        other => other,
    }
}

fn run(renderer: eframe::Renderer, path: Option<PathBuf>, fonts: FontJob) -> eframe::Result {
    let mut options = eframe::NativeOptions {
        renderer,
        viewport: egui::ViewportBuilder::default()
            .with_title("mdview")
            .with_app_id("mdview")
            .with_inner_size([1180.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            .with_drag_and_drop(true)
            .with_icon(Arc::new(icon())),
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    if prefer_x11() {
        options.event_loop_builder = Some(Box::new(|builder| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            builder.with_x11();
        }));
    }
    eframe::run_native(
        "mdview",
        options,
        Box::new(move |cc| {
            let preloaded = fonts
                .lock()
                .ok()
                .and_then(|mut job| job.take())
                .and_then(|h| h.join().ok());
            Ok(Box::new(app::App::new(cc, path, preloaded)))
        }),
    )
}

// ==== 機能: ドラッグ＆ドロップ（Linux） ====

/// winit は Wayland でのファイルのドロップに対応していないため、X11（XWayland 含む）が
/// 使えるなら X11 で起動する。環境変数 `MDVIEW_BACKEND=wayland` で Wayland を強制できる。
#[cfg(target_os = "linux")]
fn prefer_x11() -> bool {
    if std::env::var("MDVIEW_BACKEND").is_ok_and(|v| v.eq_ignore_ascii_case("wayland")) {
        return false;
    }
    std::env::var_os("DISPLAY").is_some() && x11_keyboard_lib_available()
}

/// X11 で起動するのに必要な libxkbcommon-x11 があるか（無いと winit が起動時に落ちる）。
#[cfg(target_os = "linux")]
fn x11_keyboard_lib_available() -> bool {
    const LIB: &str = "libxkbcommon-x11.so.0";
    let mut dirs: Vec<PathBuf> = std::env::var("LD_LIBRARY_PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect();
    dirs.extend(
        [
            "/usr/lib/x86_64-linux-gnu",
            "/usr/lib/aarch64-linux-gnu",
            "/lib/x86_64-linux-gnu",
            "/lib/aarch64-linux-gnu",
            "/usr/lib64",
            "/usr/lib",
            "/lib64",
            "/lib",
        ]
        .map(PathBuf::from),
    );
    dirs.iter().any(|d| d.join(LIB).exists())
}

/// Window icon drawn procedurally: a blue rounded square with a white "M↓".
fn icon() -> egui::IconData {
    const S: usize = 64;
    let mut rgba = vec![0u8; S * S * 4];
    let inside_rounded = |x: f32, y: f32| {
        let r = 12.0;
        let (cx, cy) = (x.clamp(r, S as f32 - r), y.clamp(r, S as f32 - r));
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    };
    // Stroke segments of the glyph (in a 64x64 grid).
    let segs: [((f32, f32), (f32, f32)); 7] = [
        ((12.0, 46.0), (12.0, 18.0)),
        ((12.0, 18.0), (22.0, 32.0)),
        ((22.0, 32.0), (32.0, 18.0)),
        ((32.0, 18.0), (32.0, 46.0)),
        ((46.0, 18.0), (46.0, 44.0)),
        ((38.0, 36.0), (46.0, 46.0)),
        ((54.0, 36.0), (46.0, 46.0)),
    ];
    let dist = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
    };
    for y in 0..S {
        for x in 0..S {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let i = (y * S + x) * 4;
            if !inside_rounded(p.0, p.1) {
                continue;
            }
            let d = segs
                .iter()
                .map(|(a, b)| dist(p, *a, *b))
                .fold(f32::MAX, f32::min);
            let w = (3.6 - d).clamp(0.0, 1.0); // anti-aliased stroke
            let bg = [9.0, 105.0, 218.0];
            for c in 0..3 {
                rgba[i + c] = (bg[c] * (1.0 - w) + 255.0 * w) as u8;
            }
            rgba[i + 3] = 255;
        }
    }
    egui::IconData {
        rgba,
        width: S as u32,
        height: S as u32,
    }
}
