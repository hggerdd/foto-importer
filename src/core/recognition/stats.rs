//! Personenscores für die Personenstatistik (`core::person_stats`): nur lesend, ändert keine
//! Zuordnung.
use super::*;
use std::collections::{HashMap, HashSet};

/// Werte einer Person für die Statistik, jeweils Personenscore für diese Person (Mittel der bis
/// zu drei besten Kosinuswerte gegen ihre Referenzen, [`top3_mean`] über `References`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PersonScores {
    pub person: i64,
    pub name: String,
    /// Offene Gesichter (vom Abgleich verglichen, nicht vom Nutzer benannt), deren beste
    /// zulässige Person diese ist, ab der Untergrenze.
    pub rated: Vec<f32>,
    /// Bestätigte Gesichter, ohne das Gesicht selbst und ohne Referenzen vom selben
    /// Aufnahmetag (sonst heben Serienbilder den Wert).
    pub confirmed: Vec<f32>,
    /// Gesichter mit gespeicherter Ablehnung „nicht diese Person“ (echte Gegenbeispiele), mit
    /// ihrem gewöhnlichen Personenscore wie im Abgleich.
    pub rejected: Vec<f32>,
}

/// Personenscore für `person` je Gesicht aus `faces` (Personenseite „Nicht X“): derselbe Wert,
/// mit dem die Statistik abgelehnte Gesichter zeigt (gewöhnlicher Personenscore gegen die
/// Referenzen der Person, das Gesicht selbst zählt nie als eigene Referenz). Gesichter ohne
/// Vektor des Modells oder eine Person ohne Referenzen fehlen im Ergebnis. Nur lesend; der
/// Wert einer Person hängt allein von ihren eigenen Referenzen ab, daher genügt ihre Spalte.
pub fn person_face_scores(
    collection: &Collection,
    key: &str,
    person: i64,
    faces: &[i64],
) -> Result<BTreeMap<i64, f32>> {
    let mut found = BTreeMap::new();
    if faces.is_empty() {
        return Ok(found);
    }
    let (entries, people) = References::load_entries(collection, key)?;
    let Some(column) = people.iter().position(|p| p.0 == person) else {
        return Ok(found);
    };
    let own = entries
        .into_iter()
        .filter(|entry| entry.1 == column)
        .map(|(face, _, vector)| (face, 0, vector))
        .collect();
    let single = References::from_entries(own, vec![people[column].clone()]);
    for chunk in faces.chunks(500) {
        let marks = vec!["?"; chunk.len()].join(",");
        let mut values: Vec<rusqlite::types::Value> = vec![key.to_string().into()];
        values.extend(chunk.iter().map(|id| rusqlite::types::Value::from(*id)));
        let mut statement = collection.db.prepare(&format!(
            "SELECT face_id,vec FROM face_embeddings WHERE embedder_id=? AND face_id IN ({marks})"
        ))?;
        let mut rows = statement.query(rusqlite::params_from_iter(values))?;
        while let Some(row) = rows.next()? {
            let face: i64 = row.get(0)?;
            let Some(bytes) = row.get::<_, Option<Vec<u8>>>(1)? else {
                continue;
            };
            let score = single.scores(&decode(&bytes)?, face)[0];
            if !score.is_nan() {
                found.insert(face, score);
            }
        }
    }
    Ok(found)
}

/// Personenscores aller Personen mit Referenzen für die Statistik. Die Werte offener Gesichter
/// kommen aus dem Zwischenspeicher des Abgleichs, wenn er für Sammlung und Modell bereitliegt
/// (wie [`person_candidates`]); sonst rechnet ein Durchgang über die Vektoren.
pub fn person_scores(
    collection: &Collection,
    key: &str,
    minimum: f32,
    control: &Control,
) -> Result<Vec<PersonScores>> {
    let owner = cache_owner(collection, key)?;
    let mut slot = Some(match_cache());
    let ready = slot
        .as_ref()
        .and_then(|slot| slot.as_ref())
        .is_some_and(|cache| cache.owner.as_ref() == Some(&owner));
    if !ready {
        slot = None;
    }
    let cache = slot.as_mut().and_then(|slot| slot.as_mut());
    person_scores_with(collection, key, minimum, control, cache)
}

