//! Zentraler Personenbestand zwischen mehreren Sammlungen (ADR 0062).
use foto_importer::core::{
    central::{self, Central, CentralConfig, Side},
    collection::Collection,
    faces::{self, NewPerson},
    maintenance,
};
use std::collections::{BTreeMap, BTreeSet};

struct Setup {
    _dir: tempfile::TempDir,
    a: Collection,
    b: Collection,
    cfg: CentralConfig,
}
fn setup() -> anyhow::Result<Setup> {
    let dir = tempfile::tempdir()?;
    let (pa, pb) = (dir.path().join("a"), dir.path().join("b"));
    std::fs::create_dir_all(&pa)?;
    std::fs::create_dir_all(&pb)?;
    let a = Collection::open(&pa, true)?;
    let b = Collection::open(&pb, true)?;
    let cfg = CentralConfig {
        enabled: true,
        path: dir.path().join("profile").join("central.sqlite"),
        images: false,
    };
    Ok(Setup {
        _dir: dir,
        a,
        b,
        cfg,
    })
}
fn person(name: &str) -> NewPerson {
    NewPerson {
        name: name.into(),
        ..Default::default()
    }
}
fn open(cfg: &CentralConfig) -> Central {
    Central::open_config(cfg).expect("central")
}

#[test]
fn person_created_in_a_appears_in_b_without_duplicate() -> anyhow::Result<()> {
    let mut s = setup()?;
    let (id, note) = central::add_person_synced(
        &mut s.a,
        &s.cfg,
        &NewPerson {
            name: "Oma Berta".into(),
            gender: "weiblich".into(),
            birth_year: Some(1921),
            note: String::new(),
        },
    )?;
    assert!(note.warning.is_none());
    assert_eq!(open(&s.cfg).people()?.len(), 1);
    // B kennt Berta noch nicht: sie erscheint als fehlend und lässt sich übernehmen.
    let report = central::sync(&mut s.b, &mut open(&s.cfg))?;
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.missing[0].name, "Oma Berta");
    let gids: Vec<String> = report.missing.iter().map(|p| p.gid.clone()).collect();
    let ids = central::adopt(&mut s.b, &open(&s.cfg), &gids)?;
    assert_eq!(ids.len(), 1);
    let in_b = faces::people(&s.b)?;
    assert_eq!(in_b.len(), 1);
    assert_eq!(in_b[0].birth, "1921");
    assert_eq!(in_b[0].gender, "weiblich");
    // Nochmals übernehmen oder abgleichen erzeugt keine Dublette.
    assert!(central::adopt(&mut s.b, &open(&s.cfg), &gids)?.is_empty());
    let again = central::sync(&mut s.b, &mut open(&s.cfg))?;
    assert!(again.missing.is_empty() && again.conflicts.is_empty());
    assert_eq!(faces::people(&s.b)?.len(), 1);
    // Lokale ID in A bleibt unverändert.
    assert_eq!(faces::people(&s.a)?[0].id, id);
    assert_eq!(open(&s.cfg).people()?.len(), 1);
    Ok(())
}

#[test]
fn same_name_without_conflict_links_instead_of_duplicating() -> anyhow::Result<()> {
    let mut s = setup()?;
    central::add_person_synced(&mut s.a, &s.cfg, &person("Max Muster"))?;
    // B legt lokal (ohne Zentrale) dieselbe Person an, mit zusätzlichem Jahr.
    let local = faces::add_person(
        &mut s.b,
        &NewPerson {
            name: "max  muster".into(),
            birth_year: Some(1980),
            ..Default::default()
        },
    )?;
    let report = central::sync(&mut s.b, &mut open(&s.cfg))?;
    assert_eq!(report.linked, 1);
    assert!(report.conflicts.is_empty());
    assert_eq!(
        open(&s.cfg).people()?.len(),
        1,
        "keine zweite zentrale Person"
    );
    assert_eq!(
        open(&s.cfg).people()?[0].birth_year,
        "1980",
        "Lücke zentral ergänzt"
    );
    assert_eq!(faces::people(&s.b)?[0].id, local);
    Ok(())
}

