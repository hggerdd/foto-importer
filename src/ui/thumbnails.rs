use crate::{
    config,
    core::{
        jobs::Control,
        previews::{self, Preview, PreviewRequest},
    },
};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

struct Entry {
    texture: Option<egui::TextureHandle>,
    bytes: usize,
    tick: u64,
    frame: u64,
    error: String,
}

/// Cache key without the generation: identical content keeps its texture
/// across searches and refreshes. Only `clear` (new collection/scan) drops it.
pub fn cache_key(request: &PreviewRequest) -> String {
    format!(
        "{}:{}:{:?}:{}:{}",
        request.path.display(),
        request.orientation,
        request.crop,
        request.edge,
        request
            .root
            .as_ref()
            .map_or(String::new(), |root| root.display().to_string())
    )
}

/// Evicts least recently used entries, but never entries drawn in the current
/// or previous frame, so visible images cannot thrash against the budget.
fn evict(cache: &mut HashMap<String, Entry>, bytes: &mut usize, frame: u64, max_bytes: usize) {
    while *bytes > max_bytes || cache.len() > config::CACHE_ENTRIES {
        let Some(oldest) = cache
            .iter()
            .filter(|(_, v)| v.frame + 1 < frame)
            .min_by_key(|(_, v)| v.tick)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        if let Some(entry) = cache.remove(&oldest) {
            *bytes -= entry.bytes;
        }
    }
}

