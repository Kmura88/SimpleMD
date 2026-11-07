//! # 機能: 簡易プロファイラ（開発用）
//! 環境変数 `MDVIEW_PROFILE=1` で起動すると、次を標準エラーに表示する。
//! - 起動処理の各区間の時間（フォント待ち・最初のフレームなど）
//! - 2 秒ごとの、1 フレームあたりの UI 処理時間（平均・最大）
//! - 20 ms を超えた区間（どのブロックの描画が遅いか）

use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

struct Stats {
    since: Instant,
    frames: u32,
    total: Duration,
    max: Duration,
}

static STATS: Mutex<Option<Stats>> = Mutex::new(None);

/// 計測が有効か（環境変数は起動時に 1 回だけ読む）。
static ENABLED: LazyLock<bool> = LazyLock::new(|| std::env::var_os("MDVIEW_PROFILE").is_some());

/// 1 フレーム分の処理時間を記録する（環境変数が無ければ何もしない）。
pub fn record(d: Duration) {
    if !*ENABLED {
        return;
    }
    let Ok(mut g) = STATS.lock() else { return };
    let s = g.get_or_insert(Stats {
        since: Instant::now(),
        frames: 0,
        total: Duration::ZERO,
        max: Duration::ZERO,
    });
    s.frames += 1;
    s.total += d;
    s.max = s.max.max(d);
    if s.since.elapsed() >= Duration::from_secs(2) {
        eprintln!(
            "[profile] {} frames, avg {:.2} ms, max {:.2} ms",
            s.frames,
            s.total.as_secs_f64() * 1000.0 / s.frames as f64,
            s.max.as_secs_f64() * 1000.0
        );
        *g = None;
    }
}

/// 起動処理などの区間の時間を表示する（環境変数が無ければ何もしない）。
pub fn phase(name: &str, since: Instant) {
    if *ENABLED {
        eprintln!(
            "[profile] {name}: {:.1} ms",
            since.elapsed().as_secs_f64() * 1000.0
        );
    }
}

/// 区間が 20 ms を超えたときだけ表示する（遅いフレームの原因探し用）。
pub fn slow(name: &str, since: Instant) {
    let ms = since.elapsed().as_secs_f64() * 1000.0;
    if ms > 20.0 && *ENABLED {
        eprintln!("[profile] slow {name}: {ms:.1} ms");
    }
}
