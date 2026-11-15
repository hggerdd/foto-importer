//! Zeitreise: Gesichter einer Person über die Zeit (ADR 0069).
//!
//! Der Kern liest nur. Er fragt die Gesichter einer Person mit Datum, Größe und Güte ab,
//! prüft sie gegen einstellbare Schwellen, bildet Zeitabschnitte (Jahr, Monat, Altersjahr)
//! und wählt je Abschnitt das beste Gesicht. Nutzerentscheidungen (festnageln, ausblenden)
//! liegen in `face_settings`; Datumskorrekturen werden nie in Dateien geschrieben.
use super::{collection::Collection, kinship::Family};
use crate::config;
use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

/// Längere Bildkante, mit der die Gesichtsanalyse rechnet (siehe `faces::analyse`).
const ANALYSIS_EDGE: f32 = 1600.0;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mär", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez",
];

/// Ein Kalendertag; `month` und `day` sind mindestens 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    /// Monatsindex seit Jahr 0 (Januar 0 = 0).
    pub fn months(self) -> i32 {
        self.year * 12 + (self.month as i32 - 1)
    }
    pub fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Aufnahmedatum aus EXIF-Text (`2019:05:03 12:00:00`, `2019-05-03`, auch nur `2019`).
pub fn parse_date(text: &str) -> Option<Date> {
    let mut parts = text
        .trim()
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty());
    let year: i32 = parts.next()?.parse().ok()?;
    if !(1800..=2200).contains(&year) {
        return None;
    }
    let month: u8 = parts.next().and_then(|m| m.parse().ok()).unwrap_or(1);
    let day: u8 = parts.next().and_then(|d| d.parse().ok()).unwrap_or(1);
    Some(Date {
        year,
        month: if (1..=12).contains(&month) { month } else { 1 },
        day: if (1..=31).contains(&day) { day } else { 1 },
    })
}

/// Jahr (und Monat) aus einem Ordnernamen wie `2019`, `2019-05 Urlaub` oder `Alben/2014_08`.
pub fn folder_date(dir: &str) -> Option<Date> {
    for segment in dir.rsplit('/') {
        let digits: String = segment.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.len() != 4 {
            continue;
        }
        let year: i32 = digits.parse().ok()?;
        if !(1900..=2100).contains(&year) {
            continue;
        }
        let rest: Vec<char> = segment.chars().skip(4).collect();
        let month = match rest.as_slice() {
            [separator, a, b, ..]
                if !separator.is_alphanumeric() && a.is_ascii_digit() && b.is_ascii_digit() =>
            {
                format!("{a}{b}")
                    .parse::<u8>()
                    .ok()
                    .filter(|m| (1..=12).contains(m))
            }
            _ => None,
        };
        return Some(Date {
            year,
            month: month.unwrap_or(1),
            day: 1,
        });
    }
    None
}

/// Herkunft und Verlässlichkeit eines Datums; aufsteigend nach Verlässlichkeit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DateQuality {
    Unknown,
    /// Änderungszeit der Datei: oft das Importdatum, unsicher.
    File,
    /// Jahr (und Monat) aus dem Ordnernamen.
    Folder,
    /// Aufnahmedatum aus den Metadaten.
    Exif,
}

impl DateQuality {
    pub fn label(self) -> &'static str {
        match self {
            DateQuality::Exif => "Aufnahmedatum (EXIF)",
            DateQuality::Folder => "aus dem Ordnernamen",
            DateQuality::File => "Dateidatum, unsicher",
            DateQuality::Unknown => "kein Datum",
        }
    }
    /// Ordnername, Dateidatum und Unbekannt gelten in der Ansicht als unsicher.
    pub fn uncertain(self) -> bool {
        self != DateQuality::Exif
    }
}

/// Ein Gesicht mit allem, was die Zeitreise braucht.
#[derive(Clone, Debug)]
pub struct TimeFace {
    pub id: i64,
    pub photo: i64,
    /// Relativer Pfad der Bilddatei, aus der das Gesicht stammt.
    pub source: String,
    /// Rahmen, normiert auf das ausgerichtete Quellbild.
    pub bbox: [f32; 4],
    /// Zuordnung (`user` bestätigt, `auto` sicher automatisch).
    pub assignment: String,
    pub date: Option<Date>,
    pub date_quality: DateQuality,
    /// Kürzere Gesichtsseite im Quellbild in Pixeln; `None`, wenn die Bildmaße fehlen.
    pub px: Option<u32>,
    /// Frontalität 0 bis 1 aus den Landmarks; `None` ohne Landmarks.
    pub frontal: Option<f32>,
    pub detection: f32,
    pub quality: f32,
    /// Augenmitten (links, rechts im Bild), normiert wie `bbox`; `None`, wenn nicht belastbar.
    pub eyes: Option<[[f32; 2]; 2]>,
    /// Maße des ausgerichteten Quellbilds in Pixeln (Breite, Höhe) laut Metadaten.
    pub dims: Option<(f32, f32)>,
    /// Zeitreise-Wert 0 bis 1 (Auflösung, Frontalität, Güte).
    pub value: f32,
}

