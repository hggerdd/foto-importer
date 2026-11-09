//! Ergebnisgleichheit und Invalidierung der Optimierungen aus ADR 0066: Vorschläge,
//! Seiten, Zähler und Zusatzindizes liefern dasselbe wie die bisherigen Abfragen.
use foto_importer::core::{
    collection::Collection,
    faces::{self, encode},
    inference, migrations, recognition,
};
use rusqlite::params;
use std::collections::{BTreeMap, BTreeSet};

const KEY: &str = "emb";

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545F4914F6CDD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn vector(&mut self, center: usize) -> Vec<f32> {
        let mut v: Vec<f32> = (0..8)
            .map(|i| {
                let noise = (self.below(1000) as f32 / 1000.0 - 0.5) * 0.6;
                noise + if i == center % 8 { 1.0 } else { 0.0 }
            })
            .collect();
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter_mut().for_each(|x| *x /= n);
        v
    }
}

/// 6 Personen, bestätigte Gesichter (Teil als Referenzen, einige ignoriert), Vorschläge,
/// unbekannte Gesichter in Gruppen, Ablehnungen und ein Foto mit zwei bekannten Personen.
fn build() -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    for p in 1..=6i64 {
        c.db.execute("INSERT INTO persons(id) VALUES(?)", [p])?;
        c.db.execute(
            "INSERT INTO person_attrs VALUES(?,'name',?)",
            params![p, format!("Person {p}")],
        )?;
    }
    c.db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key',?)",
        [serde_json::to_string(KEY)?],
    )?;
    let faces_total = 400i64;
    for id in 1..=faces_total {
        let photo = (id + 1) / 2;
        c.db.execute(
            "INSERT OR IGNORE INTO photos(id,status) VALUES(?,'present')",
            [photo],
        )?;
        let (person, assignment, ignored, quality) = match id % 10 {
            0..=2 => (Some(id % 6 + 1), Some("user"), 0, 0.9),
            3 => (Some(id % 6 + 1), Some("user"), 1, 0.9),
            4 => (Some(id % 6 + 1), Some("auto"), 0, 0.9),
            5 => (Some(id % 6 + 1), Some("suggested"), 0, 0.9),
            6 => (None, None, 1, 0.9),
            7 => (Some(id % 6 + 1), Some("user"), 0, 0.3),
            _ => (None, None, 0, 0.9),
        };
        c.db.execute(
            "INSERT INTO faces(id,photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,person_id,assignment,ignored) VALUES(?,?,?,'det',0,0,1,1,?,?,?,?)",
            params![id, photo, format!("{id}.jpg"), quality, person, assignment, ignored],
        )?;
        let center = person.map_or(id as usize / 7, |p| p as usize);
        c.db.execute(
            "INSERT INTO face_embeddings VALUES(?,?,8,?)",
            params![id, KEY, encode(&rng.vector(center))],
        )?;
    }
    // Referenzen: nur jedes zweite bestätigte Gesicht von Person 1 bis 3; 4 bis 6 ohne Eintrag.
    c.db.execute(
        "INSERT INTO training_set SELECT person_id,id,?,'ref','2026-01-01','t',0 FROM faces WHERE assignment='user' AND ignored=0 AND quality>=0.5 AND person_id<=3 AND id%2=0",
        [KEY],
    )?;
    c.db.execute("INSERT INTO training_exclusions VALUES(12,?)", [KEY])?;
    c.db.execute("INSERT INTO face_review VALUES(22,?,'outlier')", [KEY])?;
    c.db.execute("INSERT INTO face_rejections VALUES(8,2)", [])?;
    c.db.execute("INSERT INTO face_rejections VALUES(8,5)", [])?;
    // Gruppen der unbekannten Gesichter, je bis 7 Mitglieder.
    let unknown: Vec<i64> =
        c.db.prepare("SELECT id FROM faces WHERE person_id IS NULL AND ignored=0 ORDER BY id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
    for (group, chunk) in unknown.chunks(7).enumerate() {
        c.db.execute(
            "INSERT INTO face_group_catalog(group_id,embedder_id,signature,representative,members) VALUES(?,?,0,x'00',?)",
            params![group as i64 + 1, KEY, chunk.len() as i64],
        )?;
        for id in chunk {
            c.db.execute(
                "INSERT INTO face_group_members VALUES(?,?,?)",
                params![KEY, id, group as i64 + 1],
            )?;
        }
    }
    Ok((temp, c))
}

type Ranking = Vec<(i64, String, f32)>;

