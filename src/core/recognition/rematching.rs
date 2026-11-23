//! Abgleich aller offenen Gesichter gegen die Referenzen: inkrementell über den
//! `MatchCache` oder vollständig Foto für Foto; Schreiben der Vorschläge je Foto.
use super::*;
use std::collections::HashMap;

/// Maschinelle Zuordnung: Person, `auto` oder `suggested`, Wert.
type Proposal = (i64, &'static str, f32);
/// Eine Änderung des Abgleichs: Zuordnung eines Gesichts (`None` entfernt die maschinelle)
/// oder seine Serienmarke (`face_burst`, `None` entfernt sie).
pub(super) enum Change {
    Assign(i64, Option<Proposal>),
    Burst(i64, Option<i64>),
}
/// Vorschlag eines Gesichts vor der Entscheidung je Foto.
#[derive(Clone, Copy)]
struct Pick {
    person: i64,
    score: f32,
    kind: &'static str,
    /// Aus einer Serie mit bestätigtem Foto (`face_burst`).
    burst: bool,
}
/// Rangfolge eines Gesichts (mindestens die ersten zwei) und Personenscore der Serienperson.
type Ranked = (Vec<(i64, f32)>, Option<f32>);
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
    /// Gespeicherte Serienmarke des Modells (Person aus `face_burst`).
    pub(super) burst: Option<i64>,
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
                    coalesce(f.retained=0 AND f.quality>=0.5,0),b.person_id
             FROM faces f LEFT JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=?1
             LEFT JOIN face_burst b ON b.face_id=f.id AND b.embedder_id=?1
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
                burst: row.get(10)?,
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
/// abweichende maschinelle Zuordnungen und Serienmarken an `writes`. `rank` liefert die
/// Rangfolge eines Gesichts ohne die übergebenen Ausschlüsse (mindestens die ersten zwei
/// Einträge) und den Personenscore der übergebenen Serienperson, `None` ohne vergleichbaren
/// Vektor. `series` ist die Serienperson je Gesicht ([`burst::Series::proposals`]).
fn photo_writes(
    faces: &[MatchFace],
    rejections: &BTreeMap<i64, BTreeSet<i64>>,
    births: &BTreeMap<i64, i32>,
    limits: &Limits,
    series: &HashMap<i64, i64>,
    mut rank: impl FnMut(&MatchFace, &dyn Fn(i64) -> bool, Option<i64>) -> Option<Ranked>,
    writes: &mut Vec<Change>,
) -> Result<()> {
    // Wie `faces::photo_faces`: nur nicht historische Gesichter, nach ID.
    let candidates: Vec<&MatchFace> = faces
        .iter()
        .filter(|f| !f.retained && !f.ignored && f.assignment.as_deref() != Some("user"))
        .collect();
    let kind = |score: f32| {
        if score >= limits.sure {
            "auto"
        } else {
            "suggested"
        }
    };
    // Je Gesicht höchstens ein Kandidat: die beste zulässige Person, wenn sie die Schwelle
    // erreicht und klar vor der zweitbesten liegt, oder die Serienperson.
    let mut proposals: Vec<Option<Pick>> = Vec::with_capacity(candidates.len());
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
        // Die Serienperson unterliegt denselben Ausschlüssen.
        let burst = series.get(&face.id).copied().filter(|p| !excluded(*p));
        let Some((ranking, burst_score)) = rank(face, &excluded, burst) else {
            proposals.push(None);
            continue;
        };
        let model = limits.accept(&ranking);
        proposals.push(match (burst, model) {
            (None, model) => model.map(|(person, score)| Pick {
                person,
                score,
                kind: kind(score),
                burst: false,
            }),
            // Modell und Serie einig: der Vorschlag des Modells, als Serie gekennzeichnet.
            (Some(x), Some((person, score))) if person == x => Some(Pick {
                person,
                score,
                kind: kind(score),
                burst: true,
            }),
            // Widerspruch: Weder Serie noch Modell setzen sich durch; das Gesicht bleibt offen
            // (unter „Benennen“), der Nutzer entscheidet.
            (Some(_), Some(_)) => None,
            // Nur die Serie: immer `suggested`, nie `auto`; Wert ist der Personenscore.
            (Some(person), None) => Some(Pick {
                person,
                score: burst_score.unwrap_or(0.0),
                kind: "suggested",
                burst: true,
            }),
        });
    }
    // Gemeinsame Entscheidung je Foto: Dieselbe Person kommt höchstens einmal vor; das
    // Gesicht mit dem höheren Wert erhält sie, ein Serienvorschlag vor jedem Modellvorschlag;
    // das andere bleibt unbenannt.
    let people: Vec<i64> = proposals
        .iter()
        .flatten()
        .map(|p| p.person)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let unknown = limits.review - 0.000001;
    let mut matrix = vec![vec![unknown; people.len() + candidates.len()]; candidates.len()];
    for (row, proposal) in proposals.iter().enumerate() {
        for (col, person) in people.iter().enumerate() {
            matrix[row][col] = match proposal {
                Some(pick) if pick.person == *person && pick.burst => 2.0 + pick.score.max(-1.0),
                Some(pick) if pick.person == *person => pick.score,
                _ => -2.0,
            };
        }
    }
    let decisions = matching::assignment(&matrix)?;
    let mut wanted: BTreeMap<i64, Pick> = BTreeMap::new();
    for (row, col) in decisions.into_iter().enumerate() {
        if col < people.len()
            && matrix[row][col] >= limits.review
            && let Some(pick) = proposals[row]
        {
            wanted.insert(candidates[row].id, pick);
        }
    }
    // Nur Abweichungen schreiben: Bisher wurden alle maschinellen Zuordnungen des Fotos
    // gelöscht und neu gesetzt, was je Gesicht Trigger der Gruppen auslöste.
    for face in faces {
        let machine = matches!(face.assignment.as_deref(), Some("auto" | "suggested"));
        let pick = wanted.get(&face.id);
        match pick {
            Some(pick) => {
                let same = face.person == Some(pick.person)
                    && face.assignment.as_deref() == Some(pick.kind)
                    && face.confidence == Some(f64::from(pick.score));
                if !same {
                    writes.push(Change::Assign(
                        face.id,
                        Some((pick.person, pick.kind, pick.score)),
                    ));
                }
            }
            None if machine => writes.push(Change::Assign(face.id, None)),
            None => {}
        }
        let mark = pick.filter(|p| p.burst).map(|p| p.person);
        if mark != face.burst {
            writes.push(Change::Burst(face.id, mark));
        }
    }
    Ok(())
}

