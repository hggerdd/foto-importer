//! Liste „Wahrscheinlich X“: offene Gesichter, die einer gewählten Person ähneln.
use super::*;

/// Ein Gesicht der Liste „Wahrscheinlich X“ ([`person_candidates`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PersonCandidate {
    pub face: i64,
    /// Personenscore für die gewählte Person.
    pub score: f32,
    /// Person mit dem höchsten Personenscore und ihr Wert, wenn das nicht die gewählte Person
    /// ist (zweiter Abschnitt „Eher jemand anderes“); `None`, wenn die gewählte vorn liegt.
    pub better: Option<(i64, f32)>,
}
impl PersonCandidate {
    /// Reihenfolge der Liste: erst die Gesichter, deren beste Person die gewählte ist, dann
    /// die übrigen; in beiden Abschnitten nach Personenscore absteigend, bei Gleichstand nach
    /// Gesichts-ID. Streng, weil die IDs eindeutig sind.
    fn order(&self, other: &Self) -> std::cmp::Ordering {
        self.better
            .is_some()
            .cmp(&other.better.is_some())
            .then(other.score.total_cmp(&self.score))
            .then(self.face.cmp(&other.face))
    }
}

/// Unbestätigte Gesichter, die wahrscheinlich `person` zeigen (Personenseite, „Wahrscheinlich
/// X“): Personenscore ab `minimum`, dazu die Person mit dem höchsten Personenscore, wenn es
/// eine andere ist. Der Personenscore ist der des Abgleichs (Mittel der bis zu drei besten
/// Kosinuswerte gegen die Referenzen der Person, bitgleich). Zulässig sind die Gesichter, die
/// der Abgleich vergleicht (nicht historisch, nicht ignoriert, nicht vom Nutzer benannt,
/// Qualität ab 0,5 mit Vektor), ohne seine Ausschlüsse für diese Person: abgelehnt, im Foto
/// schon bestätigt, vor dem Geburtsjahr aufgenommen. Schwelle und Abstand gelten nicht;
/// Vorschläge für andere Personen stehen mit in der Liste.
///
/// Die beste Person bestimmt der Abgleich ebenso (`References::best_two`, Ordnung Score, Name,
/// ID) über alle Personen mit Referenzen, ohne die für dieses Gesicht ausgeschlossenen. Ist sie
/// nicht `person`, steht das Gesicht im zweiten Abschnitt ([`PersonCandidate::order`]).
///
/// Die Werte kommen aus dem Zwischenspeicher des Abgleichs ([`MatchCache`]), wenn er für diese
/// Sammlung und dieses Modell bereitliegt; er wird dabei auf den aktuellen Stand gebracht.
/// Sonst (Programmstart, Abgleich läuft gerade) rechnet ein Durchgang über die Vektoren die
/// Spalte dieser Person und nur für Gesichter ab `minimum` alle Spalten, ohne
/// Zwischenspeicher aufzubauen.
pub fn person_candidates(
    collection: &Collection,
    key: &str,
    person: i64,
    minimum: f32,
    control: &Control,
) -> Result<Vec<PersonCandidate>> {
    // Die Sperre gilt nur für diesen Leser und nur, wenn der Zwischenspeicher dieser Sammlung
    // bereitliegt; der Abgleich wartet ohne Rückgabepunkt auf sie, ein gegenseitiges Warten ist
    // daher ausgeschlossen. Ohne passenden Zwischenspeicher wird sie vor dem Rechnen frei.
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
    person_candidates_with(collection, key, person, minimum, control, cache)
}

