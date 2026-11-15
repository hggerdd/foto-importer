use super::{
    files::extension,
    jobs::{Control, command, wait_child},
};
use anyhow::{Context, Result, bail};
use image::{DynamicImage, GenericImageView};
use std::{
    io::{Read, Seek},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq)]
pub struct PreviewRequest {
    pub root: Option<PathBuf>,
    pub path: PathBuf,
    pub orientation: u32,
    pub crop: Option<[f32; 4]>,
    pub edge: u32,
    pub generation: u64,
}
pub struct Preview {
    pub request: PreviewRequest,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub fn load(
    path: &Path,
    exiftool: &str,
    orientation: u32,
    edge: u32,
    control: &Control,
) -> Result<DynamicImage> {
    control.checkpoint()?;
    let raw = ["nef", "nrw", "dng", "arw", "cr2", "cr3", "orf", "raw"]
        .contains(&extension(path).as_str());
    let decoder = if raw {
        let mut bytes = None;
        for tag in ["-JpgFromRaw", "-PreviewImage", "-ThumbnailImage"] {
            control.checkpoint()?;
            let mut file = tempfile::tempfile()?;
            let mut child = command(exiftool)
                .args(["-b", tag])
                .arg(path)
                .stdin(std::process::Stdio::null())
                .stdout(file.try_clone()?)
                .stderr(std::process::Stdio::null())
                .spawn()
                .context("RAW-Vorschau benötigt ExifTool")?;
            let status = wait_child(&mut child, control, Duration::from_secs(30))?;
            file.rewind()?;
            let mut data = Vec::new();
            file.take(64 * 1024 * 1024).read_to_end(&mut data)?;
            if status.success() && data.starts_with(&[255, 216]) {
                bytes = Some(data);
                break;
            }
        }
        image::ImageReader::new(std::io::Cursor::new(
            bytes.context("Keine eingebettete JPEG-Vorschau vorhanden")?,
        ))
        .with_guessed_format()?
    } else {
        let mut data = Vec::new();
        std::fs::File::open(path)?
            .take(128 * 1024 * 1024)
            .read_to_end(&mut data)?;
        image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format()?
    };
    let buffer = decoder.into_inner();
    let embedded_orientation = exif::Reader::new()
        .read_from_container(&mut std::io::BufReader::new(std::io::Cursor::new(
            buffer.get_ref(),
        )))
        .ok()
        .and_then(|e| {
            e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
        });
    let orientation = embedded_orientation
        .or_else(|| if raw { exif_orientation(path) } else { None })
        .unwrap_or(orientation);
    let mut decoder = image::ImageReader::new(buffer).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(384 * 1024 * 1024);
    limits.max_image_width = Some(30000);
    limits.max_image_height = Some(30000);
    decoder.limits(limits);
    let image = decoder.decode()?;
    control.checkpoint()?;
    let image = orient(image, orientation);
    Ok(image.thumbnail(edge, edge))
}

pub fn exif_orientation(path: &Path) -> Option<u32> {
    let mut reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    exif::Reader::new()
        .read_from_container(&mut reader)
        .ok()?
        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)?
        .value
        .get_uint(0)
}
pub fn orient(image: DynamicImage, orientation: u32) -> DynamicImage {
    match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    }
}

pub fn preview(request: PreviewRequest, exiftool: &str, control: &Control) -> Result<Preview> {
    if let Some(root) = &request.root {
        let root = root.canonicalize()?;
        let resolved = request.path.canonicalize()?;
        if !resolved.starts_with(root) || request.path.symlink_metadata()?.file_type().is_symlink()
        {
            bail!("Vorschau liegt außerhalb des gewählten Ordners");
        }
    }
    let mut image = load(
        &request.path,
        exiftool,
        request.orientation,
        if request.crop.is_some() {
            1600
        } else {
            request.edge
        },
        control,
    )?;
    if let Some(bbox) = request.crop {
        if bbox.iter().any(|v| !v.is_finite()) || bbox[2] <= bbox[0] || bbox[3] <= bbox[1] {
            bail!("Ungültiger Gesichtsausschnitt");
        }
        let (w, h) = image.dimensions();
        let side = ((bbox[2] - bbox[0]) * w as f32).max((bbox[3] - bbox[1]) * h as f32) * 1.6;
        if side > 6400.0 || bbox.iter().any(|v| v.abs() > 8.0) {
            bail!("Gesichtsausschnitt liegt außerhalb der zulässigen Bildgrenzen");
        }
        let x = (((bbox[0] + bbox[2]) * w as f32 - side) / 2.0).round() as i64;
        let y = (((bbox[1] + bbox[3]) * h as f32 - side) / 2.0).round() as i64;
        let side = (side.round() as u32).max(1);
        let mut canvas = DynamicImage::new_rgba8(side, side);
        image::imageops::overlay(&mut canvas, &image, -x, -y);
        image = canvas.resize_exact(
            request.edge,
            request.edge,
            image::imageops::FilterType::Lanczos3,
        );
    }
    let width = image.width() as usize;
    let height = image.height() as usize;
    Ok(Preview {
        request,
        width,
        height,
        rgba: image.to_rgba8().into_raw(),
    })
}