fn write_proposals(tx: &rusqlite::Transaction<'_>, key: &str, writes: &[Change]) -> Result<()> {
    let mut marks = false;
    for change in writes {
        match change {
            Change::Assign(id, Some((person, kind, score))) => {
                tx.execute("UPDATE faces SET person_id=?,assignment=?,confidence=?,assigned_at=datetime('now') WHERE id=? AND assignment IS NOT 'user'",params![person,kind,score,id])?;
            }
            Change::Assign(id, None) => {
                tx.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=? AND assignment IN ('auto','suggested')",[id])?;
            }
            Change::Burst(id, Some(person)) => {
                marks = true;
                tx.execute(
                    "INSERT OR REPLACE INTO face_burst VALUES(?,?,?)",
                    params![id, key, person],
                )?;
            }
            Change::Burst(id, None) => {
                marks = true;
                tx.execute("DELETE FROM face_burst WHERE face_id=?", [id])?;
            }
        }
    }
    // Eine Marke allein ändert keine Zuordnung und löst keinen Trigger aus; die Seiten sollen
    // trotzdem neu lesen.
    if marks {
        tx.execute(
            "UPDATE data_revision SET revision=revision+1 WHERE domain='faces'",
            [],
        )?;
    }
    Ok(())
}

/// Serienperson je Gesicht für den vollständigen Lauf: Serien lesen und nur die Vektoren der
/// Gesichter laden, die in einer Kette mit bestätigtem Gesicht liegen.
fn full_series(collection: &Collection, key: &str) -> Result<HashMap<i64, i64>> {
    if !burst::burst_enabled(collection)? {
        return Ok(HashMap::new());
    }
    let series = burst::Series::load(collection)?;
    let mut vectors: HashMap<i64, Vec<f32>> = HashMap::new();
    let mut statement = collection
        .db
        .prepare_cached("SELECT vec FROM face_embeddings WHERE face_id=? AND embedder_id=?")?;
    for id in series.needed() {
        let bytes: Option<Option<Vec<u8>>> = statement
            .query_row(params![id, key], |r| r.get(0))
            .optional()?;
        if let Some(Some(bytes)) = bytes {
            vectors.insert(id, decode(&bytes)?);
        }
    }
    Ok(series.proposals(
        |id| vectors.get(&id).map(Vec::as_slice),
        crate::config::model_scale(key).group_join,
    ))
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
    let series = state.series.take().map_or_else(HashMap::new, |series| {
        series.proposals(
            |id| cache.faces.get(&id).and_then(|f| f.vector.as_deref()),
            crate::config::model_scale(key).group_join,
        )
    });
    let slots: HashMap<i64, usize> = references
        .people
        .iter()
        .enumerate()
        .map(|(slot, person)| (person.0, slot))
        .collect();
    let mut writes: Vec<Change> = Vec::new();
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
            &series,
            |face, excluded, burst| {
                let cached = cache.faces.get(&face.id)?;
                cached.vector.as_ref()?;
                let score = burst
                    .and_then(|p| slots.get(&p))
                    .map(|slot| cached.scores[*slot])
                    .filter(|s| !s.is_nan());
                Some((references.best_two(&cached.scores, excluded), score))
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
        write_proposals(&tx, key, chunk)?;
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
    let series = full_series(collection, key)?;
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
        let mut writes: Vec<Change> = Vec::new();
        for photo in &photos {
            control.checkpoint()?;
            let faces = batch.faces.get(photo).map_or(&[][..], Vec::as_slice);
            photo_writes(
                faces,
                &batch.rejections,
                &births,
                &limits,
                &series,
                |face, excluded, burst| {
                    face.vector.as_deref().map(|query| {
                        let ranking = references.rank(query, face.id, excluded);
                        let score = burst
                            .and_then(|p| ranking.iter().find(|r| r.0 == p))
                            .map(|r| r.1);
                        (ranking, score)
                    })
                },
                &mut writes,
            )?;
        }
        if writes.is_empty() && saved.elapsed() < std::time::Duration::from_secs(1) {
            cursor = last;
            continue;
        }
        let tx = collection.db.transaction()?;
        write_proposals(&tx, key, &writes)?;
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