/// Wie [`person_scores`] mit ausdrücklichem Zwischenspeicher (`None`: Durchgang über die
/// Vektoren). Beide Wege ergeben bitgleiche Werte.
pub fn person_scores_with(
    collection: &Collection,
    key: &str,
    minimum: f32,
    control: &Control,
    cache: Option<&mut MatchCache>,
) -> Result<Vec<PersonScores>> {
    // Ein konsistenter Lesestand für alle Abfragen.
    let tx = if collection.db.is_autocommit() {
        Some(collection.db.unchecked_transaction()?)
    } else {
        None
    };
    control.phase("Personenstatistik lesen", "Gesichter");
    let mut state = MatchState::load(collection, key)?;
    let births = load_births(collection)?;
    let (entries, people) = state.entries.clone();
    let open: Vec<i64> = {
        let mut ids: Vec<i64> = state
            .faces
            .iter()
            .filter(|f| {
                f.wanted
                    && !f.retained
                    && !f.ignored
                    && f.assignment.as_deref() != Some("user")
                    && state.embeddings.contains_key(&f.id)
            })
            .map(|f| f.id)
            .collect();
        ids.sort_unstable();
        ids
    };
    // Bestätigte Gesichter mit Aufnahmetag; der Tag aller Referenzen (auch historischer).
    let mut days: HashMap<i64, String> = HashMap::new();
    let mut confirmed: Vec<(i64, i64)> = Vec::new();
    {
        let mut statement = collection.db.prepare(
            "SELECT id,person_id,substr(coalesce(captured_at,''),1,10),
                    ignored=0 AND retained=0 AND quality>=0.5
             FROM faces WHERE assignment='user' AND person_id IS NOT NULL",
        )?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let day: String = row.get(2)?;
            if row.get::<_, bool>(3)? && state.embeddings.contains_key(&id) {
                confirmed.push((id, row.get(1)?));
            }
            if day.len() == 10 {
                days.insert(id, day);
            }
        }
    }
    // Ablehnungen aktiver Gesichter (ignorierte sind kein Gegenbeispiel).
    let rejected: Vec<(i64, i64)> = collection
        .db
        .prepare(
            "SELECT r.face_id,r.person_id FROM face_rejections r JOIN faces f ON f.id=r.face_id
             WHERE f.ignored=0 AND f.retained=0 AND f.quality>=0.5 ORDER BY r.face_id,r.person_id",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .filter(|(face, _)| state.embeddings.contains_key(face))
        .collect();
    let exclusions = Exclusions::new(&mut state, births, &open);
    let column: HashMap<i64, usize> = people
        .iter()
        .enumerate()
        .map(|(slot, p)| (p.0, slot))
        .collect();
    // Je Person nur ihre Referenzen, für bestätigte und abgelehnte Gesichter.
    let mut own: Vec<Vec<(i64, usize, Vec<f32>)>> = vec![Vec::new(); people.len()];
    for (face, slot, vector) in &entries {
        own[*slot].push((*face, 0, vector.clone()));
    }
    let single: Vec<References> = own
        .into_iter()
        .zip(&people)
        .map(|(entries, person)| References::from_entries(entries, vec![person.clone()]))
        .collect();
    let mut result: Vec<PersonScores> = people
        .iter()
        .map(|(person, name)| PersonScores {
            person: *person,
            name: name.clone(),
            ..Default::default()
        })
        .collect();
    let rate = |result: &mut Vec<PersonScores>, references: &References, id: i64, row: &[f32]| {
        if let Some(&(person, score)) = references
            .best_two(row, |p| exclusions.excluded(id, p))
            .first()
            && score >= minimum
            && let Some(slot) = column.get(&person)
        {
            result[*slot].rated.push(score);
        }
    };
    // Vektoren bestätigter und abgelehnter Gesichter (nur für Personen mit Referenzen).
    let needed: HashSet<i64> = confirmed
        .iter()
        .chain(&rejected)
        .filter(|(_, person)| column.contains_key(person))
        .map(|(face, _)| *face)
        .collect();
    let mut vectors: HashMap<i64, Vec<f32>> = HashMap::with_capacity(needed.len());
    let owner = cache_owner(collection, key)?;
    match cache.filter(|cache| cache.owner.as_ref() == Some(&owner)) {
        Some(cache) => {
            let references = cache.sync(collection, key, &mut state, control)?;
            if references.people != people {
                anyhow::bail!("Referenzen haben sich während des Lesens geändert");
            }
            control.phase("Personenstatistik: offene Gesichter", "Gesichter");
            for (done, id) in open.iter().enumerate() {
                if done % SCORE_ROUND == 0 {
                    control.checkpoint()?;
                    control.progress(done, open.len());
                }
                if let Some(face) = cache.faces.get(id)
                    && face.vector.is_some()
                {
                    rate(&mut result, &references, *id, &face.scores);
                }
            }
            for id in &needed {
                if let Some(vector) = cache.faces.get(id).and_then(|f| f.vector.clone()) {
                    vectors.insert(*id, vector);
                }
            }
        }
        None => {
            let references = References::from_entries(entries, people.clone());
            let wanted: HashSet<i64> = open.iter().copied().collect();
            control.phase("Personenstatistik: offene Gesichter", "Gesichter");
            let mut statement = collection
                .db
                .prepare("SELECT face_id,vec FROM face_embeddings WHERE embedder_id=?")?;
            let mut rows = statement.query([key])?;
            let mut batch: Vec<(i64, Vec<f32>)> = Vec::with_capacity(SCORE_ROUND);
            let mut done = 0;
            loop {
                let row = rows.next()?;
                let end = row.is_none();
                if let Some(row) = row {
                    let id: i64 = row.get(0)?;
                    let (open, keep) = (wanted.contains(&id), needed.contains(&id));
                    if (open || keep)
                        && let Some(bytes) = row.get::<_, Option<Vec<u8>>>(1)?
                    {
                        let vector = decode(&bytes)?;
                        if keep {
                            vectors.insert(id, vector.clone());
                        }
                        if open {
                            batch.push((id, vector));
                        }
                    }
                }
                if batch.len() >= SCORE_ROUND || (end && !batch.is_empty()) {
                    control.checkpoint()?;
                    let work: Vec<(i64, Option<&[f32]>)> = batch
                        .iter()
                        .map(|(id, vector)| (*id, Some(vector.as_slice())))
                        .collect();
                    for ((id, _), row) in batch.iter().zip(score_rows(&references, &work)) {
                        rate(&mut result, &references, *id, &row);
                    }
                    done += batch.len();
                    control.progress(done, open.len());
                    batch.clear();
                }
                if end {
                    break;
                }
            }
        }
    }
    if let Some(tx) = tx {
        tx.commit()?;
    }
    // Bestätigte: gegen die eigenen Referenzen ohne dasselbe Gesicht und denselben Tag;
    // abgelehnte: gewöhnlicher Personenscore (das Gesicht ist nie eigene Referenz).
    control.phase("Personenstatistik: bestätigte Gesichter", "Gesichter");
    let work: Vec<(i64, usize, bool)> = confirmed
        .iter()
        .map(|(face, person)| (*face, *person, true))
        .chain(
            rejected
                .iter()
                .map(|(face, person)| (*face, *person, false)),
        )
        .filter_map(|(face, person, own)| Some((face, *column.get(&person)?, own)))
        .filter(|(face, ..)| vectors.contains_key(face))
        .collect();
    let one = |&(face, slot, own): &(i64, usize, bool)| -> f32 {
        let query = &vectors[&face];
        let day = days.get(&face).filter(|_| own);
        single[slot].scores_without(query, |reference| {
            reference == face || day.is_some_and(|day| days.get(&reference) == Some(day))
        })[0]
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .clamp(1, crate::config::MATCH_THREADS);
    let mut done = 0;
    for round in work.chunks(SCORE_ROUND) {
        control.checkpoint()?;
        let size = round.len().div_ceil(threads).max(1);
        let values: Vec<f32> = std::thread::scope(|scope| {
            let handles: Vec<_> = round
                .chunks(size)
                .map(|chunk| scope.spawn(move || chunk.iter().map(one).collect::<Vec<_>>()))
                .collect();
            handles
                .into_iter()
                .flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
                .collect()
        });
        for ((_, slot, own), value) in round.iter().zip(values) {
            if value.is_nan() {
                continue;
            }
            let entry = &mut result[*slot];
            if *own {
                entry.confirmed.push(value);
            } else {
                entry.rejected.push(value);
            }
        }
        done += round.len();
        control.progress(done, work.len());
    }
    Ok(result)
}