/// Ordner der Gesichtskachel-Zwischenspeicher in der Sammlung (ADR 0068). Reine, jederzeit
/// neu berechenbare Ableitung; „Biometrie löschen“ entfernt ihn mit.
pub const TILE_DIR: &str = ".myfotomanager_tiles";

/// Schlüssel einer Kachel: Quelldatei (relativ, Größe, Änderungszeit), Ausschnitt, Kantenlänge
/// und Ausrichtung. Ändert sich eines davon, entsteht eine neue Kachel.
fn tile_key(request: &PreviewRequest, root: &Path, bbox: &[f32; 4]) -> Option<String> {
    let meta = request.path.metadata().ok()?;
    let nanos = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let resolved = request.path.canonicalize().ok()?;
    let relative = resolved.strip_prefix(root).ok()?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"tile-v1\0");
    hash.update(relative.to_string_lossy().as_bytes());
    hash.update(&meta.len().to_le_bytes());
    hash.update(&nanos.to_le_bytes());
    for value in bbox {
        hash.update(&value.to_bits().to_le_bytes());
    }
    hash.update(&request.edge.to_le_bytes());
    hash.update(&request.orientation.to_le_bytes());
    Some(hash.finalize().to_hex().to_string())
}

fn tile_file(root: &Path, key: &str) -> PathBuf {
    root.join(TILE_DIR)
        .join(&key[..2])
        .join(format!("{key}.tile"))
}

fn decode_tile(bytes: &[u8]) -> Result<DynamicImage> {
    Ok(image::load_from_memory(bytes)?)
}

fn encode_tile(image: &DynamicImage) -> Result<Vec<u8>> {
    let rgba = image.to_rgba8();
    let mut out = Vec::new();
    if rgba.pixels().all(|p| p.0[3] == 255) {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&DynamicImage::ImageRgba8(rgba).to_rgb8())?;
    } else {
        // Ausschnitte über den Bildrand haben Transparenz; sie bleiben verlustfrei.
        rgba.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)?;
    }
    Ok(out)
}

/// Wie [`preview`], aber Gesichtsausschnitte kommen aus dem Kachel-Zwischenspeicher: Ein
/// Treffer liest nur eine kleine Datei statt das ganze Foto zu entschlüsseln (ADR 0068).
/// Frisch berechnete Kacheln werden durch dieselbe Kodierung geführt wie gespeicherte, damit
/// das Bild beim ersten und späteren Anzeigen identisch ist. Fehler beim Zwischenspeichern
/// (nur lesbarer Datenträger, Verknüpfung statt Ordner) werden ignoriert.
pub fn cached_preview(
    request: PreviewRequest,
    exiftool: &str,
    control: &Control,
) -> Result<Preview> {
    let (Some(bbox), Some(root)) = (request.crop, request.root.clone()) else {
        return preview(request, exiftool, control);
    };
    if bbox.iter().any(|v| !v.is_finite()) || request.edge == 0 {
        return preview(request, exiftool, control);
    }
    let root = root.canonicalize()?;
    let Some(key) = tile_key(&request, &root, &bbox) else {
        return preview(request, exiftool, control);
    };
    let file = tile_file(&root, &key);
    let from = |request: PreviewRequest, image: DynamicImage| Preview {
        request,
        width: image.width() as usize,
        height: image.height() as usize,
        rgba: image.to_rgba8().into_raw(),
    };
    if let Ok(bytes) = std::fs::read(&file) {
        match decode_tile(&bytes) {
            Ok(image) if image.width() == request.edge && image.height() == request.edge => {
                return Ok(from(request, image));
            }
            // Beschädigte Kachel (abgeleitet, nie ein Original): neu berechnen.
            _ => {
                let _ = std::fs::remove_file(&file);
            }
        }
    }
    let fresh = preview(request, exiftool, control)?;
    let request = fresh.request.clone();
    let Some(image) =
        image::RgbaImage::from_raw(fresh.width as u32, fresh.height as u32, fresh.rgba)
    else {
        bail!("Ungültige Gesichtskachel");
    };
    let image = DynamicImage::ImageRgba8(image);
    let Ok(bytes) = encode_tile(&image) else {
        return Ok(from(request, image));
    };
    store_tile(&root, &file, &bytes);
    Ok(from(request, decode_tile(&bytes).unwrap_or(image)))
}