#[test]
fn naming_a_group_adopts_central_person_instead_of_creating() -> anyhow::Result<()> {
    let mut s = setup()?;
    central::add_person_synced(&mut s.a, &s.cfg, &person("Erika"))?;
    // In B gibt es eine Gruppe mit einem Gesicht; Benennen mit demselben Namen.
    let face = insert_face(&s.b)?;
    let (undo, _) = central::name_group_synced(
        &mut s.b,
        &s.cfg,
        &[face],
        &BTreeSet::from([face]),
        None,
        "Erika",
        false,
    )?;
    assert_eq!(
        undo.created, None,
        "zentrale Person übernommen, nicht neu angelegt"
    );
    assert_eq!(faces::people(&s.b)?.len(), 1);
    assert_eq!(open(&s.cfg).people()?.len(), 1);
    // Ein neuer Name legt lokal und zentral an.
    let face2 = insert_face(&s.b)?;
    let (undo, _) = central::name_group_synced(
        &mut s.b,
        &s.cfg,
        &[face2],
        &BTreeSet::from([face2]),
        None,
        "Neu Person",
        false,
    )?;
    assert!(undo.created.is_some());
    assert_eq!(open(&s.cfg).people()?.len(), 2);
    Ok(())
}

fn insert_face(c: &Collection) -> anyhow::Result<i64> {
    c.db.execute(
        "INSERT INTO faces(photo_id,source_relpath,detector_id,bbox_x1,bbox_y1,bbox_x2,bbox_y2) VALUES(1,'x.jpg','d',0,0,1,1)",
        [],
    )?;
    Ok(c.db.last_insert_rowid())
}

#[test]
fn conflict_asks_instead_of_overwriting_and_user_decides() -> anyhow::Result<()> {
    let mut s = setup()?;
    central::add_person_synced(
        &mut s.a,
        &s.cfg,
        &NewPerson {
            name: "Anna".into(),
            birth_year: Some(1950),
            ..Default::default()
        },
    )?;
    let local = faces::add_person(
        &mut s.b,
        &NewPerson {
            name: "Anna".into(),
            birth_year: Some(1990),
            ..Default::default()
        },
    )?;
    let report = central::sync(&mut s.b, &mut open(&s.cfg))?;
    assert_eq!(report.conflicts.len(), 1);
    let conflict = &report.conflicts[0];
    assert!(!conflict.linked);
    assert_eq!(conflict.diffs[0].field, "birth_year");
    // Nichts wurde still überschrieben.
    assert_eq!(faces::people(&s.b)?[0].birth, "1990");
    assert_eq!(open(&s.cfg).people()?[0].birth_year, "1950");
    // Gleiche Person, Wahl: zentraler Wert gewinnt.
    let picks = BTreeMap::from([("birth_year".to_string(), Side::Central)]);
    let after = central::resolve(
        &mut s.b,
        &mut open(&s.cfg),
        local,
        &conflict.gid,
        true,
        &picks,
    )?;
    assert!(after.conflicts.is_empty());
    assert_eq!(faces::people(&s.b)?[0].birth, "1950");
    assert_eq!(faces::people(&s.b)?[0].id, local);
    Ok(())
}

#[test]
fn different_person_is_remembered_and_not_asked_again() -> anyhow::Result<()> {
    let mut s = setup()?;
    central::add_person_synced(
        &mut s.a,
        &s.cfg,
        &NewPerson {
            name: "Anna".into(),
            birth_year: Some(1950),
            ..Default::default()
        },
    )?;
    let local = faces::add_person(
        &mut s.b,
        &NewPerson {
            name: "Anna".into(),
            birth_year: Some(1990),
            ..Default::default()
        },
    )?;
    let report = central::sync(&mut s.b, &mut open(&s.cfg))?;
    let gid = report.conflicts[0].gid.clone();
    let after = central::resolve(
        &mut s.b,
        &mut open(&s.cfg),
        local,
        &gid,
        false,
        &BTreeMap::new(),
    )?;
    assert!(after.conflicts.is_empty());
    assert_eq!(open(&s.cfg).people()?.len(), 2, "eigene zentrale Person");
    let again = central::sync(&mut s.b, &mut open(&s.cfg))?;
    assert!(again.conflicts.is_empty());
    assert_eq!(
        again.missing.len(),
        1,
        "Annas 1950 bleibt als fehlend wählbar"
    );
    Ok(())
}

