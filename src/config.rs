use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PAGE_SIZE: usize = 48;
pub const COPY_CHUNK: usize = 1024 * 1024;
pub const CACHE_BYTES: usize = 32 * 1024 * 1024;
pub const GROUP_MAX: usize = 14;
pub const SUPPORTED: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tiff", "tif", "nef", "cr2", "cr3", "arw", "dng", "raw",
    "orf", "mp4", "mov", "avi", "mkv", "m4v",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub source: String,
    pub target: String,
    pub collection: String,
    pub metadata_date: bool,
    pub dng: bool,
    pub converter: String,
    pub exiftool: String,
    pub model_dir: String,
    pub onnx_runtime: String,
    pub dark: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source: String::new(),
            target: String::new(),
            collection: String::new(),
            metadata_date: false,
            dng: false,
            dark: true,
            converter: r"C:\Program Files\Adobe\Adobe DNG Converter\Adobe DNG Converter.exe".into(),
            exiftool: "exiftool".into(),
            model_dir: "models".into(),
            onnx_runtime: "runtime/onnxruntime.dll".into(),
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
