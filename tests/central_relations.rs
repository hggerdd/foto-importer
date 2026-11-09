//! Zentral geführte Beziehungen zwischen Sammlungen (ADR 0065).
use foto_importer::core::{
    central::{self, Central, CentralConfig, CentralDecisions, CentralEdge, RelChoice},
    collection::Collection,
    faces::{self, NewPerson},
    gaps::{self, Decisions},
    relations::{self, Edge, Kind, Role},
};
use rusqlite::params;

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
    Ok(Setup {
        a: Collection::open(&pa, true)?,
        b: Collection::open(&pb, true)?,
        cfg: CentralConfig {
            enabled: true,
            path: dir.path().join("profile").join("central.sqlite"),
            images: false,
            relations: true,
        },
        _dir: dir,
    })
}
fn open(cfg: &CentralConfig) -> Central {
    Central::open_config(cfg).expect("central")
}
fn person(name: &str) -> NewPerson {
    NewPerson {
        name: name.into(),
        gender: if name == "Anna" {
            "weiblich".into()
        } else {
            String::new()
        },
        ..Default::default()
    }
}
fn id(c: &Collection, name: &str) -> i64 {
    faces::people(c)
        .expect("people")
        .into_iter()
        .find(|p| p.name == name)
        .unwrap_or_else(|| panic!("{name} fehlt"))
        .id
}
fn edges(c: &Collection) -> Vec<Edge> {
    relations::load_edges(c).expect("edges")
}
fn sync(c: &mut Collection, cfg: &CentralConfig) -> central::SyncReport {
    central::sync(c, &mut open(cfg)).expect("sync")
}
/// Legt die Personen in A an und übernimmt sie in B (beide verknüpft).
fn share(s: &mut Setup, names: &[&str]) -> anyhow::Result<()> {
    for name in names {
        central::add_person_synced(&mut s.a, &s.cfg, &person(name))?;
    }
    let report = sync(&mut s.b, &s.cfg);
    let gids: Vec<String> = report.missing.iter().map(|p| p.gid.clone()).collect();
    central::adopt(&mut s.b, &open(&s.cfg), &gids)?;
    sync(&mut s.a, &s.cfg);
    sync(&mut s.b, &s.cfg);
    Ok(())
}
fn add(
    c: &mut Collection,
    cfg: &CentralConfig,
    who: &str,
    role: Role,
    other: &str,
) -> anyhow::Result<()> {
    let (x, y) = (id(c, who), id(c, other));
    let (_, note) = central::add_relation_synced(c, cfg, x, role, y)?;
    assert!(note.warning.is_none());
    Ok(())
}

#[test]
fn relation_in_a_reaches_b_once() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben", "Cora"])?;
    // Plain relations::add in A: erst der Abgleich bringt sie zentral.
    let (anna, ben) = (id(&s.a, "Anna"), id(&s.a, "Ben"));
    relations::add(&mut s.a, anna, Role::Parent, ben)?;
    let report = sync(&mut s.a, &s.cfg);
    assert_eq!(report.relations.central_added, 1);
    assert_eq!(open(&s.cfg).relation_edges()?.len(), 1);
    let report = sync(&mut s.b, &s.cfg);
    assert_eq!(report.relations.local_added, 1);
    assert!(report.relations.conflicts.is_empty());
    let (anna_b, ben_b) = (id(&s.b, "Anna"), id(&s.b, "Ben"));
    assert_eq!(
        edges(&s.b),
        vec![relations::canonical(anna_b, Role::Parent, ben_b)]
    );
    // Zweiter Abgleich: nichts Neues, keine Dublette.
    for c in [&mut s.a, &mut s.b] {
        let again = sync(c, &s.cfg);
        assert_eq!(
            (again.relations.local_added, again.relations.central_added),
            (0, 0)
        );
        assert_eq!(edges(c).len(), 1);
    }
    assert_eq!(open(&s.cfg).relation_edges()?.len(), 1);
    assert!(
        report
            .summary()
            .contains("Beziehungen: 0 zentral ergänzt, 1 lokal ergänzt")
    );
    Ok(())
}

