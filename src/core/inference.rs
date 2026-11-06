//! Local CPU inference. Rust adapter keys deliberately isolate unvalidated vector spaces.
use anyhow::{Context, Result, bail};
use image::{DynamicImage, RgbImage};
use ndarray::Array4;
use ort::{session::Session, value::TensorRef};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub version: String,
    pub runtime: String,
    pub file: String,
    pub sha256: String,
    pub license: String,
    #[serde(default)]
    pub dim: usize,
    #[serde(default)]
    pub input: Input,
    #[serde(default)]
    pub thresholds: Thresholds,
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Input {
    pub size: [u32; 2],
    pub color: String,
    pub mean: f32,
    pub std: f32,
    pub align: String,
}
impl Default for Input {
    fn default() -> Self {
        Self {
            size: [112, 112],
            color: "RGB".into(),
            mean: 0.0,
            std: 1.0,
            align: "arcface_5pt".into(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Thresholds {
    pub auto: f32,
    pub suggest: f32,
    pub duplicate: f32,
}
impl Default for Thresholds {
    fn default() -> Self {
        Self {
            auto: 0.6,
            suggest: 0.363,
            duplicate: 0.92,
        }
    }
}
impl Manifest {
    pub fn key(&self) -> String {
        format!("{}@{}:{}:rust-ort-v1", self.id, self.version, self.sha256)
    }
    pub fn verify(&self, folder: &Path) -> Result<PathBuf> {
        let folder = folder
            .canonicalize()
            .with_context(|| format!("Modellordner nicht verfügbar: {}", folder.display()))?;
        let expected = folder.join(&self.file);
        let path = expected.canonicalize().with_context(|| format!(
            "Modellgewicht für {} fehlt oder ist nicht zugänglich: {}. Unter Einstellungen die Modellgewichte installieren oder den Modellordner wählen.",
            self.name, expected.display()
        ))?;
        if !path.starts_with(&folder) {
            bail!("Modellpfad liegt außerhalb des Modellordners");
        }
        let mut file = std::fs::File::open(&path)?;
        let mut hash = Sha256::new();
        let mut buffer = vec![0u8; 1024 * 1024];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != self.sha256.to_lowercase() {
            bail!("SHA-256 stimmt nicht für {}", self.id);
        }
        if !["RGB", "BGR"].contains(&self.input.color.as_str())
            || self.input.std <= 0.0
            || !self.input.std.is_finite()
            || !self.input.mean.is_finite()
            || self.input.size.contains(&0)
            || self.input.size.iter().any(|v| *v > 4096)
        {
            bail!("Ungültige Eingabespezifikation");
        }
        if self.kind == "embedder"
            && (self.dim == 0
                || self.thresholds.auto < self.thresholds.suggest
                || self.thresholds.auto > 1.0
                || self.thresholds.suggest < -1.0)
        {
            bail!("Ungültiger Embedding-Raum");
        }
        Ok(path)
    }
}
pub fn manifests(folder: &Path) -> Result<Vec<Manifest>> {
    let mut list: Vec<Manifest> = Vec::new();
    for entry in std::fs::read_dir(folder)
        .with_context(|| format!("Modellordner nicht verfügbar: {}", folder.display()))?
    {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "json") {
            list.push(serde_json::from_reader(std::fs::File::open(path)?)?);
        }
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(list)
}

#[derive(Clone, Debug)]
pub struct Detection {
    pub bbox: [f32; 4],
    pub landmarks: [[f32; 2]; 5],
    pub score: f32,
}
pub struct Models {
    pub detector: Manifest,
    pub embedder: Manifest,
    det: Session,
    emb: Session,
}
impl Models {
    pub fn load(folder: &Path, dll: &Path, detector: &str, embedder: &str) -> Result<Self> {
        if !dll.is_file() {
            bail!(
                "ONNX Runtime fehlt: {}. Unter Einstellungen „ONNX Runtime herunterladen“ wählen.",
                dll.display()
            );
        }
        ort::init_from(dll.to_string_lossy()).commit()?;
        let list = manifests(folder)?;
        let detector = list
            .iter()
            .find(|m| m.id == detector && m.kind == "detector")
            .context("Detektor fehlt im Modellordner")?
            .clone();
        let embedder = list
            .iter()
            .find(|m| m.id == embedder && m.kind == "embedder")
            .context("Embedder fehlt im Modellordner")?
            .clone();
        let det = Session::builder()?
            .with_log_level(ort::logging::LogLevel::Error)?
            .with_intra_threads(2)?
            .with_intra_op_spinning(false)?
            .commit_from_file(detector.verify(folder)?)?;
        let emb = Session::builder()?
            .with_log_level(ort::logging::LogLevel::Error)?
            .with_intra_threads(2)?
            .with_intra_op_spinning(false)?
            .commit_from_file(embedder.verify(folder)?)?;
        Ok(Self {
            detector,
            embedder,
            det,
            emb,
        })
    }
    pub fn detect(&mut self, image: &DynamicImage) -> Result<Vec<Detection>> {
        let yunet = self.detector.runtime == "opencv_yunet";
        let size = if yunet {
            [640, 640]
        } else {
            self.detector.input.size
        };
        let ratio =
            (size[0] as f32 / image.width() as f32).min(size[1] as f32 / image.height() as f32);
        let scaled = image
            .resize_exact(
                (image.width() as f32 * ratio).round().max(1.0) as u32,
                (image.height() as f32 * ratio).round().max(1.0) as u32,
                image::imageops::FilterType::Triangle,
            )
            .to_rgb8();
        let mut input = Array4::zeros((1, 3, size[1] as usize, size[0] as usize));
        for (x, y, p) in scaled.enumerate_pixels() {
            for c in 0..3 {
                input[[0, c, y as usize, x as usize]] = if yunet {
                    p[2 - c] as f32
                } else {
                    (p[if self.detector.input.color == "BGR" {
                        2 - c
                    } else {
                        c
                    }] as f32
                        - self.detector.input.mean)
                        / self.detector.input.std
                };
            }
        }
        let output = self
            .det
            .run(ort::inputs![TensorRef::from_array_view(&input)?])?;
        let mut candidates = Vec::new();
        if yunet {
            for stride in [8, 16, 32] {
                let cls = output
                    .get(format!("cls_{stride}"))
                    .context("YuNet cls-Ausgabe fehlt")?
                    .try_extract_tensor::<f32>()?
                    .1;
                let obj = output
                    .get(format!("obj_{stride}"))
                    .context("YuNet obj-Ausgabe fehlt")?
                    .try_extract_tensor::<f32>()?
                    .1;
                let boxes = output
                    .get(format!("bbox_{stride}"))
                    .context("YuNet bbox-Ausgabe fehlt")?
                    .try_extract_tensor::<f32>()?
                    .1;
                let marks = output
                    .get(format!("kps_{stride}"))
                    .context("YuNet kps-Ausgabe fehlt")?
                    .try_extract_tensor::<f32>()?
                    .1;
                for i in 0..cls.len() {
                    let score = (cls[i].clamp(0.0, 1.0) * obj[i].clamp(0.0, 1.0)).sqrt();
                    if score < 0.8 {
                        continue;
                    }
                    let x = (i as u32 % (size[0] / stride)) as f32;
                    let y = (i as u32 / (size[0] / stride)) as f32;
                    let step = stride as f32 / ratio;
                    let cx = (x + boxes[i * 4]) * step;
                    let cy = (y + boxes[i * 4 + 1]) * step;
                    let w = boxes[i * 4 + 2].exp() * step;
                    let h = boxes[i * 4 + 3].exp() * step;
                    let mut landmarks = [[0.0; 2]; 5];
                    for k in 0..5 {
                        landmarks[k] = [
                            (marks[i * 10 + k * 2] + x) * step,
                            (marks[i * 10 + k * 2 + 1] + y) * step,
                        ];
                    }
                    candidates.push(Detection {
                        bbox: [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0],
                        landmarks,
                        score,
                    });
                }
            }
        } else {
            let values: Vec<Vec<f32>> = output
                .iter()
                .map(|(_, v)| v.try_extract_tensor::<f32>().map(|(_, x)| x.to_vec()))
                .collect::<ort::Result<_>>()?;
            let levels = match values.len() {
                9 => 3,
                15 => 5,
                _ => bail!("SCRFD benötigt 9 oder 15 Ausgaben"),
            };
            for (level, stride) in [8u32, 16, 32, 64, 128].into_iter().take(levels).enumerate() {
                let scores = &values[level];
                let boxes = &values[level + levels];
                let marks = &values[level + levels * 2];
                let columns = size[0] / stride;
                let anchors = scores.len() / ((size[0] / stride * size[1] / stride) as usize);
                if anchors == 0 {
                    continue;
                }
                for (i, score) in scores.iter().enumerate() {
                    if *score < 0.8 {
                        continue;
                    }
                    let index = i / anchors;
                    let cx = (index as u32 % columns * stride) as f32;
                    let cy = (index as u32 / columns * stride) as f32;
                    let step = stride as f32;
                    let mut landmarks = [[0.0; 2]; 5];
                    for k in 0..5 {
                        landmarks[k] = [
                            (cx + marks[i * 10 + k * 2] * step) / ratio,
                            (cy + marks[i * 10 + k * 2 + 1] * step) / ratio,
                        ];
                    }
                    candidates.push(Detection {
                        bbox: [
                            (cx - boxes[i * 4] * step) / ratio,
                            (cy - boxes[i * 4 + 1] * step) / ratio,
                            (cx + boxes[i * 4 + 2] * step) / ratio,
                            (cy + boxes[i * 4 + 3] * step) / ratio,
                        ],
                        landmarks,
                        score: *score,
                    });
                }
            }
        }
        candidates.retain(|d| {
            d.bbox.iter().all(|x| x.is_finite()) && d.bbox[2] > d.bbox[0] && d.bbox[3] > d.bbox[1]
        });
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        candidates.truncate(5000);
        let mut kept: Vec<Detection> = Vec::new();
        for candidate in candidates {
            if kept.iter().all(|d| iou(d.bbox, candidate.bbox) < 0.3) {
                kept.push(candidate);
            }
        }
        Ok(kept)
    }
    pub fn embed(&mut self, image: &DynamicImage, detection: &Detection) -> Result<Vec<f32>> {
        let aligned = align(&image.to_rgb8(), detection.landmarks)?;
        let mut input = Array4::zeros((1, 3, 112, 112));
        // OpenCV SFace's feature() swaps BGR to RGB; the graph receives RGB 0..255.
        let sface = self.embedder.runtime == "opencv_sface";
        for (x, y, p) in aligned.enumerate_pixels() {
            for c in 0..3 {
                let channel = if !sface && self.embedder.input.color == "BGR" {
                    2 - c
                } else {
                    c
                };
                input[[0, c, y as usize, x as usize]] = if sface {
                    p[channel] as f32
                } else {
                    (p[channel] as f32 - self.embedder.input.mean) / self.embedder.input.std
                };
            }
        }
        let output = self
            .emb
            .run(ort::inputs![TensorRef::from_array_view(&input)?])?;
        let data = output[0].try_extract_tensor::<f32>()?.1;
        if data.len() != self.embedder.dim || data.iter().any(|v| !v.is_finite()) {
            bail!("Ungültiges Embedding");
        }
        normalize(data)
    }
}

pub fn normalize(vector: &[f32]) -> Result<Vec<f32>> {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if !norm.is_finite() || norm < 1e-8 {
        bail!("Leerer oder ungültiger Vektor");
    }
    Ok(vector.iter().map(|v| v / norm).collect())
}
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let score = a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
    score.is_finite().then_some(score.clamp(-1.0, 1.0))
}
pub fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let intersection =
        (a[2].min(b[2]) - a[0].max(b[0])).max(0.0) * (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - intersection;
    if union > 0.0 {
        intersection / union
    } else {
        0.0
    }
}

pub fn align(image: &RgbImage, source: [[f32; 2]; 5]) -> Result<RgbImage> {
    let target = [
        [38.2946, 51.6963],
        [73.5318, 51.5014],
        [56.0252, 71.7366],
        [41.5493, 92.3655],
        [70.7299, 92.2041],
    ];
    let mean = |points: &[[f32; 2]; 5]| {
        [
            points.iter().map(|p| p[0]).sum::<f32>() / 5.0,
            points.iter().map(|p| p[1]).sum::<f32>() / 5.0,
        ]
    };
    let sm = mean(&source);
    let tm = mean(&target);
    let (mut num_a, mut num_b, mut denominator) = (0.0, 0.0, 0.0);
    for (s, t) in source.iter().zip(target) {
        let x = s[0] - sm[0];
        let y = s[1] - sm[1];
        let u = t[0] - tm[0];
        let v = t[1] - tm[1];
        num_a += x * u + y * v;
        num_b += x * v - y * u;
        denominator += x * x + y * y;
    }
    if denominator < 1e-6 {
        bail!("Landmarks sind degeneriert");
    }
    let a = num_a / denominator;
    let b = num_b / denominator;
    let tx = tm[0] - a * sm[0] + b * sm[1];
    let ty = tm[1] - b * sm[0] - a * sm[1];
    let det = a * a + b * b;
    if det < 1e-9 {
        bail!("Alignment ist nicht invertierbar");
    }
    let mut output = RgbImage::new(112, 112);
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let u = x as f32 - tx;
        let v = y as f32 - ty;
        let sx = (a * u + b * v) / det;
        let sy = (-b * u + a * v) / det;
        let ix = sx.floor() as i32;
        let iy = sy.floor() as i32;
        let dx = sx - ix as f32;
        let dy = sy - iy as f32;
        let mut color = [0.0; 3];
        for (ox, oy, weight) in [
            (0, 0, (1.0 - dx) * (1.0 - dy)),
            (1, 0, dx * (1.0 - dy)),
            (0, 1, (1.0 - dx) * dy),
            (1, 1, dx * dy),
        ] {
            let px = ix + ox;
            let py = iy + oy;
            if px >= 0 && py >= 0 && px < image.width() as i32 && py < image.height() as i32 {
                for (c, value) in color.iter_mut().enumerate() {
                    *value += image.get_pixel(px as u32, py as u32)[c] as f32 * weight;
                }
            }
        }
        *pixel = image::Rgb(color.map(|v| v.round().clamp(0.0, 255.0) as u8));
    }
    Ok(output)
}