/// Frontalität aus fünf Landmarks (Augen, Nase, Mundwinkel). Maßstabsfrei, nutzt nur
/// Verhältnisse; 1 ist ein gerades Gesicht, 0 stark gedreht oder gekippt.
pub fn frontality(landmarks: &[[f32; 2]; 5]) -> f32 {
    let (mut a, mut b) = (landmarks[0], landmarks[1]);
    if a[0] > b[0] {
        std::mem::swap(&mut a, &mut b);
    }
    let nose = landmarks[2];
    let mouth = [
        (landmarks[3][0] + landmarks[4][0]) / 2.0,
        (landmarks[3][1] + landmarks[4][1]) / 2.0,
    ];
    let eye = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let distance = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let drop = ((mouth[0] - eye[0]).powi(2) + (mouth[1] - eye[1]).powi(2)).sqrt();
    if distance < 1e-3 || drop < 1e-3 || mouth[1] <= eye[1] {
        return 0.0;
    }
    let roll = (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees().abs();
    let roll_score = 1.0 - roll / 35.0;
    let axis = [(b[0] - a[0]) / distance, (b[1] - a[1]) / distance];
    let offset = ((nose[0] - eye[0]) * axis[0] + (nose[1] - eye[1]) * axis[1]) / distance;
    let yaw_score = 1.0 - offset.abs() / 0.45;
    let ratio_score = 1.0 - (distance / drop).ln().abs() / 1.8_f32.ln();
    roll_score.min(yaw_score).min(ratio_score).clamp(0.0, 1.0)
}

/// Größe der Gesichtsanalyse für ein Bild der Maße `w` x `h` (ohne Hochskalieren).
fn analysis_size(w: f32, h: f32) -> (f32, f32) {
    let scale = (ANALYSIS_EDGE / w.max(h)).min(1.0);
    ((w * scale).round().max(1.0), (h * scale).round().max(1.0))
}

fn eyes_of(landmarks: &[[f32; 2]; 5], w: f32, h: f32, bbox: [f32; 4]) -> Option<[[f32; 2]; 2]> {
    let (aw, ah) = analysis_size(w, h);
    let mut eyes = [landmarks[0], landmarks[1]].map(|p| [p[0] / aw, p[1] / ah]);
    if eyes[0][0] > eyes[1][0] {
        eyes.swap(0, 1);
    }
    // Die Augen müssen im Gesichtsrahmen liegen, sonst stimmen die Bildmaße nicht.
    let (bw, bh) = (bbox[2] - bbox[0], bbox[3] - bbox[1]);
    let inside = eyes.iter().all(|p| {
        p[0] > bbox[0] - 0.25 * bw
            && p[0] < bbox[2] + 0.25 * bw
            && p[1] > bbox[1] - 0.25 * bh
            && p[1] < bbox[3] + 0.25 * bh
    });
    (inside && (eyes[1][0] - eyes[0][0]) > 0.15 * bw).then_some(eyes)
}

fn raw_value(px: Option<u32>, frontal: f32, quality: f32, detection: f32) -> f32 {
    let resolution = px.map_or(0.0, |p| ((p as f32 - 64.0) / 336.0).clamp(0.0, 1.0));
    (0.35 * resolution
        + 0.25 * frontal
        + 0.2 * quality.clamp(0.0, 1.0)
        + 0.2 * detection.clamp(0.0, 1.0))
    .clamp(0.0, 1.0)
}

/// Alle zugeordneten Gesichter einer Person (bestätigt und automatisch), ohne ignorierte,
/// verworfene und von der Person abgelehnte. Rein lesend.
pub fn load(collection: &Collection, person: i64) -> Result<Vec<TimeFace>> {
    let mut statement = collection.db.prepare(
        "SELECT f.id,f.photo_id,f.source_relpath,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2,f.landmarks,
                f.det_score,f.quality,f.assignment,nullif(f.captured_at,''),
                coalesce(mp.dt_original,m.dt_original),m.width,m.height,m.orientation,fi.dir,fi.mtime_ns
         FROM faces f
         LEFT JOIN files fi ON fi.id=f.source_file_id
         LEFT JOIN meta m ON m.file_id=f.source_file_id
         LEFT JOIN metadata_photos mp ON mp.photo_id=f.photo_id
         WHERE f.person_id=? AND f.ignored=0 AND f.retained=0 AND f.assignment IN ('user','auto')
           AND f.bbox_x1 IS NOT NULL AND f.bbox_y1 IS NOT NULL AND f.bbox_x2 IS NOT NULL
           AND f.bbox_y2 IS NOT NULL
           AND NOT EXISTS(SELECT 1 FROM face_rejections r WHERE r.face_id=f.id AND r.person_id=f.person_id)",
    )?;
    let mut rows = statement.query([person])?;
    let mut out = Vec::new();
    while let Some(r) = rows.next()? {
        let bbox: [f32; 4] = [r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?];
        let landmarks: Option<String> = r.get(7)?;
        let landmarks = landmarks.and_then(|t| serde_json::from_str::<[[f32; 2]; 5]>(&t).ok());
        let detection: f32 = r.get::<_, Option<f32>>(8)?.unwrap_or(0.0);
        let quality: f32 = r.get::<_, Option<f32>>(9)?.unwrap_or(0.0);
        let captured: Option<String> = r.get(11)?;
        let exif: Option<String> = r.get(12)?;
        let (width, height, orientation): (Option<f64>, Option<f64>, Option<i64>) =
            (r.get(13)?, r.get(14)?, r.get(15)?);
        let dir: Option<String> = r.get(16)?;
        let mtime: Option<i64> = r.get(17)?;
        let (mut date, mut date_quality) = (None, DateQuality::Unknown);
        if let Some(d) = captured.as_deref().or(exif.as_deref()).and_then(parse_date) {
            (date, date_quality) = (Some(d), DateQuality::Exif);
        } else if let Some(d) = dir.as_deref().and_then(folder_date) {
            (date, date_quality) = (Some(d), DateQuality::Folder);
        } else if let Some(d) = mtime
            .and_then(|ns| chrono::DateTime::from_timestamp(ns.div_euclid(1_000_000_000), 0))
            .and_then(|t| parse_date(&t.format("%Y-%m-%d").to_string()))
        {
            (date, date_quality) = (Some(d), DateQuality::File);
        }
        // Bildmaße im ausgerichteten Bild: bei Drehung um 90 Grad vertauscht.
        let dimensions = match (width, height) {
            (Some(w), Some(h)) if w >= 1.0 && h >= 1.0 => {
                Some(if matches!(orientation, Some(5..=8)) {
                    (h as f32, w as f32)
                } else {
                    (w as f32, h as f32)
                })
            }
            _ => None,
        };
        let px = dimensions.map(|(w, h)| {
            (((bbox[2] - bbox[0]) * w).min((bbox[3] - bbox[1]) * h))
                .max(0.0)
                .round() as u32
        });
        let frontal = landmarks.as_ref().map(frontality);
        let eyes = landmarks
            .as_ref()
            .zip(dimensions)
            .and_then(|(l, (w, h))| eyes_of(l, w, h, bbox));
        out.push(TimeFace {
            id: r.get(0)?,
            photo: r.get(1)?,
            source: r.get(2)?,
            bbox,
            assignment: r.get(10)?,
            date,
            date_quality,
            px,
            frontal,
            detection,
            quality,
            eyes,
            dims: dimensions,
            value: raw_value(px, frontal.unwrap_or(0.0), quality, detection),
        });
    }
    Ok(out)
}

/// Ziel der Augen im quadratischen Ausschnitt (0 bis 1): gleiche Lage und Größe in allen Bildern.
pub const TARGET_EYES: [[f32; 2]; 2] = [[0.35, 0.42], [0.65, 0.42]];

/// Ähnlichkeitsabbildung (Drehung, Skalierung, Verschiebung), die die Augen eines Gesichts
/// auf [`TARGET_EYES`] legt. Gerechnet im Raum des Gesichtsausschnitts (0 bis 1), so wie ihn
/// `previews::preview` mit `crop` liefert (Quadrat mit 1,6-facher Gesichtsseite).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Alignment {
    /// Augenmitte im Ausschnitt.
    pub mid: [f32; 2],
    /// Neigung der Augenlinie in Bogenmaß.
    pub angle: f32,
    pub scale: f32,
}

