#![allow(clippy::unwrap_used)]
use foto_importer::core::{
    collection::Collection,
    timeline::{self, Axis, Birth, DateQuality, Grain, Params, Reject, Thresholds},
};
use rusqlite::params;
use std::time::Instant;

struct Spec {
    date: Option<&'static str>,
    dir: &'static str,
    /// Gesichtsbreite in Pixeln bei einem 4000 x 3000 Bild.
    px: f32,
    assignment: &'static str,
    quality: f32,
    ignored: bool,
    /// Landmarks eines geraden Gesichts (sonst stark gedreht).
    frontal: bool,
}
impl Default for Spec {
    fn default() -> Self {
        Spec {
            date: Some("2019:05:03 10:00:00"),
            dir: "2019",
            px: 400.0,
            assignment: "user",
            quality: 0.9,
            ignored: false,
            frontal: true,
        }
    }
}

/// Landmarks im Analysebild (1600 x 1200) innerhalb des Gesichtsrahmens ab (0,4; 0,3).
fn straight() -> &'static str {
    "[[648.0,410.0],[688.0,410.0],[668.0,432.0],[652.0,450.0],[684.0,450.0]]"
}
fn turned() -> &'static str {
    "[[648.0,410.0],[664.0,418.0],[692.0,432.0],[652.0,460.0],[668.0,460.0]]"
}

fn person(c: &Collection, id: i64, name: &str, birth: Option<&str>) -> anyhow::Result<()> {
    c.db.execute("INSERT INTO persons(id) VALUES(?)", [id])?;
    c.db.execute(
        "INSERT INTO person_attrs VALUES(?,'name',?)",
        params![id, name],
    )?;
    if let Some(year) = birth {
        c.db.execute(
            "INSERT INTO person_attrs VALUES(?,'birth_year',?)",
            params![id, year],
        )?;
    }
    Ok(())
}

fn face(c: &Collection, id: i64, person: i64, spec: &Spec) -> anyhow::Result<()> {
    let rel = format!("{}/{id}.jpg", spec.dir);
    c.db.execute("INSERT INTO photos(id,status) VALUES(?,'present')", [id])?;
    c.db.execute("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?,?,?,?,?,?,'jpg',1,0,?,'present','done')",
        params![id, rel, rel, spec.dir, format!("{id}.jpg"), id.to_string(), id])?;
    c.db.execute(
        "INSERT INTO meta(file_id,dt_original,width,height,orientation) VALUES(?,?,4000,3000,1)",
        params![id, spec.date],
    )?;
    let w = spec.px / 4000.0;
    let bbox = [0.4_f32, 0.3, 0.4 + w, 0.3 + w * 4000.0 / 3000.0];
    c.db.execute("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment,ignored) VALUES(?,?,?,?,'d',?,?,?,?,?,0.95,?,?,?,?)",
        params![id, id, id, rel, bbox[0], bbox[1], bbox[2], bbox[3],
            if spec.frontal { straight() } else { turned() }, spec.quality, person, spec.assignment, spec.ignored])?;
    Ok(())
}

fn fresh() -> anyhow::Result<(tempfile::TempDir, Collection)> {
    let temp = tempfile::tempdir()?;
    let c = Collection::open(temp.path(), true)?;
    Ok((temp, c))
}

#[test]
fn dates_come_from_the_face_then_the_photo_then_the_folder() {
    let d = timeline::parse_date("2019:05:03 10:11:12").unwrap();
    assert_eq!((d.year, d.month, d.day), (2019, 5, 3));
    assert_eq!(
        timeline::parse_date("2019-05-03").unwrap().iso(),
        "2019-05-03"
    );
    assert_eq!(timeline::parse_date("2014").unwrap().month, 1);
    assert!(timeline::parse_date("0000:00:00 00:00:00").is_none());
    assert!(timeline::parse_date("").is_none());
    let f = timeline::folder_date("Familie/2014-08 Urlaub").unwrap();
    assert_eq!((f.year, f.month), (2014, 8));
    assert_eq!(timeline::folder_date("2021").unwrap().year, 2021);
    assert!(timeline::folder_date("Urlaub 2019").is_none());
    assert!(timeline::folder_date("20190").is_none());
}