#[test]
fn deletion_leaves_tombstone_and_never_deletes_silently() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben"])?;
    add(&mut s.a, &s.cfg, "Anna", Role::Friend, "Ben")?;
    sync(&mut s.b, &s.cfg);
    assert_eq!(edges(&s.b).len(), 1);
    // Löschen in A schreibt einen Grabstein.
    let (anna, ben) = (id(&s.a, "Anna"), id(&s.a, "Ben"));
    let note = central::remove_relation_synced(&s.a, &s.cfg, anna, Role::Friend, ben)?;
    assert!(note.warning.is_none());
    assert!(edges(&s.a).is_empty());
    assert!(open(&s.cfg).relation_edges()?.is_empty());
    assert_eq!(open(&s.cfg).relation_tombstones()?.len(), 1);
    // B: die lokale Kante bleibt, wird gemeldet und nicht zentral wieder angelegt.
    for _ in 0..2 {
        let report = sync(&mut s.b, &s.cfg);
        assert_eq!(edges(&s.b).len(), 1, "B löscht nie automatisch");
        assert_eq!(report.relations.conflicts.len(), 1);
        assert!(report.relations.conflicts[0].central.is_none());
        assert_eq!(report.relations.central_added, 0);
        assert!(open(&s.cfg).relation_edges()?.is_empty());
    }
    // A bekommt sie nicht zurück.
    let report = sync(&mut s.a, &s.cfg);
    assert_eq!(report.relations.local_added, 0);
    assert!(edges(&s.a).is_empty());
    // Der Nutzer in B entscheidet: lokal entfernen.
    let conflict = sync(&mut s.b, &s.cfg).relations.conflicts[0].clone();
    central::resolve_relation(
        &mut s.b,
        &mut open(&s.cfg),
        &conflict,
        RelChoice::TakeCentral,
    )?;
    assert!(edges(&s.b).is_empty());
    assert!(sync(&mut s.b, &s.cfg).relations.conflicts.is_empty());
    // Bewusstes Neuanlegen hebt den Grabstein auf; dann bekommt B sie wieder.
    add(&mut s.a, &s.cfg, "Anna", Role::Friend, "Ben")?;
    assert!(open(&s.cfg).relation_tombstones()?.is_empty());
    assert_eq!(sync(&mut s.b, &s.cfg).relations.local_added, 1);
    Ok(())
}

#[test]
fn stale_local_edge_can_be_restored_centrally() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben"])?;
    add(&mut s.a, &s.cfg, "Anna", Role::Friend, "Ben")?;
    sync(&mut s.b, &s.cfg);
    let (anna, ben) = (id(&s.a, "Anna"), id(&s.a, "Ben"));
    central::remove_relation_synced(&s.a, &s.cfg, anna, Role::Friend, ben)?;
    let conflict = sync(&mut s.b, &s.cfg).relations.conflicts[0].clone();
    central::resolve_relation(&mut s.b, &mut open(&s.cfg), &conflict, RelChoice::KeepLocal)?;
    assert_eq!(open(&s.cfg).relation_edges()?.len(), 1);
    assert!(open(&s.cfg).relation_tombstones()?.is_empty());
    assert_eq!(sync(&mut s.a, &s.cfg).relations.local_added, 1);
    Ok(())
}

#[test]
fn cycle_from_central_is_reported_not_created() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben", "Cora"])?;
    // A: Anna -> Ben -> Cora (Eltern-Kind-Kette), zentral.
    add(&mut s.a, &s.cfg, "Anna", Role::Parent, "Ben")?;
    add(&mut s.a, &s.cfg, "Ben", Role::Parent, "Cora")?;
    // B kennt nur Cora -> Anna (nur lokal) und würde einen Zyklus bekommen.
    let (cora, anna) = (id(&s.b, "Cora"), id(&s.b, "Anna"));
    relations::add(&mut s.b, cora, Role::Parent, anna)?;
    let before = edges(&s.b);
    let report = sync(&mut s.b, &s.cfg);
    assert_eq!(
        edges(&s.b).len(),
        before.len() + 1,
        "nur die zyklenfreie Kante"
    );
    assert_eq!(report.relations.conflicts.len(), 1);
    let conflict = &report.relations.conflicts[0];
    assert!(conflict.reason.contains("zyklus"));
    assert!(!conflict.can_take_central);
    assert!(
        central::resolve_relation(
            &mut s.b,
            &mut open(&s.cfg),
            conflict,
            RelChoice::TakeCentral
        )
        .is_err()
    );
    // Lokal behalten: die zentrale Kante wird ersetzt (Grabstein), der Konflikt verschwindet.
    let after =
        central::resolve_relation(&mut s.b, &mut open(&s.cfg), conflict, RelChoice::KeepLocal)?;
    assert!(after.relations.conflicts.is_empty());
    assert!(!open(&s.cfg).relation_tombstones()?.is_empty());
    Ok(())
}