pub struct Thumbnails {
    requests: mpsc::SyncSender<(String, PreviewRequest, String)>,
    results: mpsc::Receiver<(String, u64, anyhow::Result<Preview>)>,
    cache: HashMap<String, Entry>,
    pending: HashSet<String>,
    generation: Arc<AtomicU64>,
    tick: u64,
    bytes: usize,
}
impl Thumbnails {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(String, PreviewRequest, String)>(1);
        let (out, input) = mpsc::sync_channel(2);
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = generation.clone();
        std::thread::spawn(move || {
            while let Ok((key, request, exiftool)) = rx.recv() {
                if request.generation != worker_generation.load(Ordering::Relaxed) {
                    continue;
                }
                let _foreground = crate::core::jobs::foreground();
                let request_generation = request.generation;
                let result = previews::preview(request, &exiftool, &Control::default());
                if out.send((key, request_generation, result)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            requests: tx,
            results: input,
            cache: HashMap::new(),
            pending: HashSet::new(),
            generation,
            tick: 0,
            bytes: 0,
        }
    }
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.cache.clear();
        self.pending.clear();
        self.bytes = 0;
    }
    pub fn ready(&self, request: &PreviewRequest) -> bool {
        self.cache
            .get(&cache_key(request))
            .is_some_and(|entry| entry.texture.is_some())
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, generation, result)) = self.results.try_recv() {
            self.pending.remove(&key);
            if generation != self.generation.load(Ordering::Relaxed) {
                continue;
            }
            let (texture, bytes, error) = match result {
                Ok(p) => {
                    let bytes = p.rgba.len();
                    let image =
                        egui::ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba);
                    (
                        Some(ctx.load_texture(&key, image, egui::TextureOptions::LINEAR)),
                        bytes,
                        String::new(),
                    )
                }
                Err(e) => (None, 0, e.to_string()),
            };
            self.bytes += bytes;
            if let Some(old) = self.cache.insert(
                key,
                Entry {
                    texture,
                    bytes,
                    tick: self.tick,
                    frame: ctx.cumulative_frame_nr(),
                    error,
                },
            ) {
                self.bytes -= old.bytes;
            }
        }
        evict(
            &mut self.cache,
            &mut self.bytes,
            ctx.cumulative_frame_nr(),
            config::CACHE_BYTES,
        );
    }
    /// Textur zu einer Anfrage ohne sie zu zeichnen (z. B. für runde Ausschnitte).
    /// Stößt das Laden auf dem Worker an; `None`, solange nichts vorliegt.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        mut request: PreviewRequest,
        exiftool: &str,
    ) -> Option<(egui::TextureId, egui::Vec2)> {
        self.tick += 1;
        request.generation = self.generation.load(Ordering::Relaxed);
        let key = cache_key(&request);
        if let Some(entry) = self.cache.get_mut(&key) {
            entry.tick = self.tick;
            entry.frame = ctx.cumulative_frame_nr();
            return entry.texture.as_ref().map(|t| (t.id(), t.size_vec2()));
        }
        if !self.pending.contains(&key)
            && self
                .requests
                .try_send((key.clone(), request, exiftool.to_string()))
                .is_ok()
        {
            self.pending.insert(key);
        }
        None
    }
    pub fn image(
        &mut self,
        ui: &mut egui::Ui,
        mut request: PreviewRequest,
        exiftool: &str,
        size: egui::Vec2,
    ) -> egui::Response {
        self.tick += 1;
        request.generation = self.generation.load(Ordering::Relaxed);
        let key = cache_key(&request);
        let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                ui.is_enabled(),
                format!(
                    "{} {}",
                    if request.crop.is_some() {
                        "Gesichtsausschnitt in"
                    } else {
                        "Bild öffnen:"
                    },
                    request
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            )
        });
        if ui.is_rect_visible(rect) {
            ui.painter()
                .rect_filled(rect, 8, egui::Color32::from_rgb(13, 18, 24));
            if let Some(entry) = self.cache.get_mut(&key) {
                entry.tick = self.tick;
                entry.frame = ui.ctx().cumulative_frame_nr();
                let error = &entry.error;
                if let Some(texture) = &entry.texture {
                    let fit = texture.size_vec2();
                    let scale = (rect.width() / fit.x).min(rect.height() / fit.y);
                    let dest = egui::Rect::from_center_size(rect.center(), fit * scale);
                    response.rect = dest;
                    ui.painter().image(
                        texture.id(),
                        dest,
                        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                } else {
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Keine Vorschau",
                        egui::FontId::proportional(12.0),
                        egui::Color32::GRAY,
                    );
                    return response.on_hover_text(error.as_str());
                }
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Lädt …",
                    egui::FontId::proportional(12.0),
                    egui::Color32::GRAY,
                );
                if !self.pending.contains(&key)
                    && self
                        .requests
                        .try_send((key.clone(), request, exiftool.to_string()))
                        .is_ok()
                {
                    self.pending.insert(key);
                }
            }
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn request(path: &str, generation: u64) -> PreviewRequest {
        PreviewRequest {
            root: None,
            path: PathBuf::from(path),
            orientation: 1,
            crop: None,
            edge: 400,
            generation,
        }
    }
    fn entry(bytes: usize, tick: u64, frame: u64) -> Entry {
        Entry {
            texture: None,
            bytes,
            tick,
            frame,
            error: String::new(),
        }
    }

    #[test]
    fn key_is_stable_across_generations_but_not_variants() {
        assert_eq!(
            cache_key(&request("a.jpg", 0)),
            cache_key(&request("a.jpg", 7))
        );
        let mut other = request("a.jpg", 0);
        other.edge = 960;
        assert_ne!(cache_key(&request("a.jpg", 0)), cache_key(&other));
    }

    #[test]
    fn eviction_keeps_recent_frames_and_drops_oldest() {
        let mut cache = HashMap::new();
        cache.insert("old".to_string(), entry(60, 1, 1));
        cache.insert("older".to_string(), entry(60, 0, 1));
        cache.insert("visible".to_string(), entry(60, 5, 10));
        let mut bytes = 180;
        evict(&mut cache, &mut bytes, 10, 100);
        assert!(cache.contains_key("visible"));
        assert!(!cache.contains_key("older") && !cache.contains_key("old"));
        assert_eq!(bytes, 60);
        let mut bytes = 60;
        evict(&mut cache, &mut bytes, 10, 10);
        assert_eq!(cache.len(), 1);
    }
}