#[test]
fn face_size_in_pixels_uses_the_image_dimensions_and_rotation() -> anyhow::Result<()> {
    let (_t, c) = fresh()?;
    person(&c, 1, "A", None)?;
    face(
        &c,
        1,
        1,
        &Spec {
            px: 320.0,
            ..Default::default()
        },
    )?;
    face(&c, 2, 1, &Spec::default())?;
    c.db.execute("UPDATE meta SET orientation=6 WHERE file_id=2", [])?;
    let faces = timeline::load(&c, 1)?;
    let px = |id| faces.iter().find(|f| f.id == id).unwrap().px.unwrap();
    assert_eq!(px(1), 320);
    // bbox 0,1 x 0,1333 normiert; bei vertauschten Maßen 3000 x 4000 ist die kürzere Seite 300.
    assert_eq!(px(2), 300);
    assert!(faces.iter().find(|f| f.id == 1).unwrap().eyes.is_some());
    Ok(())
}

#[test]
fn only_large_frontal_dated_confirmed_faces_are_selected() -> anyhow::Result<()> {
    let (_t, c) = fresh()?;
    person(&c, 1, "A", None)?;
    person(&c, 2, "B", None)?;
    let cases = [
        (1, Spec::default()),
        (
            2,
            Spec {
                px: 100.0,
                ..Default::default()
            },
        ),
        (
            3,
            Spec {
                frontal: false,
                ..Default::default()
            },
        ),
        (
            4,
            Spec {
                date: None,
                dir: "ohne",
                ..Default::default()
            },
        ),
        (
            5,
            Spec {
                assignment: "auto",
                ..Default::default()
            },
        ),
        (
            6,
            Spec {
                quality: 0.2,
                ..Default::default()
            },
        ),
        (
            7,
            Spec {
                ignored: true,
                ..Default::default()
            },
        ),
        (
            8,
            Spec {
                date: None,
                dir: "2015-03",
                ..Default::default()
            },
        ),
    ];
    for (id, spec) in &cases {
        face(&c, *id, 1, spec)?;
    }
    // Ein Gesicht einer anderen Person und ein von der Person abgelehntes fehlen.
    face(&c, 9, 2, &Spec::default())?;
    face(&c, 10, 1, &Spec::default())?;
    c.db.execute("UPDATE faces SET retained=1 WHERE id=10", [])?;
    face(&c, 11, 1, &Spec::default())?;
    c.db.execute("INSERT INTO face_rejections VALUES(11,1)", [])?;
    let faces = timeline::load(&c, 1)?;
    let ids: std::collections::BTreeSet<i64> = faces.iter().map(|f| f.id).collect();
    assert_eq!(ids, [1, 2, 3, 4, 5, 6, 8].into());
    let out = timeline::compute(&faces, &Params::default());
    let chosen: Vec<i64> = out.items.iter().map(|i| i.face.id).collect();
    // 8 hat nur ein Ordnerdatum (2015), das als Quelle zugelassen ist.
    assert_eq!(chosen, [8, 1]);
    assert_eq!(out.rejected[&Reject::TooSmall], 1);
    assert_eq!(out.rejected[&Reject::NotFrontal], 1);
    // 4 hat nur das Dateidatum (mtime), das unter der Mindestquelle „Ordner“ liegt.
    assert_eq!(out.rejected[&Reject::WeakDate], 1);
    assert_eq!(out.rejected[&Reject::Automatic], 1);
    assert_eq!(out.rejected[&Reject::Quality], 1);
    // Mit automatischen Zuordnungen kommt 5 dazu; strenge Datumsquelle wirft 8 hinaus.
    let strict = Params {
        thresholds: Thresholds {
            include_auto: true,
            min_date: DateQuality::Exif,
            ..Default::default()
        },
        ..Default::default()
    };
    let strict = timeline::compute(&faces, &strict);
    let chosen: Vec<i64> = strict.items.iter().map(|i| i.face.id).collect();
    assert_eq!(chosen, [1, 5]);
    // Kleinere Mindestauflösung nimmt auch das 100-px-Gesicht.
    let low = Params {
        thresholds: Thresholds::with_min_px(96),
        ..Default::default()
    };
    assert!(
        timeline::compute(&faces, &low)
            .items
            .iter()
            .any(|i| i.face.id == 2)
    );
    // Der Regler ist begrenzt.
    assert_eq!(Thresholds::with_min_px(10).min_px, 96);
    assert_eq!(Thresholds::with_min_px(9999).min_px, 320);
    Ok(())
}