#[test]
fn contradiction_asks_and_changes_nothing() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben"])?;
    add(&mut s.a, &s.cfg, "Anna", Role::Partner, "Ben")?;
    let (anna, ben) = (id(&s.b, "Anna"), id(&s.b, "Ben"));
    relations::add(&mut s.b, anna, Role::Parent, ben)?;
    let report = sync(&mut s.b, &s.cfg);
    assert_eq!(report.relations.conflicts.len(), 1);
    let conflict = report.relations.conflicts[0].clone();
    assert_eq!(conflict.local_text, ["Anna ist Mutter von Ben"]);
    assert_eq!(conflict.central_text, "zentral: Anna und Ben sind Partner");
    assert_eq!(report.pending(), 1);
    assert_eq!(edges(&s.b).len(), 1);
    assert_eq!(edges(&s.b)[0].kind, Kind::Parent, "lokal unverändert");
    let central_edges = open(&s.cfg).relation_edges()?;
    assert_eq!(central_edges.len(), 1);
    assert_eq!(
        central_edges.iter().next().map(|e| e.kind),
        Some(Kind::Partner)
    );
    // Wiederholter Abgleich ändert weiter nichts.
    assert_eq!(sync(&mut s.b, &s.cfg).relations.conflicts.len(), 1);
    // „Zentral übernehmen“: lokal wird Partner, Freundschaft ist weg und bleibt weg.
    central::resolve_relation(
        &mut s.b,
        &mut open(&s.cfg),
        &conflict,
        RelChoice::TakeCentral,
    )?;
    assert_eq!(edges(&s.b).len(), 1);
    assert_eq!(edges(&s.b)[0].kind, Kind::Partner);
    assert!(sync(&mut s.b, &s.cfg).relations.conflicts.is_empty());
    assert_eq!(open(&s.cfg).relation_edges()?.len(), 1);
    Ok(())
}

#[test]
fn neither_removes_both_sides() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben"])?;
    add(&mut s.a, &s.cfg, "Anna", Role::Partner, "Ben")?;
    let (anna, ben) = (id(&s.b, "Anna"), id(&s.b, "Ben"));
    relations::add(&mut s.b, anna, Role::Parent, ben)?;
    let conflict = sync(&mut s.b, &s.cfg).relations.conflicts[0].clone();
    central::resolve_relation(&mut s.b, &mut open(&s.cfg), &conflict, RelChoice::Neither)?;
    assert!(edges(&s.b).is_empty());
    assert!(open(&s.cfg).relation_edges()?.is_empty());
    // A hat die Partnerschaft noch: sie wird als „zentral gelöscht“ gemeldet, nicht entfernt.
    let report = sync(&mut s.a, &s.cfg);
    assert_eq!(edges(&s.a).len(), 1);
    assert_eq!(report.relations.conflicts.len(), 1);
    Ok(())
}

#[test]
fn missing_person_is_skipped_then_pulled_on_adopt() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben"])?;
    central::add_person_synced(&mut s.a, &s.cfg, &person("Cora"))?;
    add(&mut s.a, &s.cfg, "Cora", Role::Parent, "Anna")?;
    add(&mut s.a, &s.cfg, "Anna", Role::Friend, "Ben")?;
    let report = sync(&mut s.b, &s.cfg);
    assert_eq!(report.relations.skipped_missing, 1);
    assert_eq!(report.relations.local_added, 1, "Anna–Ben kommt sofort");
    let gids: Vec<String> = report.missing.iter().map(|p| p.gid.clone()).collect();
    central::adopt(&mut s.b, &open(&s.cfg), &gids)?;
    // Direkt beim Übernehmen mitgezogen, ohne weiteren Abgleich.
    let (cora, anna) = (id(&s.b, "Cora"), id(&s.b, "Anna"));
    assert!(edges(&s.b).contains(&relations::canonical(cora, Role::Parent, anna)));
    assert_eq!(edges(&s.b).len(), 2);
    let again = sync(&mut s.b, &s.cfg);
    assert_eq!(again.relations.skipped_missing, 0);
    Ok(())
}

