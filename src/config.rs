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
/// Lokaler Vorschau-Cache (`core::thumb_cache`) im Cache-Ordner des Arbeitsplatzes: nur ganze
/// Vorschauen dieser Kantenlängen (Galerie 400, Bildinspektor 960).
pub const THUMB_CACHE_EDGES: &[u32] = &[400, 960];
/// Obergrenze aller Einträge zusammen; eine Galerievorschau hat rund 25 KB, eine des
/// Inspektors rund 120 KB. Darüber werden die am längsten unbenutzten gelöscht.
pub const THUMB_CACHE_BYTES: u64 = 1024 * 1024 * 1024;
/// JPEG-Qualität der gespeicherten Vorschauen.
pub const THUMB_CACHE_QUALITY: u8 = 85;
/// Nach so vielen neuen Einträgen räumt der Cache auf (und einmal beim Start).
pub const THUMB_CACHE_TRIM_EVERY: usize = 256;
/// Ein Treffer frischt die Änderungszeit eines Eintrags höchstens so oft auf.
pub const THUMB_CACHE_TOUCH: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
/// Lokale Ableitungen (`core::local_cache`: Startansicht, Personenliste, Personengraph);
/// größere Dateien werden ignoriert (Startansicht mit 48 Fotos rund 20 KB, 56 Personen mit
/// Graph rund 60 KB).
pub const LOCAL_CACHE_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// Höchstzahl paralleler Vorschau-Arbeiter (ADR 0068); weniger bei weniger Kernen.
pub const THUMBNAIL_WORKERS: usize = 4;
/// Zusätzliche Vorschau-Arbeiter nur für das Foto unter der Maus: Es wartet nie hinter einer
/// laufenden Kachelrunde (eine DNG-Vorschau kostet auf der USB-Platte 1,5 bis 2 s).
pub const THUMBNAIL_HOVER_WORKERS: usize = 1;
/// Wartende Vorschauaufträge, die so viele Frames nicht mehr gezeichnet wurden (weggescrollt,
/// Maus weiter), werden verworfen.
pub const THUMBNAIL_STALE_FRAMES: u64 = 2;
/// Kantenlänge des Fotos unter der Maus; dieselbe wie im Bildinspektor, damit der lokale
/// Vorschau-Cache (`THUMB_CACHE_EDGES`) greift.
pub const HOVER_PREVIEW_EDGE: u32 = 960;
/// Höchstzahl Mitglieder einer gespeicherten Ähnlichkeitsgruppe unbenannter Gesichter
/// (`faces::prepare_groups`, `grouping`). Das ist Gruppenbildung, keine Anzeigegröße: Eine
/// Änderung gruppiert bestehende Sammlungen erst beim nächsten Neuaufbau anders.
pub const CLUSTER_MAX: usize = 28;
/// Anzeigegröße der Gesichtergruppen (Regler „Gruppengröße“, `Settings::face_group_size`):
/// kleinster, größter und voreingestellter Wert. Die Grenzen sind nicht gemessen; die
/// Obergrenze ist zugleich die Höchstzahl Gesichter einer Gruppenentscheidung
/// (`faces::name_group`).
pub const GROUP_SIZE_MIN: usize = 5;
pub const GROUP_SIZE_MAX: usize = 100;
pub const GROUP_SIZE_DEFAULT: usize = 28;
// Eine gespeicherte Gruppe passt immer in eine Entscheidung (`faces::name_group`).
const _: () = assert!(CLUSTER_MAX <= GROUP_SIZE_MAX);
/// Anzeigegröße innerhalb der Grenzen (fehlerhafte oder alte Einstellungen).
pub fn group_size(requested: usize) -> usize {
    requested.clamp(GROUP_SIZE_MIN, GROUP_SIZE_MAX)
}
/// Gruppenbildung (`faces::prepare_groups`): Ein Gesicht tritt einer Gruppe nur bei, wenn es
/// mit allen Mitgliedern mindestens diesen Kosinuswert erreicht (Complete-Link).
pub const GROUP_JOIN_MIN: f32 = 0.6;
/// Nachlauf `grouping::merge_small_groups`: Gruppen bis zu dieser Größe gelten als klein und
/// werden an die ähnlichste größere Gruppe gehängt.
pub const GROUP_SMALL: usize = 2;
/// Nötige mittlere Ähnlichkeit fürs Anhängen (je Modell überschreibbar,
/// `rust.thresholds.<Modell>.group_merge`); kein Paar darf unter `GROUP_MERGE_MIN_PAIR` liegen.
pub const GROUP_MERGE_MEAN: f32 = 0.5;
pub const GROUP_MERGE_MIN_PAIR: f32 = 0.3;
/// So viele Gruppen mit dem ähnlichsten Mittelpunkt werden paarweise geprüft.
pub const GROUP_MERGE_CANDIDATES: usize = 8;
/// Mindestgüte eines Gesichts für Merkmale, Referenzen, Abgleich und Gruppen. SQL-Texte nennen
/// den Wert wörtlich (`quality>=0.5`, `quality<0.5`); ein Test prüft jeden gegen diese Konstante.
pub const FACE_QUALITY_MIN: f32 = 0.5;
/// Längere Bildkante für die Nachberechnung kleiner, klar erkannter Gesichter.
pub const FACE_HIRES_EDGE: u32 = 3200;
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
/// Referenzauswahl (`recognition`, `matching::select_references`): Je Person werden
/// `reference_limit(n)` der `n` bestätigten Gesichter ausgewählt, eine sättigende Funktion ohne
/// Untergrenze: `ceil(S·(1−exp(−n/S)))`, höchstens `n`, mit `S = REFERENCE_SATURATION`. Für
/// kleine `n` sind das fast alle Gesichter (30 → 30, 100 → 95), für häufige Personen gegen
/// `S` (500 → 372, 1000 → 571, 5000 → 799). Gemessen am 2026-11-21 mit
/// `examples/reference_limit_eval.rs` (SFace, eine Sammlung, 17.023 bestätigte Gesichter,
/// 50 Personen, feste Testmenge 2.931 Gesichter): Personen über 500 Gesichtern erreichen 0,60
/// mit Abstand in 90 % statt 63 % (fest 100), Verwechslungen sinken von 5,4 % auf 0,1 %. Mehr
/// als 800 (bis „alle“) brachte dort kaum mehr, aber mehr Verwechslungen bei mittleren
/// Personen (1,4 % statt 0,5 %).
pub const REFERENCE_SATURATION: f32 = 800.0;
/// Höchstzahl Kandidaten je Person (Pins kommen hinzu), je Jahr nach Qualität im Wechsel.
/// Ohne diese Grenze war die Auswahl bei 800 schlechter (bei 0,70: 76 % statt 78 % der
/// richtigen Person über 500 Gesichtern), weil die Jahresverteilung verloren geht.
pub const REFERENCE_CANDIDATES_MAX: usize = 2048;
/// Validierung (nur für „Validierung auswerten“, nie im Abgleich): 20 % der Auswahl, höchstens
/// `REFERENCE_VALIDATION_MAX`; unter `REFERENCE_VALIDATION_FROM` Ausgewählten keine, damit
/// seltene Personen alle Gesichter als Referenz nutzen.
pub const REFERENCE_VALIDATION_MAX: usize = 40;
pub const REFERENCE_VALIDATION_FROM: usize = 50;
/// Kennung der geltenden Auswahlregel. Ändert sich Grenze oder Validierung, wird sie erhöht:
/// Beim nächsten Öffnen werden alle Personen neu ausgewählt
/// (`recognition::renew_outdated_references`).
pub const REFERENCE_POLICY: &str = "saturating-800-v1";
/// Wie viele der `confirmed` bestätigten Gesichter einer Person ausgewählt werden.
pub fn reference_limit(confirmed: usize) -> usize {
    let max = REFERENCE_SATURATION;
    ((max * (1.0 - (-(confirmed as f32) / max).exp())).ceil() as usize).min(confirmed)
}
/// Wie viele von `chosen` ausgewählten Gesichtern als Validierung zurückgehalten werden.
pub fn reference_validation(chosen: usize) -> usize {
    if chosen < REFERENCE_VALIDATION_FROM {
        0
    } else {
        ((chosen as f32 * 0.2).round() as usize).min(REFERENCE_VALIDATION_MAX)
    }
}
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
/// Personenstatistik (`core::person_stats`, nur Anzeige): Wertebereich der Achse. Werte
/// außerhalb zählen in Anzahl und Kennzahlen mit, gezeichnet werden sie am Rand.
pub const PERSON_STATS_AXIS: (f32, f32) = (0.3, 1.0);
/// Stützstellen der Kerndichte über die Achse.
pub const PERSON_STATS_GRID: usize = 141;
/// Unter so vielen Werten zeigt die Statistik Punkte statt einer Violine (zu wenig für eine
/// Dichte).
pub const PERSON_STATS_MIN_VIOLIN: usize = 5;
/// Höchstens so viele Threads rechnen im Abgleich Personenwerte (auf dem Wartungs-Worker).
pub const MATCH_THREADS: usize = 4;
/// So lange gilt die Prüfung „Laufzeit und Modellgewichte vorhanden“ der Ansicht „Gesichter“,
/// bevor sie die Dateien erneut ansieht (sonst nur bei geänderten Einstellungen oder nach
/// Installation/Download); vorher lief sie in jedem Frame.
pub const SETUP_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// Anzeigedauer eines normalen Hinweises; Fehler bleiben bis zum Schließen.
pub const NOTICE_SECONDS: std::time::Duration = std::time::Duration::from_secs(15);
/// Höchstens so viele Gruppen je Seite in den Gesichtsreitern.
pub const FACE_GROUPS_PER_PAGE: usize = 8;
/// Höchstens so viele Gesichter je Seite (Kacheln, Ablehnungen, Ladezeit): acht Gruppen der
/// voreingestellten Größe. Große Gruppen füllen eine Seite daher mit weniger Gruppen.
pub const FACE_PAGE_FACES: usize = FACE_GROUPS_PER_PAGE * GROUP_SIZE_DEFAULT;
/// Gruppen je Seite bei der Anzeigegröße `group_size`: so viele, wie in
/// [`FACE_PAGE_FACES`] passen, mindestens eine, höchstens [`FACE_GROUPS_PER_PAGE`]
/// (5 → 8, 28 → 8, 50 → 4, 100 → 2).
pub fn face_groups_per_page(group_size: usize) -> usize {
    (FACE_PAGE_FACES / self::group_size(group_size)).clamp(1, FACE_GROUPS_PER_PAGE)
}
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
/// Zeitreise: so viele zuletzt gelesene Personen bleiben im Speicher, damit der Wechsel zurück
/// ohne Datenbank geht (je Person höchstens einige tausend Gesichter, rund 1 MB).
pub const TIMELINE_CACHED_PEOPLE: usize = 12;
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
/// Höchstzahl gleichzeitig angezeigter Rückfragen zu Lücken im Stammbaum (`gaps`).
pub const GAPS_MAX_SUGGESTIONS: usize = 30;
/// Verwandtschaft (`kinship`): Höchstzahl Generationen je Richtung (aufwärts zum gemeinsamen
/// Vorfahren und abwärts). Cousins bis zum 7. Grades sind damit auffindbar.
pub const KINSHIP_MAX_GENERATIONS: u8 = 8;
/// Höchstzahl Einträge je Gruppe in `Family::relatives_of`; der Rest steht in `more`.
pub const KINSHIP_MAX_PER_GROUP: usize = 100;
/// Verschwägerung wird nur über nahe Blutsverwandte berechnet (Summe der Schritte).
pub const KINSHIP_AFFINITY_MAX_STEPS: u8 = 4;
/// Zentraler Personenbestand: Obergrenzen für Referenzausschnitte (klein halten, Datenschutz).
pub const CENTRAL_MAX_REFERENCE_BYTES: usize = 128 * 1024;
pub const CENTRAL_MAX_REFERENCES_PER_PERSON: i64 = 64;
/// Fortschritt (`progress`): Messpunkte näher beieinander werden zusammengefasst, damit die
/// Schätzung ruhig bleibt.
pub const PROGRESS_SAMPLE_GAP: std::time::Duration = std::time::Duration::from_millis(500);
/// Messpunkte, bevor eine Restzeit gezeigt wird; vorher „wird geschätzt“.
pub const PROGRESS_SAMPLES_FOR_ESTIMATE: usize = 4;
/// Gewicht des neuesten Messpunkts im gleitenden Mittel.
pub const PROGRESS_SMOOTHING: f64 = 0.3;
/// Diagnose-Log (`core::diag`): Größe einer Logdatei und Grenze aller Logdateien zusammen;
/// darüber wird die älteste Datei gelöscht (10 Dateien zu 30 MB).
pub const DIAG_FILE_BYTES: u64 = 30 * 1024 * 1024;
pub const DIAG_TOTAL_BYTES: u64 = 300 * 1024 * 1024;
/// Zeilen, die auf den Schreib-Thread warten dürfen; darüber gehen Zeilen verloren (gezählt),
/// statt die Oberfläche zu blockieren.
pub const DIAG_QUEUE: usize = 8192;
/// Höchstabstand, in dem der Schreib-Thread Geschriebenes bis auf die Platte durchschreibt
/// (`sync_data`), damit auch ein Stromausfall höchstens so viel kostet. An das Betriebssystem
/// übergeben wird jede Zeile sofort nach dem Leeren der Warteschlange; Fehler und Paniken
/// werden sofort durchgeschrieben.
pub const DIAG_SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
/// So lange wartet der Panik-Haken höchstens, bis die Panikzeile auf der Platte steht.
pub const DIAG_PANIC_FLUSH: std::time::Duration = std::time::Duration::from_secs(2);
/// Frames ab dieser Dauer (Aufbau, Tessellierung, Zeichnen) stehen einzeln im Log. 50 ms
/// sind drei verpasste Bilder bei 60 Hz und als Ruckeln sichtbar.
pub const DIAG_SLOW_FRAME_MS: f32 = 50.0;
/// Abstand der Frame-Zusammenfassungen, solange die Oberfläche zeichnet.
pub const DIAG_FRAME_SUMMARY: std::time::Duration = std::time::Duration::from_secs(10);
/// Vorschauen, deren Dekodierung länger dauert, stehen einzeln im Log (sonst 100 bis 300 ms).
pub const DIAG_SLOW_PREVIEW_MS: u128 = 1000;
/// Öffnen einer Sammlungsdatenbank ab dieser Dauer steht im Log (sonst wenige Millisekunden).
pub const DIAG_SLOW_DB_OPEN_MS: u128 = 200;
/// Datensicherung (`core::backup`): Dateiname im Sammlungsordner ist dieser Anfang plus eine
/// mindestens dreistellige Nummer (`.myimporter.backup.001`). Nur Dateien genau dieses Musters
/// gelten als Sicherung und werden beim Aufräumen gelöscht.
pub const BACKUP_PREFIX: &str = ".myimporter.backup.";
pub const BACKUP_DIGITS: usize = 3;
/// Anfang der Zwischendateien und -ordner einer laufenden Sicherung im Sammlungsordner.
pub const BACKUP_TEMP_PREFIX: &str = ".myimporter.backup.tmp-";
/// Zwischenstände eines abgestürzten Laufs, die älter sind, werden beim nächsten Lauf entfernt.
pub const BACKUP_STALE_TEMP: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
/// So viele Sicherungen bleiben standardmäßig liegen (Einstellung „Sicherungen behalten“) und
/// zulässiger Bereich der Einstellung.
pub const BACKUP_KEEP: usize = 5;
pub const BACKUP_KEEP_RANGE: (usize, usize) = (1, 99);
/// Deflate-Stufe der Sicherungs-ZIPs (0 bis 9). Gemessen am 2026-11-20 mit
/// `examples/backup_bench.rs` (synthetische Sammlungsdatenbank 804 MB, lokale SSD, Release):
/// Stufe 1 packt in 7,1 s auf 480 MB, Stufe 6 in 16,8 s auf 416 MB, Stufe 9 in 23,9 s auf
/// 395 MB. Gesichtsvektoren lassen sich kaum packen; 13 % mehr Platz für die halbe Zeit.
pub const BACKUP_COMPRESSION_LEVEL: i64 = 1;
/// Seiten je Schritt der SQLite-Online-Sicherung; dazwischen Fortschritt und Abbruch.
pub const BACKUP_STEP_PAGES: i32 = 2048;
/// Zuschlag auf den geschätzten Platzbedarf einer Sicherung.
pub const BACKUP_SPACE_MARGIN: u64 = 64 * 1024 * 1024;
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
    /// Symbole oben rechts auf den Gruppenkacheln (Kreuz, Haken, Fragezeichen) zeigen.
    pub tile_icons: bool,
    /// Reglerstellung für die Kachelgröße der Gesichtsgruppen (0 bis 1, Mitte = Standard).
    pub face_tile_scale: f32,
    /// Anzeigegröße der Gesichtergruppen (Regler „Gruppengröße“), begrenzt durch
    /// [`group_size`]; fehlt der Eintrag, gilt [`GROUP_SIZE_DEFAULT`].
    pub face_group_size: usize,
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
    /// Datensicherung: so viele `.myimporter.backup.NNN` bleiben im Sammlungsordner liegen.
    pub backup_keep: usize,
    /// Beim ersten Öffnen einer Sammlung an einem Tag automatisch sichern (niedrige Priorität).
    pub daily_backup: bool,
    /// Datum (`JJJJ-MM-TT`) der letzten täglichen Sicherung je Sammlungspfad.
    pub daily_backup_dates: std::collections::BTreeMap<String, String>,
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
            tile_icons: true,
            face_tile_scale: 0.5,
            face_group_size: GROUP_SIZE_DEFAULT,
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
            backup_keep: BACKUP_KEEP,
            daily_backup: true,
            daily_backup_dates: Default::default(),
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
    /// Arbeitsplatzdateien, die zu einer Datensicherung gehören: diese Einstellungsdatei und
    /// die Zentraldatei am eingestellten Ort (auch wenn der Abgleich gerade aus ist; fehlt
    /// sie, steht das im Manifest).
    pub fn backup_app_data(&self) -> crate::core::backup::AppData {
        crate::core::backup::AppData {
            settings: Some(Self::path()),
            central: Some(self.central().path),
        }
    }
    pub fn load() -> Self {
        std::fs::read(Self::path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    /// Lokaler Cache-Ordner (Startansicht, Vorschauen), nie in einer Sammlung:
    /// `%LOCALAPPDATA%\FotoImporter\cache`, weil er groß werden darf und nicht mit dem Profil
    /// wandern soll. Mit eigener Einstellungsdatei (Tests, `FOTO_IMPORTER_SETTINGS`) liegt er
    /// neben ihr; `FOTO_IMPORTER_CACHE` überschreibt den Ort.
    pub fn cache_dir() -> PathBuf {
        if let Some(dir) = std::env::var_os("FOTO_IMPORTER_CACHE") {
            return PathBuf::from(dir);
        }
        if cfg!(test) || std::env::var_os("FOTO_IMPORTER_SETTINGS").is_some() {
            return Self::path().with_file_name("cache");
        }
        std::env::var_os("LOCALAPPDATA")
            .map(|dir| PathBuf::from(dir).join("FotoImporter").join("cache"))
            .unwrap_or_else(|| Self::path().with_file_name("cache"))
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
        let bytes = serde_json::to_vec_pretty(self)?;
        crate::core::files::write_atomic(&path, &bytes, crate::core::files::Mode::Replace, true)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /// SQL-Texte nennen die Mindestgüte wörtlich (Vergleich `>=` oder `<` direkt nach
    /// `quality`); jeder solche Wert in `src/` muss `FACE_QUALITY_MIN` sein.
    #[test]
    fn sql_quality_literals_match_the_constant() {
        let expected = super::FACE_QUALITY_MIN.to_string();
        let mut found = 0;
        let mut stack = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(path) = stack.pop() {
            if path.is_dir() {
                for entry in std::fs::read_dir(&path).expect("Ordner") {
                    stack.push(entry.expect("Eintrag").path());
                }
                continue;
            }
            if !path.extension().is_some_and(|e| e == "rs" || e == "sql") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("Quelltext");
            for (at, word) in text.match_indices("quality") {
                let rest = text[at + word.len()..].trim_start();
                let Some(rest) = rest.strip_prefix(">=").or_else(|| rest.strip_prefix('<')) else {
                    continue;
                };
                let number: String = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '.')
                    .collect();
                if number.is_empty() {
                    continue;
                }
                assert_eq!(number, expected, "{} bei Byte {at}", path.display());
                found += 1;
            }
        }
        // 12 SQL-Stellen und die Doku der Konstante.
        assert!(found >= 13, "nur {found} Stellen gefunden");
    }
    #[test]
    fn group_size_is_clamped_and_pages_hold_fewer_large_groups() {
        use super::*;
        assert_eq!(group_size(0), GROUP_SIZE_MIN);
        assert_eq!(group_size(GROUP_SIZE_MAX + 1), GROUP_SIZE_MAX);
        assert_eq!(group_size(GROUP_SIZE_DEFAULT), GROUP_SIZE_DEFAULT);
        assert_eq!(face_groups_per_page(GROUP_SIZE_MIN), FACE_GROUPS_PER_PAGE);
        assert_eq!(
            face_groups_per_page(GROUP_SIZE_DEFAULT),
            FACE_GROUPS_PER_PAGE
        );
        assert_eq!(face_groups_per_page(GROUP_SIZE_MAX), 2);
        for size in GROUP_SIZE_MIN..=GROUP_SIZE_MAX {
            let groups = face_groups_per_page(size);
            assert!(groups >= 1 && groups * size <= FACE_PAGE_FACES.max(size));
        }
    }
    #[test]
    fn group_size_setting_defaults_to_28_and_round_trips() {
        use super::*;
        assert_eq!(Settings::default().face_group_size, GROUP_SIZE_DEFAULT);
        // Ältere Einstellungsdateien ohne Eintrag erhalten die Voreinstellung.
        let old: Settings = serde_json::from_str(r#"{"tile_icons":false}"#).expect("alt");
        assert_eq!(old.face_group_size, GROUP_SIZE_DEFAULT);
        let settings = Settings {
            face_group_size: GROUP_SIZE_MAX,
            ..Settings::default()
        };
        settings.save().expect("speichern");
        assert_eq!(Settings::load().face_group_size, GROUP_SIZE_MAX);
    }
}