#[test]
fn frontality_rejects_turned_and_tilted_faces() {
    let straight = [
        [40.0, 40.0],
        [60.0, 40.0],
        [50.0, 52.0],
        [42.0, 60.0],
        [58.0, 60.0],
    ];
    assert!(timeline::frontality(&straight) > 0.9);
    let profile = [
        [40.0, 40.0],
        [48.0, 44.0],
        [62.0, 52.0],
        [42.0, 66.0],
        [50.0, 66.0],
    ];
    assert!(timeline::frontality(&profile) < 0.3);
    let tilted = [
        [40.0, 40.0],
        [58.0, 58.0],
        [52.0, 56.0],
        [38.0, 62.0],
        [54.0, 72.0],
    ];
    assert!(timeline::frontality(&tilted) < 0.3);
}

#[test]
fn sections_pick_the_best_face_and_follow_the_granularity() -> anyhow::Result<()> {
    let (_t, c) = fresh()?;
    person(&c, 1, "A", Some("2015"))?;
    let dates = [
        (1, "2018:01:10 00:00:00", 200.0),
        (2, "2018:06:10 00:00:00", 500.0),
        (3, "2018:07:10 00:00:00", 300.0),
        (4, "2020:02:10 00:00:00", 250.0),
    ];
    for (id, date, px) in dates {
        face(
            &c,
            id,
            1,
            &Spec {
                date: Some(date),
                px,
                ..Default::default()
            },
        )?;
    }
    let faces = timeline::load(&c, 1)?;
    let by_year = timeline::compute(
        &faces,
        &Params {
            grain: Some(Grain::Year),
            ..Default::default()
        },
    );
    assert_eq!(by_year.sections.len(), 2);
    assert_eq!(by_year.sections[0].label, "2018");
    // Das größte Gesicht (id 2) gewinnt im Jahr 2018.
    assert_eq!(by_year.face(by_year.sections[0].best.unwrap()).id, 2);
    assert_eq!(by_year.sections[0].key, "dy:2018");
    // Dichte ist lückenlos, 2019 hat null.
    assert_eq!(by_year.density.first, 2018);
    assert_eq!(by_year.density.counts, [3, 0, 1]);
    assert_eq!(by_year.gaps.len(), 1);
    assert_eq!((by_year.gaps[0].from, by_year.gaps[0].to), (2019, 2019));
    assert_eq!(by_year.gaps[0].start, "2019-01-01");
    // Spanne von gut zwei Jahren: automatisch Monatsansicht.
    let auto = timeline::compute(&faces, &Params::default());
    assert!(auto.automatic);
    assert_eq!(auto.grain, Grain::Month);
    assert_eq!(auto.sections.len(), 4);
    assert_eq!(auto.sections[0].label, "Jan 2018");
    // Altersachse ohne Geburtsmonat: nur Jahre.
    let birth = timeline::birth(&c, 1)?;
    assert_eq!(
        birth,
        Some(Birth {
            year: 2015,
            month: None
        })
    );
    let age = timeline::compute(
        &faces,
        &Params {
            axis: Axis::Age,
            birth,
            grain: Some(Grain::Month),
            ..Default::default()
        },
    );
    assert_eq!(age.grain, Grain::Year);
    let labels: Vec<&str> = age.sections.iter().map(|s| s.label.as_str()).collect();
    assert_eq!(labels, ["3 Jahre", "5 Jahre"]);
    assert_eq!(age.age_years(age.face(0)), Some(3));
    Ok(())
}