#[test]
fn declined_decisions_are_united_and_never_lost() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben", "Cora"])?;
    let ids = |c: &Collection| (id(c, "Anna"), id(c, "Ben"), id(c, "Cora"));
    let (aa, ab, _) = ids(&s.a);
    let (ba, bb, bc) = ids(&s.b);
    gaps::save_decisions(
        &s.a,
        &Decisions {
            declined: [(ab, aa)].into(),
            declined_friends: [(aa.min(ab), aa.max(ab))].into(),
        },
    )?;
    gaps::save_decisions(
        &s.b,
        &Decisions {
            declined: [(bc, bb)].into(),
            declined_friends: [(ba.min(bc), ba.max(bc))].into(),
        },
    )?;
    sync(&mut s.a, &s.cfg);
    sync(&mut s.b, &s.cfg);
    let report = sync(&mut s.a, &s.cfg);
    assert_eq!(report.relations.decisions_central, 0);
    let (a, b) = (gaps::load_decisions(&s.a)?, gaps::load_decisions(&s.b)?);
    let (aa, ab, ac) = ids(&s.a);
    assert_eq!(a.declined, [(ab, aa), (ac, ab)].into());
    assert_eq!(b.declined.len(), 2);
    assert_eq!(a.declined_friends.len(), 2);
    assert_eq!(b.declined_friends.len(), 2);
    let c: CentralDecisions = open(&s.cfg).relation_decisions()?;
    assert_eq!((c.declined.len(), c.friends.len()), (2, 2));
    Ok(())
}

#[test]
fn merge_in_the_middle_of_a_tree_rehangs_relations() -> anyhow::Result<()> {
    let mut s = setup()?;
    for name in ["Oma", "Mum", "Mutti", "Kind", "Freund"] {
        central::add_person_synced(&mut s.a, &s.cfg, &person(name))?;
    }
    add(&mut s.a, &s.cfg, "Oma", Role::Parent, "Mum")?;
    add(&mut s.a, &s.cfg, "Mutti", Role::Parent, "Kind")?;
    add(&mut s.a, &s.cfg, "Mutti", Role::Friend, "Freund")?;
    add(&mut s.a, &s.cfg, "Mum", Role::Friend, "Freund")?;
    let (mum, mutti) = (id(&s.a, "Mum"), id(&s.a, "Mutti"));
    let gid_mutti = open(&s.cfg)
        .people()?
        .into_iter()
        .find(|p| p.name == "Mutti")
        .map(|p| p.gid)
        .unwrap_or_default();
    let note = central::merge_people_synced(&mut s.a, &s.cfg, mutti, mum)?;
    assert!(note.warning.is_none());
    let local = edges(&s.a);
    assert_eq!(local.len(), 3, "Dublette Freund entfällt: {local:?}");
    let central = open(&s.cfg);
    let rels = central.relation_edges()?;
    assert_eq!(rels.len(), 3);
    assert!(
        rels.iter().all(|e| e.a != gid_mutti && e.b != gid_mutti),
        "nichts hängt mehr an der alten gid"
    );
    // Neue Sammlung folgt dem Alias: vollständiger Baum ohne Dublette.
    let dir = tempfile::tempdir()?;
    let mut c = Collection::open(dir.path(), true)?;
    let report = sync(&mut c, &s.cfg);
    let gids: Vec<String> = report.missing.iter().map(|p| p.gid.clone()).collect();
    assert_eq!(gids.len(), 4);
    central::adopt(&mut c, &open(&s.cfg), &gids)?;
    assert_eq!(edges(&c).len(), 3);
    Ok(())
}

