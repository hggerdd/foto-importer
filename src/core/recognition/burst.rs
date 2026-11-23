//! Serienbilder: Gesichter an fast derselben Stelle in Fotos, die wenige Sekunden
//! auseinanderliegen (gleicher Ordner, gleiche Kamera), zeigen fast immer dieselbe Person.
//! Bestätigt der Nutzer ein Gesicht einer solchen Kette, schlägt der Abgleich die Person für
//! die übrigen Gesichter der Kette vor; bestätigt wird nie automatisch.
use super::*;
use std::collections::HashMap;

/// Schlüssel in `face_settings`: Serienvorschläge an (`true`) oder aus (`false`); ohne Eintrag
/// gilt `config::BURST_DEFAULT`. Nicht modellbezogen.
pub const BURST_KEY: &str = "burst_suggestions";

/// Gilt die Sammlungsoption „Serienbilder“?
pub fn burst_enabled(collection: &Collection) -> Result<bool> {
    let raw: Option<String> = collection
        .db
        .query_row(
            "SELECT value FROM face_settings WHERE key=?",
            [BURST_KEY],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.map_or(crate::config::BURST_DEFAULT, |v| v == "true"))
}

/// Schaltet Serienvorschläge der Sammlung und merkt den Abgleich des aktiven Modells vor: Der
/// nächste Lauf setzt oder entfernt die Serienvorschläge. Bestätigungen bleiben unberührt.
pub fn set_burst_enabled(collection: &mut Collection, enabled: bool) -> Result<()> {
    collection.writable()?;
    let key = faces::setting(collection, faces::EMBEDDER_KEY)?;
    let tx = collection.db.transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![BURST_KEY, enabled.to_string()],
    )?;
    if let Some(key) = key {
        tx.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
    }
    tx.commit()?;
    Ok(())
}

/// Ein Gesicht mit Aufnahmezeit, wie es die Serienerkennung braucht.
struct Item {
    id: i64,
    photo: i64,
    series: u32,
    /// Sekunden (lokale Aufnahmezeit samt Sekundenbruchteil).
    time: f64,
    bbox: [f32; 4],
    /// Bestätigte Person (`assignment='user'`).
    confirmed: Option<i64>,
}

/// Mögliche Serienpaare eines Lesestands, schon auf Ketten mit mindestens einem bestätigten
/// Gesicht beschränkt (ohne Vektoren bestimmt).
#[derive(Default)]
pub(super) struct Series {
    items: Vec<Item>,
    pairs: Vec<(u32, u32)>,
}

/// Aufnahmezeit aus EXIF (`2024:05:01 12:00:00`, auch mit `-`) und Sekundenbruchteil.
fn seconds(date: &str, subsec: Option<&str>) -> Option<f64> {
    let date = date.get(..19)?;
    let parsed = chrono::NaiveDateTime::parse_from_str(date, "%Y:%m:%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S"))
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(date, "%Y-%m-%dT%H:%M:%S"))
        .ok()?;
    let mut time = parsed.and_utc().timestamp() as f64;
    if let Some(digits) = subsec
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 9 && s.bytes().all(|b| b.is_ascii_digit()))
    {
        time += digits.parse::<f64>().unwrap_or(0.0) / 10f64.powi(digits.len() as i32);
    }
    Some(time)
}

/// Überdeckung (IoU) zweier normierter Rahmen.
pub(crate) fn iou(a: [f32; 4], b: [f32; 4]) -> f32 {
    let width = (a[2].min(b[2]) - a[0].max(b[0])).max(0.0);
    let height = (a[3].min(b[3]) - a[1].max(b[1])).max(0.0);
    let inter = width * height;
    let union = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter;
    if union > 0.0 { inter / union } else { 0.0 }
}

fn find(parent: &mut [u32], mut x: u32) -> u32 {
    while parent[x as usize] != x {
        let up = parent[parent[x as usize] as usize];
        parent[x as usize] = up;
        x = up;
    }
    x
}
fn union(parent: &mut [u32], a: u32, b: u32) {
    let (a, b) = (find(parent, a), find(parent, b));
    if a != b {
        parent[a.max(b) as usize] = a.min(b);
    }
}