impl Alignment {
    /// Ausschnittspunkt (0 bis 1) -> Zielbild (0 bis 1).
    pub fn forward(&self, uv: [f32; 2]) -> [f32; 2] {
        let out = [
            (TARGET_EYES[0][0] + TARGET_EYES[1][0]) / 2.0,
            TARGET_EYES[0][1],
        ];
        let (s, c) = (-self.angle).sin_cos();
        let d = [uv[0] - self.mid[0], uv[1] - self.mid[1]];
        [
            out[0] + self.scale * (c * d[0] - s * d[1]),
            out[1] + self.scale * (s * d[0] + c * d[1]),
        ]
    }
    /// Zielbild (0 bis 1) -> Ausschnittspunkt.
    pub fn inverse(&self, p: [f32; 2]) -> [f32; 2] {
        let out = [
            (TARGET_EYES[0][0] + TARGET_EYES[1][0]) / 2.0,
            TARGET_EYES[0][1],
        ];
        let (s, c) = self.angle.sin_cos();
        let d = [(p[0] - out[0]) / self.scale, (p[1] - out[1]) / self.scale];
        [
            self.mid[0] + c * d[0] - s * d[1],
            self.mid[1] + s * d[0] + c * d[1],
        ]
    }
}

/// Ausrichtung eines Gesichts an den Augen; `None` ohne belastbare Landmarks oder Bildmaße.
pub fn alignment(face: &TimeFace) -> Option<Alignment> {
    let (w, h) = face.dims?;
    let eyes = face.eyes?;
    let bbox = face.bbox;
    let side = 1.6 * ((bbox[2] - bbox[0]) * w).max((bbox[3] - bbox[1]) * h);
    if side < 1.0 {
        return None;
    }
    let origin = [
        (bbox[0] + bbox[2]) / 2.0 * w - side / 2.0,
        (bbox[1] + bbox[3]) / 2.0 * h - side / 2.0,
    ];
    let uv = eyes.map(|p| [(p[0] * w - origin[0]) / side, (p[1] * h - origin[1]) / side]);
    let d = [uv[1][0] - uv[0][0], uv[1][1] - uv[0][1]];
    let distance = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if distance < 1e-3 {
        return None;
    }
    Some(Alignment {
        mid: [(uv[0][0] + uv[1][0]) / 2.0, (uv[0][1] + uv[1][1]) / 2.0],
        angle: d[1].atan2(d[0]),
        scale: (TARGET_EYES[1][0] - TARGET_EYES[0][0]) / distance,
    })
}

