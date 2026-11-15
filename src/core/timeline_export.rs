//! Export der Zeitreise als neue Dateien (ADR 0069): PNG-Collage, PNG-Kontaktbogen, Zeitraffer
//! als animiertes GIF oder PNG-Bildfolge. Es werden nur Dateien in dem vom Nutzer gewählten
//! Ordner angelegt; bestehende Dateien und Originale werden nie ersetzt oder verändert.
//! Alles Langsame (Bilder lesen, Ausschnitte rechnen) gehört auf einen Worker.
use super::{
    jobs::Control,
    previews::{self, PreviewRequest},
    timeline::{self, TimeFace},
};
use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};
use std::{
    fs::OpenOptions,
    path::{Path, PathBuf},
};

const BACKGROUND: Rgba<u8> = Rgba([13, 18, 24, 255]);
const INK: Rgba<u8> = Rgba([230, 236, 242, 255]);

/// Ein Bild für den Export: Gesicht plus Beschriftung (nur Ziffern, `-`, `J`, `M`).
#[derive(Clone, Debug)]
pub struct Tile {
    pub face: TimeFace,
    pub label: String,
}

/// Legt eine neue Datei an und schreibt nie über eine bestehende: bei Namensgleichheit wird
/// `-2`, `-3` … angehängt. Gibt den Pfad und das geöffnete Handle zurück.
pub fn create_new(dir: &Path, stem: &str, ext: &str) -> Result<(PathBuf, std::fs::File)> {
    for n in 1..10_000 {
        let name = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("{} nicht anlegbar", path.display())),
        }
    }
    bail!("Kein freier Dateiname für {stem}")
}

fn create_dir(parent: &Path, stem: &str) -> Result<PathBuf> {
    for n in 1..10_000 {
        let path = parent.join(if n == 1 {
            stem.to_owned()
        } else {
            format!("{stem}-{n}")
        });
        match std::fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("{} nicht anlegbar", path.display())),
        }
    }
    bail!("Kein freier Ordnername für {stem}")
}

/// Dateiname-tauglicher Teil eines Personennamens.
pub fn safe_stem(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('_').to_owned();
    if cleaned.is_empty() {
        "Person".into()
    } else {
        cleaned
    }
}

/// Gesichtsausschnitt in `size` x `size`: bei `aligned` an den Augen ausgerichtet (gleiche
/// Augenlage und -größe), sonst der einfache Ausschnitt um den Rahmen.
pub fn render_face(
    root: &Path,
    face: &TimeFace,
    size: u32,
    aligned: bool,
    exiftool: &str,
    control: &Control,
) -> Result<RgbaImage> {
    let alignment = if aligned {
        timeline::alignment(face)
    } else {
        None
    };
    let edge = alignment.map_or(size, |a| {
        ((size as f32 / a.scale).ceil() as u32).clamp(64, 1200)
    });
    let preview = previews::preview(
        PreviewRequest {
            root: Some(root.to_path_buf()),
            path: root.join(&face.source),
            orientation: 1,
            crop: Some(face.bbox),
            edge,
            generation: 0,
        },
        exiftool,
        control,
    )?;
    let source = RgbaImage::from_raw(preview.width as u32, preview.height as u32, preview.rgba)
        .context("Vorschau hat unerwartete Größe")?;
    let Some(a) = alignment else {
        return Ok(image::imageops::resize(
            &source,
            size,
            size,
            image::imageops::FilterType::Lanczos3,
        ));
    };
    let mut out = RgbaImage::from_pixel(size, size, BACKGROUND);
    let (w, h) = (source.width() as f32, source.height() as f32);
    for y in 0..size {
        for x in 0..size {
            let uv = a.inverse([
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            ]);
            if let Some(color) = sample(&source, uv[0] * w - 0.5, uv[1] * h - 0.5) {
                blend(out.get_pixel_mut(x, y), color);
            }
        }
    }
    Ok(out)
}

