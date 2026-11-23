//! Lesende Abfragen für die Gesichterseiten (Gruppen, Vorschläge, Prüfen, ungeeignet)
//! und die Gesichter eines Fotos.
use super::*;

pub fn photo_faces(collection: &Collection, photo: i64) -> Result<Vec<Face>> {
    Ok(collection
        .db
        .prepare(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f WHERE f.photo_id=? AND f.retained=0 AND f.ignored<>2 ORDER BY f.id"
        ))?
        .query_map([photo], face_row)?
        .collect::<rusqlite::Result<_>>()?)
}
pub fn photo_analysis_done(collection: &Collection, photo: i64) -> Result<bool> {
    let Some(detector) = active_detector_key(collection)? else {
        return Ok(false);
    };
    Ok(collection.db.query_row(
        "SELECT EXISTS(SELECT 1 FROM face_jobs WHERE photo_id=? AND stage='detect' AND model_id=? AND status='done')",
        params![photo, detector],
        |row| row.get(0),
    )?)
}
pub fn active_detector_key(collection: &Collection) -> Result<Option<String>> {
    Ok(setting(collection, "rust.detector_key")?.map(|key| inference::presence_detector_key(&key)))
}
/// Abgelehnte Personen für alle Gesichter der Seite mit einer einzigen Abfrage.
fn page_rejections(
    collection: &Collection,
    groups: &[Vec<Face>],
) -> Result<BTreeMap<i64, Vec<i64>>> {
    let ids: Vec<i64> = groups.iter().flatten().map(|face| face.id).collect();
    let mut found: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
    if ids.is_empty() {
        return Ok(found);
    }
    let marks = vec!["?"; ids.len()].join(",");
    let mut statement = collection.db.prepare(&format!(
        "SELECT face_id,person_id FROM face_rejections WHERE face_id IN ({marks}) ORDER BY face_id,person_id"
    ))?;
    let rows = statement.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (face, person) = row?;
        found.entry(face).or_default().push(person);
    }
    Ok(found)
}
/// Gesichts-IDs je Teilgruppe und offene Vorschläge je Person.
type ReviewParts = (Vec<Vec<i64>>, BTreeMap<i64, usize>);
/// Teilgruppen des Reiters „Vorschläge prüfen“ als Gesichts-IDs, sicherste zuerst, dazu
/// die Zahl offener Vorschläge je Person.
///
/// Je Person werden die Vorschläge nach Ähnlichkeit absteigend in Teile zu höchstens
/// `size` (Anzeigegröße) geschnitten; die Teile aller Personen sind nach ihrer **mittleren
/// Ähnlichkeit** absteigend geordnet (bei Gleichstand Person, dann Teil). Das Mittel statt
/// des besten Werts verhindert, dass ein einzelner hoher Treffer eine schwache Gruppe nach
/// vorn zieht. Eine Abfrage über den Teilindex `ix_faces_review`, keine Fensterfunktion.
fn review_parts(collection: &Collection, only: Option<i64>, size: usize) -> Result<ReviewParts> {
    let mut by_person: BTreeMap<Option<i64>, Vec<(f32, i64)>> = BTreeMap::new();
    let mut statement = collection.db.prepare_cached(REVIEW_ROWS)?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        by_person.entry(row.get(0)?).or_default().push((
            row.get::<_, Option<f32>>(1)?.unwrap_or_default(),
            row.get(2)?,
        ));
    }
    let people = by_person
        .iter()
        .filter_map(|(person, faces)| Some(((*person)?, faces.len())))
        .collect();
    let mut parts: Vec<(f32, Option<i64>, usize, Vec<i64>)> = Vec::new();
    for (person, mut faces) in by_person {
        if only.is_some() && person != only {
            continue;
        }
        faces.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        for (part, chunk) in faces.chunks(size).enumerate() {
            let mean = chunk.iter().map(|face| face.0).sum::<f32>() / chunk.len() as f32;
            parts.push((mean, person, part, chunk.iter().map(|f| f.1).collect()));
        }
    }
    parts.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    Ok((parts.into_iter().map(|part| part.3).collect(), people))
}
/// Alle offenen Vorschläge mit Person und Ähnlichkeit (Reiter „Vorschläge prüfen“).
pub const REVIEW_ROWS: &str = "SELECT f.person_id,f.confidence,f.id FROM faces f WHERE f.retained=0 AND f.assignment IN ('suggested','auto') AND f.ignored=0 ORDER BY f.person_id,f.id";
/// Gesichter in genau der Reihenfolge von `ids`; inzwischen entfallene fehlen.
fn faces_in_order(collection: &Collection, ids: &[i64]) -> Result<Vec<Face>> {
    let marks = vec!["?"; ids.len()].join(",");
    let mut found: BTreeMap<i64, Face> = collection
        .db
        .prepare_cached(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f WHERE f.id IN ({marks})"
        ))?
        .query_map(rusqlite::params_from_iter(ids.iter()), face_row)?
        .map(|face| face.map(|face| (face.id, face)))
        .collect::<rusqlite::Result<_>>()?;
    Ok(ids.iter().filter_map(|id| found.remove(id)).collect())
}
/// Seite des Reiters „unsuitable“: offene Gesichter ohne Gruppe ([`UNSUITABLE`]), neueste
/// Fotos zuerst und Gesichter eines Fotos nebeneinander, in Gruppen zu `size`.
/// Ergebnis: Gruppen der Seite und Zahl aller solcher Gesichter.
fn unsuitable_page(
    collection: &Collection,
    embedder: &str,
    page: usize,
    size: usize,
) -> Result<(Vec<Vec<Face>>, usize)> {
    let total: usize = collection.db.query_row(
        &format!("SELECT count(*) FROM faces f WHERE {UNSUITABLE}"),
        [embedder],
        |r| r.get(0),
    )?;
    let per_page = (size * crate::config::face_groups_per_page(size)) as i64;
    let rows: Vec<Face> = collection
        .db
        .prepare_cached(&format!(
            "SELECT {FACE_COLUMNS} FROM faces f WHERE {UNSUITABLE} ORDER BY f.photo_id DESC,f.id LIMIT ?2 OFFSET ?3"
        ))?
        .query_map(params![embedder, per_page, page as i64 * per_page], face_row)?
        .collect::<rusqlite::Result<_>>()?;
    let groups = rows.chunks(size).map(<[Face]>::to_vec).collect();
    Ok((groups, total))
}
/// Seite des Reiters „unknown“ („Benennen“): die gespeicherten Ähnlichkeitsgruppen des
/// Embedders (höchstens `CLUSTER_MAX` Mitglieder), größte zuerst. Eine Gruppe über der
/// Anzeigegröße `size` erscheint in aufeinanderfolgenden Teilen zu `size` (nach Gesichts-ID);
/// Gruppen werden nie zusammengelegt, weil zwei Gruppen einander nicht ähnlich genug sein
/// müssen. Ergebnis: Gruppen der Seite und Zahl aller Teile.
fn unknown_page(
    collection: &Collection,
    embedder: &str,
    page: usize,
    size: usize,
) -> Result<(Vec<Vec<Face>>, usize)> {
    let total: i64 = collection.db.query_row(
        "SELECT coalesce(sum((members+?2-1)/?2),0) FROM face_group_catalog WHERE embedder_id=?1 AND members>0",
        params![embedder, size as i64],
        |r| r.get(0),
    )?;
    let per_page = crate::config::face_groups_per_page(size);
    let skip = page * per_page;
    let over = crate::config::CLUSTER_MAX + 1;
    let mut groups = Vec::new();
    // Teile vor dieser Gruppe; der Katalog zählt die offenen Mitglieder (Trigger).
    let mut before = 0;
    let mut clusters = collection.db.prepare_cached(
        "SELECT group_id,members FROM face_group_catalog WHERE embedder_id=? AND members>0 ORDER BY members DESC,group_id",
    )?;
    let mut rows = clusters.query([embedder])?;
    while groups.len() < per_page
        && let Some(row) = rows.next()?
    {
        let (id, members) = (row.get::<_, i64>(0)?, row.get::<_, i64>(1)? as usize);
        let parts = members.div_ceil(size);
        if before + parts <= skip {
            before += parts;
            continue;
        }
        let cluster=collection.db.prepare_cached(&format!("SELECT {FACE_COLUMNS} FROM faces f JOIN face_group_members g ON g.face_id=f.id WHERE g.group_id=? AND f.retained=0 AND f.ignored=0 AND f.person_id IS NULL ORDER BY f.id LIMIT {over}"))?.query_map([id],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        if cluster.len() <= crate::config::CLUSTER_MAX {
            let rest = per_page - groups.len();
            groups.extend(
                cluster
                    .chunks(size)
                    .skip(skip.saturating_sub(before))
                    .take(rest)
                    .map(<[Face]>::to_vec),
            );
        }
        before += parts;
    }
    Ok((groups, total as usize))
}
/// Seite des Reiters „probable“ (Gruppen der Seite und alles, was die Seite dazu zeigt).
#[derive(Default)]
struct ProbablePage {
    groups: Vec<Vec<Face>>,
    total_groups: usize,
    faces_total: usize,
    others: usize,
    better: BTreeMap<i64, (i64, f32)>,
}
/// Seite des Reiters „probable“: unbestätigte Gesichter, die wahrscheinlich `person` zeigen
/// (`recognition::person_candidates`). Zuerst die Gesichter, deren beste Person `person` ist,
/// dann („Eher jemand anderes“) die, bei denen eine andere Person vorn liegt; jeder Abschnitt
/// nach Personenscore absteigend für sich in Gruppen zu `size` geschnitten, sodass keine
/// Gruppe beide Abschnitte mischt. Jedes Gesicht trägt für die Seite `person` als Vorschlag
/// und seinen Personenscore als `score`, damit die Karte wie in „Vorschläge prüfen“ bestätigt
/// und ablehnt.
fn probable_page(
    collection: &Collection,
    embedder: &str,
    person: Option<i64>,
    page: usize,
    size: usize,
    control: &Control,
) -> Result<ProbablePage> {
    let Some(person) = person.filter(|_| !embedder.is_empty()) else {
        return Ok(ProbablePage::default());
    };
    let candidates = crate::core::recognition::person_candidates(
        collection,
        embedder,
        person,
        crate::config::PERSON_CANDIDATE_MIN,
        control,
    )?;
    let split = candidates.partition_point(|c| c.better.is_none());
    let (first, second) = candidates.split_at(split);
    let parts: Vec<&[crate::core::recognition::PersonCandidate]> =
        first.chunks(size).chain(second.chunks(size)).collect();
    let mut result = ProbablePage {
        total_groups: parts.len(),
        faces_total: candidates.len(),
        others: second.len(),
        ..ProbablePage::default()
    };
    let per_page = crate::config::face_groups_per_page(size);
    for part in parts.iter().skip(page * per_page).take(per_page) {
        let ids: Vec<i64> = part.iter().map(|c| c.face).collect();
        let scores: BTreeMap<i64, f32> = part.iter().map(|c| (c.face, c.score)).collect();
        result
            .better
            .extend(part.iter().filter_map(|c| Some((c.face, c.better?))));
        let mut rows = faces_in_order(collection, &ids)?;
        for face in &mut rows {
            face.person = Some(person);
            face.score = scores[&face.id];
        }
        if !rows.is_empty() {
            result.groups.push(rows);
        }
    }
    Ok(result)
}
/// Seite `page` des Reiters `mode` mit der voreingestellten Gruppengröße.
pub fn page(collection: &Collection, mode: &str, page: usize) -> Result<FacePage> {
    page_filtered(collection, mode, page, None)
}
/// Wie [`page`], zusätzlich mit Personenfilter: `person` beschränkt die Reiter „review“,
/// „confirmed“ und „ignored“ auf Gesichter dieser Person und wählt die Person der Reiter
/// „probable“ und „person-confirmed“ (ohne Person leer); andere Reiter beachten ihn nicht.
pub fn page_filtered(
    collection: &Collection,
    mode: &str,
    page: usize,
    person: Option<i64>,
) -> Result<FacePage> {
    page_with(
        collection,
        mode,
        page,
        person,
        crate::config::GROUP_SIZE_DEFAULT,
        &Control::default(),
    )
}
/// Wie [`page_filtered`] mit der Anzeigegröße `group_size` (Gesichter je Gruppe, begrenzt
/// durch `config::group_size`; die Gruppen je Seite folgen `config::face_groups_per_page`).
/// Alle Reiter mit Gesichtsgruppen schneiden mit dieser einen Größe. `control` bricht das
/// Rechnen der Personenwerte in „probable“ ab.
pub fn page_with(
    collection: &Collection,
    mode: &str,
    page: usize,
    person: Option<i64>,
    group_size: usize,
    control: &Control,
) -> Result<FacePage> {
    let size = crate::config::group_size(group_size);
    let max = size as i64;
    let per_page = crate::config::face_groups_per_page(size);
    let status = status(collection)?;
    let mut groups = Vec::new();
    let total_groups: usize;
    let mut faces_total = 0;
    let mut review_people = BTreeMap::new();
    let mut probable_others = 0;
    let mut better = BTreeMap::new();
    if matches!(mode, "persons" | "tree" | "graph") {
        // Diese Reiter zeigen keine Gesichtsgruppen: die teure Gruppenabfrage entfällt.
        let (relations, family, questions) = tree_state(collection)?;
        let people = people(collection)?;
        let (rows, portraits) =
            table_state(collection, mode, &people, &relations, &family, &questions)?;
        return Ok(FacePage {
            groups,
            people,
            status,
            suggestions: BTreeMap::new(),
            total_groups: 0,
            relations,
            family: Box::new(family),
            questions,
            rows,
            portraits,
            rejections: BTreeMap::new(),
            review_people: BTreeMap::new(),
            faces_total: 0,
            probable_others: 0,
            better: BTreeMap::new(),
            group_size: size,
            revisions: None,
        });
    }
    if mode == "unknown" && status.embedder.is_empty() {
        let (relations, family, questions) = tree_state(collection)?;
        return Ok(FacePage {
            groups,
            people: people(collection)?,
            status,
            suggestions: BTreeMap::new(),
            total_groups: 0,
            relations,
            family: Box::new(family),
            questions,
            rows: Vec::new(),
            portraits: BTreeMap::new(),
            rejections: BTreeMap::new(),
            review_people: BTreeMap::new(),
            faces_total: 0,
            probable_others: 0,
            better: BTreeMap::new(),
            group_size: size,
            revisions: None,
        });
    }
    if mode == "unsuitable" {
        (groups, faces_total) = unsuitable_page(collection, &status.embedder, page, size)?;
        total_groups = faces_total.div_ceil(size);
    } else if mode == "probable" {
        let probable = probable_page(collection, &status.embedder, person, page, size, control)?;
        (groups, total_groups, faces_total) =
            (probable.groups, probable.total_groups, probable.faces_total);
        (probable_others, better) = (probable.others, probable.better);
    } else if mode == "person-confirmed" && person.is_none() {
        total_groups = 0;
    } else if mode == "unknown" && !status.embedder.is_empty() {
        (groups, total_groups) = unknown_page(collection, &status.embedder, page, size)?;
    } else if mode == "review" {
        let (parts, people) = review_parts(collection, person, size)?;
        review_people = people;
        total_groups = parts.len();
        for ids in parts.iter().skip(page * per_page).take(per_page) {
            let rows = faces_in_order(collection, ids)?;
            if !rows.is_empty() {
                groups.push(rows);
            }
        }
    } else {
        // Teilgruppen je Person zu höchstens `max` Gesichtern; die Reihenfolge (größte zuerst,
        // dann Person, dann Teil) entsteht aus den Zählungen je Person statt aus Fensterfunktionen
        // über alle Gesichter des Reiters (ADR 0066).
        let predicate = match mode {
            "ignored" => "f.ignored=1",
            "confirmed" | "person-confirmed" => "f.assignment='user' AND f.ignored=0",
            _ => "f.person_id IS NULL AND f.ignored=0",
        };
        let predicate = match person {
            Some(id) if matches!(mode, "ignored" | "confirmed" | "person-confirmed") => {
                format!("{predicate} AND f.person_id={id}")
            }
            _ => predicate.to_string(),
        };
        let counts: Vec<(Option<i64>, i64)> = collection
            .db
            .prepare(&format!("SELECT f.person_id,count(*) FROM faces f WHERE f.retained=0 AND {predicate} GROUP BY f.person_id ORDER BY f.person_id"))?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        faces_total = counts.iter().map(|(_, count)| *count as usize).sum();
        let mut parts: Vec<(i64, Option<i64>, i64)> = Vec::new();
        for (person, count) in &counts {
            for part in 0..(count + max - 1) / max {
                parts.push(((count - part * max).min(max), *person, part));
            }
        }
        // SQLite ordnet NULL vor Zahlen; `Option` verhält sich ebenso.
        parts.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        total_groups = parts.len();
        for (_, person, part) in parts.into_iter().skip(page * per_page).take(per_page) {
            let rows=collection.db.prepare_cached(&format!("SELECT {FACE_COLUMNS} FROM faces f WHERE f.person_id IS ? AND f.retained=0 AND {predicate} ORDER BY f.id LIMIT {max} OFFSET ?"))?.query_map(params![person,part*max],face_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
            if !rows.is_empty() {
                groups.push(rows);
            }
        }
    }
    let mut suggestions = BTreeMap::new();
    // Ungeeignete Gesichter haben keine Namensmerkmale, also auch keine Vorschläge.
    if !status.embedder.is_empty() && mode != "unsuitable" {
        let mut cache = crate::core::recognition::SuggestionCache::default();
        for group in groups.iter().filter(|g| g[0].person.is_none()) {
            let ids: Vec<i64> = group.iter().map(|f| f.id).collect();
            // Fehlende Vorschläge dürfen die Gruppenansicht nicht blockieren.
            if let Ok(found) = crate::core::recognition::group_suggestions_cached(
                collection,
                &mut cache,
                &ids,
                &status.embedder,
            ) && !found.items.is_empty()
            {
                suggestions.insert(group[0].id, found);
            }
        }
    }
    let rejections = page_rejections(collection, &groups)?;
    let (relations, family, questions) = tree_state(collection)?;
    let people = people(collection)?;
    let (rows, mut portraits) =
        table_state(collection, mode, &people, &relations, &family, &questions)?;
    if matches!(mode, "review" | "probable" | "person-confirmed") && !status.embedder.is_empty() {
        for id in groups.iter().filter_map(|group| group[0].person) {
            // Ein fehlendes Bild darf die Seite nicht verhindern.
            if !portraits.contains_key(&id)
                && let Ok(Some(face)) = person_portrait(collection, id, &status.embedder)
            {
                portraits.insert(id, face);
            }
        }
    }
    Ok(FacePage {
        groups,
        people,
        status,
        suggestions,
        total_groups,
        relations,
        family: Box::new(family),
        questions,
        rows,
        portraits,
        rejections,
        review_people,
        faces_total,
        probable_others,
        better,
        group_size: size,
        revisions: None,
    })
}