/// Wie [`person_candidates`] mit ausdrücklichem Zwischenspeicher: `None` oder ein
/// Zwischenspeicher einer anderen Sammlung rechnet in einem Durchgang über die Vektoren.
pub fn person_candidates_with(
    collection: &Collection,
    key: &str,
    person: i64,
    minimum: f32,
    control: &Control,
    cache: Option<&mut MatchCache>,
) -> Result<Vec<PersonCandidate>> {
    let mut state = MatchState::load(collection, key)?;
    let births = load_births(collection)?;
    let ids = open_for_person(&state, person, births.get(&person).copied());
    let exclusions = Exclusions::new(&mut state, births, &ids);
    let wanted = Wanted {
        person,
        minimum,
        exclusions: &exclusions,
    };
    let owner = cache_owner(collection, key)?;
    let mut found = match cache.filter(|cache| cache.owner.as_ref() == Some(&owner)) {
        Some(cache) => {
            cached_candidates(collection, key, &wanted, cache, &mut state, &ids, control)?
        }
        None => streamed_candidates(collection, key, &wanted, &ids, control)?,
    };
    found.sort_by(PersonCandidate::order);
    Ok(found)
}

/// Gesichter, die für `person` in Frage kommen (siehe [`person_candidates`]), nach ID.
fn open_for_person(state: &MatchState, person: i64, birth: Option<i32>) -> Vec<i64> {
    // Wie im Abgleich: im Foto bestätigt zählt auch für historische Gesichter.
    let occupied: BTreeSet<i64> = state
        .faces
        .iter()
        .filter(|f| f.assignment.as_deref() == Some("user") && f.person == Some(person))
        .map(|f| f.photo)
        .collect();
    let mut ids: Vec<i64> = state
        .faces
        .iter()
        .filter(|f| {
            f.wanted
                && !f.retained
                && !f.ignored
                && f.assignment.as_deref() != Some("user")
                && state.embeddings.contains_key(&f.id)
                && !occupied.contains(&f.photo)
                && !state
                    .rejections
                    .get(&f.id)
                    .is_some_and(|rejected| rejected.contains(&person))
                && !f.year.zip(birth).is_some_and(|(year, born)| year < born)
        })
        .map(|f| f.id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Ausschlüsse des Abgleichs je Gesicht (wie `photo_writes`): abgelehnte Personen, im Foto
/// bestätigte Personen (auch historischer Gesichter), vor dem Geburtsjahr aufgenommen.
struct Exclusions {
    rejections: BTreeMap<i64, BTreeSet<i64>>,
    births: BTreeMap<i64, i32>,
    /// Bestätigte Personen je Foto.
    occupied: std::collections::HashMap<i64, Vec<i64>>,
    /// Foto und Aufnahmejahr je Kandidat.
    faces: std::collections::HashMap<i64, (i64, Option<i32>)>,
}
impl Exclusions {
    /// Übernimmt die Ablehnungen aus `state` (`sync` braucht sie nicht).
    fn new(state: &mut MatchState, births: BTreeMap<i64, i32>, ids: &[i64]) -> Self {
        let wanted: std::collections::HashSet<i64> = ids.iter().copied().collect();
        let mut occupied: std::collections::HashMap<i64, Vec<i64>> = Default::default();
        let mut faces = std::collections::HashMap::with_capacity(ids.len());
        for face in &state.faces {
            if face.assignment.as_deref() == Some("user")
                && let Some(person) = face.person
            {
                occupied.entry(face.photo).or_default().push(person);
            }
            if wanted.contains(&face.id) {
                faces.insert(face.id, (face.photo, face.year));
            }
        }
        Self {
            rejections: std::mem::take(&mut state.rejections),
            births,
            occupied,
            faces,
        }
    }
    fn excluded(&self, face: i64, person: i64) -> bool {
        let Some(&(photo, year)) = self.faces.get(&face) else {
            return false;
        };
        self.rejections
            .get(&face)
            .is_some_and(|rejected| rejected.contains(&person))
            || self
                .occupied
                .get(&photo)
                .is_some_and(|occupied| occupied.contains(&person))
            || year
                .zip(self.births.get(&person))
                .is_some_and(|(year, born)| year < *born)
    }
}

/// Was [`person_candidates`] sucht: Person, Mindestwert und Ausschlüsse.
struct Wanted<'a> {
    person: i64,
    minimum: f32,
    exclusions: &'a Exclusions,
}
impl Wanted<'_> {
    /// Eintrag für `face` aus seinen Personenscores (Spalten wie `references.people`, `column`
    /// die der gewählten Person); `None` unter dem Mindestwert oder ohne Wert.
    fn candidate(
        &self,
        references: &References,
        column: usize,
        face: i64,
        scores: &[f32],
    ) -> Option<PersonCandidate> {
        let score = scores[column];
        if score.is_nan() || score < self.minimum {
            return None;
        }
        // Die gewählte Person ist für ihre Kandidaten nie ausgeschlossen (`open_for_person`).
        let best = references
            .best_two(scores, |other| {
                other != self.person && self.exclusions.excluded(face, other)
            })
            .into_iter()
            .next()
            .filter(|best| best.0 != self.person);
        Some(PersonCandidate {
            face,
            score,
            better: best,
        })
    }
}