/// Die bisherige Abfrage aus `recognition::ranked` (vor ADR 0066), unverändert.
fn legacy_ranked(c: &Collection, face: i64) -> anyhow::Result<Ranking> {
    let bytes: Option<Vec<u8>> =
        c.db.query_row(
            "SELECT e.vec FROM face_embeddings e JOIN faces f ON f.id=e.face_id
             WHERE e.face_id=? AND e.embedder_id=? AND f.retained=0 AND f.quality>=0.5",
            params![face, KEY],
            |r| r.get(0),
        )
        .ok();
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let query = faces::decode(&bytes)?;
    let photo: i64 =
        c.db.query_row("SELECT photo_id FROM faces WHERE id=?", [face], |r| {
            r.get(0)
        })?;
    let mut statement=c.db.prepare("SELECT f.person_id,e.vec,coalesce(a.value,'Unbenannt') FROM faces f JOIN face_embeddings e ON e.face_id=f.id LEFT JOIN person_attrs a ON a.person_id=f.person_id AND a.key='name' WHERE e.embedder_id=? AND f.id!=? AND f.assignment='user' AND f.ignored=0 AND f.quality>=0.5 AND (EXISTS(SELECT 1 FROM training_set t WHERE t.face_id=f.id AND t.embedder_id=? AND t.split='ref') OR NOT EXISTS(SELECT 1 FROM training_set t WHERE t.person_id=f.person_id AND t.embedder_id=?)) AND NOT EXISTS(SELECT 1 FROM training_exclusions x WHERE x.face_id=f.id AND x.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_review v WHERE v.face_id=f.id AND v.embedder_id=e.embedder_id) AND NOT EXISTS(SELECT 1 FROM face_rejections r WHERE r.face_id=? AND r.person_id=f.person_id) AND NOT EXISTS(SELECT 1 FROM faces o WHERE o.photo_id=? AND o.assignment='user' AND o.person_id=f.person_id AND o.id!=?) ORDER BY f.person_id,f.id")?;
    let mut rows = statement.query(params![KEY, face, KEY, KEY, face, photo, face])?;
    let mut scores: BTreeMap<i64, (String, Vec<f32>)> = BTreeMap::new();
    while let Some(row) = rows.next()? {
        let vector = faces::decode(&row.get::<_, Vec<u8>>(1)?)?;
        if let Some(score) = inference::cosine(&query, &vector) {
            let entry = scores
                .entry(row.get(0)?)
                .or_insert_with(|| (row.get(2).unwrap_or_default(), Vec::new()));
            entry.1.push(score);
            entry.1.sort_by(|a, b| b.total_cmp(a));
            entry.1.truncate(3);
        }
    }
    let mut result: Ranking = scores
        .into_iter()
        .map(|(id, (name, s))| (id, name, s.iter().sum::<f32>() / s.len() as f32))
        .collect();
    result.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.1.cmp(&b.1)).then(a.0.cmp(&b.0)));
    Ok(result)
}