#[test]
fn read_only_collection_is_never_written() -> anyhow::Result<()> {
    let mut s = setup()?;
    central::add_person_synced(&mut s.a, &s.cfg, &person("Opa"))?;
    let root = s.b.root.clone();
    drop(s.b);
    let mut ro = Collection::open(&root, false)?;
    assert!(central::sync(&mut ro, &mut open(&s.cfg)).is_err());
    assert_eq!(
        central::missing(&ro, &open(&s.cfg))?.len(),
        1,
        "lesend sichtbar"
    );
    assert!(faces::people(&ro)?.is_empty());
    Ok(())
}

#[test]
fn unreachable_central_does_not_block_local_creation() -> anyhow::Result<()> {
    let mut s = setup()?;
    // Pfad ist ein Ordner: kein Öffnen als Datenbank möglich.
    let broken = CentralConfig {
        path: s.a.root.clone(),
        ..s.cfg.clone()
    };
    let (id, note) = central::add_person_synced(&mut s.a, &broken, &person("Tante Uschi"))?;
    assert!(note.warning.is_some(), "Hinweis statt Fehler");
    assert_eq!(faces::people(&s.a)?[0].id, id);
    // Später mit funktionierender Zentrale wird nachgetragen.
    let report = central::sync(&mut s.a, &mut open(&s.cfg))?;
    assert_eq!(report.created_central, 1);
    // Gesperrte Datei: Schreibsperre durch eine andere Verbindung.
    let hold = rusqlite::Connection::open(&s.cfg.path)?;
    hold.execute_batch("BEGIN EXCLUSIVE;")?;
    let started = std::time::Instant::now();
    let (_, note) = central::add_person_synced(&mut s.a, &s.cfg, &person("Onkel Paul"))?;
    assert!(note.warning.is_some());
    assert!(started.elapsed().as_secs() < 30);
    assert_eq!(faces::people(&s.a)?.len(), 2);
    hold.execute_batch("ROLLBACK;")?;
    Ok(())
}

#[test]
fn reference_images_are_opt_in_and_wipeable() -> anyhow::Result<()> {
    let s = setup()?;
    let mut off = Central::open(&s.cfg.path, false)?;
    let gid = off.add(&person("Bild Person"))?;
    assert!(off.add_reference(&gid, "sface-1", &[1, 2, 3]).is_err());
    assert_eq!(off.reference_count()?, 0);
    let mut on = Central::open(&s.cfg.path, true)?;
    on.add_reference(&gid, "sface-1", &[1, 2, 3])?;
    assert!(
        on.add_reference(&gid, " ", &[1]).is_err(),
        "Modellschlüssel Pflicht"
    );
    assert!(
        on.add_reference(&gid, "k", &vec![0; central::MAX_REFERENCE_BYTES + 1])
            .is_err()
    );
    assert_eq!(on.references(&gid)?.len(), 1);
    assert!(
        off.references(&gid)?.is_empty(),
        "ohne Opt-in keine Ausgabe"
    );
    // Die Konfiguration mit Opt-in aus bereinigt trotzdem alles.
    let cfg = CentralConfig {
        images: false,
        ..s.cfg.clone()
    };
    assert_eq!(central::wipe_references(&cfg)?, 1);
    assert_eq!(on.reference_count()?, 0);
    assert_eq!(on.people()?.len(), 1, "Personen bleiben");
    Ok(())
}

