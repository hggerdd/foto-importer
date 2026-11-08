use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PAGE_SIZE: usize = 48;
pub const COPY_CHUNK: usize = 1024 * 1024;
pub const CACHE_BYTES: usize = 192 * 1024 * 1024;
pub const CACHE_ENTRIES: usize = 512;
/// Höchstzahl Gesichter einer Gruppe; breite Fenster zeigen sie in zwei Reihen.
pub const GROUP_MAX: usize = 28;
/// Gruppen je Seite in den Gesichtsreitern.
pub const FACE_GROUPS_PER_PAGE: usize = 8;
pub const SUPPORTED: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "nef", "cr2", "cr3", "arw", "dng", "raw",
    "orf", "mp4", "mov", "avi", "mkv", "m4v",
];
fn resource_root() -> PathBuf {
    let executable = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    if executable.join("models").is_dir() {
        return executable;
    }
    let development = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if development.join("models").is_dir() {
        development
    } else {
        executable
    }
}

pub fn model_sources(configured: &str) -> [PathBuf; 3] {
    [
        PathBuf::from(configured),
        resource_root().join("models"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("models"),
    ]
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub source: String,
    pub target: String,
    pub collection: String,
    pub metadata_date: bool,
    pub dng: bool,
    pub skip_imported: bool,
    pub auto_metadata: bool,
    pub converter: String,
    pub exiftool: String,
    pub model_dir: String,
    pub onnx_runtime: String,
    pub dark: bool,
    /// Im Personenbereich das ganze Foto unter der Maus zeigen.
    pub hover_preview: bool,
    pub window_size: [f32; 2],
    pub dng_update: crate::core::dng_updates::UpdateCache,
}

impl Default for Settings {
    fn default() -> Self {
        let resources = resource_root();
        Self {
            source: String::new(),
            target: String::new(),
            collection: String::new(),
            metadata_date: false,
            dng: false,
            skip_imported: true,
            auto_metadata: false,
            dark: true,
            hover_preview: true,
            window_size: [1280.0, 800.0],
            dng_update: Default::default(),
            converter: r"C:\Program Files\Adobe\Adobe DNG Converter\Adobe DNG Converter.exe".into(),
            exiftool: "exiftool".into(),
            model_dir: resources.join("models").to_string_lossy().into_owned(),
            onnx_runtime: resources
                .join("runtime/onnxruntime.dll")
                .to_string_lossy()
                .into_owned(),
        }
    }
}

impl Settings {
    pub fn path() -> PathBuf {
        std::env::var_os("FOTO_IMPORTER_SETTINGS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("APPDATA").unwrap_or_else(|| ".".into()))
                    .join("FotoImporter")
                    .join("settings.json")
            })
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::path();
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Einstellungspfad ungültig"))?;
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }
}