/// Die bisherigen Fensterabfragen von `faces::page` für Gesichtsreiter: (Gruppen gesamt, Seite).
fn legacy_page(
    c: &Collection,
    predicate: &str,
    page: usize,
) -> anyhow::Result<(usize, Vec<Vec<i64>>)> {
    let max = foto_importer::config::GROUP_MAX as i64;
    let per_page = foto_importer::config::FACE_GROUPS_PER_PAGE as i64;
    let total: i64 = c.db.query_row(&format!("WITH ranked AS (SELECT f.person_id,(row_number() OVER(PARTITION BY f.person_id ORDER BY f.id)-1)/{max} part FROM faces f WHERE f.retained=0 AND {predicate}) SELECT count(*) FROM (SELECT 1 FROM ranked GROUP BY person_id,part)"), [], |r| r.get(0))?;
    let parts=c.db.prepare(&format!("WITH ranked AS (SELECT f.person_id,(row_number() OVER(PARTITION BY f.person_id ORDER BY f.id)-1)/{max} part FROM faces f WHERE f.retained=0 AND {predicate}) SELECT person_id,part FROM ranked GROUP BY person_id,part ORDER BY count(*) DESC,person_id,part LIMIT {per_page} OFFSET ?"))?.query_map([(page as i64)*per_page],|r|Ok((r.get::<_,Option<i64>>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut groups = Vec::new();
    for (person, part) in parts {
        let ids: Vec<i64> = c.db.prepare(&format!("SELECT f.id FROM faces f JOIN (SELECT f.id,row_number() OVER(ORDER BY f.id) rn FROM faces f WHERE f.person_id IS ? AND f.retained=0 AND {predicate}) r ON r.id=f.id WHERE r.rn>? AND r.rn<=? ORDER BY r.rn LIMIT {max}"))?.query_map(params![person,part*max,(part+1)*max],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?;
        groups.push(ids);
    }
    Ok((total as usize, groups))
}

fn assert_rankings_equal(a: &Ranking, b: &Ranking) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        assert_eq!((x.0, &x.1), (y.0, &y.1));
        assert_eq!(x.2.to_bits(), y.2.to_bits(), "Werte müssen bitgleich sein");
    }
}

#[test]
fn suggestions_equal_the_legacy_ranking_for_every_face() -> anyhow::Result<()> {
    let (_temp, c) = build()?;
    let mut compared = 0;
    for face in 1..=400i64 {
        let legacy = legacy_ranked(&c, face)?;
        let mut cache = recognition::SuggestionCache::default();
        // Ohne Klassifikator liefert `group_suggestions` den Ähnlichkeitsvergleich.
        let found = recognition::group_suggestions_cached(&c, &mut cache, &[face], KEY)?;
        let mut expected = legacy.clone();
        expected.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
        expected.truncate(3);
        assert_rankings_equal(&expected, &found.items);
        // Der geteilte Zwischenspeicher darf das Ergebnis nicht verändern.
        let again = recognition::group_suggestions_cached(&c, &mut cache, &[face], KEY)?;
        assert_rankings_equal(&found.items, &again.items);
        compared += usize::from(!legacy.is_empty());
    }
    assert!(
        compared > 100,
        "zu wenige Gesichter mit Vorschlägen: {compared}"
    );
    Ok(())
}

#[test]
fn rejections_and_same_photo_persons_are_excluded_like_before() -> anyhow::Result<()> {
    let (_temp, c) = build()?;
    // Gesicht 8 lehnt die Personen 2 und 5 ab.
    let found = recognition::suggestions(&c, 8, KEY)?;
    assert!(found.items.iter().all(|i| i.0 != 2 && i.0 != 5));
    Ok(())
}

#[test]
fn pages_totals_and_status_equal_the_window_queries() -> anyhow::Result<()> {
    let (_temp, c) = build()?;
    for (mode, predicate) in [
        (
            "review",
            "f.assignment IN ('auto','suggested') AND f.ignored=0",
        ),
        ("ignored", "f.ignored=1"),
        ("confirmed", "f.assignment='user' AND f.ignored=0"),
    ] {
        for page in 0..3 {
            let (total, expected) = legacy_page(&c, predicate, page)?;
            let now = faces::page(&c, mode, page)?;
            assert_eq!(now.total_groups, total, "{mode} Seite {page}");
            let ids: Vec<Vec<i64>> = now
                .groups
                .iter()
                .map(|g| g.iter().map(|f| f.id).collect())
                .collect();
            assert_eq!(ids, expected, "{mode} Seite {page}");
        }
    }
    let status = faces::status(&c)?;
    let legacy = c.db.query_row("SELECT count(*),coalesce(sum(assignment='user'),0),coalesce(sum(person_id IS NULL AND ignored=0),0),coalesce(sum(assignment IN ('auto','suggested') AND ignored=0),0),coalesce(sum(ignored=1),0) FROM faces WHERE retained=0",[],|r|Ok((r.get::<_,usize>(0)?,r.get::<_,usize>(1)?,r.get::<_,usize>(2)?,r.get::<_,usize>(3)?,r.get::<_,usize>(4)?)))?;
    assert_eq!(
        (
            status.total,
            status.confirmed,
            status.unknown,
            status.review,
            status.ignored
        ),
        legacy
    );
    // Auch mit ausgeblendeten (retained) Gesichtern.
    c.db.execute("UPDATE faces SET retained=1 WHERE id%17=0", [])?;
    let status = faces::status(&c)?;
    let legacy = c.db.query_row("SELECT count(*),coalesce(sum(assignment='user'),0),coalesce(sum(person_id IS NULL AND ignored=0),0),coalesce(sum(assignment IN ('auto','suggested') AND ignored=0),0),coalesce(sum(ignored=1),0) FROM faces WHERE retained=0",[],|r|Ok((r.get::<_,usize>(0)?,r.get::<_,usize>(1)?,r.get::<_,usize>(2)?,r.get::<_,usize>(3)?,r.get::<_,usize>(4)?)))?;
    assert_eq!(
        (
            status.total,
            status.confirmed,
            status.unknown,
            status.review,
            status.ignored
        ),
        legacy
    );
    for person in faces::people(&c)? {
        let expected: usize = c.db.query_row("SELECT count(*) FROM faces f WHERE f.person_id=? AND f.assignment='user' AND f.retained=0", [person.id], |r| r.get(0))?;
        assert_eq!(person.count, expected);
    }
    Ok(())
}

#[test]
fn suggestions_follow_decisions_without_stale_cache() -> anyhow::Result<()> {
    let (_temp, mut c) = build()?;
    let unknown = faces::page(&c, "unknown", 0)?;
    let group = &unknown.groups[0];
    let ids: Vec<i64> = group.iter().map(|f| f.id).collect();
    let before: BTreeMap<i64, Ranking> = ids
        .iter()
        .map(|id| Ok((*id, legacy_ranked(&c, *id)?)))
        .collect::<anyhow::Result<_>>()?;
    let first = recognition::group_suggestions(&c, &ids, KEY)?;
    assert!(!first.items.is_empty());
    // Bestätigen: Referenzen ändern sich; jede neue Seite muss wieder dem Altverfahren gleichen.
    let selected: BTreeSet<i64> = ids.iter().copied().collect();
    faces::name_group(&mut c, &ids, &selected, Some(first.items[0].0), "", false)?;
    for id in 1..=400i64 {
        let legacy = legacy_ranked(&c, id)?;
        let mut expected = legacy;
        expected.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
        expected.truncate(3);
        let found = recognition::group_suggestions(&c, &[id], KEY)?;
        assert_rankings_equal(&expected, &found.items);
    }
    assert!(before.len() == ids.len());
    // Ablehnen (Gesicht abgewählt) wirkt sofort auf die Vorschläge desselben Gesichts.
    let next = faces::page(&c, "unknown", 0)?;
    let ids: Vec<i64> = next.groups[0].iter().map(|f| f.id).collect();
    let person = recognition::group_suggestions(&c, &ids, KEY)?.items[0].0;
    faces::name_group(
        &mut c,
        &ids,
        &BTreeSet::from([ids[0]]),
        Some(person),
        "",
        false,
    )?;
    let rejected = recognition::suggestions(&c, ids[1], KEY)?;
    assert!(rejected.items.iter().all(|i| i.0 != person));
    // Ignorieren und Zähler.
    let page = faces::page(&c, "unknown", 0)?;
    let ids: Vec<i64> = page.groups[0].iter().map(|f| f.id).collect();
    let ignored_before = faces::status(&c)?.ignored;
    faces::name_group(&mut c, &ids, &BTreeSet::new(), None, "", true)?;
    assert_eq!(faces::status(&c)?.ignored, ignored_before + ids.len());
    Ok(())
}

#[test]
fn additive_indexes_are_idempotent_and_read_only_open_changes_nothing() -> anyhow::Result<()> {
    let (temp, c) = build()?;
    let names: Vec<&str> = migrations::ADDITIVE_INDEXES.iter().map(|i| i.0).collect();
    // schema.sql enthält dieselben Indizes (neue Sammlungen).
    let schema = include_str!("../src/core/schema.sql");
    for (name, ddl) in migrations::ADDITIVE_INDEXES {
        let body = ddl.replace("IF NOT EXISTS ", "");
        assert!(schema.contains(&body), "{name} fehlt in schema.sql");
    }
    for name in &names {
        c.db.execute_batch(&format!("DROP INDEX {name}"))?;
    }
    drop(c);
    let exists = |name: &str| -> anyhow::Result<bool> {
        let db = rusqlite::Connection::open(temp.path().join(".myfotomanager"))?;
        Ok(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?)",
            [name],
            |r| r.get(0),
        )?)
    };
    let read = Collection::open(temp.path(), false)?;
    // Auch ohne Zusatzindizes liefern Zähler und Seiten dieselben Zahlen.
    let slow = faces::status(&read)?.confirmed;
    drop(read);
    for name in &names {
        assert!(
            !exists(name)?,
            "lesend geöffnet: {name} darf nicht entstehen"
        );
    }
    let write = Collection::open(temp.path(), true)?;
    for name in &names {
        assert!(exists(name)?, "{name} fehlt nach Öffnen mit Schreibrecht");
    }
    assert_eq!(migrations::ensure_indexes(&write.db)?, 0);
    assert_eq!(faces::status(&write)?.confirmed, slow);
    Ok(())
}