#[test]
fn central_merge_with_conflicting_relations_still_succeeds() -> anyhow::Result<()> {
    let s = setup()?;
    let mut c = open(&s.cfg);
    let ids: Vec<String> = ["X", "Y", "Z"]
        .iter()
        .map(|n| c.add(&person(n)))
        .collect::<anyhow::Result<_>>()?;
    let e = |a: &str, k, b: &str| CentralEdge::new(a, k, b);
    // Z Elternteil von X und von Y; X und Y verschmelzen: Selbstbezug/Eltern bleiben konsistent.
    c.add_relations(
        &[
            e(&ids[2], Kind::Parent, &ids[0]),
            e(&ids[2], Kind::Parent, &ids[1]),
            e(&ids[0], Kind::Friend, &ids[1]),
        ],
        false,
    )?;
    c.merge(&ids[0], &ids[1])?;
    let rels = c.relation_edges()?;
    assert_eq!(rels.len(), 1, "{rels:?}");
    assert_eq!(
        rels.iter().next().map(|r| r.b.clone()),
        Some(ids[1].clone())
    );
    Ok(())
}

fn v1_file(path: &std::path::Path) -> anyhow::Result<()> {
    let db = rusqlite::Connection::open(path)?;
    db.execute_batch(
        "CREATE TABLE people(gid TEXT PRIMARY KEY, name TEXT NOT NULL,
            gender TEXT NOT NULL DEFAULT '', birth_year TEXT NOT NULL DEFAULT '',
            note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL, merged_into TEXT);
         CREATE TABLE refs(id INTEGER PRIMARY KEY, gid TEXT NOT NULL,
            model_key TEXT NOT NULL, jpeg BLOB NOT NULL, created_at TEXT NOT NULL);
         CREATE INDEX refs_gid ON refs(gid);
         CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO meta VALUES('created_by','foto-importer');
         INSERT INTO people VALUES('g1','Alt Eins','w','1950','Notiz','t','t',NULL);
         INSERT INTO people VALUES('g2','Alt Zwei','','','','t','t',NULL);
         INSERT INTO refs(gid,model_key,jpeg,created_at) VALUES('g1','m',x'0102','t');
         PRAGMA user_version=1;",
    )?;
    Ok(())
}
fn version(path: &std::path::Path) -> i64 {
    rusqlite::Connection::open(path)
        .and_then(|c| c.query_row("PRAGMA user_version", [], |r| r.get(0)))
        .expect("version")
}

#[test]
fn v1_file_is_migrated_additively_with_backup() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("central.sqlite");
    v1_file(&path)?;
    let mut c = Central::open(&path, true)?;
    assert_eq!(version(&path), 2);
    let people = c.people()?;
    assert_eq!(people.len(), 2);
    assert_eq!(people[0].birth_year, "1950");
    assert_eq!(c.reference_count()?, 1);
    assert_eq!(c.references("g1")?.len(), 1);
    assert_eq!(
        c.add_relations(&[CentralEdge::new("g1", Kind::Partner, "g2")], false)?,
        1
    );
    drop(c);
    // Idempotent: erneutes Öffnen ändert nichts; Sicherung der alten Datei bleibt Version 1.
    let again = Central::open(&path, true)?;
    assert_eq!(again.relation_edges()?.len(), 1);
    assert_eq!(again.people()?.len(), 2);
    drop(again);
    let backup = dir.path().join("central.sqlite.v1.bak");
    assert_eq!(version(&backup), 1);
    assert_eq!(version(&path), 2);
    Ok(())
}

#[test]
fn newer_version_is_rejected_and_untouched() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("central.sqlite");
    v1_file(&path)?;
    rusqlite::Connection::open(&path)?.execute_batch("PRAGMA user_version=3;")?;
    let error = Central::open(&path, false).err().map(|e| e.to_string());
    assert!(error.is_some_and(|e| e.contains("Schema 3")));
    assert_eq!(version(&path), 3);
    let db = rusqlite::Connection::open(&path)?;
    let tables: i64 = db.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name IN ('rels','rel_tombstones','rel_decisions')",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(tables, 0, "keine Tabellen ergänzt");
    assert!(!dir.path().join("central.sqlite.v1.bak").exists());
    Ok(())
}

