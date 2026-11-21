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
            self.requery_collection();
            ctx.request_repaint();
        }
    }

    /// What is still caught up in the background after the running job.
    pub(super) fn catch_up_hint(&self) -> Option<String> {
        let status = &self.status;
        (status.pending > 0 && status.files > 0).then(|| {
            let rest = if self.metadata_bg_running() {
                "der Rest wird im Hintergrund gelesen"
            } else if status.unread == 0 || self.settings.exiftool.trim().is_empty() {
                "den Rest mit „Metadaten lesen“ nachholen"
            } else if self.metadata_paused {
                "Hintergrundlauf pausiert"
            } else if self.metadata_bg_failed {
                "Hintergrundlauf gestoppt, den Rest mit „Metadaten lesen“ nachholen"
            } else {
                "der Rest wird im Hintergrund gelesen"
            };
            format!(
                "Metadaten: {} von {} gelesen · {rest}",
                crate::core::progress::group(status.files.saturating_sub(status.pending)),
                crate::core::progress::group(status.files),
            )
        })
    }

    fn metadata_bg_running(&self) -> bool {
        self.tasks
            .iter()
            .any(|t| t.kind == JobKind::MetadataBg && t.collection == self.settings.collection)
    }

    /// Hinweiszeile zum Nachholen; ein pausierter Hintergrundlauf bietet „Fortsetzen“ an.
    pub(super) fn catch_up_row(&mut self, ui: &mut egui::Ui) {
        let Some(hint) = self.catch_up_hint() else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(hint).small().weak());
            if self.metadata_paused
                && self.status.unread > 0
                && ui.small_button("Fortsetzen").clicked()
            {
                self.metadata_paused = false;
            }
        });
    }
}
