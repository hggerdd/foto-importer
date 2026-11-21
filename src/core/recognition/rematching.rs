//! Abgleich aller offenen Gesichter gegen die Referenzen: inkrementell über den
//! `MatchCache` oder vollständig Foto für Foto; Schreiben der Vorschläge je Foto.
use super::*;

/// Maschinelle Zuordnung: Person, `auto` oder `suggested`, Wert.
type Proposal = (i64, &'static str, f32);
/// Ein Gesicht eines Abgleichschritts mit allem, was der Abgleich braucht.
pub(super) struct MatchFace {
    pub(super) id: i64,
    pub(super) photo: i64,
    pub(super) person: Option<i64>,
    pub(super) assignment: Option<String>,
    pub(super) confidence: Option<f64>,
    pub(super) ignored: bool,
    pub(super) retained: bool,
    /// Jahr der Aufnahme (erste vier Zeichen von `captured_at`).
    pub(super) year: Option<i32>,
    /// Nicht historisch und Qualität ab 0,5: nur dann zählt ein Vektor (wie `ranked_with`).
    pub(super) wanted: bool,
    /// Vektor nur für `wanted` (im inkrementellen Abgleich stets `None`, dort im Cache).
    pub(super) vector: Option<Vec<f32>>,
}
/// Gesichter (auch historische) und Ablehnungen einiger Fotos, je Foto nach ID geordnet.
struct MatchBatch {
    faces: BTreeMap<i64, Vec<MatchFace>>,
    rejections: BTreeMap<i64, BTreeSet<i64>>,
}
impl MatchBatch {
    fn load(collection: &Collection, key: &str, photos: &[i64]) -> Result<Self> {
        let marks = vec!["?"; photos.len()].join(",");
        let mut values: Vec<rusqlite::types::Value> = vec![key.to_string().into()];
        values.extend(photos.iter().map(|p| rusqlite::types::Value::from(*p)));
        let mut faces: BTreeMap<i64, Vec<MatchFace>> = BTreeMap::new();
        let mut statement = collection.db.prepare_cached(&format!(
            "SELECT f.id,f.photo_id,f.person_id,f.assignment,f.confidence,f.ignored,f.retained,
                    f.captured_at,CASE WHEN f.retained=0 AND f.quality>=0.5 THEN e.vec END,
                    coalesce(f.retained=0 AND f.quality>=0.5,0)
             FROM faces f LEFT JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1
             WHERE f.photo_id IN ({marks}) ORDER BY f.photo_id,f.id"
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(values.iter()))?;
        while let Some(row) = rows.next()? {
            let bytes: Option<Vec<u8>> = row.get(8)?;
            let captured: Option<String> = row.get(7)?;
            faces.entry(row.get(1)?).or_default().push(MatchFace {
                id: row.get(0)?,
                photo: row.get(1)?,
                person: row.get(2)?,
                assignment: row.get(3)?,
                confidence: row.get(4)?,
                ignored: row.get(5)?,
                retained: row.get(6)?,
                year: captured
                    .as_deref()
                    .and_then(|v| v.get(..4))
                    .and_then(|v| v.parse().ok()),
                wanted: row.get(9)?,
                vector: bytes.as_deref().map(decode).transpose()?,
            });
        }
        drop(rows);
        let mut rejections: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
        let mut statement = collection.db.prepare_cached(&format!(
            "SELECT r.face_id,r.person_id FROM face_rejections r JOIN faces f ON f.id=r.face_id
             WHERE f.photo_id IN ({})",
            vec!["?"; photos.len()].join(",")
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(photos.iter()))?;
        while let Some(row) = rows.next()? {
            rejections
                .entry(row.get(0)?)
                .or_default()
                .insert(row.get(1)?);
        }
        Ok(Self { faces, rejections })
    }
}

pub(super) fn load_births(collection: &Collection) -> Result<BTreeMap<i64, i32>> {
    Ok(collection
        .db
        .prepare("SELECT person_id,value FROM person_attrs WHERE key='birth_year'")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .filter_map(|r| r.ok().and_then(|(id, year)| Some((id, year.parse().ok()?))))
        .collect())
}

/// Entscheidet die Gesichter eines Fotos (alle, auch historische, nach ID) und hängt
/// abweichende maschinelle Zuordnungen an `writes`. `rank` liefert die Rangfolge eines
/// Gesichts ohne die übergebenen Ausschlüsse (mindestens die ersten zwei Einträge), `None`
/// ohne vergleichbaren Vektor.
fn photo_writes(
    faces: &[MatchFace],
    rejections: &BTreeMap<i64, BTreeSet<i64>>,
    births: &BTreeMap<i64, i32>,
    limits: &Limits,
    mut rank: impl FnMut(&MatchFace, &dyn Fn(i64) -> bool) -> Option<Vec<(i64, f32)>>,
    writes: &mut Vec<(i64, Option<Proposal>)>,
) -> Result<()> {
    // Wie `faces::photo_faces`: nur nicht historische Gesichter, nach ID.
    let candidates: Vec<&MatchFace> = faces
        .iter()
        .filter(|f| !f.retained && !f.ignored && f.assignment.as_deref() != Some("user"))
        .collect();
    // Je Gesicht höchstens ein Kandidat: die beste zulässige Person, wenn sie die Schwelle
    // erreicht und klar vor der zweitbesten liegt.
    let mut proposals: Vec<Option<(i64, f32)>> = Vec::with_capacity(candidates.len());
    // Im Foto bestätigte Personen (auch historischer Gesichter). Kandidaten sind nie
    // bestätigt, ihr eigenes Gesicht ist also nie darunter.
    let occupied: Vec<i64> = faces
        .iter()
        .filter(|o| o.assignment.as_deref() == Some("user"))
        .filter_map(|o| o.person)
        .collect();
    for face in &candidates {
        // Wie `ranked_with`: Gesichter ohne Vektor oder unter Qualität 0,5 erhalten keinen
        // Vorschlag; abgelehnte und im Foto bestätigte Personen zählen nicht, ebenso wenig
        // Personen, die bei der Aufnahme noch nicht geboren waren. Ausschließen vor oder nach
        // dem Sortieren ergibt dieselbe Reihenfolge.
        let rejected = rejections.get(&face.id);
        let excluded = |person: i64| {
            rejected.is_some_and(|r| r.contains(&person))
                || occupied.contains(&person)
                || face
                    .year
                    .zip(births.get(&person))
                    .is_some_and(|(y, b)| y < *b)
        };
        let Some(ranking) = rank(face, &excluded) else {
            proposals.push(None);
            continue;
        };
        proposals.push(limits.accept(&ranking));
    }
    // Gemeinsame Entscheidung je Foto: Dieselbe Person kommt höchstens einmal vor; das
    // Gesicht mit dem höheren Wert erhält sie, das andere bleibt unbenannt.
    let people: Vec<i64> = proposals
        .iter()
        .flatten()
        .map(|p| p.0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let unknown = limits.review - 0.000001;
    let mut matrix = vec![vec![unknown; people.len() + candidates.len()]; candidates.len()];
    for (row, proposal) in proposals.iter().enumerate() {
        for (col, person) in people.iter().enumerate() {
            matrix[row][col] = match proposal {
                Some((id, score)) if id == person => *score,
                _ => -2.0,
            };
        }
    }
    let decisions = matching::assignment(&matrix)?;
    let mut wanted: BTreeMap<i64, Proposal> = BTreeMap::new();
    for (row, col) in decisions.into_iter().enumerate() {
        if col < people.len() && matrix[row][col] >= limits.review {
            let score = matrix[row][col];
            let kind = if score >= limits.sure {
                "auto"
            } else {
                "suggested"
            };
            wanted.insert(candidates[row].id, (people[col], kind, score));
        }
    }
    // Nur Abweichungen schreiben: Bisher wurden alle maschinellen Zuordnungen des Fotos
    // gelöscht und neu gesetzt, was je Gesicht Trigger der Gruppen auslöste.
    for face in faces {
        let machine = matches!(face.assignment.as_deref(), Some("auto" | "suggested"));
        match wanted.get(&face.id) {
            Some(&(person, kind, score)) => {
                let same = face.person == Some(person)
                    && face.assignment.as_deref() == Some(kind)
                    && face.confidence == Some(f64::from(score));
                if !same {
                    writes.push((face.id, Some((person, kind, score))));
                }
            }
            None if machine => writes.push((face.id, None)),
            None => {}
        }
    }
    Ok(())
}

fn write_proposals(
    tx: &rusqlite::Transaction<'_>,
    writes: &[(i64, Option<Proposal>)],
) -> Result<()> {
    for (id, value) in writes {
        match value {
            Some((person, kind, score)) => {
                tx.execute("UPDATE faces SET person_id=?,assignment=?,confidence=?,assigned_at=datetime('now') WHERE id=? AND assignment IS NOT 'user'",params![person,kind,score,id])?;
            }
            None => {
                tx.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=? AND assignment IN ('auto','suggested')",[id])?;
            }
        }
    }
    Ok(())
}

/// Schließt einen Abgleich ab, wenn sich seit seinem Beginn nichts geändert hat.
fn finish_rematch(collection: &mut Collection, key: &str, generation: Option<i64>) -> Result<()> {
    let tx = collection.db.transaction()?;
    tx.execute(
        "DELETE FROM face_maintenance WHERE embedder_id=? AND generation IS ?",
        params![key, generation],
    )?;
    let open: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM face_maintenance WHERE embedder_id=?)",
        [key],
        |r| r.get(0),
    )?;
    if !open {
        faces::save_setting(&tx, &applied_key(key), &chrono::Utc::now().to_rfc3339())?;
    }
    tx.commit()?;
    Ok(())
}

fn maintenance_state(collection: &Collection, key: &str) -> Result<(i64, Option<i64>)> {
    Ok(collection
        .db
        .query_row(
            "SELECT after_photo,generation FROM face_maintenance WHERE embedder_id=?",
            [key],
            |r| Ok((r.get(0)?, Some(r.get(1)?))),
        )
        .optional()?
        .unwrap_or((0, None)))
}

/// Abgleich nach Bestätigungen: inkrementell über den Zwischenspeicher des Programms
/// ([`MatchCache`]), Ergebnis bitgleich zum vollständigen Lauf ([`rematch_full`]).
pub fn rematch(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    let mut cache = match_cache().take().unwrap_or_default();
    let mut span = crate::core::diag::span("gesichter", "abgleich");
    span.field("cache_bereit", cache.owner.is_some());
    let result = rematch_with(collection, key, control, &mut cache);
    span.field("cache_gesichter", cache.len())
        .field("ok", result.is_ok());
    drop(span);
    *match_cache() = Some(cache);
    result
}

/// Abgleich mit ausdrücklichem Zwischenspeicher. Ein leerer oder fremder Zwischenspeicher
/// wird zuerst vollständig aufgebaut (alle Vektoren lesen, alle Spalten rechnen).
pub fn rematch_with(
    collection: &mut Collection,
    key: &str,
    control: &Control,
    cache: &mut MatchCache,
) -> Result<()> {
    control.set_face_phase(crate::core::jobs::FacePhase::Matching);
    control.checkpoint()?;
    // Eine Nutzeränderung während des Laufs erhöht per Trigger `generation`. Geschrieben wird
    // nur, solange die Generation noch die vom Beginn ist; sonst bleibt der Auftrag für einen
    // neuen Lauf (nach der Referenzauswahl der Wartung) stehen.
    let (_, generation) = maintenance_state(collection, key)?;
    let limits = Limits::load(collection)?;
    let births = load_births(collection)?;
    let mut state = MatchState::load(collection, key)?;
    let owner = cache_owner(collection, key)?;
    if cache.owner.as_ref() != Some(&owner) {
        *cache = MatchCache {
            owner: Some(owner),
            ..MatchCache::default()
        };
    }
    let compared = state
        .faces
        .iter()
        .filter(|f| f.wanted && state.embeddings.contains_key(&f.id))
        .count();
    let dim = state.entries.0.first().map_or(512, |e| e.2.len());
    let bytes = compared * ((dim + state.entries.1.len()) * 4 + 96);
    if bytes > crate::config::MATCH_CACHE_MAX_BYTES {
        log::info!(
            target: "gesichter",
            "abgleich_vollstaendig grund=cache_zu_gross bytes={bytes} gesichter={compared}"
        );
        *cache = MatchCache::default();
        return rematch_full(collection, key, control);
    }
    let references = cache.sync(collection, key, &mut state, control)?;
    control.checkpoint()?;
    let mut writes: Vec<(i64, Option<Proposal>)> = Vec::new();
    for photo in state.faces.chunk_by(|a, b| a.photo == b.photo) {
        // Wie der vollständige Lauf: nur Fotos mit mindestens einem aktuellen Gesicht.
        if photo.iter().all(|f| f.retained) {
            continue;
        }
        photo_writes(
            photo,
            &state.rejections,
            &births,
            &limits,
            |face, excluded| {
                let cached = cache.faces.get(&face.id)?;
                cached.vector.as_ref()?;
                Some(references.best_two(&cached.scores, excluded))
            },
            &mut writes,
        )?;
    }
    for chunk in writes.chunks(WRITE_CHUNK) {
        control.checkpoint()?;
        let tx = collection.db.transaction()?;
        let current: Option<i64> = tx
            .query_row(
                "SELECT generation FROM face_maintenance WHERE embedder_id=?",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        if current != generation {
            // Neuer Auftrag: Die übrigen Zuordnungen beruhen auf einem veralteten Stand.
            return Ok(());
        }
        write_proposals(&tx, chunk)?;
        tx.commit()?;
    }
    control.checkpoint()?;
    finish_rematch(collection, key, generation)
}

/// Vollständiger Abgleich ohne Zwischenspeicher, Foto für Foto mit Cursor. Rückfall, wenn der
/// Zwischenspeicher die Speichergrenze überschritte, und Maßstab für den inkrementellen Lauf.
pub fn rematch_full(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    control.set_face_phase(crate::core::jobs::FacePhase::Matching);
    // Eine Nutzeränderung während des Laufs setzt per Trigger den Cursor auf 0 und erhöht
    // `generation`. Der Lauf schreibt seinen Cursor und das Ende nur für die Generation, mit
    // der er begann; sonst bleibt der Auftrag stehen und der nächste Lauf beginnt von vorn.
    let (mut cursor, generation) = maintenance_state(collection, key)?;
    let limits = Limits::load(collection)?;
    let births = load_births(collection)?;
    let references = References::load(collection, key)?;
    // Höchstens so lange bleibt ein Fortschritt ohne Änderungen ungespeichert; Schritte mit
    // Änderungen werden sofort geschrieben.
    let mut saved = std::time::Instant::now();
    loop {
        control.checkpoint()?;
        let photos=collection.db.prepare_cached("SELECT DISTINCT photo_id FROM faces WHERE photo_id>? AND retained=0 ORDER BY photo_id LIMIT 32")?.query_map([cursor],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let Some(&last) = photos.last() else {
            break;
        };
        // Alle Gesichter, Vektoren und Ablehnungen des Schritts mit zwei Abfragen lesen und
        // erst danach schreiben: Die Schreibtransaktion bleibt kurz und wartet nie auf
        // `checkpoint`, während eine Nutzeraktion schreiben will.
        let batch = MatchBatch::load(collection, key, &photos)?;
        let mut writes: Vec<(i64, Option<Proposal>)> = Vec::new();
        for photo in &photos {
            control.checkpoint()?;
            let faces = batch.faces.get(photo).map_or(&[][..], Vec::as_slice);
            photo_writes(
                faces,
                &batch.rejections,
                &births,
                &limits,
                |face, excluded| {
                    face.vector
                        .as_deref()
                        .map(|query| references.rank(query, face.id, excluded))
                },
                &mut writes,
            )?;
        }
        if writes.is_empty() && saved.elapsed() < std::time::Duration::from_secs(1) {
            cursor = last;
            continue;
        }
        let tx = collection.db.transaction()?;
        write_proposals(&tx, &writes)?;
        tx.execute(
            "UPDATE face_maintenance SET after_photo=? WHERE embedder_id=? AND generation IS ?",
            params![last, key, generation],
        )?;
        tx.commit()?;
        saved = std::time::Instant::now();
        cursor = last;
    }
    control.checkpoint()?;
    finish_rematch(collection, key, generation)
}
