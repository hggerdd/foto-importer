use super::{
    collection::Collection,
    faces::{self, decode},
    inference,
    jobs::Control,
    matching::{self, Candidate},
};
use anyhow::{Context, Result};
use rusqlite::{OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

mod references;
use references::{Entries, ranked_with};
pub use references::{References, reference_vectors};
pub(crate) use references::{top3_mean, top3_means};
mod name_suggestions;
pub use name_suggestions::{
    SuggestionCache, Suggestions, group_suggestions, group_suggestions_cached, suggestions,
};
mod rematching;
use rematching::{MatchFace, load_births};
pub use rematching::{rematch, rematch_full, rematch_with};
mod cache;
pub use cache::MatchCache;
use cache::{MatchState, SCORE_ROUND, WRITE_CHUNK, cache_owner, match_cache, score_rows};
mod candidates;
use candidates::Exclusions;
pub use candidates::{PersonCandidate, person_candidates, person_candidates_with};
mod stats;
pub use stats::{PersonScores, person_scores, person_scores_with};

fn option(collection: &Collection, name: &str, default: f32) -> Result<f32> {
    let model = faces::setting(collection, faces::EMBEDDER_KEY)?.unwrap_or_default();
    let name = format!(
        "rust.thresholds.{model}.{}",
        name.strip_prefix("rust.").unwrap_or(name)
    );
    let raw: Option<String> = collection
        .db
        .query_row(
            "SELECT value FROM face_settings WHERE key=?",
            [&name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|v| v.parse().ok()).unwrap_or(default))
}

/// Grenzen für maschinelle Namensvorschläge im Abgleich. Vorgaben aus `config`, je Sammlung
/// und Modell überschreibbar (`rust.thresholds.<Modell>.review`, `.margin`, `.sure`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    /// Mindestwert, ab dem ein Gesicht einen Vorschlag erhält.
    pub review: f32,
    /// Mindestabstand zur zweitbesten Person.
    pub margin: f32,
    /// Ab hier gilt ein Vorschlag als sicher (`auto`), darunter als `suggested`.
    pub sure: f32,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            review: crate::config::FACE_REVIEW_MIN,
            margin: crate::config::FACE_REVIEW_MARGIN,
            sure: crate::config::FACE_SURE_MIN,
        }
    }
}
impl Limits {
    pub fn load(collection: &Collection) -> Result<Self> {
        let default = Self::default();
        Ok(Self {
            review: option(collection, "rust.review", default.review)?,
            margin: option(collection, "rust.margin", default.margin)?,
            sure: option(collection, "rust.sure", default.sure)?,
        })
    }
    /// Die Person, die ein Gesicht vorgeschlagen bekommt: die beste, wenn sie die Schwelle
    /// erreicht und mindestens `margin` vor der zweitbesten liegt. `ranking` ist absteigend
    /// sortiert und enthält nur zulässige Personen.
    pub fn accept(&self, ranking: &[(i64, f32)]) -> Option<(i64, f32)> {
        let (person, best) = *ranking.first()?;
        let second = ranking.get(1).map_or(f32::NEG_INFINITY, |r| r.1);
        (best >= self.review && best - second >= self.margin).then_some((person, best))
    }
    /// Der sichere Vorschlag einer Gruppe: der erste Eintrag, wenn er dieselbe Regel wie der
    /// Abgleich besteht (`accept`). `items` ist absteigend sortiert.
    pub fn sure_suggestion<'a>(
        &self,
        items: &'a [(i64, String, f32)],
    ) -> Option<&'a (i64, String, f32)> {
        let ranking: Vec<(i64, f32)> = items.iter().map(|(id, _, score)| (*id, *score)).collect();
        self.accept(&ranking)?;
        items.first()
    }
}
/// Zulässiger Bereich der einstellbaren Vorschlagsschwelle.
pub const REVIEW_RANGE: (f32, f32) = (0.5, 0.75);
/// Setzt die Vorschlagsschwelle der Sammlung für das aktive Modell und stößt den Abgleich
/// neu an. Bestätigte Namen bleiben unberührt.
pub fn set_review_threshold(collection: &mut Collection, value: f32) -> Result<()> {
    collection.writable()?;
    if !value.is_finite() || value < REVIEW_RANGE.0 || value > REVIEW_RANGE.1 {
        anyhow::bail!(
            "Die Schwelle muss zwischen {} und {} liegen",
            REVIEW_RANGE.0,
            REVIEW_RANGE.1
        );
    }
    let key =
        faces::setting(collection, faces::EMBEDDER_KEY)?.context("Zuerst Fotos analysieren")?;
    let tx = collection.db.transaction()?;
    tx.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![format!("rust.thresholds.{key}.review"), value.to_string()],
    )?;
    tx.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
    tx.commit()?;
    Ok(())
}

