//! Messung des lokalen Vorschau-Caches (ADR Sammlungsansicht): Galerievorschau (400) ohne und
//! mit Treffer. Liest die Originale nur; geschrieben wird allein in den angegebenen
//! Cache-Ordner.
//!
//! `cargo run --release --example thumb_cache_bench -- <Ordner mit Fotos> <Cache-Ordner> [Anzahl]`

use foto_importer::core::{
    jobs::Control,
    previews::{PreviewRequest, cached_preview},
    thumb_cache,
};
use std::{path::PathBuf, time::Instant};

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let folder = PathBuf::from(args.next().expect("Ordner mit Fotos"));
    let cache = PathBuf::from(args.next().expect("Cache-Ordner"));
    let count: usize = args.next().map_or(Ok(12), |n| n.parse())?;
    if cache.starts_with(&folder) {
        anyhow::bail!("Der Cache-Ordner darf nicht im Fotoordner liegen");
    }
    thumb_cache::configure(Some(cache));
    let mut photos: Vec<PathBuf> = std::fs::read_dir(&folder)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
        })
        .collect();
    photos.sort();
    photos.truncate(count);
    for pass in ["ohne Treffer", "mit Treffer"] {
        let mut times = Vec::new();
        for path in &photos {
            let request = PreviewRequest {
                root: Some(folder.clone()),
                path: path.clone(),
                orientation: 1,
                crop: None,
                edge: 400,
                generation: 0,
            };
            let started = Instant::now();
            cached_preview(request, "exiftool", &Control::default())?;
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        let sum: f64 = times.iter().sum();
        println!(
            "{pass}: {} Fotos, Mittel {:.1} ms, Median {:.1} ms, Maximum {:.1} ms",
            times.len(),
            sum / times.len().max(1) as f64,
            times.get(times.len() / 2).copied().unwrap_or_default(),
            times.last().copied().unwrap_or_default()
        );
    }
    Ok(())
}
