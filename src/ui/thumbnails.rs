use crate::{
    config,
    core::{
        jobs::Control,
        previews::{self, Preview, PreviewRequest},
    },
    ui::preview_queue::{PreviewQueue, Priority},
};
use std::{
    collections::HashMap,
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

/// Fehler und auffällig langsame Vorschauen ins Diagnose-Log.
fn log_preview(
    path: &std::path::Path,
    elapsed: std::time::Duration,
    result: &anyhow::Result<Preview>,
) {
    let millis = crate::core::diag::millis(elapsed);
    match result {
        Err(e) => log::warn!(
            target: "vorschau",
            "fehler datei={:?} dauer_ms={millis:.1} fehler={:?}",
            path.display().to_string(),
            format!("{e:#}")
        ),
        Ok(_) if elapsed.as_millis() >= config::DIAG_SLOW_PREVIEW_MS => log::info!(
            target: "vorschau",
            "langsam datei={:?} dauer_ms={millis:.1}",
            path.display().to_string()
        ),
        Ok(_) => {}
    }
}

type Job = (PreviewRequest, String);
type Done = (String, u64, std::time::Duration, anyhow::Result<Preview>);

/// Wartender oder laufender Auftrag der Oberfläche: wann zuerst angefragt und mit welchem
/// Vorrang (für die Latenz des Fotos unter der Maus im Diagnose-Log).
struct Pending {
    since: std::time::Instant,
    priority: Priority,
    path: std::path::PathBuf,
}

/// Foto unter der Maus: Zeit von der ersten Anfrage bis zur fertigen Textur (gezeigt im
/// nächsten Frame), davon Dekodierung bzw. Cachelesen und Warten.
fn log_hover(
    path: &std::path::Path,
    total: std::time::Duration,
    decode: std::time::Duration,
    ok: bool,
) {
    let millis = crate::core::diag::millis;
    log::info!(
        target: "vorschau",
        "maus gezeigt datei={:?} dauer_ms={:.1} dekodieren_ms={:.1} warten_ms={:.1} ergebnis={}",
        path.display().to_string(),
        millis(total),
        millis(decode),
        millis(total.saturating_sub(decode)),
        if ok { "ok" } else { "fehler" }
    );
}

pub struct Thumbnails {
    queue: Arc<PreviewQueue<Job>>,
    results: mpsc::Receiver<Done>,
    cache: HashMap<String, Entry>,
    pending: HashMap<String, Pending>,
    generation: Arc<AtomicU64>,
    tick: u64,
    bytes: usize,
}
impl Drop for Thumbnails {
    fn drop(&mut self) {
        self.queue.close();
    }
}
impl Thumbnails {
    pub fn new(ctx: egui::Context) -> Self {
        // Mehrere Arbeiter dekodieren Fotos gleichzeitig (ein Foto kostet 100 bis 300 ms, eine
        // Seite zeigt Dutzende Gesichter, ADR 0068); die Kachelbahn bleibt kurz, damit
        // sichtbare Kacheln Vorrang vor längst weggescrollten behalten. Dazu kommen reservierte
        // Arbeiter nur für das Foto unter der Maus.
        let workers = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(2, config::THUMBNAIL_WORKERS);
        let queue: Arc<PreviewQueue<Job>> =
            Arc::new(PreviewQueue::new(workers, config::THUMBNAIL_STALE_FRAMES));
        let (out, input) = mpsc::sync_channel(workers * 2 + config::THUMBNAIL_HOVER_WORKERS);
        let generation = Arc::new(AtomicU64::new(0));
        for index in 0..workers + config::THUMBNAIL_HOVER_WORKERS {
            let hover_only = index >= workers;
            let (queue, out, ctx) = (queue.clone(), out.clone(), ctx.clone());
            let worker_generation = generation.clone();
            std::thread::spawn(move || {
                while let Some((key, (request, exiftool), _)) = queue.pop(hover_only) {
                    if request.generation != worker_generation.load(Ordering::Relaxed) {
                        // Die Oberfläche hat ihren Stand vergessen (`clear`); nichts melden.
                        continue;
                    }
                    let _foreground = crate::core::jobs::foreground();
                    let request_generation = request.generation;
                    let started = std::time::Instant::now();
                    let path = crate::core::diag::enabled().then(|| request.path.clone());
                    let result = previews::cached_preview(request, &exiftool, &Control::default());
                    let elapsed = started.elapsed();
                    if let Some(path) = path {
                        log_preview(&path, elapsed, &result);
                    }
                    if out
                        .send((key, request_generation, elapsed, result))
                        .is_err()
                    {
                        break;
                    }
                    ctx.request_repaint();
                }
            });
        }
        Self {
            queue,
            results: input,
            cache: HashMap::new(),
            pending: HashMap::new(),
            generation,
            tick: 0,
            bytes: 0,
        }
    }
    /// Fordert eine Vorschau an oder hält eine wartende Anfrage am Leben (sie wurde in diesem
    /// Frame gezeichnet). Eine Mausanfrage holt eine schon wartende Kachel nach vorn.
    fn request(
        &mut self,
        key: String,
        request: PreviewRequest,
        exiftool: &str,
        priority: Priority,
    ) {
        if let Some(pending) = self.pending.get_mut(&key) {
            if priority == Priority::Hover && pending.priority == Priority::Tile {
                pending.priority = Priority::Hover;
                self.queue.promote(&key);
            }
            self.queue.touch(&key);
            return;
        }
        let path = request.path.clone();
        if self
            .queue
            .push(key.clone(), (request, exiftool.to_string()), priority)
        {
            self.pending.insert(
                key,
                Pending {
                    since: std::time::Instant::now(),
                    priority,
                    path,
                },
            );
        }
    }
    pub fn clear(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.queue.clear();
        self.cache.clear();
        self.pending.clear();
        self.bytes = 0;
    }
    pub fn ready(&self, request: &PreviewRequest) -> bool {
        self.cache
            .get(&cache_key(request))
            .is_some_and(|entry| entry.texture.is_some())
    }
    /// Übernimmt fertige Vorschauen; liefert die Zahl hochgeladener Texturen.
    pub fn poll(&mut self, ctx: &egui::Context) -> usize {
        let mut uploaded = 0;
        self.queue.set_frame(ctx.cumulative_frame_nr());
        for key in self.queue.take_dropped() {
            self.pending.remove(&key);
        }
        while let Ok((key, generation, decode, result)) = self.results.try_recv() {
            let pending = self.pending.remove(&key);
            if generation != self.generation.load(Ordering::Relaxed) {
                continue;
            }
            if let Some(pending) = pending.filter(|p| p.priority == Priority::Hover)
                && crate::core::diag::enabled()
            {
                log_hover(
                    &pending.path,
                    pending.since.elapsed(),
                    decode,
                    result.is_ok(),
                );
            }
            let (texture, bytes, error) = match result {
                Ok(p) => {
                    let bytes = p.rgba.len();
                    uploaded += 1;
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
        uploaded
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
        self.request(key, request, exiftool, Priority::Tile);
        None
    }
    pub fn image(
        &mut self,
        ui: &mut egui::Ui,
        request: PreviewRequest,
        exiftool: &str,
        size: egui::Vec2,
    ) -> egui::Response {
        self.image_with(ui, request, exiftool, size, Priority::Tile)
            .0
    }
    /// Wie [`Self::image`] für das Foto unter der Maus: geht allen wartenden Kacheln vor und
    /// ersetzt eine noch wartende frühere Mausanfrage. Liefert zusätzlich die Fläche, auf
    /// der das Bild tatsächlich gezeichnet wurde (`None`, solange es lädt oder fehlt).
    pub fn hover_image(
        &mut self,
        ui: &mut egui::Ui,
        request: PreviewRequest,
        exiftool: &str,
        size: egui::Vec2,
    ) -> Option<egui::Rect> {
        self.image_with(ui, request, exiftool, size, Priority::Hover)
            .1
    }
    fn image_with(
        &mut self,
        ui: &mut egui::Ui,
        mut request: PreviewRequest,
        exiftool: &str,
        size: egui::Vec2,
        priority: Priority,
    ) -> (egui::Response, Option<egui::Rect>) {
        let mut drawn = None;
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
                    let dest = fit(rect, texture.size_vec2());
                    response.rect = dest;
                    drawn = Some(dest);
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
                    return (response.on_hover_text(error.as_str()), None);
                }
            } else {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Lädt …",
                    egui::FontId::proportional(12.0),
                    egui::Color32::GRAY,
                );
                self.request(key, request, exiftool, priority);
            }
        }
        (response, drawn)
    }
}

/// Fläche eines Bilds der Größe `image`, unverzerrt und mittig in `rect` eingepasst.
pub fn fit(rect: egui::Rect, image: egui::Vec2) -> egui::Rect {
    let scale = (rect.width() / image.x).min(rect.height() / image.y);
    egui::Rect::from_center_size(rect.center(), image * scale)
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