#[test]
fn birth_month_makes_ages_month_exact_and_is_local() -> anyhow::Result<()> {
    let (_t, c) = fresh()?;
    person(&c, 1, "Kind", Some("2017"))?;
    timeline::set_birth_month(&c, 1, Some(5))?;
    let birth = timeline::birth(&c, 1)?.unwrap();
    assert_eq!(birth.month, Some(5));
    let date = |s| timeline::parse_date(s).unwrap();
    let months = |s| timeline::ordinal(Axis::Age, Grain::Month, date(s), Some(birth));
    assert_eq!(months("2017:05:20"), Some(0));
    assert_eq!(months("2017:11:02"), Some(6));
    assert_eq!(months("2018:05:02"), Some(12));
    assert_eq!(months("2017:03:02"), None);
    assert_eq!(timeline::label(Axis::Age, Grain::Month, 6), "6 Monate");
    assert_eq!(timeline::label(Axis::Age, Grain::Month, 1), "1 Monat");
    assert_eq!(timeline::label(Axis::Age, Grain::Month, 40), "3 J. 4 M.");
    // Jahresalter beginnt im Geburtsmonat, nicht im Januar.
    let year = |s| timeline::ordinal(Axis::Age, Grain::Year, date(s), Some(birth));
    assert_eq!(year("2018:04:30"), Some(0));
    assert_eq!(year("2018:05:01"), Some(1));
    assert_eq!(
        timeline::month_range(Axis::Age, Grain::Year, 1, Some(birth)),
        Some((2018 * 12 + 4, 2018 * 12 + 15))
    );
    // Ungültiger Monat wird abgelehnt, Löschen entfernt das Merkmal.
    assert!(timeline::set_birth_month(&c, 1, Some(13)).is_err());
    timeline::set_birth_month(&c, 1, None)?;
    assert_eq!(timeline::birth(&c, 1)?.unwrap().month, None);
    // Ohne Geburtsjahr gibt es keine Altersachse.
    person(&c, 2, "Ohne", None)?;
    assert!(timeline::birth(&c, 2)?.is_none());
    assert_eq!(
        timeline::ordinal(Axis::Age, Grain::Year, date("2018:05:01"), None),
        None
    );
    Ok(())
}

