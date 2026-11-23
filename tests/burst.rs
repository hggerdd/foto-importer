//! Serienbilder: Bestätigte Gesichter schlagen ihre Person für Gesichter an derselben Stelle
//! in Fotos derselben Serie vor (gleicher Ordner, gleiche Kamera, wenige Sekunden), nie als
//! Bestätigung. Synthetische Sammlung, keine Modelle.
use foto_importer::core::{collection::Collection, faces, jobs::Control, migrations, recognition};
use rusqlite::params;

const KEY: &str = "test";
const ANNA: i64 = 1;
const BEN: i64 = 2;
/// Rahmen in der Bildmitte; `SHIFTED` überdeckt ihn kaum.
const CENTER: [f32; 4] = [0.40, 0.30, 0.50, 0.45];
const SHIFTED: [f32; 4] = [0.47, 0.30, 0.57, 0.45];
const LEFT: [f32; 4] = [0.05, 0.30, 0.15, 0.45];

/// Einheitsvektor mit Kosinus `anna` zu Anna (`e0`) und `ben` zu Ben (`e1`), Rest in `e2`.
fn vector(anna: f32, ben: f32) -> Vec<f32> {
    vector_on(anna, ben, 2)
}
/// Wie [`vector`], Rest in Achse `axis` (andere Achsen sind einander unähnlich).
fn vector_on(anna: f32, ben: f32, axis: usize) -> Vec<f32> {
    let rest = (1.0 - anna * anna - ben * ben).max(0.0).sqrt();
    let mut v = vec![0.0; 8];
    v[0] = anna;
    v[1] = ben;
    v[axis] = rest;
    v
}

/// Gesicht, Person, Zuordnung, Serienmarke.
type Row = (i64, Option<i64>, Option<String>, Option<i64>);

struct Fixture {
    _temp: tempfile::TempDir,
    c: Collection,
}

