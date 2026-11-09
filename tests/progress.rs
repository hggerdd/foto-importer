use foto_importer::core::{
    jobs::Control,
    progress::{Meter, Progress, format_duration, format_rate, group},
};
use std::time::Duration;

fn steady(samples: u64, per_second: usize) -> Meter {
    let mut meter = Meter::default();
    for second in 0..=samples {
        meter.record(Duration::from_secs(second), second as usize * per_second);
    }
    meter
}
fn progress(done: usize, total: usize, meter: &Meter) -> Progress {
    Progress {
        phase: "Dateien eintragen".into(),
        unit: "Dateien",
        done,
        total,
        rate: meter.rate(),
        eta: meter.eta(done, total),
        elapsed: Duration::from_secs(10),
    }
}

#[test]
fn rate_and_remaining_time_need_enough_measurements() {
    let few = steady(1, 100);
    assert_eq!(few.rate(), None);
    assert_eq!(few.eta(100, 1000), None);
    let some = steady(3, 100);
    assert!(some.rate().is_some());
    assert_eq!(
        some.eta(300, 1000),
        None,
        "three samples are too few for a time"
    );
    let enough = steady(5, 100);
    let eta = enough.eta(500, 1000).expect("remaining time");
    assert!((eta.as_secs_f64() - 5.0).abs() < 0.01);
    assert_eq!(enough.eta(1000, 1000), None);
    assert_eq!(
        enough.eta(500, 0),
        None,
        "unknown total never yields a time"
    );
}

#[test]
fn rate_is_smoothed_and_restart_does_not_go_negative() {
    let mut meter = steady(6, 100);
    // One slow second (a batch commit) must not halve the estimate.
    meter.record(Duration::from_secs(7), 600);
    let rate = meter.rate().expect("rate");
    assert!(rate > 60.0 && rate < 100.0, "{rate}");
    // A new portion restarts the count at zero.
    meter.record(Duration::from_secs(8), 0);
    assert_eq!(meter.rate(), None);
    meter.record(Duration::from_secs(9), 50);
    meter.record(Duration::from_secs(10), 100);
    assert!((meter.rate().expect("rate") - 50.0).abs() < 0.01);
}

#[test]
fn samples_closer_than_half_a_second_are_merged() {
    let mut meter = Meter::default();
    meter.record(Duration::ZERO, 0);
    for step in 1..=100u64 {
        meter.record(Duration::from_millis(step * 4), step as usize * 4);
    }
    // 100 calls within 400 ms are not enough for a single sample.
    assert_eq!(meter.rate(), None);
}

#[test]
fn unknown_total_is_stated_instead_of_inventing_a_percentage() {
    let text = progress(34195, 0, &Meter::default()).describe();
    assert!(text.contains("34.195 Dateien gefunden"), "{text}");
    assert!(text.contains("Gesamtzahl unbekannt"), "{text}");
    assert!(!text.contains('%'), "{text}");
    assert_eq!(progress(34195, 0, &Meter::default()).fraction(), None);
}

#[test]
fn percentage_never_shows_100_before_everything_is_done() {
    let almost = progress(999, 1000, &Meter::default());
    assert_eq!(almost.percent(), Some(99));
    assert_eq!(progress(1000, 1000, &Meter::default()).percent(), Some(100));
    assert_eq!(
        progress(5000, 1000, &Meter::default()).fraction(),
        Some(1.0)
    );
}

#[test]
fn known_total_shows_counts_speed_and_a_time_or_the_estimate_note() {
    let early = progress(1200, 34195, &steady(1, 100)).describe();
    assert!(early.contains("1.200 von 34.195 Dateien · 3 %"), "{early}");
    assert!(early.contains("Restzeit wird geschätzt"), "{early}");
    let later = progress(1200, 34195, &steady(8, 120)).describe();
    assert!(later.contains("120 Dateien/s"), "{later}");
    assert!(later.contains("noch etwa 5 Min."), "{later}");
    assert!(!later.contains("geschätzt …"), "{later}");
    let near = progress(1000, 1500, &steady(8, 120)).describe();
    assert!(near.contains("noch unter 1 Min."), "{near}");
    let done = progress(10, 10, &steady(8, 120)).describe();
    assert!(
        !done.contains("noch etwa") && !done.contains("Restzeit"),
        "{done}"
    );
}

#[test]
fn numbers_and_durations_are_formatted_in_german() {
    assert_eq!(group(0), "0");
    assert_eq!(group(999), "999");
    assert_eq!(group(1000), "1.000");
    assert_eq!(group(34195), "34.195");
    assert_eq!(group(1234567), "1.234.567");
    assert_eq!(format_rate(3.26), "3,3");
    assert_eq!(format_rate(103.4), "103");
    assert_eq!(format_duration(Duration::from_secs(20)), "unter 1 Min.");
    assert_eq!(format_duration(Duration::from_secs(61)), "2 Min.");
    assert_eq!(format_duration(Duration::from_secs(3600)), "1 Std. 00 Min.");
    assert_eq!(format_duration(Duration::from_secs(5400)), "1 Std. 30 Min.");
}

#[test]
fn control_phases_restart_counts_and_record_their_duration() {
    let control = Control::default();
    control.phase("Ordner lesen", "Dateien");
    control.progress(42, 0);
    let first = control.snapshot();
    assert_eq!(
        (first.phase.as_str(), first.done, first.total),
        ("Ordner lesen", 42, 0)
    );
    control.phase("Dateien eintragen", "Dateien");
    let second = control.snapshot();
    assert_eq!((second.done, second.total), (0, 0));
    control.progress(5, 10);
    let names: Vec<_> = control.timings().into_iter().map(|t| t.0).collect();
    assert_eq!(names, ["Ordner lesen", "Dateien eintragen"]);
    assert_eq!(control.snapshot().percent(), Some(50));
}