impl Series {
    /// Liest alle aktiven Gesichter mit Merkmalsqualität und Aufnahmezeit (EXIF der
    /// Analysequelle) und bestimmt die Paare derselben Serie mit genügend Überdeckung.
    pub(super) fn load(collection: &Collection) -> Result<Self> {
        let mut keys: HashMap<(String, String, String, String), u32> = HashMap::new();
        let mut items = Vec::new();
        let mut statement = collection.db.prepare_cached(
            "SELECT f.id,f.photo_id,f.source_relpath,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2,
                    CASE WHEN f.assignment='user' THEN f.person_id END,
                    m.dt_original,m.subsec,coalesce(m.make,''),coalesce(m.model,''),coalesce(m.serial,'')
             FROM faces f JOIN meta m ON m.file_id=f.source_file_id
             WHERE f.retained=0 AND f.ignored=0 AND f.quality>=0.5 AND m.dt_original IS NOT NULL",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let date: String = row.get(8)?;
            let subsec: Option<String> = row.get(9)?;
            let Some(time) = seconds(&date, subsec.as_deref()) else {
                continue;
            };
            let bbox: [Option<f32>; 4] = [row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?];
            let [Some(x1), Some(y1), Some(x2), Some(y2)] = bbox else {
                continue;
            };
            let path: String = row.get(2)?;
            let folder = path.rsplit_once('/').map_or("", |(dir, _)| dir).to_string();
            let key = (folder, row.get(10)?, row.get(11)?, row.get(12)?);
            let next = keys.len() as u32;
            let series = *keys.entry(key).or_insert(next);
            items.push(Item {
                id: row.get(0)?,
                photo: row.get(1)?,
                series,
                time,
                bbox: [x1, y1, x2, y2],
                confirmed: row.get(7)?,
            });
        }
        drop(rows);
        drop(statement);
        items.sort_by(|a, b| {
            a.series
                .cmp(&b.series)
                .then(a.time.total_cmp(&b.time))
                .then(a.id.cmp(&b.id))
        });
        let mut pairs = Vec::new();
        for (i, a) in items.iter().enumerate() {
            for (j, b) in items.iter().enumerate().skip(i + 1) {
                if b.series != a.series || b.time - a.time > crate::config::BURST_GAP_SECONDS {
                    break;
                }
                if b.photo != a.photo && iou(a.bbox, b.bbox) >= crate::config::BURST_IOU_MIN {
                    pairs.push((i as u32, j as u32));
                }
            }
        }
        // Nur Ketten mit einem bestätigten Gesicht können etwas vorschlagen; die übrigen Paare
        // brauchen keinen Vektorvergleich.
        let mut parent: Vec<u32> = (0..items.len() as u32).collect();
        for (a, b) in &pairs {
            union(&mut parent, *a, *b);
        }
        let mut useful = vec![false; items.len()];
        for (index, item) in items.iter().enumerate() {
            if item.confirmed.is_some() {
                let root = find(&mut parent, index as u32);
                useful[root as usize] = true;
            }
        }
        pairs.retain(|(a, _)| {
            let root = find(&mut parent, *a);
            useful[root as usize]
        });
        Ok(Self { items, pairs })
    }

    /// Gesichter, deren Vektoren [`Self::proposals`] vergleicht.
    pub(super) fn needed(&self) -> BTreeSet<i64> {
        self.pairs
            .iter()
            .flat_map(|(a, b)| [self.items[*a as usize].id, self.items[*b as usize].id])
            .collect()
    }

    /// Serienperson je unbestätigtem Gesicht: Paare mit Kosinus ab `min_cosine` bilden Ketten;
    /// enthält eine Kette bestätigte Gesichter genau einer Person, gilt diese für alle übrigen
    /// Gesichter der Kette. Ketten mit bestätigten Gesichtern verschiedener Personen schlagen
    /// nichts vor. Ausschlüsse des Gesichts prüft der Abgleich.
    pub(super) fn proposals<'a>(
        &self,
        vector: impl Fn(i64) -> Option<&'a [f32]>,
        min_cosine: f32,
    ) -> HashMap<i64, i64> {
        let mut parent: Vec<u32> = (0..self.items.len() as u32).collect();
        for (a, b) in &self.pairs {
            let (x, y) = (&self.items[*a as usize], &self.items[*b as usize]);
            let linked = vector(x.id)
                .zip(vector(y.id))
                .and_then(|(p, q)| inference::cosine(p, q))
                .is_some_and(|c| c >= min_cosine);
            if linked {
                union(&mut parent, *a, *b);
            }
        }
        // Bestätigte Person je Kette; `None` bei mehreren Personen.
        let mut person: HashMap<u32, Option<i64>> = HashMap::new();
        for (index, item) in self.items.iter().enumerate() {
            if let Some(confirmed) = item.confirmed {
                let root = find(&mut parent, index as u32);
                person
                    .entry(root)
                    .and_modify(|p| {
                        if *p != Some(confirmed) {
                            *p = None;
                        }
                    })
                    .or_insert(Some(confirmed));
            }
        }
        let mut result = HashMap::new();
        for (index, item) in self.items.iter().enumerate() {
            if item.confirmed.is_some() {
                continue;
            }
            let root = find(&mut parent, index as u32);
            if let Some(Some(x)) = person.get(&root) {
                result.insert(item.id, *x);
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_times_include_fractions_and_reject_garbage() {
        let a = seconds("2024:05:01 12:00:00", Some("25")).expect("Zeit");
        let b = seconds("2024-05-01 12:00:02", None).expect("Zeit");
        assert!((b - a - 1.75).abs() < 1e-6);
        assert_eq!(seconds("", None), None);
        assert_eq!(seconds("0000:00:00 00:00:00", None), None);
        let c = seconds("2024:05:01 12:00:00", Some("x")).expect("Zeit");
        assert!((c - a + 0.25).abs() < 1e-6);
    }

    #[test]
    fn overlap_of_normalized_boxes() {
        assert!((iou([0.0, 0.0, 1.0, 1.0], [0.0, 0.0, 1.0, 1.0]) - 1.0).abs() < 1e-6);
        assert_eq!(iou([0.0, 0.0, 0.1, 0.1], [0.5, 0.5, 0.6, 0.6]), 0.0);
        let half = iou([0.0, 0.0, 0.2, 0.1], [0.1, 0.0, 0.3, 0.1]);
        assert!((half - 1.0 / 3.0).abs() < 1e-6);
    }
}