#[test]
fn biometric_cleanup_keeps_central_people_and_wipe_removes_references() -> anyhow::Result<()> {
    let mut s = setup()?;
    let (_, _) = central::add_person_synced(&mut s.a, &s.cfg, &person("Rita"))?;
    let gid = open(&s.cfg).people()?[0].gid.clone();
    Central::open(&s.cfg.path, true)?.add_reference(&gid, "m", &[9, 9])?;
    maintenance::delete_biometrics(&s.a)?;
    assert!(faces::people(&s.a)?.is_empty());
    let on = CentralConfig {
        images: true,
        ..s.cfg.clone()
    };
    assert_eq!(central::wipe_references(&on)?, 1);
    assert_eq!(open(&s.cfg).people()?.len(), 1);
    // Fehlende Datei ist kein Fehler.
    let none = CentralConfig {
        path: s.cfg.path.with_file_name("gibt-es-nicht.sqlite"),
        ..s.cfg.clone()
    };
    assert_eq!(central::wipe_references(&none)?, 0);
    Ok(())
}

#[test]
fn merge_is_recorded_as_alias_and_followed() -> anyhow::Result<()> {
    let mut s = setup()?;
    let (x, _) = central::add_person_synced(&mut s.a, &s.cfg, &person("Heinz"))?;
    let (y, _) = central::add_person_synced(&mut s.a, &s.cfg, &person("Heinrich"))?;
    let gids = open(&s.cfg).people()?;
    let gx = gids
        .iter()
        .find(|p| p.name == "Heinz")
        .expect("x")
        .gid
        .clone();
    let gy = gids
        .iter()
        .find(|p| p.name == "Heinrich")
        .expect("y")
        .gid
        .clone();
    // B hat Heinz bereits (verknüpft) bevor er in A zusammengeführt wird.
    central::adopt(&mut s.b, &open(&s.cfg), std::slice::from_ref(&gx))?;
    central::merge_people_synced(&mut s.a, &s.cfg, x, y)?;
    let c = open(&s.cfg);
    assert_eq!(
        c.people()?.len(),
        1,
        "Datensatz bleibt als Alias, nicht aktiv"
    );
    assert_eq!(c.aliases()?.get(&gx), Some(&gy));
    // B folgt dem Alias, ohne Personen zu löschen.
    let report = central::sync(&mut s.b, &mut open(&s.cfg))?;
    assert_eq!(report.relinked, 1);
    assert_eq!(faces::people(&s.b)?.len(), 1);
    Ok(())
}

#[test]
fn parallel_writers_lose_nothing() -> anyhow::Result<()> {
    let s = setup()?;
    let path = s.cfg.path.clone();
    open(&s.cfg); // Schema einmal anlegen
    let handles: Vec<_> = (0..4)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || -> anyhow::Result<()> {
                for i in 0..25 {
                    let mut c = Central::open(&path, false)?;
                    c.add(&NewPerson {
                        name: format!("Person {t}-{i}"),
                        ..Default::default()
                    })?;
                }
                Ok(())
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread")?;
    }
    assert_eq!(open(&s.cfg).people()?.len(), 100);
    Ok(())
}

#[test]
fn thousands_of_people_sync_quickly() -> anyhow::Result<()> {
    let mut s = setup()?;
    {
        let tx = s.a.db.transaction()?;
        for i in 0..3000 {
            tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
            tx.execute(
                "INSERT INTO person_attrs VALUES(?,'name',?)",
                rusqlite::params![tx.last_insert_rowid(), format!("Person {i}")],
            )?;
        }
        tx.commit()?;
    }
    let started = std::time::Instant::now();
    let report = central::sync(&mut s.a, &mut open(&s.cfg))?;
    assert_eq!(report.created_central, 3000);
    let report = central::sync(&mut s.b, &mut open(&s.cfg))?;
    assert_eq!(report.missing.len(), 3000);
    assert!(started.elapsed().as_secs() < 60);
    Ok(())
}
