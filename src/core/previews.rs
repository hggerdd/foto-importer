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
