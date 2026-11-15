//! Messreihe für Gesichtskacheln (ADR 0068): Zeit je Kachel ohne Zwischenspeicher
//! (`previews::preview`), mit kaltem und mit warmem Kachel-Zwischenspeicher
//! (`previews::cached_preview`) sowie kalt auf mehreren Threads. Ausschließlich synthetische
//! Bilder.
//!
//! ```text
//! cargo run --release --example face_thumb_bench -- <Ordner> [Fotos] [Breite] [Höhe] [Threads]
//! ```
use foto_importer::core::{
    jobs::Control,
    previews::{self, PreviewRequest},
};
use std::{path::Path, time::Instant};

fn request(root: &Path, i: usize) -> PreviewRequest {
    PreviewRequest {
        root: Some(root.to_path_buf()),
        path: root.join(format!("p{i}.jpg")),
        orientation: 1,
        crop: Some([0.4, 0.3, 0.5, 0.45]),
        edge: 160,
        generation: 0,
    }
}

fn series(label: &str, count: usize, each: impl Fn(usize) -> anyhow::Result<()>) -> f64 {
    let mut times = Vec::new();
    let started = Instant::now();
    for i in 0..count {
        let one = Instant::now();
        each(i).expect("Kachel");
        times.push(one.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(f64::total_cmp);
    let total = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "  {label:<34} Median {:>7.1} ms, gesamt {:>8.0} ms",
        times[times.len() / 2],
        total
    );
    total
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = Path::new(args.first().map_or("tmp/bench-thumbs", String::as_str)).to_path_buf();
    let count: usize = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(24);
    let width: u32 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(6000);
    let height: u32 = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(4000);
    let threads: usize = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(4);
    std::fs::create_dir_all(&root)?;
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    for i in 0..count {
        let path = root.join(format!("p{i}.jpg"));
        if path.exists() {
            continue;
        }
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let noise = (state & 15) as u32;
            image::Rgb([
                ((x * 255 / width + noise) % 256) as u8,
                ((y * 255 / height + noise) % 256) as u8,
                (((x + y) * 255 / (width + height) + i as u32 * 7) % 256) as u8,
            ])
        });
        image.save(&path)?;
    }
    let control = Control::default();
    println!("{count} Kacheln aus {width}x{height}-Fotos, {threads} Threads:");
    let _ = std::fs::remove_dir_all(root.join(previews::TILE_DIR));
    series("ohne Zwischenspeicher (seriell)", count, |i| {
        previews::preview(request(&root, i), "", &control).map(|_| ())
    });
    series("Zwischenspeicher kalt (seriell)", count, |i| {
        previews::cached_preview(request(&root, i), "", &control).map(|_| ())
    });
    series("Zwischenspeicher warm (seriell)", count, |i| {
        previews::cached_preview(request(&root, i), "", &control).map(|_| ())
    });
    let _ = std::fs::remove_dir_all(root.join(previews::TILE_DIR));
    let started = Instant::now();
    std::thread::scope(|scope| {
        for t in 0..threads {
            let (root, control) = (&root, &control);
            scope.spawn(move || {
                for i in (t..count).step_by(threads) {
                    previews::cached_preview(request(root, i), "", control).expect("Kachel");
                }
            });
        }
    });
    println!(
        "  {:<34} gesamt {:>8.0} ms",
        format!("kalt auf {threads} Threads"),
        started.elapsed().as_secs_f64() * 1000.0
    );
    let _ = std::fs::remove_dir_all(root.join(previews::TILE_DIR));
    Ok(())
}
