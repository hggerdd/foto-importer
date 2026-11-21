//! Namensvorschläge für ein Gesicht oder eine Gruppe (mit Porträts und Seiten-Cache).
use super::*;

/// Namensvorschläge für ein Gesicht nach dem Ähnlichkeitsvergleich (Score = Kosinus).
#[derive(Clone, Default)]
pub struct Suggestions {
    pub items: Vec<(i64, String, f32)>,
    /// Repräsentatives Gesicht je vorgeschlagener Person (Medoid).
    pub portraits: BTreeMap<i64, faces::Face>,
}
/// Je Seite geteilter Zwischenspeicher (ADR 0066): Referenzvektoren und
/// Porträts werden einmal je konsistentem Lesestand geladen. Er lebt nur so lange wie
/// der Aufruf, daher kann er nie veraltete Zuordnungen zeigen.
#[derive(Default)]
pub struct SuggestionCache {
    references: Option<References>,
    portraits: BTreeMap<i64, Option<faces::Face>>,
}
impl SuggestionCache {
    fn references(&mut self, collection: &Collection, key: &str) -> Result<&References> {
        if self.references.is_none() {
            self.references = Some(References::load(collection, key)?);
        }
        Ok(self.references.as_ref().expect("geladen"))
    }
}
fn with_portraits(
    collection: &Collection,
    cache: &mut SuggestionCache,
    mut found: Suggestions,
    key: &str,
) -> Suggestions {
    for (person, _, _) in &found.items {
        // Ein fehlendes Bild darf den Vorschlag nicht verhindern.
        let face = cache.portraits.entry(*person).or_insert_with(|| {
            faces::person_portrait(collection, *person, key)
                .ok()
                .flatten()
        });
        if let Some(face) = face {
            found.portraits.insert(*person, face.clone());
        }
    }
    found
}
pub fn suggestions(collection: &Collection, face: i64, key: &str) -> Result<Suggestions> {
    let mut cache = SuggestionCache::default();
    let mut all = scored(collection, &mut cache, face, key)?;
    all.items.truncate(3);
    Ok(with_portraits(collection, &mut cache, all, key))
}
/// Gemeinsamer Vorschlag für eine Gruppe: Mittel der Einzelwerte über
/// höchstens `config::GROUP_SUGGESTION_FACES` Gesichter (begrenzt die Lesezeit je Seite).
pub fn group_suggestions(collection: &Collection, faces: &[i64], key: &str) -> Result<Suggestions> {
    group_suggestions_cached(collection, &mut SuggestionCache::default(), faces, key)
}
pub fn group_suggestions_cached(
    collection: &Collection,
    cache: &mut SuggestionCache,
    faces: &[i64],
    key: &str,
) -> Result<Suggestions> {
    let mut totals: BTreeMap<i64, (String, f32)> = BTreeMap::new();
    let mut used = 0usize;
    for face in faces.iter().take(crate::config::GROUP_SUGGESTION_FACES) {
        let one = scored(collection, cache, *face, key)?;
        if one.items.is_empty() {
            continue;
        }
        used += 1;
        for (id, name, score) in one.items {
            totals.entry(id).or_insert((name, 0.0)).1 += score;
        }
    }
    let mut items: Vec<_> = totals
        .into_iter()
        .map(|(id, (name, sum))| (id, name, sum / used.max(1) as f32))
        .collect();
    items.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
    items.truncate(3);
    Ok(with_portraits(
        collection,
        cache,
        Suggestions {
            items,
            portraits: BTreeMap::new(),
        },
        key,
    ))
}
fn scored(
    collection: &Collection,
    cache: &mut SuggestionCache,
    face: i64,
    key: &str,
) -> Result<Suggestions> {
    let items = ranked_with(collection, cache.references(collection, key)?, face, key)?;
    Ok(Suggestions {
        items,
        portraits: BTreeMap::new(),
    })
}