/// Bilinear; `None` außerhalb des Bildes.
fn sample(image: &RgbaImage, x: f32, y: f32) -> Option<[f32; 4]> {
    let (w, h) = (image.width() as f32, image.height() as f32);
    if x < -0.5 || y < -0.5 || x > w - 0.5 || y > h - 0.5 {
        return None;
    }
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let get = |px: f32, py: f32| {
        let p = image.get_pixel(px.clamp(0.0, w - 1.0) as u32, py.clamp(0.0, h - 1.0) as u32);
        p.0.map(f32::from)
    };
    let (a, b, c, d) = (
        get(x0, y0),
        get(x0 + 1.0, y0),
        get(x0, y0 + 1.0),
        get(x0 + 1.0, y0 + 1.0),
    );
    let mut out = [0.0; 4];
    for i in 0..4 {
        out[i] =
            (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy;
    }
    Some(out)
}

fn blend(target: &mut Rgba<u8>, source: [f32; 4]) {
    let alpha = source[3] / 255.0;
    for (channel, value) in target.0.iter_mut().zip(source).take(3) {
        *channel = (value * alpha + f32::from(*channel) * (1.0 - alpha)).round() as u8;
    }
}

/// 5 x 7 Punktschrift für Ziffern und wenige Zeichen (keine Schriftart nötig).
fn glyph(c: char) -> Option<[u8; 7]> {
    Some(match c {
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        '6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        ' ' => [0; 7],
        _ => return None,
    })
}

/// Zeichnet `text` mit Punktschrift; `scale` ist die Punktgröße in Pixeln.
pub fn draw_text(image: &mut RgbaImage, x: u32, y: u32, text: &str, scale: u32) {
    let mut pen = x;
    for c in text.chars() {
        let Some(rows) = glyph(c) else { continue };
        for (row, bits) in rows.iter().enumerate() {
            for col in 0..5u32 {
                if bits & (0x10 >> col) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let (px, py) = (pen + col * scale + dx, y + row as u32 * scale + dy);
                            if px < image.width() && py < image.height() {
                                image.put_pixel(px, py, INK);
                            }
                        }
                    }
                }
            }
        }
        pen += 6 * scale;
    }
}

/// Beschriftung für Abschnitte, nur mit Zeichen der Punktschrift.
pub fn plain_label(axis: timeline::Axis, grain: timeline::Grain, ord: i32) -> String {
    use timeline::{Axis, Grain};
    match (axis, grain) {
        (Axis::Date, Grain::Year) => ord.to_string(),
        (Axis::Date, Grain::Month) => {
            format!("{}-{:02}", ord.div_euclid(12), ord.rem_euclid(12) + 1)
        }
        (Axis::Age, Grain::Year) => format!("{ord} J"),
        (Axis::Age, Grain::Month) => format!("{}J {}M", ord / 12, ord % 12),
    }
}

/// Raster aus Kacheln als Collage: `columns` Spalten, Beschriftung unter jeder Kachel.
pub fn collage(
    root: &Path,
    tiles: &[Tile],
    columns: u32,
    size: u32,
    aligned: bool,
    exiftool: &str,
    control: &Control,
) -> Result<RgbaImage> {
    if tiles.is_empty() {
        bail!("Keine Gesichter zum Exportieren");
    }
    let columns = columns.clamp(1, 16).min(tiles.len() as u32);
    let rows = (tiles.len() as u32).div_ceil(columns);
    let (gap, caption) = (8, 18);
    let mut out = RgbaImage::from_pixel(
        gap + columns * (size + gap),
        gap + rows * (size + caption + gap),
        BACKGROUND,
    );
    control
        .total
        .store(tiles.len(), std::sync::atomic::Ordering::Relaxed);
    for (index, tile) in tiles.iter().enumerate() {
        control.checkpoint()?;
        control
            .done
            .store(index, std::sync::atomic::Ordering::Relaxed);
        let (col, row) = (index as u32 % columns, index as u32 / columns);
        let (x, y) = (gap + col * (size + gap), gap + row * (size + caption + gap));
        // Ein unlesbares Bild lässt eine leere Kachel statt den Export abzubrechen.
        if let Ok(face) = render_face(root, &tile.face, size, aligned, exiftool, control) {
            image::imageops::replace(&mut out, &face, i64::from(x), i64::from(y));
        }
        draw_text(&mut out, x, y + size + 4, &tile.label, 2);
    }
    Ok(out)
}