#[test]
fn pins_and_hidden_faces_survive_recomputation() -> anyhow::Result<()> {
    let (temp, c) = fresh()?;
    person(&c, 1, "A", None)?;
    for (id, px) in [(1, 500.0), (2, 300.0), (3, 250.0)] {
        face(
            &c,
            id,
            1,
            &Spec {
                px,
                ..Default::default()
            },
        )?;
    }
    let load = |c: &Collection| -> anyhow::Result<timeline::Computed> {
        let params = Params {
            grain: Some(Grain::Year),
            decisions: timeline::load_decisions(c, 1)?,
            ..Default::default()
        };
        Ok(timeline::compute(&timeline::load(c, 1)?, &params))
    };
    let auto = load(&c)?;
    assert_eq!(auto.face(auto.sections[0].best.unwrap()).id, 1);
    // Festnageln ersetzt die automatische Wahl.
    timeline::pin(&c, 1, "dy:2019", Some(3))?;
    let pinned = load(&c)?;
    assert!(pinned.sections[0].pinned);
    assert_eq!(pinned.face(pinned.sections[0].best.unwrap()).id, 3);
    // Ausblenden des festgenagelten Gesichts nimmt die Festlegung zurück.
    timeline::set_hidden(&c, 1, 3, true)?;
    let hidden = load(&c)?;
    assert!(!hidden.sections[0].pinned);
    assert_eq!(hidden.face(hidden.sections[0].best.unwrap()).id, 1);
    assert_eq!(hidden.density.counts, [2]);
    // Alle ausgeblendet: Abschnitt bleibt, ohne Wahl.
    timeline::set_hidden(&c, 1, 1, true)?;
    timeline::set_hidden(&c, 1, 2, true)?;
    assert_eq!(load(&c)?.sections[0].best, None);
    for id in [1, 2, 3] {
        timeline::set_hidden(&c, 1, id, false)?;
    }
    timeline::pin(&c, 1, "dy:2019", Some(2))?;
    // Neuberechnung nach erneutem Öffnen: Entscheidungen bleiben.
    drop(c);
    let c = Collection::open(temp.path(), true)?;
    let again = load(&c)?;
    assert_eq!(again.face(again.sections[0].best.unwrap()).id, 2);
    assert_eq!(timeline::load_decisions(&c, 1)?.pins["dy:2019"], 2);
    // Ein nicht mehr geeignetes festgenageltes Gesicht fällt auf die automatische Wahl zurück,
    // die Entscheidung selbst bleibt gespeichert.
    c.db.execute("UPDATE faces SET ignored=1 WHERE id=2", [])?;
    let fallback = load(&c)?;
    assert!(!fallback.sections[0].pinned);
    assert_eq!(fallback.face(fallback.sections[0].best.unwrap()).id, 1);
    assert_eq!(timeline::load_decisions(&c, 1)?.pins["dy:2019"], 2);
    // Aufheben löscht den Eintrag; die Zeitreise verändert keine Gesichter.
    timeline::pin(&c, 1, "dy:2019", None)?;
    assert!(timeline::load_decisions(&c, 1)?.pins.is_empty());
    let ignored: i64 =
        c.db.query_row("SELECT ignored FROM faces WHERE id=1", [], |r| r.get(0))?;
    assert_eq!(ignored, 0);
    // Schreibgeschützt: lesen ja, ändern nein.
    drop(c);
    let read = Collection::open(temp.path(), false)?;
    assert!(timeline::pin(&read, 1, "dy:2019", Some(1)).is_err());
    assert!(timeline::set_hidden(&read, 1, 1, true).is_err());
    assert!(timeline::set_birth_month(&read, 1, Some(2)).is_err());
    Ok(())
}

#[test]
fn gaps_report_small_faces_in_the_missing_period() -> anyhow::Result<()> {
    let (_t, c) = fresh()?;
    person(&c, 1, "A", None)?;
    let early = Spec {
        date: Some("2010:01:01 00:00:00"),
        ..Default::default()
    };
    face(&c, 1, 1, &early)?;
    let late = Spec {
        date: Some("2013:01:01 00:00:00"),
        ..Default::default()
    };
    face(&c, 2, 1, &late)?;
    let small = Spec {
        date: Some("2011:06:01 00:00:00"),
        px: 90.0,
        ..Default::default()
    };
    face(&c, 3, 1, &small)?;
    let undated = Spec {
        date: None,
        dir: "ohne",
        ..Default::default()
    };
    face(&c, 4, 1, &undated)?;
    let out = timeline::compute(
        &timeline::load(&c, 1)?,
        &Params {
            grain: Some(Grain::Year),
            ..Default::default()
        },
    );
    assert_eq!(out.gaps.len(), 1);
    assert_eq!(
        (out.gaps[0].before.as_str(), out.gaps[0].after.as_str()),
        ("2010", "2013")
    );
    assert_eq!(out.gaps[0].candidates, 1);
    assert_eq!(out.gaps[0].end, "2012-12-28");
    assert_eq!(out.rejected[&Reject::WeakDate], 1);
    assert!(out.items.iter().all(|i| i.face.date.is_some()));
    Ok(())
}