/// Kandidaten aus dem Zwischenspeicher dieser Sammlung; er wird dabei auf den Stand gebracht.
fn cached_candidates(
    collection: &Collection,
    key: &str,
    wanted: &Wanted,
    cache: &mut MatchCache,
    state: &mut MatchState,
    ids: &[i64],
    control: &Control,
) -> Result<Vec<PersonCandidate>> {
    let references = cache.sync(collection, key, state, control)?;
    let Some(column) = references.people.iter().position(|p| p.0 == wanted.person) else {
        return Ok(Vec::new());
    };
    Ok(ids
        .iter()
        .filter_map(|id| {
            let face = cache.faces.get(id)?;
            face.vector.as_ref()?;
            wanted.candidate(&references, column, *id, &face.scores)
        })
        .collect())
}

/// Kandidaten in einem Durchgang über die Vektoren des Modells: zuerst nur die Spalte der
/// gewählten Person, für Gesichter ab dem Mindestwert dann alle Spalten (die beste Person).
/// Gleicher Rechenweg wie der Zwischenspeicher; der Wert einer Person hängt nur von ihren
/// eigenen Referenzen ab.
fn streamed_candidates(
    collection: &Collection,
    key: &str,
    wanted: &Wanted,
    ids: &[i64],
    control: &Control,
) -> Result<Vec<PersonCandidate>> {
    let (entries, people) = References::load_entries(collection, key)?;
    let Some(column) = people.iter().position(|p| p.0 == wanted.person) else {
        return Ok(Vec::new());
    };
    let own = entries
        .iter()
        .filter(|entry| entry.1 == column)
        .map(|(face, _, vector)| (*face, 0, vector.clone()))
        .collect();
    let single = References::from_entries(own, vec![people[column].clone()]);
    let all = References::from_entries(entries, people);
    let open: std::collections::HashSet<i64> = ids.iter().copied().collect();
    let mut found = Vec::new();
    let mut statement = collection
        .db
        .prepare("SELECT face_id,vec FROM face_embeddings WHERE embedder_id=?")?;
    let mut rows = statement.query([key])?;
    let mut batch: Vec<(i64, Vec<f32>)> = Vec::with_capacity(SCORE_ROUND);
    loop {
        let row = rows.next()?;
        let end = row.is_none();
        if let Some(row) = row
            && open.contains(&row.get::<_, i64>(0)?)
            && let Some(bytes) = row.get::<_, Option<Vec<u8>>>(1)?
        {
            batch.push((row.get(0)?, decode(&bytes)?));
        }
        if batch.len() >= SCORE_ROUND || (end && !batch.is_empty()) {
            control.checkpoint()?;
            let work: Vec<(i64, Option<&[f32]>)> = batch
                .iter()
                .map(|(id, vector)| (*id, Some(vector.as_slice())))
                .collect();
            let first = score_rows(&single, &work);
            let near: Vec<(i64, Option<&[f32]>)> = work
                .into_iter()
                .zip(first)
                .filter(|(_, row)| row[0] >= wanted.minimum)
                .map(|(work, _)| work)
                .collect();
            let rows = score_rows(&all, &near);
            found.extend(
                near.iter()
                    .zip(rows)
                    .filter_map(|((id, _), row)| wanted.candidate(&all, column, *id, &row)),
            );
            batch.clear();
        }
        if end {
            break;
        }
    }
    Ok(found)
}