pub fn apply_knowledge(collection: &mut Collection, control: &Control) -> Result<String> {
    rebuild(collection, control, true)
}
pub fn maintain(collection: &mut Collection, control: &Control) -> Result<String> {
    rebuild(collection, control, false)
}
/// Schlüssel in `face_settings`: nach welcher Regel (`config::REFERENCE_POLICY`) die Referenzen
/// eines Modells zuletzt ausgewählt wurden.
fn policy_key(embedder: &str) -> String {
    format!("rust.reference_policy:{embedder}")
}
/// Beim Öffnen mit Schreibrecht: Wurden die Referenzen eines Modells nach einer älteren Regel
/// ausgewählt (andere oder fehlende `config::REFERENCE_POLICY`), werden alle Personen mit
/// Referenzauswahl als geändert markiert und der Abgleich vorgemerkt. Die nächste Wartung
/// wählt dann neu aus; Pins, Ausschlüsse und Zuordnungen bleiben unberührt.
pub(crate) fn renew_outdated_references(db: &rusqlite::Connection) -> Result<()> {
    let embedders: Vec<String> = db
        .prepare("SELECT DISTINCT embedder_id FROM training_set")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let current = serde_json::to_string(crate::config::REFERENCE_POLICY)?;
    for key in embedders {
        let stored: Option<String> = db
            .query_row(
                "SELECT value FROM face_settings WHERE key=?",
                [policy_key(&key)],
                |r| r.get(0),
            )
            .optional()?;
        if stored.as_deref() == Some(current.as_str()) {
            continue;
        }
        db.execute_batch("BEGIN IMMEDIATE")?;
        let result = (|| -> Result<()> {
            db.execute(
                "INSERT OR IGNORE INTO face_reference_dirty SELECT DISTINCT embedder_id,person_id FROM training_set WHERE embedder_id=? AND person_id IS NOT NULL",
                [&key],
            )?;
            db.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
            faces::save_setting(db, &policy_key(&key), crate::config::REFERENCE_POLICY)
        })();
        db.execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })?;
        result?;
    }
    Ok(())
}
pub(super) fn applied_key(embedder: &str) -> String {
    format!("rust.recognition_applied:{embedder}")
}
fn rebuild(collection: &mut Collection, control: &Control, reset: bool) -> Result<String> {
    collection.writable()?;
    control.set_face_phase(super::jobs::FacePhase::References);
    let key =
        faces::setting(collection, faces::EMBEDDER_KEY)?.context("Zuerst Fotos analysieren")?;
    // Offene Gesichter zuerst gruppieren: Abgelehnte und neu analysierte Gesichter sollen
    // nach Sekunden unter „Benennen“ stehen, nicht erst nach dem vollen Abgleich.
    while faces::prepare_groups(collection, &key, control)? > 0 {}
    if reset {
        collection.db.execute("INSERT INTO face_maintenance VALUES(?,0,1) ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",[&key])?;
    }
    select_references(collection, &key, control, reset)?;
    rematch(collection, &key, control)?;
    control.set_face_phase(super::jobs::FacePhase::Groups);
    super::grouping::rebuild_groups(collection, &key, control)?;
    Ok("Referenzen balanciert und Namen vorgeschlagen".into())
}
/// Wählt die Referenzen der geänderten Personen (`face_reference_dirty`) neu aus, wie die
/// Wartung vor dem Abgleich (für Messungen ohne Gruppenarbeit).
pub fn update_references(collection: &mut Collection, key: &str, control: &Control) -> Result<()> {
    collection.writable()?;
    select_references(collection, key, control, false)
}
fn select_references(
    collection: &mut Collection,
    key: &str,
    control: &Control,
    reset: bool,
) -> Result<()> {
    let mut people = faces::people(collection)?;
    if !reset {
        let dirty: BTreeSet<i64> = collection
            .db
            .prepare("SELECT person_id FROM face_reference_dirty WHERE embedder_id=?")?
            .query_map([&key], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        people.retain(|p| dirty.contains(&p.id));
    }
    control.progress(0, people.len());
    for (index, person) in people.iter().enumerate() {
        control.checkpoint()?;
        let mut statement=collection.db.prepare("SELECT f.id,f.quality,coalesce(f.captured_at,''),e.vec,EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id=? AND t.pinned=1) FROM faces f JOIN face_embeddings e ON e.face_id=f.id WHERE e.embedder_id=? AND f.person_id=? AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND (f.id IN (SELECT face_id FROM training_set WHERE person_id=? AND embedder_id=? AND pinned=1) OR f.id IN (SELECT id FROM (SELECT id,substr(captured_at,1,4) year,ROW_NUMBER() OVER(PARTITION BY substr(captured_at,1,4) ORDER BY quality DESC,id) rank FROM faces WHERE person_id=? AND assignment='user' AND ignored=0 AND quality>=0.5) ORDER BY rank,year,id LIMIT ?)) ORDER BY f.id")?;
        let candidates = statement
            .query_map(
                params![
                    key,
                    key,
                    person.id,
                    person.id,
                    key,
                    person.id,
                    crate::config::REFERENCE_CANDIDATES_MAX as i64
                ],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, f32>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, Vec<u8>>(3)?,
                        r.get::<_, bool>(4)?,
                    ))
                },
            )?
            .map(|r| {
                let (id, quality, captured, bytes, pinned) = r?;
                Ok(Candidate {
                    id,
                    quality,
                    captured,
                    pinned,
                    vector: decode(&bytes)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        drop(statement);
        // Die Auswahlgröße folgt der Zahl aller bestätigten Gesichter mit Merkmalen, nicht
        // nur der (begrenzten) Kandidaten.
        let confirmed: usize = collection.db.query_row(
            "SELECT count(*) FROM faces f JOIN face_embeddings e ON e.face_id=f.id AND e.embedder_id=? WHERE f.person_id=? AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5",
            params![key, person.id],
            |r| r.get(0),
        )?;
        let selection = matching::select_references(
            &candidates,
            // Pins sind Nutzerentscheidungen und gehen nie verloren, auch über der Grenze.
            crate::config::reference_limit(confirmed)
                .max(candidates.iter().filter(|c| c.pinned).count()),
            crate::config::reference_validation,
            42,
            option(collection, "rust.duplicate", 0.92)?,
            control,
        )?;
        let tx = collection.db.transaction()?;
        tx.execute(
            "DELETE FROM training_set WHERE person_id=? AND embedder_id=?",
            params![person.id, key],
        )?;
        tx.execute("DELETE FROM face_review WHERE embedder_id=? AND face_id IN (SELECT id FROM faces WHERE person_id=?)",params![key,person.id])?;
        for (split, ids) in [("ref", selection.references), ("val", selection.validation)] {
            for id in ids {
                let pinned = candidates
                    .iter()
                    .find(|r| r.id == id)
                    .is_some_and(|r| r.pinned);
                tx.execute(
                    "INSERT INTO training_set VALUES(?,?,?,?,datetime('now'),?,?)",
                    params![
                        person.id,
                        id,
                        key,
                        split,
                        if pinned { "manual_pin" } else { "rust-fps-v1" },
                        pinned
                    ],
                )?;
            }
        }
        for id in selection.outliers {
            tx.execute(
                "INSERT OR REPLACE INTO face_review VALUES(?,?,'outlier')",
                params![id, key],
            )?;
        }
        tx.execute(
            "DELETE FROM face_reference_dirty WHERE embedder_id=? AND person_id=?",
            params![key, person.id],
        )?;
        control.checkpoint()?;
        tx.commit()?;
        control.progress(index + 1, people.len());
    }
    // Alle Auswahlen dieses Modells folgen jetzt der geltenden Regel.
    let policy = policy_key(key);
    if faces::setting(collection, &policy)?.as_deref() != Some(crate::config::REFERENCE_POLICY) {
        faces::save_setting(&collection.db, &policy, crate::config::REFERENCE_POLICY)?;
    }
    if reset || !people.is_empty() {
        // Gespeicherter Altwert des entfernten Klassifikators.
        collection.db.execute(
            "DELETE FROM face_settings WHERE key LIKE 'rust.classifier:%'",
            [],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use references::Top3;

    #[test]
    fn match_cache_owner_follows_the_collection_identity_not_only_the_path() -> Result<()> {
        let root = tempfile::tempdir()?;
        let c = Collection::open(root.path(), true)?;
        let before = cache_owner(&c, "emb")?;
        assert_eq!(before, cache_owner(&c, "emb")?);
        assert_ne!(before, cache_owner(&c, "anderes")?);
        // Andere Datenbank unter demselben Pfad: anderer Eigentümer.
        c.db.execute("UPDATE collection SET id='andere-sammlung'", [])?;
        assert_ne!(before, cache_owner(&c, "emb")?);
        Ok(())
    }

    fn items(scores: &[f32]) -> Vec<(i64, String, f32)> {
        scores
            .iter()
            .enumerate()
            .map(|(i, s)| (i as i64 + 1, format!("P{i}"), *s))
            .collect()
    }

    /// Bisheriger Vergleich je Referenz, als Maßstab für die Blockrechnung.
    fn naive_rank(
        entries: &[(i64, i64, Vec<f32>)],
        names: &BTreeMap<i64, String>,
        query: &[f32],
        own: i64,
        excluded: &BTreeSet<i64>,
    ) -> Vec<(i64, f32)> {
        let mut scores: BTreeMap<i64, Vec<f32>> = BTreeMap::new();
        for (id, person, vector) in entries {
            if *id == own || excluded.contains(person) {
                continue;
            }
            if let Some(score) = inference::cosine(query, vector) {
                let entry = scores.entry(*person).or_default();
                entry.push(score);
                entry.sort_by(|a, b| b.total_cmp(a));
                entry.truncate(3);
            }
        }
        let mut result: Vec<_> = scores
            .into_iter()
            .map(|(id, s)| {
                (
                    id,
                    names[&id].clone(),
                    s.iter().sum::<f32>() / s.len() as f32,
                )
            })
            .collect();
        result.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
        result.into_iter().map(|(id, _, s)| (id, s)).collect()
    }

    #[test]
    fn block_ranking_is_bit_identical_to_the_single_comparison() {
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 40) as f32 / (1u64 << 24) as f32 - 0.5
        };
        let unit = |next: &mut dyn FnMut() -> f32, dim: usize| {
            let v: Vec<f32> = (0..dim).map(|_| next()).collect();
            inference::normalize(&v).expect("Vektor")
        };
        // 7 Personen, 61 Referenzen (kein Vielfaches von acht), dazu eine mit falscher Länge.
        let people: Vec<(i64, String)> = (0..7)
            .map(|p| (p * 3 + 1, format!("P{}", 6 - p % 4)))
            .collect();
        let names: BTreeMap<i64, String> = people.iter().cloned().collect();
        let mut entries: Vec<(i64, i64, Vec<f32>)> = Vec::new();
        for id in 0..61i64 {
            let slot = (id as usize * 5) % people.len();
            entries.push((100 + id, people[slot].0, unit(&mut next, 128)));
        }
        entries.push((999, people[2].0, unit(&mut next, 64)));
        // Doppelte Werte prüfen die Reihenfolge bei Gleichstand.
        let copy = entries[3].2.clone();
        entries.push((998, people[4].0, copy));
        entries.sort_by_key(|e| (e.1, e.0));
        let slots: BTreeMap<i64, usize> =
            people.iter().enumerate().map(|(i, p)| (p.0, i)).collect();
        let references = References::from_entries(
            entries
                .iter()
                .map(|(id, person, v)| (*id, slots[person], v.clone()))
                .collect(),
            people.clone(),
        );
        for round in 0..40 {
            let query = if round == 0 {
                entries[3].2.clone()
            } else {
                unit(&mut next, if round == 39 { 64 } else { 128 })
            };
            let own = if round % 3 == 0 { entries[round].0 } else { -1 };
            let excluded: BTreeSet<i64> = if round % 4 == 1 {
                [people[round % 7].0].into()
            } else {
                BTreeSet::new()
            };
            let fast = references.rank(&query, own, |p| excluded.contains(&p));
            let slow = naive_rank(&entries, &names, &query, own, &excluded);
            let two =
                references.best_two(&references.scores(&query, own), |p| excluded.contains(&p));
            assert_eq!(two.len(), slow.len().min(2), "Runde {round}");
            for (a, b) in two.iter().zip(&slow) {
                assert_eq!(a.0, b.0, "Runde {round}");
                assert_eq!(a.1.to_bits(), b.1.to_bits(), "Runde {round}");
            }
            assert_eq!(fast.len(), slow.len(), "Runde {round}");
            for (a, b) in fast.iter().zip(&slow) {
                assert_eq!(a.0, b.0, "Runde {round}");
                assert_eq!(a.1.to_bits(), b.1.to_bits(), "Runde {round}");
            }
        }
    }

    #[test]
    fn top_three_keeps_the_best_values_in_order() {
        let mut top = Top3::default();
        for v in [0.2, 0.9, 0.5, 0.9, 0.1, 0.7] {
            top.push(v);
        }
        assert_eq!(&top.values[..top.len], &[0.9, 0.9, 0.7]);
        assert_eq!(top.mean().to_bits(), ((0.9f32 + 0.9 + 0.7) / 3.0).to_bits());
    }

    /// Die früheren Kopien (sortieren, auf drei kürzen, summieren) und die gemeinsame
    /// Funktion ergeben auf Zufallsdaten bitgleiche Werte, auch bei Gleichständen.
    #[test]
    fn shared_top_three_matches_the_former_copies_bit_for_bit() {
        fn sorted(values: &[f32]) -> Option<f32> {
            let mut v = values.to_vec();
            v.sort_by(|a, b| b.total_cmp(a));
            v.truncate(3);
            (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
        }
        fn threshold(values: &[f32]) -> Option<f32> {
            let mut top = [f32::NEG_INFINITY; 3];
            for value in values {
                if *value > top[2] {
                    top[2] = *value;
                    top.sort_by(|x, y| y.total_cmp(x));
                }
            }
            let used = values.len().min(3);
            (used > 0).then(|| top[..used].iter().sum::<f32>() / used as f32)
        }
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..2000 {
            let len = (next() % 12) as usize;
            let values: Vec<f32> = (0..len)
                .map(|_| {
                    // Grobe Stufen erzeugen Gleichstände; `+ 0.0` vermeidet `-0.0`, das die
                    // `>`-Kopie anders einsortierte als `total_cmp` (Kosinuswerte sind praktisch
                    // nie exakt null).
                    let v = (next() >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0;
                    if round % 2 == 0 {
                        (v * 8.0).round() / 8.0 + 0.0
                    } else {
                        v
                    }
                })
                .collect();
            let shared = top3_mean(values.iter().copied()).map(f32::to_bits);
            assert_eq!(shared, sorted(&values).map(f32::to_bits), "Runde {round}");
            assert_eq!(
                shared,
                threshold(&values).map(f32::to_bits),
                "Runde {round}"
            );
            let keyed: Vec<(i64, f32)> = values
                .iter()
                .enumerate()
                .map(|(i, v)| ((i % 3) as i64, *v))
                .collect();
            for (key, mean) in top3_means(keyed.iter().copied()) {
                let own: Vec<f32> = keyed.iter().filter(|e| e.0 == key).map(|e| e.1).collect();
                assert_eq!(Some(mean.to_bits()), sorted(&own).map(f32::to_bits));
            }
        }
    }

    #[test]
    fn sure_suggestion_follows_the_rematch_rule() {
        let limits = Limits {
            review: 0.6,
            margin: 0.05,
            sure: 0.8,
        };
        let sure = |scores: &[f32]| limits.sure_suggestion(&items(scores)).map(|i| i.0);
        assert_eq!(sure(&[0.9, 0.7, 0.5]), Some(1), "klarer Vorsprung");
        assert_eq!(sure(&[0.55, 0.3]), None, "unter der Schwelle");
        assert_eq!(sure(&[0.8, 0.78]), None, "zwei knappe Kandidaten");
        assert_eq!(sure(&[0.7]), Some(1), "einziger Kandidat über der Schwelle");
        assert_eq!(sure(&[]), None);
    }
}