/// Schwellen der Auswahl „hohe Auflösung“ (Standardwerte in `config.rs`).
#[derive(Clone, Debug, PartialEq)]
pub struct Thresholds {
    /// Kürzere Gesichtsseite im Quellbild, mindestens.
    pub min_px: u32,
    pub min_quality: f32,
    pub min_detection: f32,
    pub min_frontal: f32,
    /// Auch sichere automatische Zuordnungen, nicht nur bestätigte.
    pub include_auto: bool,
    /// Schlechteste zugelassene Datumsquelle.
    pub min_date: DateQuality,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self::with_min_px(config::TIMELINE_MIN_PX.1)
    }
}

impl Thresholds {
    pub fn with_min_px(px: u32) -> Self {
        Self {
            min_px: px.clamp(config::TIMELINE_MIN_PX.0, config::TIMELINE_MIN_PX.2),
            min_quality: config::TIMELINE_MIN_QUALITY,
            min_detection: config::TIMELINE_MIN_DETECTION,
            min_frontal: config::TIMELINE_MIN_FRONTAL,
            include_auto: false,
            min_date: DateQuality::Folder,
        }
    }
}

/// Warum ein Gesicht nicht in der Zeitreise erscheint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Reject {
    /// Nur automatisch zugeordnet, und das ist abgeschaltet.
    Automatic,
    NoDate,
    WeakDate,
    SizeUnknown,
    TooSmall,
    Quality,
    NoLandmarks,
    NotFrontal,
}

impl Reject {
    pub fn label(self) -> &'static str {
        match self {
            Reject::Automatic => "nur automatisch zugeordnet",
            Reject::NoDate => "ohne Datum",
            Reject::WeakDate => "Datum zu unsicher",
            Reject::SizeUnknown => "Bildgröße unbekannt",
            Reject::TooSmall => "zu klein",
            Reject::Quality => "geringe Güte",
            Reject::NoLandmarks => "ohne Landmarks",
            Reject::NotFrontal => "nicht frontal",
        }
    }
}

impl Thresholds {
    pub fn reject(&self, face: &TimeFace) -> Option<Reject> {
        if face.assignment != "user" && !self.include_auto {
            return Some(Reject::Automatic);
        }
        if face.date.is_none() {
            return Some(Reject::NoDate);
        }
        if face.date_quality < self.min_date {
            return Some(Reject::WeakDate);
        }
        let Some(px) = face.px else {
            return Some(Reject::SizeUnknown);
        };
        if px < self.min_px {
            return Some(Reject::TooSmall);
        }
        if face.quality < self.min_quality || face.detection < self.min_detection {
            return Some(Reject::Quality);
        }
        match face.frontal {
            None => Some(Reject::NoLandmarks),
            Some(f) if f < self.min_frontal => Some(Reject::NotFrontal),
            _ => None,
        }
    }
}

/// Geburtsjahr, optional Geburtsmonat (nur lokal gespeichert, ADR 0069).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Birth {
    pub year: i32,
    pub month: Option<u8>,
}