impl Fixture {
    /// Sammlung mit Anna und Ben (je eine Referenz außerhalb jeder Serie) und Schwelle 0,70,
    /// damit Gesichter mit Personenscore 0,65 nur über die Serie einen Vorschlag bekommen.
    fn new() -> anyhow::Result<Self> {
        let temp = tempfile::tempdir()?;
        let c = Collection::open(temp.path(), true)?;
        c.db.execute_batch(
            "INSERT OR REPLACE INTO face_settings VALUES('rust.embedder_key','\"test\"');
             INSERT OR REPLACE INTO face_settings VALUES('rust.thresholds.test.review','0.7');
             INSERT OR REPLACE INTO face_settings VALUES('rust.thresholds.test.sure','0.8');
             INSERT INTO persons(id) VALUES(1),(2);
             INSERT INTO person_attrs VALUES(1,'name','Anna'),(2,'name','Ben');",
        )?;
        let fixture = Self { _temp: temp, c };
        // Referenzen in eigenen Ordnern, Jahre auseinander.
        fixture.shot(900, "anna/ref.jpg", "2001:01:01 10:00:00", None, "1")?;
        fixture.face(900, 900, CENTER, &vector(1.0, 0.0))?;
        fixture.confirm(900, ANNA)?;
        fixture.shot(901, "ben/ref.jpg", "2002:01:01 10:00:00", None, "1")?;
        fixture.face(901, 901, CENTER, &vector(0.0, 1.0))?;
        fixture.confirm(901, BEN)?;
        Ok(fixture)
    }
    /// Foto `photo` mit einer Datei (ID = Foto) und EXIF-Zeit, Kamera „Z 6“ mit `serial`.
    fn shot(
        &self,
        photo: i64,
        relpath: &str,
        date: &str,
        subsec: Option<&str>,
        serial: &str,
    ) -> anyhow::Result<()> {
        let (dir, name) = relpath.rsplit_once('/').unwrap_or(("", relpath));
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        self.c
            .db
            .execute("INSERT INTO photos(id,status) VALUES(?,'present')", [photo])?;
        self.c.db.execute(
            "INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state)
             VALUES(?,?,?,?,?,?,'jpg',1,1,?,'present','done')",
            params![photo, relpath, relpath.to_lowercase(), dir, name, stem, photo],
        )?;
        self.c.db.execute(
            "INSERT INTO meta(file_id,make,model,serial,dt_original,subsec) VALUES(?,'NIKON','Z 6',?,?,?)",
            params![photo, serial, date, subsec],
        )?;
        Ok(())
    }
    /// Gesicht `id` auf Foto `photo` (Quelle ist dessen Datei) mit Rahmen und Vektor.
    fn face(&self, id: i64, photo: i64, bbox: [f32; 4], vector: &[f32]) -> anyhow::Result<()> {
        let (relpath, date): (String, String) = self.c.db.query_row(
            "SELECT f.relpath,m.dt_original FROM files f JOIN meta m ON m.file_id=f.id WHERE f.id=?",
            [photo],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let captured = format!("{}-{}-{}", &date[..4], &date[5..7], &date[8..10]);
        self.c.db.execute(
            "INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,captured_at)
             VALUES(?,?,?,?,'det',?,?,?,?,0.9,?)",
            params![id, photo, photo, relpath, bbox[0], bbox[1], bbox[2], bbox[3], captured],
        )?;
        self.c.db.execute(
            "INSERT INTO face_embeddings VALUES(?,?,?,?)",
            params![id, KEY, vector.len() as i64, faces::encode(vector)],
        )?;
        Ok(())
    }
    fn confirm(&self, id: i64, person: i64) -> anyhow::Result<()> {
        self.c.db.execute(
            "UPDATE faces SET person_id=?,assignment='user',confidence=1 WHERE id=?",
            params![person, id],
        )?;
        Ok(())
    }
    fn rematch(&mut self) -> anyhow::Result<()> {
        recognition::rematch_full(&mut self.c, KEY, &Control::default())
    }
    /// Person, Zuordnung und Serienmarke eines Gesichts.
    fn state(&self, id: i64) -> anyhow::Result<(Option<i64>, Option<String>, Option<i64>)> {
        Ok(self.c.db.query_row(
            "SELECT f.person_id,f.assignment,b.person_id FROM faces f LEFT JOIN face_burst b ON b.face_id=f.id WHERE f.id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }
    fn suggested(&self, id: i64, person: i64) -> anyhow::Result<bool> {
        let (p, assignment, mark) = self.state(id)?;
        assert_ne!(
            assignment.as_deref(),
            Some("user"),
            "Gesicht {id} nie bestätigt"
        );
        Ok(p == Some(person) && mark == Some(person))
    }
    fn open(&self, id: i64) -> anyhow::Result<bool> {
        Ok(self.state(id)? == (None, None, None))
    }
}

/// Serie im Ordner „fest“: Foto 1 zeigt die bestätigte Anna (Gesicht 1), Fotos 2 bis 4 folgen
/// im Abstand von zwei Sekunden mit Gesichtern an derselben Stelle (Personenscore 0,65, unter
/// der Schwelle 0,70). Foto 4 liegt vier Sekunden nach Foto 1, hängt aber über Foto 3 an der
/// Kette.
fn series(f: &Fixture) -> anyhow::Result<()> {
    for (photo, time) in [(1, "00"), (2, "02"), (3, "04"), (4, "06")] {
        f.shot(
            photo,
            &format!("fest/{photo}.jpg"),
            &format!("2024:05:01 12:00:{time}"),
            Some("50"),
            "42",
        )?;
    }
    f.face(1, 1, CENTER, &vector(1.0, 0.0))?;
    f.confirm(1, ANNA)?;
    for id in 2..=4 {
        f.face(id, id, CENTER, &vector(0.65, 0.0))?;
    }
    Ok(())
}

#[test]
fn a_confirmed_face_suggests_its_person_along_the_series_without_confirming() -> anyhow::Result<()>
{
    let mut f = Fixture::new()?;
    series(&f)?;
    f.rematch()?;
    for id in 2..=4 {
        assert!(f.suggested(id, ANNA)?, "Gesicht {id}: {:?}", f.state(id)?);
        let (assignment, confidence): (String, f64) = f.c.db.query_row(
            "SELECT assignment,confidence FROM faces WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // Nur die Serie: immer „suggested“, Wert ist der Personenscore.
        assert_eq!(assignment, "suggested");
        assert!((confidence - 0.65).abs() < 1e-4, "{confidence}");
    }
    // Automatische Zuordnungen lernen nie: keine neue Referenz, keine Bestätigung.
    let users: i64 = f.c.db.query_row(
        "SELECT count(*) FROM faces WHERE assignment='user'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(users, 3);
    Ok(())
}

#[test]
fn time_gap_folder_camera_overlap_and_cosine_end_a_series() -> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    series(&f)?;
    // Zehn Sekunden nach dem letzten Foto: neue Serie.
    f.shot(10, "fest/10.jpg", "2024:05:01 12:00:16", Some("50"), "42")?;
    f.face(10, 10, CENTER, &vector(0.65, 0.0))?;
    // Gleiche Zeit, anderer Ordner.
    f.shot(11, "anders/11.jpg", "2024:05:01 12:00:01", None, "42")?;
    f.face(11, 11, CENTER, &vector(0.65, 0.0))?;
    // Gleiche Zeit und gleicher Ordner, andere Kamera (Seriennummer).
    f.shot(12, "fest/12.jpg", "2024:05:01 12:00:01", None, "77")?;
    f.face(12, 12, CENTER, &vector(0.65, 0.0))?;
    // Gleiche Serie, Rahmen kaum überdeckt (Kameraschwenk).
    f.shot(13, "fest/13.jpg", "2024:05:01 12:00:01", None, "42")?;
    f.face(13, 13, SHIFTED, &vector(0.65, 0.0))?;
    // Gleiche Serie und Stelle, aber unähnlich (Kosinus unter der Gruppengrenze 0,6).
    f.shot(14, "fest/14.jpg", "2024:05:01 12:00:01", None, "42")?;
    f.face(14, 14, CENTER, &vector_on(0.3, 0.0, 3))?;
    f.rematch()?;
    assert!(f.suggested(4, ANNA)?, "Kette über Foto 3");
    for id in 10..=14 {
        assert!(f.open(id)?, "Gesicht {id}: {:?}", f.state(id)?);
    }
    Ok(())
}

#[test]
fn rejections_confirmed_people_in_the_photo_and_birth_year_exclude_the_series_person()
-> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    series(&f)?;
    // Foto 2: „nicht Anna“; Foto 3: Anna schon auf einem anderen Gesicht bestätigt.
    f.c.db
        .execute("INSERT INTO face_rejections VALUES(2,?)", [ANNA])?;
    f.face(30, 3, LEFT, &vector(1.0, 0.0))?;
    f.confirm(30, ANNA)?;
    f.rematch()?;
    assert!(f.open(2)?, "{:?}", f.state(2)?);
    assert!(f.open(3)?, "{:?}", f.state(3)?);
    assert!(f.suggested(4, ANNA)?);
    // Anna ist erst nach der Aufnahme geboren.
    f.c.db.execute(
        "INSERT INTO person_attrs VALUES(?,'birth_year','2030')",
        [ANNA],
    )?;
    f.c.db
        .execute("UPDATE face_maintenance SET generation=generation+1", [])?;
    f.rematch()?;
    assert!(f.open(4)?, "{:?}", f.state(4)?);
    Ok(())
}

#[test]
fn a_different_sure_model_proposal_conflicts_and_leaves_the_face_open() -> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    series(&f)?;
    // Gesicht 5, eine Sekunde nach Foto 1, ähnelt Ben (0,79 über der Schwelle), Anna nur
    // 0,61: Die Serie verbindet (ab 0,6), das Modell schlägt Ben vor. Keiner setzt sich durch.
    f.shot(5, "fest/5.jpg", "2024:05:01 12:00:01", None, "42")?;
    f.face(5, 5, CENTER, &vector_on(0.61, 0.79, 4))?;
    // Gesicht 2 ähnelt Anna sicher: Modell und Serie einig, `auto` mit Serienmarke.
    f.c.db.execute(
        "UPDATE face_embeddings SET vec=? WHERE face_id=2",
        [faces::encode(&vector(0.9, 0.0))],
    )?;
    f.rematch()?;
    assert!(f.open(5)?, "{:?}", f.state(5)?);
    assert_eq!(f.state(2)?, (Some(ANNA), Some("auto".into()), Some(ANNA)));
    Ok(())
}

#[test]
fn withdrawing_the_confirmation_or_the_setting_removes_series_suggestions() -> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    series(&f)?;
    f.rematch()?;
    assert!(f.suggested(2, ANNA)?);
    assert!(recognition::burst_enabled(&f.c)?, "Voreinstellung an");
    recognition::set_burst_enabled(&mut f.c, false)?;
    assert!(!faces::status(&f.c)?.burst);
    f.rematch()?;
    for id in 2..=4 {
        assert!(f.open(id)?, "aus: Gesicht {id}: {:?}", f.state(id)?);
    }
    recognition::set_burst_enabled(&mut f.c, true)?;
    f.rematch()?;
    assert!(f.suggested(2, ANNA)?, "wieder an");
    // Rücknahme der Bestätigung (wie Strg+Z): Der nächste Abgleich entfernt die Vorschläge.
    f.c.db.execute(
        "UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=1",
        [],
    )?;
    f.rematch()?;
    for id in 2..=4 {
        assert!(
            f.open(id)?,
            "zurückgenommen: Gesicht {id}: {:?}",
            f.state(id)?
        );
    }
    let marks: i64 =
        f.c.db
            .query_row("SELECT count(*) FROM face_burst", [], |r| r.get(0))?;
    assert_eq!(marks, 0);
    Ok(())
}

#[test]
fn incremental_and_full_rematch_agree_on_series() -> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    series(&f)?;
    f.c.db.execute(
        "UPDATE face_embeddings SET vec=? WHERE face_id=3",
        [faces::encode(&vector(0.9, 0.0))],
    )?;
    let control = Control::default();
    let mut cache = recognition::MatchCache::default();
    let copy = tempfile::tempdir()?;
    let rows = |c: &Collection| -> anyhow::Result<Vec<Row>> {
        Ok(c.db
            .prepare("SELECT f.id,f.person_id,f.assignment,b.person_id FROM faces f LEFT JOIN face_burst b ON b.face_id=f.id ORDER BY f.id")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?)
    };
    for step in 0..3 {
        match step {
            1 => f.confirm(3, ANNA)?,
            2 => {
                f.c.db.execute(
                    "UPDATE faces SET person_id=NULL,assignment=NULL WHERE id=1",
                    [],
                )?;
            }
            _ => {}
        }
        let target = copy.path().join(".myfotomanager");
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        f.c.db
            .execute("VACUUM INTO ?", [target.to_string_lossy()])?;
        let mut full = Collection::open(copy.path(), true)?;
        recognition::rematch_full(&mut full, KEY, &control)?;
        recognition::rematch_with(&mut f.c, KEY, &control, &mut cache)?;
        assert_eq!(rows(&f.c)?, rows(&full)?, "Schritt {step}");
    }
    // Nach Schritt 1 (Gesicht 3 bestätigt) trägt Gesicht 4 Anna über die Serie, nach der
    // Rücknahme von Gesicht 1 weiterhin über Gesicht 3.
    assert!(f.suggested(4, ANNA)?);
    assert!(f.suggested(2, ANNA)?);
    Ok(())
}

#[test]
fn review_page_marks_series_suggestions_and_lists_them_first() -> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    series(&f)?;
    // Ein Modellvorschlag für Ben mit hohem Wert außerhalb jeder Serie.
    f.shot(50, "ben/50.jpg", "2010:01:01 10:00:00", None, "1")?;
    f.face(50, 50, CENTER, &vector(0.0, 0.99))?;
    f.rematch()?;
    let page = faces::page(&f.c, "review", 0)?;
    let first: Vec<i64> = page.groups[0].iter().map(|face| face.id).collect();
    assert_eq!(
        first,
        vec![2, 3, 4],
        "Seriengruppe vorn trotz niedrigerem Wert"
    );
    assert_eq!(page.groups[1][0].id, 50);
    assert_eq!(page.burst.get(&2), Some(&ANNA));
    assert!(!page.burst.contains_key(&50));
    // „Wahrscheinlich Anna“: Seriengesichter als eigener Abschnitt vorn, gekennzeichnet.
    let probable = faces::page_filtered(&f.c, "probable", 0, Some(ANNA))?;
    let first: Vec<i64> = probable.groups[0].iter().map(|face| face.id).collect();
    assert_eq!(first, vec![2, 3, 4]);
    assert_eq!(probable.burst.get(&3), Some(&ANNA));
    Ok(())
}

#[test]
fn the_series_table_is_additive_and_read_only_pages_work_without_it() -> anyhow::Result<()> {
    let schema = include_str!("../src/core/schema.sql");
    for (name, ddl) in migrations::ADDITIVE_TABLES {
        assert!(
            schema.contains(&ddl.replace("IF NOT EXISTS ", "")),
            "{name} fehlt in schema.sql"
        );
    }
    let mut f = Fixture::new()?;
    series(&f)?;
    f.rematch()?;
    f.c.db.execute_batch("DROP TABLE face_burst")?;
    let root = f._temp.path().to_path_buf();
    drop(f.c);
    let read = Collection::open(&root, false)?;
    assert!(!migrations::table_exists(&read.db, "face_burst")?);
    let page = faces::page(&read, "review", 0)?;
    assert!(page.burst.is_empty());
    drop(read);
    let write = Collection::open(&root, true)?;
    assert!(
        migrations::table_exists(&write.db, "face_burst")?,
        "beim Öffnen mit Schreibrecht"
    );
    Ok(())
}
