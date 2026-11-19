use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PAGE_SIZE: usize = 48;
/// Zulässige Breite des Ordnerbaums (Minimum, Maximum) in Punkten.
pub const FOLDER_TREE_WIDTH: (f32, f32) = (170.0, 380.0);
pub const COPY_CHUNK: usize = 1024 * 1024;
/// Gleichzeitige Läufe des Adobe DNG Converters bei einem Import (er nutzt je Datei kaum
/// mehr als einen Kern).
pub const DNG_PARALLEL: usize = 3;
/// Dateien je ExifTool-Aufruf beim ausdrücklichen Metadatenlauf.
pub const METADATA_CHUNK: usize = 128;
/// Dateien je ExifTool-Aufruf im Hintergrundlauf: klein, damit ein Nutzerauftrag binnen
/// etwa einer Sekunde Vorrang bekommt.
pub const METADATA_BACKGROUND_CHUNK: usize = 32;
/// Dateien je Hintergrundauftrag; danach startet die App den nächsten.
pub const METADATA_BACKGROUND_PORTION: usize = 512;
/// Dateien je Transaktion beim Indizieren (ADR 0064).
pub const SCAN_BATCH: usize = 500;
/// Dateien je Transaktion beim Speichern neuer BLAKE3-Werte.
pub const HASH_PORTION: usize = 100;
pub const CACHE_BYTES: usize = 192 * 1024 * 1024;
pub const CACHE_ENTRIES: usize = 512;
/// Höchstzahl paralleler Vorschau-Arbeiter (ADR 0068); weniger bei weniger Kernen.
pub const THUMBNAIL_WORKERS: usize = 4;
/// Höchstzahl Gesichter einer Gruppe; breite Fenster zeigen sie in zwei Reihen.
pub const GROUP_MAX: usize = 28;
/// So viele Gesichter einer Gruppe fließen in ihren gemeinsamen Namensvorschlag ein.
/// Gemessen am 2026-11-16 mit `face_save_bench` (synthetisch, 200.000 Gesichter), Seite
/// „Benennen“ nach dem Speichern: 3 Gesichter 139 ms, 5 Gesichter 153 ms, 8 Gesichter 172 ms.
pub const GROUP_SUGGESTION_FACES: usize = 5;
/// Namensvorschläge im Abgleich: Mindestähnlichkeit, Mindestabstand zur zweitbesten Person
/// und Grenze für „sicher“. Gemessen am 2026-11-15 (SFace, eine Sammlung): bei 0,60 mit
/// Abstand 0,05 erreichen 88 % der richtigen Personen den Vorschlag, eine unbekannte Person
/// wird in 10 % der Fälle fälschlich zugeordnet; bei 0,70 in 1 %. Siehe
/// `examples/face_eval.rs`.
pub const FACE_REVIEW_MIN: f32 = 0.60;
pub const FACE_REVIEW_MARGIN: f32 = 0.05;
pub const FACE_SURE_MIN: f32 = 0.70;
/// Obergrenze für den Speicher des inkrementellen Abgleichs (Vektoren und Personenwerte je
/// Gesicht, `recognition::MatchCache`). 100.000 Gesichter mit 128 Dimensionen und 40 Personen
/// brauchen rund 70 MB; darüber hinaus fällt der Abgleich auf den vollständigen Lauf zurück.
pub const MATCH_CACHE_MAX_BYTES: usize = 768 * 1024 * 1024;
/// Personenseite, „Wahrscheinlich X“ (`recognition::person_candidates`): Mindest-Personenscore
/// eines unbestätigten Gesichts, um in der Liste zu stehen. Keine Entscheidungsschwelle (die
/// Liste ist nach Wert geordnet, der Nutzer entscheidet), nur eine Begrenzung der Länge; nicht
/// gemessen. Zum Vergleich (Messung 2026-11-15, ADR Gesichtserkennung): 0,363 erreichen 99,7 %
/// der richtigen Personen, 0,50 noch 98,1 %.
pub const PERSON_CANDIDATE_MIN: f32 = 0.40;
/// Höchstens so viele Threads rechnen im Abgleich Personenwerte (auf dem Wartungs-Worker).
pub const MATCH_THREADS: usize = 4;
/// Anzeigedauer eines normalen Hinweises; Fehler bleiben bis zum Schließen.
pub const NOTICE_SECONDS: std::time::Duration = std::time::Duration::from_secs(15);
/// Gruppen je Seite in den Gesichtsreitern.
pub const FACE_GROUPS_PER_PAGE: usize = 8;
/// Zeitreise (ADR 0069): kürzere Gesichtsseite im Quellbild in Pixeln (kleinster, Standard,
/// größter Wert des Reglers).
pub const TIMELINE_MIN_PX: (u32, u32, u32) = (96, 160, 320);
/// Zeitreise: Mindestgüte, Mindest-Erkennungswert und Mindest-Frontalität eines Gesichts.
pub const TIMELINE_MIN_QUALITY: f32 = 0.5;
pub const TIMELINE_MIN_DETECTION: f32 = 0.8;
pub const TIMELINE_MIN_FRONTAL: f32 = 0.5;
/// Personengraph: Anziehung einer Kante für das Layout (`relationship_graph::layout_weight`).
/// Gemeinsame Fotos zählen `1 + ln(Fotos)`; jede eingetragene Beziehung gibt diesen Zuschlag.
pub const GRAPH_RECORDED_BONUS: f32 = 1.5;
/// Faktor für Eltern/Kind und Partner: rücken unabhängig von der Fotozahl deutlich näher
/// zusammen. Der Sollabstand wächst etwa mit Faktor^(-1/3) (Fruchterman-Reingold).
pub const GRAPH_CLOSE_FAMILY: f32 = 2.5;
/// Faktor für Geschwister (eingetragen oder aus gemeinsamen Eltern) und Cousins (Behelf):
/// mäßig näher.
pub const GRAPH_RELATIVES: f32 = 1.6;
/// Faktor für Freunde und Kanten nur aus gemeinsamen Fotos: neutral.
pub const GRAPH_NEUTRAL: f32 = 1.0;
/// Faktor für „Bekannter von“ und frühere Partner: rücken unabhängig von der Fotozahl
/// weiter auseinander.
pub const GRAPH_DISTANT: f32 = 0.35;
/// Personendetails: so viele Personen nennt „Häufig zusammen mit“ (meiste gemeinsame Fotos).
/// Höchstens `relationship_graph::NEIGHBOR_LIMIT`.
pub const PERSON_COMPANIONS: usize = 8;
/// Personendetails, „Fotos je Jahr“: plausible Aufnahmejahre. Andere Werte (z. B. 0000 aus
/// nicht gestellten Kamerauhren) zählen als „ohne Datum“, damit das Diagramm keine
/// Jahrhunderte überspannt.
pub const PERSON_YEARS: std::ops::RangeInclusive<i32> = 1826..=2200;
/// Rückfrage „Möglicherweise dieselbe Person“ (`duplicates`): Mindestähnlichkeit zweier
/// Personen, gemessen als Median der Mittel der drei besten Kosinuswerte je Referenzgesicht
/// gegen die andere Person (beide Richtungen). Gemessen am 2026-11-18 (SFace, eine Sammlung,
/// 42 Personen mit 1916 Referenzen, nur lesend): verschiedene Personen ohne gemeinsames Foto
/// höchstens 0,53; dieselbe Person, Referenzen in zwei Hälften geteilt, im Median 0,72 und zu
/// 90 % ab 0,60; drei Gesichter gegen den Rest derselben Person im Median 0,54. Zwei Paare über
/// 0,55 (0,69 und 0,58) standen gemeinsam auf Fotos und sind deshalb ausgeschlossen.
pub const DUPLICATE_PERSON_MIN: f32 = 0.55;
/// Höchstzahl gleichzeitig gezeigter Rückfragen „Möglicherweise dieselbe Person“.
pub const DUPLICATE_PERSON_MAX: usize = 20;
/// Diagnose-Log (`core::diag`): Größe einer Logdatei und Grenze aller Logdateien zusammen;
/// darüber wird die älteste Datei gelöscht (10 Dateien zu 30 MB).
pub const DIAG_FILE_BYTES: u64 = 30 * 1024 * 1024;
pub const DIAG_TOTAL_BYTES: u64 = 300 * 1024 * 1024;
/// Zeilen, die auf den Schreib-Thread warten dürfen; darüber gehen Zeilen verloren (gezählt),
/// statt die Oberfläche zu blockieren.
pub const DIAG_QUEUE: usize = 8192;
/// Frames ab dieser Dauer (Aufbau, Tessellierung, Zeichnen) stehen einzeln im Log. 50 ms
/// sind drei verpasste Bilder bei 60 Hz und als Ruckeln sichtbar.
pub const DIAG_SLOW_FRAME_MS: f32 = 50.0;
/// Abstand der Frame-Zusammenfassungen, solange die Oberfläche zeichnet.
pub const DIAG_FRAME_SUMMARY: std::time::Duration = std::time::Duration::from_secs(10);
/// Vorschauen, deren Dekodierung länger dauert, stehen einzeln im Log (sonst 100 bis 300 ms).
pub const DIAG_SLOW_PREVIEW_MS: u128 = 1000;
/// Öffnen einer Sammlungsdatenbank ab dieser Dauer steht im Log (sonst wenige Millisekunden).
pub const DIAG_SLOW_DB_OPEN_MS: u128 = 200;
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
    /// Reglerstellung für die Kachelgröße der Gesichtsgruppen (0 bis 1, Mitte = Standard).
    pub face_tile_scale: f32,
    /// Fotos je Analyse- oder Metadatenlauf, neueste zuerst; 0 bedeutet alle (ADR 0059).
    pub work_portion: usize,
    /// Läufe nur für den in der Sammlung gewählten Ordner ausführen.
    pub work_in_folder: bool,
    /// Ordnerbaum links neben der Galerie sichtbar.
    pub folder_tree_open: bool,
    /// Breite des Ordnerbaums in Punkten, begrenzt auf [`FOLDER_TREE_WIDTH`].
    pub folder_tree_width: f32,
    /// Zentralen Personenbestand verwenden (ADR 0062).
    pub central_enabled: bool,
    /// Ort der Zentraldatei; leer bedeutet `central.sqlite` neben der Einstellungsdatei.
    pub central_path: String,
    /// Opt-in: Referenzausschnitte zentral speichern (Biometrie, Standard aus).
    pub central_images: bool,
    /// Beziehungen zentral mitführen (ADR 0065); wirkt nur bei `central_enabled`.
    pub central_relations: bool,
    /// Zeitreise: Mindestauflösung eines Gesichts in Pixeln (kürzere Seite).
    pub timeline_min_px: u32,
    pub window_size: [f32; 2],
    pub dng_update: crate::core::dng_updates::UpdateCache,
    /// Sammlungsordner, in die importiert wurde und deren Index die neuen Dateien noch nicht
    /// kennt. Bleibt über das Programmende hinaus erhalten (ADR Import).
    pub index_pending: Vec<String>,
    /// Diagnose-Log mit Frame- und Auftragszeiten und Fehlern schreiben (`core::diag`).
    pub diagnostic_log: bool,
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
            face_tile_scale: 0.5,
            work_portion: 500,
            work_in_folder: false,
            folder_tree_open: true,
            folder_tree_width: 230.0,
            central_enabled: true,
            central_path: String::new(),
            central_images: false,
            central_relations: true,
            timeline_min_px: TIMELINE_MIN_PX.1,
            window_size: [1280.0, 800.0],
            dng_update: Default::default(),
            index_pending: Vec::new(),
            diagnostic_log: true,
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
        // Tests schreiben nie in die echten Arbeitsplatzeinstellungen; jeder Testthread hat
        // eine eigene Datei, damit parallele Tests sich nicht überschreiben.
        #[cfg(test)]
        if std::env::var_os("FOTO_IMPORTER_SETTINGS").is_none() {
            let thread = format!("{:?}", std::thread::current().id());
            return std::env::temp_dir()
                .join(format!("foto-importer-tests-{}", std::process::id()))
                .join(thread.trim_start_matches("ThreadId(").trim_end_matches(')'))
                .join("settings.json");
        }
        std::env::var_os("FOTO_IMPORTER_SETTINGS")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(std::env::var_os("APPDATA").unwrap_or_else(|| ".".into()))
                    .join("FotoImporter")
                    .join("settings.json")
            })
    }
    /// Standardort der Zentraldatei im Benutzerprofil.
    pub fn default_central_path() -> PathBuf {
        Self::path().with_file_name("central.sqlite")
    }
    pub fn central(&self) -> crate::core::central::CentralConfig {
        crate::core::central::CentralConfig {
            enabled: self.central_enabled,
            path: if self.central_path.trim().is_empty() {
                Self::default_central_path()
            } else {
                PathBuf::from(self.central_path.trim())
            },
            images: self.central_images,
            relations: self.central_relations,
        }
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    /// Ordner des Diagnose-Logs neben der Einstellungsdatei, nie in einer Sammlung.
    pub fn log_dir() -> PathBuf {
        Self::path().with_file_name("logs")
    }
    pub fn save(&self) -> anyhow::Result<()> {
        self.write().inspect_err(|e| {
            log::warn!(target: "einstellungen", "speichern fehlgeschlagen fehler={:?}", format!("{e:#}"));
        })
    }
    fn write(&self) -> anyhow::Result<()> {
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
