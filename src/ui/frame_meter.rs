//! Frame-Zeiten fürs Diagnose-Log: langsame Frames einzeln mit Ansicht und Anlass, dazu alle
//! `DIAG_FRAME_SUMMARY` eine Zusammenfassung, solange gezeichnet wird.

use crate::config;
use std::time::{Duration, Instant};

/// Was im gemessenen Frame passiert ist.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameNote {
    /// Ansicht und Reiter, z. B. `faces/unknown`.
    pub place: String,
    /// Dauer von `update` (nur der Aufbau der Oberfläche) in Millisekunden.
    pub update_ms: f32,
    /// Hochgeladene Vorschau-Texturen.
    pub textures: usize,
    /// Fertig gewordene Aufträge, deren Ergebnis übernommen wurde.
    pub jobs: usize,
}

#[derive(Debug, PartialEq)]
pub struct Summary {
    pub frames: usize,
    pub avg_ms: f32,
    pub p95_ms: f32,
    pub max_ms: f32,
    pub slow: usize,
    pub textures: usize,
    pub span: Duration,
}

pub struct FrameMeter {
    threshold_ms: f32,
    period: Duration,
    window_start: Option<Instant>,
    times: Vec<f32>,
    slow: usize,
    textures: usize,
    /// Angaben zum Frame, dessen Dauer der nächste Aufruf von [`FrameMeter::record`] liefert.
    pub current: FrameNote,
}
impl Default for FrameMeter {
    fn default() -> Self {
        Self::new(config::DIAG_SLOW_FRAME_MS, config::DIAG_FRAME_SUMMARY)
    }
}
impl FrameMeter {
    pub fn new(threshold_ms: f32, period: Duration) -> Self {
        Self {
            threshold_ms,
            period,
            window_start: None,
            times: Vec::new(),
            slow: 0,
            textures: 0,
            current: FrameNote::default(),
        }
    }
    /// Ist ein Frame dieser Dauer langsam?
    pub fn is_slow(&self, frame_ms: f32) -> bool {
        frame_ms >= self.threshold_ms
    }
    /// Verbucht den fertigen Frame (`frame_ms` aus eframe, `current` aus seinem `update`).
    /// Liefert die Angaben, falls er langsam war, und die Zusammenfassung des abgelaufenen
    /// Zeitfensters. Ein Fenster beginnt mit dem ersten Frame nach dem vorigen; Leerlauf ohne
    /// Frames erzeugt also keine Zeilen.
    pub fn record(&mut self, now: Instant, frame_ms: f32) -> (Option<FrameNote>, Option<Summary>) {
        let note = std::mem::take(&mut self.current);
        let summary = match self.window_start {
            Some(start) if now.saturating_duration_since(start) >= self.period => {
                let summary = self.summary(now.saturating_duration_since(start));
                self.times.clear();
                self.slow = 0;
                self.textures = 0;
                self.window_start = Some(now);
                summary
            }
            Some(_) => None,
            None => {
                self.window_start = Some(now);
                None
            }
        };
        self.times.push(frame_ms);
        self.textures += note.textures;
        let slow = self.is_slow(frame_ms);
        if slow {
            self.slow += 1;
        }
        (slow.then_some(note), summary)
    }
    fn summary(&self, span: Duration) -> Option<Summary> {
        if self.times.is_empty() {
            return None;
        }
        let mut sorted = self.times.clone();
        sorted.sort_by(f32::total_cmp);
        let index = ((sorted.len() as f32 * 0.95).ceil() as usize).clamp(1, sorted.len()) - 1;
        Some(Summary {
            frames: sorted.len(),
            avg_ms: sorted.iter().sum::<f32>() / sorted.len() as f32,
            p95_ms: sorted[index],
            max_ms: sorted[sorted.len() - 1],
            slow: self.slow,
            textures: self.textures,
            span,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_frames_are_reported_with_their_note() {
        let mut meter = FrameMeter::new(50.0, Duration::from_secs(10));
        let start = Instant::now();
        assert!(!meter.is_slow(49.9));
        assert!(meter.is_slow(50.0));
        meter.current.place = "faces/unknown".into();
        meter.current.textures = 4;
        let (slow, summary) = meter.record(start, 73.0);
        assert_eq!(
            slow.map(|n| (n.place, n.textures)),
            Some(("faces/unknown".into(), 4))
        );
        assert!(summary.is_none());
        // Der Eintrag gehört genau zu einem Frame.
        let (slow, _) = meter.record(start, 12.0);
        assert!(slow.is_none());
        assert_eq!(meter.current, FrameNote::default());
    }

    #[test]
    fn the_summary_covers_one_window() {
        let mut meter = FrameMeter::new(50.0, Duration::from_secs(10));
        let start = Instant::now();
        for (i, ms) in (1..=100).map(|ms| ms as f32).enumerate() {
            meter.current.textures = 1;
            let (_, summary) = meter.record(start + Duration::from_millis(i as u64 * 50), ms);
            assert!(summary.is_none());
        }
        let (_, summary) = meter.record(start + Duration::from_secs(12), 5.0);
        let summary = summary.expect("Fenster abgelaufen");
        assert_eq!(summary.frames, 100);
        assert_eq!(summary.max_ms, 100.0);
        assert_eq!(summary.p95_ms, 95.0);
        assert_eq!(summary.avg_ms, 50.5);
        assert_eq!(summary.slow, 51);
        assert_eq!(summary.textures, 100);
        // Das neue Fenster beginnt mit diesem Frame; der nächste nach Ablauf schließt es.
        let (_, summary) = meter.record(start + Duration::from_secs(23), 5.0);
        assert_eq!(summary.map(|s| (s.frames, s.slow)), Some((1, 0)));
    }
}