#[test]
fn contact_sheet_and_relatives_follow_the_data() -> anyhow::Result<()> {
    use foto_importer::core::{
        kinship::Family,
        relations::{Edge, Kind},
    };
    let (_t, c) = fresh()?;
    person(&c, 1, "A", None)?;
    for (id, px) in [(1, 500.0), (2, 450.0), (3, 400.0), (4, 300.0)] {
        face(
            &c,
            id,
            1,
            &Spec {
                px,
                ..Default::default()
            },
        )?;
    }
    let out = timeline::compute(
        &timeline::load(&c, 1)?,
        &Params {
            grain: Some(Grain::Month),
            ..Default::default()
        },
    );
    assert_eq!(out.years.len(), 1);
    let top = out.top(&out.years[0], 3);
    let ids: Vec<i64> = top.iter().map(|i| out.face(*i).id).collect();
    assert_eq!(ids, [1, 2, 3]);
    let edges = [
        Edge {
            from: 10,
            kind: Kind::Parent,
            to: 1,
        },
        Edge {
            from: 10,
            kind: Kind::Parent,
            to: 2,
        },
        Edge {
            from: 1,
            kind: Kind::Parent,
            to: 3,
        },
        Edge {
            from: 1,
            kind: Kind::Partner,
            to: 4,
        },
    ];
    let family = Family::new(&edges, &Default::default());
    let rel = timeline::relatives(&family, 1);
    assert_eq!(
        rel,
        [
            (10, "Elternteil"),
            (2, "Geschwister"),
            (3, "Kind"),
            (4, "Partner")
        ]
    );
    Ok(())
}

/// Abfrage für eine Person mit 100.000 Gesichtern (Synthetik): Messwert, grobe Obergrenze.
#[test]
fn a_person_with_100000_faces_loads_and_computes_quickly() -> anyhow::Result<()> {
    let (_t, mut c) = fresh()?;
    person(&c, 1, "Viele", Some("1980"))?;
    let tx = c.db.transaction()?;
    {
        let mut photo = tx.prepare("INSERT INTO photos(id,status) VALUES(?,'present')")?;
        let mut file = tx.prepare("INSERT INTO files(id,relpath,relpath_fold,dir,name,stem,ext,size,mtime_ns,photo_id,status,meta_state) VALUES(?1,?2,?2,'d',?3,?3,'jpg',1,0,?1,'present','done')")?;
        let mut meta = tx.prepare(
            "INSERT INTO meta(file_id,dt_original,width,height,orientation) VALUES(?,?,4000,3000,1)",
        )?;
        let mut faces = tx.prepare("INSERT INTO faces(id,photo_id,source_file_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2,landmarks,det_score,quality,person_id,assignment) VALUES(?1,?1,?1,?2,'d',0.4,0.3,?3,0.5,?4,0.95,0.9,1,'user')")?;
        for id in 1..=100_000i64 {
            let rel = format!("d/{id}.jpg");
            photo.execute([id])?;
            file.execute(params![id, rel, format!("{id}.jpg")])?;
            let date = format!("{}:{:02}:15 12:00:00", 1990 + id % 35, 1 + id % 12);
            meta.execute(params![id, date])?;
            faces.execute(params![
                id,
                rel,
                0.4 + 0.04 + (id % 7) as f32 * 0.01,
                straight()
            ])?;
        }
    }
    tx.commit()?;
    let start = Instant::now();
    let loaded = timeline::load(&c, 1)?;
    let loading = start.elapsed();
    let start = Instant::now();
    let out = timeline::compute(
        &loaded,
        &Params {
            birth: timeline::birth(&c, 1)?,
            ..Default::default()
        },
    );
    let computing = start.elapsed();
    eprintln!("100000 Gesichter: Abfrage {loading:?}, Berechnung {computing:?}");
    assert_eq!(loaded.len(), 100_000);
    assert!(out.sections.len() >= 35);
    // Grobe Obergrenze, auch für Debug-Läufe; im Release liegt der Wert deutlich darunter.
    let limit = if cfg!(debug_assertions) { 20.0 } else { 1.0 };
    assert!(loading.as_secs_f32() < limit);
    assert!(computing.as_secs_f32() < limit);
    Ok(())
}
