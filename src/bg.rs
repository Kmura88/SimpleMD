//! # 機能: バックグラウンド処理の共通部品
//! 重い処理（フォルダのスキャン・フォントの読み込みなど）を別スレッドで実行し、
//! 終わったら UI に再描画を頼む。UI 側は毎フレーム [`BgTask::poll`] で結果を受け取る。
//!
//! ```ignore
//! let task = BgTask::spawn(&ctx, || heavy_work());
//! // 毎フレーム:
//! if let Some(result) = task.poll() { ... }
//! ```

use std::sync::mpsc;

/// 実行中のバックグラウンド処理。結果は 1 回だけ受け取れる。
pub struct BgTask<T> {
    rx: mpsc::Receiver<T>,
}

impl<T: Send + 'static> BgTask<T> {
    /// `work` を別スレッドで実行する。終わったら `ctx` に再描画を頼む。
    pub fn spawn(ctx: &egui::Context, work: impl FnOnce() -> T + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(work());
            ctx.request_repaint();
        });
        Self { rx }
    }

    /// 終わっていれば結果を返す（まだなら `None`）。
    pub fn poll(&self) -> Option<T> {
        self.rx.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivers_result_once() {
        let ctx = egui::Context::default();
        let task = BgTask::spawn(&ctx, || 40 + 2);
        let mut got = None;
        for _ in 0..200 {
            if let Some(v) = task.poll() {
                got = Some(v);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(got, Some(42));
        assert_eq!(task.poll(), None);
    }
}