impl Birth {
    /// Monatsindex des Beginns der Lebenszeit (ohne Monat: Januar des Geburtsjahrs).
    fn base(self) -> i32 {
        self.year * 12 + self.month.map_or(0, |m| m as i32 - 1)
    }
}

pub fn birth(collection: &Collection, person: i64) -> Result<Option<Birth>> {
    let attr = |key: &str| -> Result<Option<String>> {
        Ok(collection
            .db
            .query_row(
                "SELECT value FROM person_attrs WHERE person_id=? AND key=?",
                params![person, key],
                |r| r.get(0),
            )
            .optional()?)
    };
    let Some(year) = attr("birth_year")?.and_then(|v| v.trim().parse::<i32>().ok()) else {
        return Ok(None);
    };
    let month = attr("birth_month")?
        .and_then(|v| v.trim().parse::<u8>().ok())
        .filter(|m| (1..=12).contains(m));
    Ok(Some(Birth { year, month }))
}

/// Setzt oder löscht den Geburtsmonat einer Person (nur lokal, nicht in der Zentraldatei).
pub fn set_birth_month(collection: &Collection, person: i64, month: Option<u8>) -> Result<()> {
    collection.writable()?;
    match month {
        Some(m) if (1..=12).contains(&m) => {
            collection.db.execute(
                "INSERT OR REPLACE INTO person_attrs(person_id,key,value) VALUES(?,'birth_month',?)",
                params![person, m.to_string()],
            )?;
        }
        Some(_) => bail!("Geburtsmonat muss zwischen 1 und 12 liegen"),
        None => {
            collection.db.execute(
                "DELETE FROM person_attrs WHERE person_id=? AND key='birth_month'",
                [person],
            )?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Kalenderzeit.
    Date,
    /// Lebensalter (braucht das Geburtsjahr).
    Age,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grain {
    Year,
    Month,
}

impl Grain {
    fn key(self) -> char {
        match self {
            Grain::Year => 'y',
            Grain::Month => 'm',
        }
    }
}

/// Ob die Achse mit diesem Geburtsstand in dieser Körnung rechenbar ist; sonst die gröbere.
pub fn effective_grain(axis: Axis, grain: Grain, birth: Option<Birth>) -> Grain {
    if axis == Axis::Age && grain == Grain::Month && birth.and_then(|b| b.month).is_none() {
        Grain::Year
    } else {
        grain
    }
}

/// Ordnungszahl eines Datums auf der Achse (Jahr, Monatsindex, Altersjahre oder Altersmonate).
pub fn ordinal(axis: Axis, grain: Grain, date: Date, birth: Option<Birth>) -> Option<i32> {
    match axis {
        Axis::Date => Some(match grain {
            Grain::Year => date.year,
            Grain::Month => date.months(),
        }),
        Axis::Age => {
            let birth = birth?;
            let months = if birth.month.is_some() {
                date.months() - birth.base()
            } else {
                (date.year - birth.year) * 12
            };
            (months >= 0).then_some(match grain {
                Grain::Year => months / 12,
                Grain::Month => months,
            })
        }
    }
}

/// Kalendermonate (erster, letzter; Monatsindex), die eine Ordnungszahl umfasst.
pub fn month_range(axis: Axis, grain: Grain, ord: i32, birth: Option<Birth>) -> Option<(i32, i32)> {
    match (axis, grain) {
        (Axis::Date, Grain::Year) => Some((ord * 12, ord * 12 + 11)),
        (Axis::Date, Grain::Month) => Some((ord, ord)),
        (Axis::Age, Grain::Year) => {
            let base = birth?.base();
            Some((base + ord * 12, base + ord * 12 + 11))
        }
        (Axis::Age, Grain::Month) => {
            let base = birth?.base();
            Some((base + ord, base + ord))
        }
    }
}

pub fn label(axis: Axis, grain: Grain, ord: i32) -> String {
    match (axis, grain) {
        (Axis::Date, Grain::Year) => ord.to_string(),
        (Axis::Date, Grain::Month) => format!(
            "{} {}",
            MONTHS[ord.rem_euclid(12) as usize],
            ord.div_euclid(12)
        ),
        (Axis::Age, Grain::Year) => {
            if ord == 1 {
                "1 Jahr".into()
            } else {
                format!("{ord} Jahre")
            }
        }
        (Axis::Age, Grain::Month) => {
            let (years, months) = (ord / 12, ord % 12);
            match (years, months) {
                (0, 1) => "1 Monat".into(),
                (0, m) => format!("{m} Monate"),
                (y, 0) => label(Axis::Age, Grain::Year, y),
                (y, m) => format!("{y} J. {m} M."),
            }
        }
    }
}

/// Nutzerentscheidungen einer Person.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Decisions {
    /// Festgenagelte Gesichter je Abschnittsschlüssel (siehe [`Section::key`]).
    pub pins: BTreeMap<String, i64>,
    /// Ausgeblendete Gesichter.
    pub hidden: BTreeSet<i64>,
}

fn pins_key(person: i64) -> String {
    format!("timeline.pins:{person}")
}
fn hidden_key(person: i64) -> String {
    format!("timeline.hidden:{person}")
}

pub fn load_decisions(collection: &Collection, person: i64) -> Result<Decisions> {
    let get = |key: String| -> Result<String> {
        Ok(collection
            .db
            .query_row("SELECT value FROM face_settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or_default())
    };
    let pins = get(pins_key(person))?
        .split(';')
        .filter_map(|pair| {
            let (key, face) = pair.rsplit_once('=')?;
            Some((key.to_owned(), face.parse().ok()?))
        })
        .collect();
    let hidden = get(hidden_key(person))?
        .split(',')
        .filter_map(|id| id.parse().ok())
        .collect();
    Ok(Decisions { pins, hidden })
}

fn store(collection: &Collection, key: String, value: String) -> Result<()> {
    if value.is_empty() {
        collection
            .db
            .execute("DELETE FROM face_settings WHERE key=?", [key])?;
    } else {
        collection.db.execute(
            "INSERT OR REPLACE INTO face_settings(key,value) VALUES(?,?)",
            params![key, value],
        )?;
    }
    Ok(())
}

/// Nagelt `face` für den Abschnitt `key` fest (`None` hebt die Entscheidung auf).
pub fn pin(collection: &Collection, person: i64, key: &str, face: Option<i64>) -> Result<()> {
    collection.writable()?;
    let mut decisions = load_decisions(collection, person)?;
    match face {
        Some(face) => {
            decisions.pins.insert(key.to_owned(), face);
            // Ein festgenageltes Gesicht ist nicht ausgeblendet.
            decisions.hidden.remove(&face);
        }
        None => {
            decisions.pins.remove(key);
        }
    }
    write_decisions(collection, person, &decisions)
}

/// Blendet ein Gesicht aus der Zeitreise aus oder wieder ein.
pub fn set_hidden(collection: &Collection, person: i64, face: i64, hidden: bool) -> Result<()> {
    collection.writable()?;
    let mut decisions = load_decisions(collection, person)?;
    if hidden {
        decisions.hidden.insert(face);
        decisions.pins.retain(|_, pinned| *pinned != face);
    } else {
        decisions.hidden.remove(&face);
    }
    write_decisions(collection, person, &decisions)
}

fn write_decisions(collection: &Collection, person: i64, decisions: &Decisions) -> Result<()> {
    store(
        collection,
        pins_key(person),
        decisions
            .pins
            .iter()
            .map(|(key, face)| format!("{key}={face}"))
            .collect::<Vec<_>>()
            .join(";"),
    )?;
    store(
        collection,
        hidden_key(person),
        decisions
            .hidden
            .iter()
            .map(i64::to_string)
            .collect::<Vec<_>>()
            .join(","),
    )
}

/// Ein für die Zeitreise geeignetes Gesicht, mit der Ausblend-Entscheidung.
#[derive(Clone, Debug)]
pub struct Item {
    pub face: TimeFace,
    pub hidden: bool,
}

/// Ein Zeitabschnitt mit seinen Gesichtern.
#[derive(Clone, Debug)]
pub struct Section {
    /// Stabiler Schlüssel für Entscheidungen, z. B. `dy:2019`, `dm:24228`, `ay:3`, `am:40`.
    pub key: String,
    pub ord: i32,
    pub label: String,
    /// Indizes in [`Computed::items`], bester Wert zuerst.
    pub members: Vec<usize>,
    /// Gewähltes Gesicht (festgenagelt oder bester Wert); `None`, wenn alle ausgeblendet sind.
    pub best: Option<usize>,
    pub pinned: bool,
}

/// Verteilung der Gesichter über die Achse (Dichtekurve), lückenlos von der ersten bis zur
/// letzten Ordnungszahl.
#[derive(Clone, Debug, Default)]
pub struct Density {
    pub first: i32,
    pub counts: Vec<u32>,
}

/// Eine Lücke: aufeinanderfolgende Abschnitte ohne brauchbares Gesicht.
#[derive(Clone, Debug)]
pub struct Gap {
    pub from: i32,
    pub to: i32,
    /// Letzter Abschnitt davor und erster danach (Beschriftung).
    pub before: String,
    pub after: String,
    /// Gesichter der Person in diesem Zeitraum, die nur zu klein, unscharf oder nicht
    /// frontal sind.
    pub candidates: usize,
    /// Zeitraum als Datum für die Sammlungssuche (`JJJJ-MM-TT`).
    pub start: String,
    pub end: String,
}

/// Ein aussortiertes Gesicht mit Datum, das eine Lücke füllen könnte.
#[derive(Clone, Debug)]
pub struct Missed {
    pub face: i64,
    pub date: Date,
    pub reason: Reject,
}

#[derive(Clone, Debug)]
pub struct Params {
    pub thresholds: Thresholds,
    pub axis: Axis,
    /// `None` wählt die Körnung nach Datenlage.
    pub grain: Option<Grain>,
    pub birth: Option<Birth>,
    pub decisions: Decisions,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            thresholds: Thresholds::default(),
            axis: Axis::Date,
            grain: None,
            birth: None,
            decisions: Decisions::default(),
        }
    }
}

/// Ergebnis der Berechnung für eine Person.
#[derive(Clone, Debug)]
pub struct Computed {
    pub axis: Axis,
    pub grain: Grain,
    /// Die Körnung wurde nach Datenlage gewählt (nicht vom Nutzer).
    pub automatic: bool,
    pub birth: Option<Birth>,
    pub items: Vec<Item>,
    /// Abschnitte in der gewählten Körnung, älteste zuerst.
    pub sections: Vec<Section>,
    /// Abschnitte je Jahr bzw. Altersjahr (Kontaktbogen), älteste zuerst.
    pub years: Vec<Section>,
    pub density: Density,
    pub gaps: Vec<Gap>,
    /// Alle geladenen Gesichter und davon aussortierte nach Grund.
    pub total: usize,
    pub rejected: BTreeMap<Reject, usize>,
    pub missed: Vec<Missed>,
}

/// Wählt die Körnung nach Datenlage: Monate bei kurzer Zeitspanne oder viel Material.
pub fn auto_grain(items: &[Item], axis: Axis, birth: Option<Birth>) -> Grain {
    let ords: Vec<i32> = items
        .iter()
        .filter_map(|i| {
            i.face
                .date
                .and_then(|d| ordinal(axis, Grain::Month, d, birth))
        })
        .collect();
    let (Some(min), Some(max)) = (ords.iter().min(), ords.iter().max()) else {
        return Grain::Year;
    };
    let span = (max - min + 1) as f32;
    let grain = if span <= 48.0 || ords.len() as f32 / (span / 12.0) >= 36.0 {
        Grain::Month
    } else {
        Grain::Year
    };
    effective_grain(axis, grain, birth)
}

fn sections_of(
    items: &[Item],
    axis: Axis,
    grain: Grain,
    birth: Option<Birth>,
    pins: &BTreeMap<String, i64>,
) -> Vec<Section> {
    let mut groups: BTreeMap<i32, Vec<usize>> = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        if let Some(ord) = item.face.date.and_then(|d| ordinal(axis, grain, d, birth)) {
            groups.entry(ord).or_default().push(index);
        }
    }
    groups
        .into_iter()
        .map(|(ord, mut members)| {
            members.sort_by(|a, b| {
                items[*b]
                    .face
                    .value
                    .total_cmp(&items[*a].face.value)
                    .then(items[*a].face.id.cmp(&items[*b].face.id))
            });
            let key = format!(
                "{}{}:{ord}",
                if axis == Axis::Date { 'd' } else { 'a' },
                grain.key()
            );
            let pinned = pins.get(&key).and_then(|face| {
                members
                    .iter()
                    .copied()
                    .find(|m| items[*m].face.id == *face && !items[*m].hidden)
            });
            let best = pinned.or_else(|| members.iter().copied().find(|m| !items[*m].hidden));
            Section {
                key,
                ord,
                label: label(axis, grain, ord),
                members,
                best,
                pinned: pinned.is_some(),
            }
        })
        .collect()
}

fn density_of(sections: &[Section], items: &[Item]) -> Density {
    let (Some(first), Some(last)) = (sections.first(), sections.last()) else {
        return Density::default();
    };
    let mut counts = vec![0u32; (last.ord - first.ord + 1) as usize];
    for section in sections {
        counts[(section.ord - first.ord) as usize] = section
            .members
            .iter()
            .filter(|m| !items[**m].hidden)
            .count() as u32;
    }
    Density {
        first: first.ord,
        counts,
    }
}

/// Mindestlänge einer Lücke in Abschnitten der jeweiligen Körnung.
fn min_gap(grain: Grain) -> i32 {
    match grain {
        Grain::Year => 1,
        Grain::Month => 3,
    }
}

fn gaps_of(
    sections: &[Section],
    missed: &[Missed],
    axis: Axis,
    grain: Grain,
    birth: Option<Birth>,
) -> Vec<Gap> {
    let shown: Vec<&Section> = sections.iter().filter(|s| s.best.is_some()).collect();
    let mut gaps = Vec::new();
    for pair in shown.windows(2) {
        let (from, to) = (pair[0].ord + 1, pair[1].ord - 1);
        if to - from + 1 < min_gap(grain) {
            continue;
        }
        let range = month_range(axis, grain, from, birth)
            .zip(month_range(axis, grain, to, birth))
            .map(|(a, b)| (a.0, b.1));
        let (start, end) = range.map_or((String::new(), String::new()), |(a, b)| {
            (
                format!("{:04}-{:02}-01", a.div_euclid(12), a.rem_euclid(12) + 1),
                format!("{:04}-{:02}-28", b.div_euclid(12), b.rem_euclid(12) + 1),
            )
        });
        let candidates = missed
            .iter()
            .filter(|m| ordinal(axis, grain, m.date, birth).is_some_and(|o| o >= from && o <= to))
            .count();
        gaps.push(Gap {
            from,
            to,
            before: pair[0].label.clone(),
            after: pair[1].label.clone(),
            candidates,
            start,
            end,
        });
    }
    gaps
}

/// Wählt aus allen geladenen Gesichtern die geeigneten, bildet Abschnitte, Dichte und Lücken.
pub fn compute(faces: &[TimeFace], params: &Params) -> Computed {
    let mut items = Vec::new();
    let mut rejected: BTreeMap<Reject, usize> = BTreeMap::new();
    let mut missed = Vec::new();
    for face in faces {
        match params.thresholds.reject(face) {
            None => items.push(Item {
                hidden: params.decisions.hidden.contains(&face.id),
                face: face.clone(),
            }),
            Some(reason) => {
                *rejected.entry(reason).or_default() += 1;
                if let (Some(date), Reject::TooSmall | Reject::Quality | Reject::NotFrontal) =
                    (face.date, reason)
                {
                    missed.push(Missed {
                        face: face.id,
                        date,
                        reason,
                    });
                }
            }
        }
    }
    items.sort_by(|a, b| {
        a.face
            .date
            .cmp(&b.face.date)
            .then(a.face.id.cmp(&b.face.id))
    });
    let (axis, birth) = (params.axis, params.birth);
    let automatic = params.grain.is_none();
    let grain = effective_grain(
        axis,
        params
            .grain
            .unwrap_or_else(|| auto_grain(&items, axis, birth)),
        birth,
    );
    let pins = &params.decisions.pins;
    let sections = sections_of(&items, axis, grain, birth, pins);
    let years = if grain == Grain::Year {
        sections.clone()
    } else {
        sections_of(&items, axis, Grain::Year, birth, pins)
    };
    Computed {
        axis,
        grain,
        automatic,
        birth,
        density: density_of(&sections, &items),
        gaps: gaps_of(&sections, &missed, axis, grain, birth),
        total: faces.len(),
        rejected,
        missed,
        items,
        sections,
        years,
    }
}

impl Computed {
    pub fn face(&self, index: usize) -> &TimeFace {
        &self.items[index].face
    }
    /// Gewählte Gesichter in zeitlicher Reihenfolge (Zeitraffer, Vorher/Nachher).
    pub fn frames(&self) -> Vec<usize> {
        self.sections.iter().filter_map(|s| s.best).collect()
    }
    /// Die besten `n` sichtbaren Gesichter eines Abschnitts (Kontaktbogen): das gewählte
    /// zuerst, dann nach Wert.
    pub fn top(&self, section: &Section, n: usize) -> Vec<usize> {
        let mut out: Vec<usize> = section.best.into_iter().collect();
        for member in &section.members {
            if out.len() >= n {
                break;
            }
            if !self.items[*member].hidden && !out.contains(member) {
                out.push(*member);
            }
        }
        out
    }
    /// Alter in Jahren zum Zeitpunkt der Aufnahme (nur mit Geburtsjahr).
    pub fn age_years(&self, face: &TimeFace) -> Option<i32> {
        ordinal(Axis::Age, Grain::Year, face.date?, self.birth)
    }
}

/// Angehörige einer Person aus dem Stammbaum (Eltern, Kinder, Partner, Geschwister).
pub fn relatives(family: &Family, person: i64) -> Vec<(i64, &'static str)> {
    let mut out: Vec<(i64, &'static str)> = Vec::new();
    let mut seen = BTreeSet::from([person]);
    let mut add = |ids: Vec<i64>, label: &'static str, out: &mut Vec<(i64, &'static str)>| {
        for id in ids {
            if seen.insert(id) {
                out.push((id, label));
            }
        }
    };
    add(
        family.parents_of(person).iter().copied().collect(),
        "Elternteil",
        &mut out,
    );
    let siblings = family
        .parents_of(person)
        .iter()
        .flat_map(|p| family.children_of(*p).iter().copied())
        .collect();
    add(siblings, "Geschwister", &mut out);
    add(
        family.children_of(person).iter().copied().collect(),
        "Kind",
        &mut out,
    );
    add(
        family.partners_of(person).iter().copied().collect(),
        "Partner",
        &mut out,
    );
    out
}