#[test]
fn locked_or_read_only_v1_file_is_not_damaged() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("central.sqlite");
    v1_file(&path)?;
    // Gesperrt: eine andere Verbindung hält die Schreibsperre.
    let hold = rusqlite::Connection::open(&path)?;
    hold.execute_batch("BEGIN EXCLUSIVE;")?;
    assert!(Central::open(&path, false).is_err());
    hold.execute_batch("ROLLBACK;")?;
    drop(hold);
    assert_eq!(version(&path), 1);
    // Schreibgeschützt.
    let original = std::fs::metadata(&path)?.permissions();
    let mut locked = original.clone();
    locked.set_readonly(true);
    std::fs::set_permissions(&path, locked)?;
    let result = Central::open(&path, false);
    std::fs::set_permissions(&path, original)?;
    if result.is_err() {
        assert_eq!(version(&path), 1, "Fehler ohne Veränderung");
    }
    drop(result);
    let c = Central::open(&path, false)?;
    assert_eq!(c.people()?.len(), 2);
    Ok(())
}

#[test]
fn locked_central_keeps_local_relation_and_warns() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben"])?;
    let hold = rusqlite::Connection::open(&s.cfg.path)?;
    hold.execute_batch("BEGIN EXCLUSIVE;")?;
    let (anna, ben) = (id(&s.a, "Anna"), id(&s.a, "Ben"));
    let (fresh, note) = central::add_relation_synced(&mut s.a, &s.cfg, anna, Role::Friend, ben)?;
    assert!(fresh && note.warning.is_some());
    assert_eq!(edges(&s.a).len(), 1);
    hold.execute_batch("ROLLBACK;")?;
    // Der nächste Abgleich holt nach.
    assert_eq!(sync(&mut s.a, &s.cfg).relations.central_added, 1);
    Ok(())
}

#[test]
fn disabled_relations_touch_nothing() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben"])?;
    let off = CentralConfig {
        relations: false,
        ..s.cfg.clone()
    };
    add(&mut s.a, &off, "Anna", Role::Friend, "Ben")?;
    let report = sync(&mut s.a, &off);
    assert!(!report.relations.active);
    assert!(open(&off).relation_edges()?.is_empty());
    let (anna, ben) = (id(&s.a, "Anna"), id(&s.a, "Ben"));
    central::remove_relation_synced(&s.a, &off, anna, Role::Friend, ben)?;
    assert!(open(&off).relation_tombstones()?.is_empty());
    assert!(edges(&s.a).is_empty());
    Ok(())
}

