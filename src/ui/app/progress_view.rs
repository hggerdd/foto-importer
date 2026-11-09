//! Shared progress display for footer and task view (ADR 0064).
use super::*;
use crate::core::progress::Progress;

/// "6:12" or "1:02:03".
pub(super) fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

impl PhotoApp {
    /// Bar with counter, speed and remaining time. Unknown totals get an animated bar and no
    /// percentage; the text says so.
    pub(super) fn progress_bar(ui: &mut egui::Ui, progress: &Progress, width: f32) {
        let mut text = progress.counter();
        let timing = progress.timing();
        if !timing.is_empty() {
            text.push_str(" · ");
            text.push_str(&timing);
        }
        let mut bar = egui::ProgressBar::new(progress.fraction().unwrap_or(0.0))
            .desired_width(width)
            .text(text);
        if progress.fraction().is_none() {
            bar = bar.animate(true);
        }
        ui.add(bar);
    }

    /// While an index run enters batches, the gallery shows them (at most every few seconds,
    /// and only when new batches were committed). Readers never block the writer (ADR 0008).
    pub(super) fn live_gallery(&mut self, ctx: &egui::Context) {
        let Some(running) = self.tasks.iter().find(|t| {
            t.write
                && t.task.label == "Sammlung indizieren"
                && t.collection == self.settings.collection
        }) else {
            return;
        };
        let committed = running.task.control.committed.load(Ordering::Relaxed);
        if self.view == View::Collection
            && committed != self.live_refresh.0
            && self.live_refresh.1.elapsed() >= Duration::from_secs(4)
        {
            self.live_refresh = (committed, std::time::Instant::now());
            self.query_collection(self.applied.clone());
            ctx.request_repaint();
        }
    }

    /// What is still caught up in the background after the running job.
    pub(super) fn catch_up_hint(&self) -> Option<String> {
        let status = &self.status;
        (status.pending > 0 && status.files > 0).then(|| {
            format!(
                "Metadaten: {} von {} gelesen · {}",
                crate::core::progress::group(status.files.saturating_sub(status.pending)),
                crate::core::progress::group(status.files),
                if self.settings.auto_metadata {
                    "der Rest wird im Hintergrund nachgeholt"
                } else {
                    "den Rest mit „Metadaten lesen“ nachholen"
                }
            )
        })
    }
}