/// Kontaktbogen: eine Zeile je Beschriftung, links die Beschriftung, rechts die Gesichter.
pub fn contact_sheet(
    root: &Path,
    rows: &[(String, Vec<TimeFace>)],
    size: u32,
    exiftool: &str,
    control: &Control,
) -> Result<RgbaImage> {
    let widest = rows.iter().map(|r| r.1.len()).max().unwrap_or(0) as u32;
    if widest == 0 {
        bail!("Keine Gesichter zum Exportieren");
    }
    let (gap, label) = (8, 90);
    let mut out = RgbaImage::from_pixel(
        label + gap + widest * (size + gap),
        gap + rows.len() as u32 * (size + gap),
        BACKGROUND,
    );
    control.total.store(
        rows.iter().map(|r| r.1.len()).sum(),
        std::sync::atomic::Ordering::Relaxed,
    );
    let mut done = 0;
    for (row, (name, faces)) in rows.iter().enumerate() {
        let y = gap + row as u32 * (size + gap);
        draw_text(&mut out, gap, y + size / 2 - 7, name, 2);
        for (col, face) in faces.iter().enumerate() {
            control.checkpoint()?;
            control
                .done
                .store(done, std::sync::atomic::Ordering::Relaxed);
            done += 1;
            if let Ok(tile) = render_face(root, face, size, false, exiftool, control) {
                let x = label + gap + col as u32 * (size + gap);
                image::imageops::replace(&mut out, &tile, i64::from(x), i64::from(y));
            }
        }
    }
    Ok(out)
}

/// Schreibt ein Bild als neue PNG-Datei.
pub fn save_png(image: &RgbaImage, dir: &Path, stem: &str) -> Result<PathBuf> {
    let (path, file) = create_new(dir, stem, "png")?;
    image
        .write_to(&mut std::io::BufWriter::new(file), image::ImageFormat::Png)
        .with_context(|| format!("{} nicht schreibbar", path.display()))?;
    Ok(path)
}

/// Zeitraffer als animiertes GIF (Endlosschleife); `fps` Bilder je Sekunde.
pub fn timelapse_gif(frames: &[RgbaImage], fps: f32, dir: &Path, stem: &str) -> Result<PathBuf> {
    use image::codecs::gif::{GifEncoder, Repeat};
    let Some(first) = frames.first() else {
        bail!("Keine Gesichter zum Exportieren");
    };
    let (path, file) = create_new(dir, stem, "gif")?;
    let mut encoder = GifEncoder::new_with_speed(std::io::BufWriter::new(file), 10);
    encoder.set_repeat(Repeat::Infinite)?;
    let delay =
        image::Delay::from_numer_denom_ms((1000.0 / fps.clamp(0.2, 30.0)).round() as u32, 1);
    debug_assert!(frames.iter().all(|f| f.dimensions() == first.dimensions()));
    encoder.encode_frames(
        frames
            .iter()
            .map(|f| image::Frame::from_parts(f.clone(), 0, 0, delay)),
    )?;
    Ok(path)
}

/// Zeitraffer als PNG-Bildfolge in einem neuen Unterordner; verlustfrei.
pub fn timelapse_sequence(frames: &[RgbaImage], dir: &Path, stem: &str) -> Result<PathBuf> {
    if frames.is_empty() {
        bail!("Keine Gesichter zum Exportieren");
    }
    let folder = create_dir(dir, stem)?;
    for (index, frame) in frames.iter().enumerate() {
        let (path, file) = create_new(&folder, &format!("{stem}-{:04}", index + 1), "png")?;
        frame
            .write_to(&mut std::io::BufWriter::new(file), image::ImageFormat::Png)
            .with_context(|| format!("{} nicht schreibbar", path.display()))?;
    }
    Ok(folder)
}