#[test]
fn parallel_writers_do_not_lose_relations() -> anyhow::Result<()> {
    let mut s = setup()?;
    share(&mut s, &["Anna", "Ben", "Cora", "Dora"])?;
    let cfg = s.cfg.clone();
    std::thread::scope(|scope| {
        let (a, b) = (&mut s.a, &mut s.b);
        let t1 = scope.spawn(|| add(a, &cfg, "Anna", Role::Friend, "Ben"));
        let t2 = scope.spawn(|| add(b, &cfg, "Cora", Role::Friend, "Dora"));
        t1.join().expect("t1").expect("add a");
        t2.join().expect("t2").expect("add b");
    });
    sync(&mut s.a, &s.cfg);
    sync(&mut s.b, &s.cfg);
    sync(&mut s.a, &s.cfg);
    assert_eq!(edges(&s.a).len(), 2);
    assert_eq!(edges(&s.b).len(), 2);
    assert_eq!(open(&s.cfg).relation_edges()?.len(), 2);
    // Viele Schreiber auf Zentralebene.
    let mut c = open(&s.cfg);
    let ids: Vec<String> = (0..40)
        .map(|i| c.add(&person(&format!("P{i}"))))
        .collect::<anyhow::Result<_>>()?;
    let handles: Vec<_> = (0..4)
        .map(|t| {
            let (cfg, ids) = (s.cfg.clone(), ids.clone());
            std::thread::spawn(move || -> anyhow::Result<()> {
                for i in 0..10 {
                    let mut c = Central::open_config(&cfg)?;
                    c.add_relations(
                        &[CentralEdge::new(
                            &ids[t * 10 + i],
                            Kind::Friend,
                            &ids[(t * 10 + i + 1) % 40],
                        )],
                        false,
                    )?;
                }
                Ok(())
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread")?;
    }
    assert_eq!(open(&s.cfg).relation_edges()?.len(), 42);
    Ok(())
}

#[test]
fn five_thousand_people_and_ten_thousand_relations_sync_in_seconds() -> anyhow::Result<()> {
    let mut s = setup()?;
    const PEOPLE: i64 = 5000;
    {
        let tx = s.a.db.transaction()?;
        for i in 0..PEOPLE {
            tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
            tx.execute(
                "INSERT INTO person_attrs VALUES(?,'name',?)",
                params![tx.last_insert_rowid(), format!("Person {i}")],
            )?;
        }
        tx.commit()?;
    }
    relations::ensure_types(&s.a)?;
    {
        let ids: Vec<i64> = faces::people(&s.a)?.iter().map(|p| p.id).collect();
        let mut by_number: Vec<(i64, i64)> = Vec::new();
        for p in faces::people(&s.a)? {
            let n: i64 = p.name.trim_start_matches("Person ").parse()?;
            by_number.push((n, p.id));
        }
        by_number.sort();
        let id_of = |n: i64| by_number[n as usize].1;
        assert_eq!(ids.len() as i64, PEOPLE);
        let tx = s.a.db.transaction()?;
        // Binärbaum (je Person ein Elternteil) und Freundschaften: zusammen rund 10 000 Kanten.
        for k in 1..PEOPLE {
            tx.execute(
                "INSERT INTO relations VALUES(?,1,?)",
                params![id_of((k - 1) / 2), id_of(k)],
            )?;
        }
        for k in 0..PEOPLE {
            let (x, y) = (id_of(k), id_of((k + 7) % PEOPLE));
            tx.execute(
                "INSERT OR IGNORE INTO relations VALUES(?,6,?)",
                params![x.min(y), x.max(y)],
            )?;
        }
        tx.commit()?;
    }
    let total = edges(&s.a).len();
    assert!(total >= 9900, "{total}");
    let started = std::time::Instant::now();
    let report = sync(&mut s.a, &s.cfg);
    let up = started.elapsed();
    assert_eq!(report.relations.central_added, total);
    assert!(report.relations.conflicts.is_empty());
    let started = std::time::Instant::now();
    let report = sync(&mut s.b, &s.cfg);
    let gids: Vec<String> = report.missing.iter().map(|p| p.gid.clone()).collect();
    assert_eq!(gids.len() as i64, PEOPLE);
    central::adopt(&mut s.b, &open(&s.cfg), &gids)?;
    let down = started.elapsed();
    assert_eq!(edges(&s.b).len(), total);
    let started = std::time::Instant::now();
    let again = sync(&mut s.b, &s.cfg);
    let idle = started.elapsed();
    assert_eq!(
        (again.relations.local_added, again.relations.central_added),
        (0, 0)
    );
    eprintln!("5000/{total}: hoch {up:?}, herunter (inkl. Personen) {down:?}, Leerlauf {idle:?}");
    assert!(up.as_secs() < 30 && down.as_secs() < 30 && idle.as_secs() < 30);
    Ok(())
}

#[test]
fn report_text_and_pending_count_include_relations() {
    let mut report = central::SyncReport::default();
    assert_eq!(report.pending(), 0);
    assert!(!report.summary().contains("Beziehungen"));
    report.relations.active = true;
    report.relations.central_added = 2;
    report.relations.local_added = 3;
    report.relations.skipped_missing = 4;
    report.relations.blocked = 1;
    report.relations.conflicts.push(central::RelConflict {
        local: vec![],
        central: None,
        local_text: vec![],
        central_text: String::new(),
        reason: String::new(),
        can_take_central: true,
    });
    assert_eq!(report.pending(), 1);
    let text = report.summary();
    assert!(text.contains(
        "Beziehungen: 2 zentral ergänzt, 3 lokal ergänzt, 1 Rückfragen/Konflikte, 4 übersprungen (Person fehlt)"
    ));
    assert!(text.contains("1 nicht zentral übernommen"));
}
