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
    let mut decoder = if raw {
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
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(384 * 1024 * 1024);
    limits.max_image_width = Some(30000);
    limits.max_image_height = Some(30000);
    decoder.limits(limits);
    let orientation = exif_orientation(path).unwrap_or(orientation);
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
        let x = (((bbox[0] + bbox[2]) * w as f32 - side) / 2.0)
            .max(0.0)
            .min((w - 1) as f32) as u32;
        let y = (((bbox[1] + bbox[3]) * h as f32 - side) / 2.0)
            .max(0.0)
            .min((h - 1) as f32) as u32;
        image = image
            .crop_imm(
                x,
                y,
                (side as u32).max(1).min(w - x),
                (side as u32).max(1).min(h - y),
            )
            .thumbnail(request.edge, request.edge);
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
