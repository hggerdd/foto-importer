//! Porträts: das repräsentative bestätigte Gesicht je Person (Medoid).
use super::*;

/// Das „durchschnittlichste“ bestätigte Gesicht einer Person: das Medoid ihrer
/// hochwertigen Embeddings (höchste mittlere Ähnlichkeit zu den übrigen).
pub fn person_portrait(
    collection: &Collection,
    person: i64,
    embedder: &str,
) -> Result<Option<Face>> {
    let rows: Vec<(i64, Vec<u8>)> = collection
        .db
        .prepare(
            "SELECT f.id,e.vec FROM faces f JOIN face_embeddings e ON e.face_id=f.id
             WHERE f.person_id=? AND f.assignment='user' AND f.ignored=0 AND f.retained=0
               AND f.quality>=0.5 AND e.embedder_id=?
             ORDER BY f.quality DESC,f.id LIMIT 100",
        )?
        .query_map(params![person, embedder], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let vectors: Vec<(i64, Vec<f32>)> = rows
        .into_iter()
        .filter_map(|(id, bytes)| Some((id, inference::normalize(&decode(&bytes).ok()?).ok()?)))
        .collect();
    let best = medoid(&vectors);
    let Some(id) = best else {
        return Ok(None);
    };
    Ok(collection
        .db
        .query_row(
            &format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.id=?"),
            [id],
            face_row,
        )
        .optional()?)
}
/// ID des Vektors mit der höchsten mittleren Ähnlichkeit zu den übrigen (Medoid);
/// bei Gleichstand die kleinere ID.
fn medoid(vectors: &[(i64, Vec<f32>)]) -> Option<i64> {
    vectors
        .iter()
        .map(|(id, v)| {
            let total: f32 = vectors
                .iter()
                .filter(|(other, _)| other != id)
                .filter_map(|(_, w)| inference::cosine(v, w))
                .sum();
            (*id, total)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(id, _)| id)
}
/// Porträt für viele Personen auf einmal (Personentabelle, Personengraph): je Person das
/// bestätigte Gesicht mit der besten Qualität, in einer Fensterabfrage ohne Merkmalsvektoren.
pub fn person_portraits(collection: &Collection, persons: &[i64]) -> Result<BTreeMap<i64, Face>> {
    let wanted: BTreeSet<i64> = persons.iter().copied().collect();
    let mut chosen: BTreeMap<i64, i64> = BTreeMap::new();
    let fallback: Vec<(i64, i64)> = collection
        .db
        .prepare(
            "SELECT person_id,id FROM (
               SELECT person_id,id,
                      row_number() OVER (PARTITION BY person_id ORDER BY coalesce(quality,0) DESC,id) rn
               FROM faces WHERE assignment='user' AND ignored=0 AND retained=0 AND person_id IS NOT NULL
                 AND bbox_x1 IS NOT NULL AND bbox_y1 IS NOT NULL AND bbox_x2 IS NOT NULL AND bbox_y2 IS NOT NULL
             ) WHERE rn=1",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    for (person, id) in fallback {
        if wanted.contains(&person) {
            chosen.entry(person).or_insert(id);
        }
    }
    let mut result = BTreeMap::new();
    for (person, id) in chosen {
        // Ein einzelnes defektes Gesicht darf die übrigen Porträts nicht verhindern.
        if let Some(face) = collection
            .db
            .query_row(
                &format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.id=?"),
                [id],
                face_row,
            )
            .optional()
            .ok()
            .flatten()
        {
            result.insert(person, face);
        }
    }
    Ok(result)
}
