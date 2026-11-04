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

pub struct Thumbnails {
    requests: mpsc::SyncSender<(String, PreviewRequest, String)>,
    results: mpsc::Receiver<(String, anyhow::Result<Preview>)>,
    cache: HashMap<String, (Option<egui::TextureHandle>, usize, u64, String)>,
    pending: HashSet<String>,
    generation: Arc<AtomicU64>,
    tick: u64,
    bytes: usize,
}
impl Thumbnails {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(String, PreviewRequest, String)>(48);
        let (out, input) = mpsc::sync_channel(48);
        let generation = Arc::new(AtomicU64::new(0));
        let worker_generation = generation.clone();
        std::thread::spawn(move || {
            while let Ok((key, request, exiftool)) = rx.recv() {
                if request.generation != worker_generation.load(Ordering::Relaxed) {
                    continue;
                }
                let result = previews::preview(request, &exiftool, &Control::default());
                if out.send((key, result)).is_err() {
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
    pub fn poll(&mut self, ctx: &egui::Context) {
        while let Ok((key, result)) = self.results.try_recv() {
            self.pending.remove(&key);
            let generation = self.generation.load(Ordering::Relaxed);
            if !key.starts_with(&format!("{generation}:")) {
                continue;
            }
            let (value, bytes, error) = match result {
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
            self.cache.insert(key, (value, bytes, self.tick, error));
        }
        while self.bytes > config::CACHE_BYTES || self.cache.len() > 128 {
            let Some(oldest) = self
                .cache
                .iter()
                .min_by_key(|(_, v)| v.2)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some((_, bytes, _, _)) = self.cache.remove(&oldest) {
                self.bytes -= bytes;
            }
        }
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
        let key = format!(
            "{}:{}:{}:{:?}:{}",
            request.generation,
            request.path.display(),
            request.orientation,
            request.crop,
            request.edge
        );
        let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
        if ui.is_rect_visible(rect) {
            ui.painter()
                .rect_filled(rect, 8, egui::Color32::from_rgb(13, 18, 24));
            if let Some((texture, _, tick, error)) = self.cache.get_mut(&key) {
                *tick = self.tick;
                if let Some(texture) = texture {
                    let fit = texture.size_vec2();
                    let scale = (rect.width() / fit.x).min(rect.height() / fit.y);
                    let dest = egui::Rect::from_center_size(rect.center(), fit * scale);
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