/// Schreibt eine Kachel atomar und ersetzt nie eine vorhandene Datei.
fn store_tile(root: &Path, file: &Path, bytes: &[u8]) {
    let Some(folder) = file.parent() else { return };
    let base = root.join(TILE_DIR);
    if base
        .symlink_metadata()
        .is_ok_and(|m| m.file_type().is_symlink())
    {
        return;
    }
    if std::fs::create_dir_all(folder).is_err() {
        return;
    }
    let Ok(mut temp) = tempfile::NamedTempFile::new_in(folder) else {
        return;
    };
    if std::io::Write::write_all(&mut temp, bytes).is_ok() {
        let _ = temp.persist_noclobber(file);
    }
}

#[cfg(test)]
mod tile_tests {
    use super::*;

    fn photo(root: &Path, name: &str, shade: u8) -> PathBuf {
        let path = root.join(name);
        image::RgbImage::from_fn(400, 300, |x, y| {
            image::Rgb([shade, (x % 256) as u8, (y % 256) as u8])
        })
        .save(&path)
        .expect("Foto");
        path
    }
    fn request(root: &Path, path: PathBuf, bbox: [f32; 4]) -> PreviewRequest {
        PreviewRequest {
            root: Some(root.to_path_buf()),
            path,
            orientation: 1,
            crop: Some(bbox),
            edge: 64,
            generation: 0,
        }
    }

    #[test]
    fn cached_tile_equals_the_first_result_and_needs_no_source_decode() {
        let temp = tempfile::tempdir().expect("Ordner");
        let path = photo(temp.path(), "a.jpg", 10);
        let bbox = [0.4, 0.3, 0.6, 0.5];
        let control = Control::default();
        let first = cached_preview(request(temp.path(), path.clone(), bbox), "", &control)
            .expect("erste Kachel");
        let before = std::fs::metadata(&path).expect("stat");
        let tiles: Vec<_> = walk(&temp.path().join(TILE_DIR));
        assert_eq!(tiles.len(), 1, "genau eine Kachel angelegt");
        let second = cached_preview(request(temp.path(), path.clone(), bbox), "", &control)
            .expect("zweite Kachel");
        assert_eq!(first.rgba, second.rgba);
        assert_eq!((first.width, first.height), (64, 64));
        assert_eq!(std::fs::metadata(&path).expect("stat").len(), before.len());
        assert_eq!(walk(&temp.path().join(TILE_DIR)).len(), 1);
    }

    #[test]
    fn changed_source_or_crop_gets_a_new_tile_and_the_original_is_untouched() {
        let temp = tempfile::tempdir().expect("Ordner");
        let path = photo(temp.path(), "a.jpg", 10);
        let original = std::fs::read(&path).expect("lesen");
        let control = Control::default();
        let a = cached_preview(
            request(temp.path(), path.clone(), [0.4, 0.3, 0.6, 0.5]),
            "",
            &control,
        )
        .expect("a");
        let b = cached_preview(
            request(temp.path(), path.clone(), [0.1, 0.1, 0.3, 0.3]),
            "",
            &control,
        )
        .expect("b");
        assert_ne!(a.rgba, b.rgba);
        assert_eq!(walk(&temp.path().join(TILE_DIR)).len(), 2);
        assert_eq!(std::fs::read(&path).expect("lesen"), original);
        // Anderes Foto unter demselben Namen (andere Größe/Zeit) entwertet die Kachel.
        std::thread::sleep(std::time::Duration::from_millis(20));
        photo(temp.path(), "a.jpg", 200);
        let c = cached_preview(
            request(temp.path(), path, [0.4, 0.3, 0.6, 0.5]),
            "",
            &control,
        )
        .expect("c");
        assert_ne!(a.rgba, c.rgba);
    }

    #[test]
    fn corrupt_tiles_are_recomputed_and_uncropped_requests_skip_the_cache() {
        let temp = tempfile::tempdir().expect("Ordner");
        let path = photo(temp.path(), "a.jpg", 10);
        let bbox = [0.4, 0.3, 0.6, 0.5];
        let control = Control::default();
        let good =
            cached_preview(request(temp.path(), path.clone(), bbox), "", &control).expect("gut");
        let tile = walk(&temp.path().join(TILE_DIR)).remove(0);
        std::fs::write(&tile, b"kaputt").expect("schreiben");
        let again =
            cached_preview(request(temp.path(), path.clone(), bbox), "", &control).expect("neu");
        assert_eq!(good.rgba, again.rgba);
        let mut full = request(temp.path(), path, bbox);
        full.crop = None;
        full.edge = 100;
        cached_preview(full, "", &control).expect("ganzes Bild");
        assert_eq!(walk(&temp.path().join(TILE_DIR)).len(), 1);
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    found.extend(walk(&path));
                } else {
                    found.push(path);
                }
            }
        }
        found
    }
}
