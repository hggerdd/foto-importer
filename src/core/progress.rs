//! Honest progress for long jobs: named phases, smoothed rate, remaining time (ADR 0064).
//!
//! `Meter` is pure (time is passed in) so it is testable without sleeping. `Control` owns one
//! `PhaseState` that every `Control::progress` call feeds; the UI only reads `Progress`.
use std::time::{Duration, Instant};

/// Rate samples closer together than this are merged to keep the estimate calm.
const SAMPLE_GAP: Duration = Duration::from_millis(500);
/// Samples needed before a remaining time is shown; earlier it is "wird geschätzt".
const SAMPLES_FOR_ESTIMATE: usize = 4;
/// Weight of the newest sample in the exponential moving average.
const SMOOTHING: f64 = 0.3;

#[derive(Clone, Debug, Default)]
pub struct Meter {
    last_at: Option<Duration>,
    last_done: usize,
    rate: Option<f64>,
    samples: usize,
}
impl Meter {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    /// Record that `done` units were finished `at` after the phase started.
    pub fn record(&mut self, at: Duration, done: usize) {
        let Some(last_at) = self.last_at else {
            self.last_at = Some(at);
            self.last_done = done;
            return;
        };
        if done < self.last_done {
            // A restarted count (new portion) must not produce a negative rate.
            self.reset();
            self.last_at = Some(at);
            self.last_done = done;
            return;
        }
        let gap = at.saturating_sub(last_at);
        if gap < SAMPLE_GAP {
            return;
        }
        let instant = (done - self.last_done) as f64 / gap.as_secs_f64();
        self.rate = Some(match self.rate {
            Some(rate) => SMOOTHING * instant + (1.0 - SMOOTHING) * rate,
            None => instant,
        });
        self.samples += 1;
        self.last_at = Some(at);
        self.last_done = done;
    }
    /// Smoothed units per second, available from the second sample on.
    pub fn rate(&self) -> Option<f64> {
        self.rate.filter(|_| self.samples >= 2)
    }
    /// Remaining time, only with enough samples, a positive rate and a known total.
    pub fn eta(&self, done: usize, total: usize) -> Option<Duration> {
        let rate = self
            .rate()
            .filter(|r| *r > 0.0 && self.samples >= SAMPLES_FOR_ESTIMATE)?;
        (total > done).then(|| Duration::from_secs_f64((total - done) as f64 / rate))
    }
}

pub struct PhaseState {
    pub name: String,
    pub unit: &'static str,
    started: Instant,
    meter: Meter,
    history: Vec<(String, Duration)>,
}
impl Default for PhaseState {
    fn default() -> Self {
        Self {
            name: String::new(),
            unit: "",
            started: Instant::now(),
            meter: Meter::default(),
            history: Vec::new(),
        }
    }
}
impl PhaseState {
    pub fn begin(&mut self, name: &str, unit: &'static str) {
        if !self.name.is_empty() {
            self.history
                .push((self.name.clone(), self.started.elapsed()));
        }
        self.name = name.into();
        self.unit = unit;
        self.started = Instant::now();
        self.meter.reset();
    }
    pub fn record(&mut self, done: usize) {
        let at = self.started.elapsed();
        self.meter.record(at, done);
    }
    pub fn snapshot(&self, done: usize, total: usize) -> Progress {
        Progress {
            phase: self.name.clone(),
            unit: self.unit,
            done,
            total,
            rate: self.meter.rate(),
            eta: self.meter.eta(done, total),
            elapsed: self.started.elapsed(),
        }
    }
    /// Finished phases plus the running one, in order.
    pub fn timings(&self) -> Vec<(String, Duration)> {
        let mut all = self.history.clone();
        if !self.name.is_empty() {
            all.push((self.name.clone(), self.started.elapsed()));
        }
        all
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub phase: String,
    pub unit: &'static str,
    pub done: usize,
    /// 0 means the total is unknown.
    pub total: usize,
    pub rate: Option<f64>,
    pub eta: Option<Duration>,
    pub elapsed: Duration,
}
impl Progress {
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).min(1.0))
    }
    /// Whole percent, rounded down: 100 is only shown when everything is done.
    pub fn percent(&self) -> Option<usize> {
        (self.total > 0).then(|| (self.done.min(self.total) * 100) / self.total)
    }
    /// Counter text such as "1.200 von 34.195 Dateien · 3 %".
    pub fn counter(&self) -> String {
        let unit = if self.unit.is_empty() {
            String::new()
        } else {
            format!(" {}", self.unit)
        };
        match self.percent() {
            Some(percent) => format!(
                "{} von {}{unit} · {percent} %",
                group(self.done),
                group(self.total)
            ),
            None if self.done > 0 => {
                format!("{}{unit} gefunden · Gesamtzahl unbekannt", group(self.done))
            }
            None => "wird vorbereitet …".into(),
        }
    }
    /// Speed and remaining time, empty while nothing was measured yet.
    pub fn timing(&self) -> String {
        let mut parts = Vec::new();
        if let Some(rate) = self.rate {
            let unit = if self.unit.is_empty() {
                "Einträge"
            } else {
                self.unit
            };
            parts.push(format!("{} {unit}/s", format_rate(rate)));
        }
        if self.total > 0 && self.done < self.total {
            parts.push(match self.eta {
                Some(eta) if eta.as_secs_f64().ceil() < 45.0 => "noch unter 1 Min.".into(),
                Some(eta) => format!("noch etwa {}", format_duration(eta)),
                None => "Restzeit wird geschätzt …".into(),
            });
        }
        parts.join(" · ")
    }
    pub fn describe(&self) -> String {
        let timing = self.timing();
        let mut text = self.counter();
        if !timing.is_empty() {
            text.push_str(" · ");
            text.push_str(&timing);
        }
        if self.phase.is_empty() {
            text
        } else {
            format!("{}: {text}", self.phase)
        }
    }
}

/// German thousands grouping: 34195 becomes "34.195".
pub fn group(number: usize) -> String {
    let digits = number.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push('.');
        }
        text.push(digit);
    }
    text
}
pub fn format_rate(rate: f64) -> String {
    if rate >= 10.0 {
        group(rate.round() as usize)
    } else {
        format!("{rate:.1}").replace('.', ",")
    }
}
/// Minutes are rounded up so the estimate is never shorter than the shown value.
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs_f64().ceil() as u64;
    if seconds < 45 {
        "unter 1 Min.".into()
    } else {
        let minutes = seconds.div_ceil(60);
        if minutes < 60 {
            format!("{minutes} Min.")
        } else {
            format!("{} Std. {:02} Min.", minutes / 60, minutes % 60)
        }
    }
}
