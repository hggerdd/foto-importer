pub use super::recognition::{apply_knowledge, rematch, suggestions};
use super::{
    collection::Collection,
    inference::{self, Models},
    jobs::Control,
    previews,
    scope::Scope,
};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod portraits;
pub use portraits::{person_portrait, person_portraits};
mod pages;
pub use pages::{
    REVIEW_ROWS, active_detector_key, page, page_filtered, page_with, photo_analysis_done,
    photo_faces,
};
mod analysis;
pub use analysis::{analyze, analyze_scoped, decode, encode, prepare_groups};
mod decisions;
use decisions::delete_face_rows;
pub use decisions::{
    DecisionSummary, FaceSnapshot, Undo, delete_face, lift_rejection, mark_not_face, name_group,
    undo,
};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub birth: String,
    pub count: usize,
    pub gender: String,
    pub note: String,
}
#[derive(Clone, Debug)]
pub struct Face {
    pub id: i64,
    pub photo: i64,
    pub source: String,
    pub bbox: [f32; 4],
    pub person: Option<i64>,
    pub assignment: Option<String>,
    pub ignored: bool,
    pub score: f32,
}
#[derive(Clone, Default)]
pub struct Status {
    pub total: usize,
    pub confirmed: usize,
    pub unknown: usize,
    pub review: usize,
    pub ignored: usize,
    pub references: usize,
    pub reference_people: usize,
    pub dirty_people: usize,
    pub analysed_photos: usize,
    pub present_photos: usize,
    pub nameable: usize,
    pub pending: usize,
    pub applied: String,
    pub detector: String,
    pub embedder: String,
    pub auto_import: bool,
    pub nightly: bool,
    pub detector_id: String,
    pub embedder_id: String,
    /// Geltende Vorschlagsschwelle der Sammlung (`recognition::Limits::review`).
    pub review_threshold: f32,
    /// Geltender Mindestabstand zur zweitbesten Person (`recognition::Limits::margin`).
    pub review_margin: f32,
}
impl Status {
    /// Offene Gesichter ohne Gruppe unter „Benennen“: ohne Namensmerkmale (zu klein oder
    /// unscharf) oder noch nicht gruppiert. Genau diese zeigt der Reiter „unsuitable“
    /// ([`UNSUITABLE`]); Gruppenmitglieder sind stets offene Gesichter.
    pub fn unsuitable(&self) -> usize {
        self.unknown.saturating_sub(self.nameable)
    }
}
/// Wert von `faces.ignored` für die Nutzerentscheidung „Kein Gesicht“ (0 = aktiv,
/// 1 = „Nicht interessiert“). Alle Abfragen auf aktive Gesichter prüfen `ignored=0`, der
/// Reiter „Ignoriert“ `ignored=1`; so fällt ein solches Gesicht aus allen Zählungen,
/// Gruppen und Vorschlägen. Die Analyse übernimmt `ignored` beim Wiederfinden (IoU) und
/// behält nicht wiedergefundene ignorierte Gesichter als historisch.
pub const NOT_A_FACE: i64 = 2;
/// Offene Gesichter ohne Gruppe des Embedders `?1` (Reiter „unsuitable“); dieselbe Menge wie
/// [`Status::unsuitable`].
const UNSUITABLE: &str = "f.ignored=0 AND f.retained=0 AND f.person_id IS NULL AND NOT EXISTS(SELECT 1 FROM face_group_members g WHERE g.face_id=f.id AND g.embedder_id=?1)";
#[derive(Default)]
pub struct FacePage {
    pub groups: Vec<Vec<Face>>,
    pub people: Vec<Person>,
    pub status: Status,
    /// Namensvorschläge je Gruppe, Schlüssel ist die ID des ersten Gesichts.
    pub suggestions: BTreeMap<i64, super::recognition::Suggestions>,
    /// Gruppen insgesamt im gewählten Reiter (für die Seitenzahl).
    pub total_groups: usize,
    /// Alle gespeicherten Beziehungen, kanonisch (für die Personenübersicht).
    pub relations: Vec<super::relations::Edge>,
    /// Stammbaum für Verwandtschaftsrechner und Stammbaumansicht (ADR 0061).
    pub family: Box<super::kinship::Family>,
    /// Offene Rückfragen zu wahrscheinlich fehlenden Eltern (ADR 0061).
    pub questions: Vec<super::gaps::Suggestion>,
    /// Zeilen der Personentabelle; nur im Reiter „persons“ gefüllt (ADR 0063).
    pub rows: Vec<super::people_table::PersonRow>,
    /// Porträt je Person: im Reiter „persons“ das Gesicht mit bester Qualität (ohne
    /// Merkmale berechnet), im Reiter „review“ das Referenzporträt der Personen der Seite.
    pub portraits: BTreeMap<i64, Face>,
    /// Offene Vorschläge je Person (ohne Personenfilter gezählt); nur im Reiter „review“.
    pub review_people: BTreeMap<i64, usize>,
    /// Abgelehnte Personen je Gesicht der geladenen Gruppen (Gesichts-ID → Personen-IDs).
    pub rejections: BTreeMap<i64, Vec<i64>>,
    /// Gesichter insgesamt im Reiter über alle Seiten („unsuitable“, „probable“, „confirmed“,
    /// „person-confirmed“, „ignored“; sonst 0).
    pub faces_total: usize,
    /// Reiter „probable“: Gesichter des zweiten Abschnitts („Eher jemand anderes“), deren
    /// beste Person nicht die gewählte ist; in `faces_total` enthalten.
    pub probable_others: usize,
    /// Reiter „probable“: beste andere Person (ID, Personenscore) je Gesicht der Seite aus dem
    /// zweiten Abschnitt. Gruppen mischen die Abschnitte nie.
    pub better: BTreeMap<i64, (i64, f32)>,
    /// Gesichter mit Ablehnung „nicht X“ der gewählten Person (Umschalter „Nicht X“), gezählt
    /// auf den Seiten „probable“, „person-confirmed“ und „person-rejected“; sonst 0.
    pub rejected_total: usize,
    /// Reiter „person-rejected“: Gesichter der Seite, die einer anderen Person bestätigt
    /// gehören (Gesichts-ID → Person); die Seite selbst trägt X als Person jedes Gesichts.
    pub confirmed_as: BTreeMap<i64, i64>,
    /// Anzeigegröße, mit der die Seite geschnitten wurde (`config::group_size`); bestimmt mit
    /// `config::face_groups_per_page` die Seitenzahl. 0 nur vor dem ersten Lesen.
    pub group_size: usize,
    /// Datenstand, zu dem die Seite gelesen wurde (`core::revision`; `None` ohne Revision).
    /// Caches der Oberfläche, die aus denselben Daten entstehen, prüfen sich daran.
    pub revisions: Option<super::revision::Revisions>,
}

/// Tabellenzeilen und Porträts für den Reiter „persons“, sonst leer.
fn table_state(
    collection: &Collection,
    mode: &str,
    people: &[Person],
    edges: &[super::relations::Edge],
    family: &super::kinship::Family,
    questions: &[super::gaps::Suggestion],
) -> Result<(Vec<super::people_table::PersonRow>, BTreeMap<i64, Face>)> {
    if mode != "persons" {
        return Ok((Vec::new(), BTreeMap::new()));
    }
    let stats = super::people_table::load_stats(collection)?;
    let rows = super::people_table::build_rows(
        people,
        &stats,
        edges,
        family,
        questions,
        super::people_table::current_year(),
    );
    let ids: Vec<i64> = people.iter().map(|p| p.id).collect();
    // Ohne Merkmale: eine Fensterabfrage, keine Vektoren dekodieren (Tabelle bleibt schnell).
    // Fehlende Porträts verhindern die Tabelle nicht, werden aber geloggt.
    let portraits = person_portraits(collection, &ids).unwrap_or_else(|error| {
        log::warn!(target: "personen", "portraets_nicht_gelesen ansicht=tabelle fehler={error:#}");
        Default::default()
    });
    Ok((rows, portraits))
}

/// Kanten, Stammbaum und Rückfragen in einem Zug laden.
fn tree_state(
    collection: &Collection,
) -> Result<(
    Vec<super::relations::Edge>,
    super::kinship::Family,
    Vec<super::gaps::Suggestion>,
)> {
    let edges = super::relations::load_edges(collection)?;
    let family = super::kinship::Family::new(&edges, &super::relations::load_genders(collection)?);
    let questions = super::gaps::suggestions(&edges, &super::gaps::load_decisions(collection)?);
    Ok((edges, family, questions))
}

/// Schlüssel in `face_settings`: der aktive Modellraum (Embedder) der Sammlung.
pub const EMBEDDER_KEY: &str = "rust.embedder_key";

pub fn setting(collection: &Collection, key: &str) -> Result<Option<String>> {
    let raw: Option<String> = collection
        .db
        .query_row("SELECT value FROM face_settings WHERE key=?", [key], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(raw.and_then(|v| serde_json::from_str::<String>(&v).ok()))
}
pub(crate) fn save_setting(db: &rusqlite::Connection, key: &str, value: &str) -> Result<()> {
    db.execute(
        "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}
pub fn people(collection: &Collection) -> Result<Vec<Person>> {
    // Bestätigte Gesichter je Person in einer Gruppierung (Teilindex ix_faces_user_counts)
    // statt einer korrelierten Unterabfrage je Person (ADR 0066).
    let counts: BTreeMap<i64, usize> = collection
        .db
        .prepare("SELECT person_id,count(*) FROM faces WHERE assignment='user' AND retained=0 AND person_id IS NOT NULL GROUP BY person_id")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(collection.db.prepare("SELECT p.id,coalesce(a.value,'Unbenannt'),coalesce(b.value,''),coalesce((SELECT value FROM person_attrs WHERE person_id=p.id AND key='gender'),''),coalesce((SELECT value FROM person_attrs WHERE person_id=p.id AND key='note'),'') FROM persons p LEFT JOIN person_attrs a ON a.person_id=p.id AND a.key='name' LEFT JOIN person_attrs b ON b.person_id=p.id AND b.key='birth_year' WHERE p.merged_into IS NULL ORDER BY a.value,p.id")?
        .query_map([],|r|{let id:i64=r.get(0)?;Ok(Person{id,name:r.get(1)?,birth:r.get(2)?,count:counts.get(&id).copied().unwrap_or(0),gender:r.get(3)?,note:r.get(4)?})})?.collect::<rusqlite::Result<_>>()?)
}

/// Bereiche der Datenrevision, von denen die Personenliste abhängt: Personen und ihre Angaben
/// (`people`) sowie die Zahl bestätigter Gesichter je Person (`faces`).
pub const PEOPLE_DOMAINS: &[super::revision::Domain] = &[
    super::revision::Domain::Faces,
    super::revision::Domain::People,
];

/// Datenstand und Personenliste in einer Lesetransaktion. Gleicht der Stand (`PEOPLE_DOMAINS`)
/// dem bekannten `known`, bleibt die Liste ungelesen (`None`); ohne Revision (Schema 6) wird
/// immer gelesen. Klein und schnell: eine Revisionsabfrage, bei Änderung zwei Abfragen über
/// Personen und bestätigte Gesichter.
pub fn people_if_changed(
    collection: &Collection,
    known: Option<&super::revision::Key>,
) -> Result<(Option<super::revision::Revisions>, Option<Vec<Person>>)> {
    collection.db.execute_batch("BEGIN")?;
    let result = (|| {
        let revisions = super::revision::read(&collection.db)?;
        let unchanged = revisions
            .as_ref()
            .is_some_and(|r| known == Some(&r.key(PEOPLE_DOMAINS)));
        let people = if unchanged {
            None
        } else {
            Some(people(collection)?)
        };
        Ok((revisions, people))
    })();
    collection.db.execute_batch("ROLLBACK")?;
    result
}
pub fn configure_automatic(
    collection: &mut Collection,
    auto_import: bool,
    nightly: bool,
) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    for (key, value) in [("auto_import", auto_import), ("nightly", nightly)] {
        tx.execute(
            "INSERT OR REPLACE INTO face_settings VALUES(?,?)",
            params![key, value.to_string()],
        )?;
    }
    tx.commit()?;
    Ok(())
}
pub fn configure_models(
    collection: &mut Collection,
    folder: &Path,
    detector: &str,
    embedder: &str,
) -> Result<()> {
    collection.writable()?;
    let manifests = inference::manifests(folder)?;
    if !manifests
        .iter()
        .any(|m| m.id == detector && m.kind == "detector")
        || !manifests
            .iter()
            .any(|m| m.id == embedder && m.kind == "embedder")
    {
        bail!("Modellauswahl ist im gewählten Modellordner nicht verfügbar");
    }
    let tx = collection.db.transaction()?;
    save_setting(&tx, "active_detector", detector)?;
    save_setting(&tx, "active_embedder", embedder)?;
    tx.commit()?;
    Ok(())
}
pub fn status(collection: &Collection) -> Result<Status> {
    let count =
        |sql: &str| -> Result<usize> { Ok(collection.db.query_row(sql, [], |r| r.get(0))?) };
    // Jede Zählung nutzt einen passenden Teilindex statt die ganze Gesichtstabelle zu lesen
    // (ADR 0066); die Bedingungen entsprechen wörtlich den Indexdefinitionen.
    let mut status = Status {
        total: count(
            "SELECT (SELECT count(*) FROM faces)-(SELECT count(*) FROM faces WHERE retained=1)-(SELECT count(*) FROM faces WHERE ignored=2 AND retained=0)",
        )?,
        confirmed: count("SELECT count(*) FROM faces WHERE assignment='user' AND retained=0")?,
        unknown: count(
            "SELECT count(*) FROM faces WHERE ignored=0 AND retained=0 AND person_id IS NULL",
        )?,
        review: count(
            "SELECT count(*) FROM faces WHERE ignored=0 AND retained=0 AND assignment IN ('suggested','auto')",
        )?,
        ignored: count("SELECT count(*) FROM faces WHERE ignored=1 AND retained=0")?,
        ..Default::default()
    };
    status.detector = active_detector_key(collection)?.unwrap_or_default();
    status.embedder = setting(collection, EMBEDDER_KEY)?.unwrap_or_default();
    let limits = super::recognition::Limits::load(collection)?;
    status.review_threshold = limits.review;
    status.review_margin = limits.margin;
    status.detector_id = setting(collection, "active_detector")?.unwrap_or_else(|| "yunet".into());
    status.embedder_id = setting(collection, "active_embedder")?.unwrap_or_else(|| "sface".into());
    for (key, target) in [
        ("auto_import", &mut status.auto_import),
        ("nightly", &mut status.nightly),
    ] {
        let raw: Option<String> = collection
            .db
            .query_row("SELECT value FROM face_settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?;
        *target = raw
            .and_then(|v| serde_json::from_str::<bool>(&v).ok())
            .unwrap_or(false);
    }
    (status.references, status.reference_people) = collection.db.query_row(
        "SELECT count(*),count(DISTINCT person_id) FROM training_set WHERE embedder_id=? AND split='ref'",
        [&status.embedder],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    status.dirty_people = collection.db.query_row(
        "SELECT count(*) FROM face_reference_dirty WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.nameable = collection.db.query_row(
        "SELECT coalesce(sum(members),0) FROM face_group_catalog WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.analysed_photos = collection.db.query_row(
        "SELECT count(*) FROM face_jobs j WHERE j.status='done' AND j.stage='detect' AND j.model_id=? AND EXISTS(SELECT 1 FROM photos p WHERE p.id=j.photo_id AND p.status='present')",
        [&status.detector], |r| r.get(0),
    )?;
    status.present_photos = collection.db.query_row(
        "SELECT count(*) FROM photos WHERE status='present'",
        [],
        |r| r.get(0),
    )?;
    status.pending = collection.db.query_row(
        "SELECT count(*) FROM face_maintenance WHERE embedder_id=?",
        [&status.embedder],
        |r| r.get(0),
    )?;
    status.applied = setting(
        collection,
        &super::recognition::applied_key(&status.embedder),
    )?
    .unwrap_or_default();
    Ok(status)
}
fn face_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Face> {
    Ok(Face {
        id: r.get(0)?,
        photo: r.get(1)?,
        source: r.get(2)?,
        bbox: [r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?],
        person: r.get(7)?,
        assignment: r.get(8)?,
        ignored: r.get(9)?,
        score: r.get::<_, Option<f32>>(10)?.unwrap_or_default(),
    })
}
const FACE_COLUMNS: &str = "f.id,f.photo_id,f.source_relpath,f.bbox_x1,f.bbox_y1,f.bbox_x2,f.bbox_y2,f.person_id,f.assignment,f.ignored,f.confidence";

/// Speichert "`source` ist `kind` von `target`" kanonisch und einmalig.
/// `kind`: parent, child, partner, sibling, cousin, friend, acquaintance (ADR 0060).
pub fn add_relation(
    collection: &mut Collection,
    source: i64,
    kind: &str,
    target: i64,
) -> Result<()> {
    let Some(role) = super::relations::Role::from_key(kind) else {
        bail!("Unbekannter Beziehungstyp");
    };
    super::relations::add(collection, source, role, target)?;
    Ok(())
}
pub fn remove_relation(
    collection: &Collection,
    source: i64,
    kind: &str,
    target: i64,
) -> Result<()> {
    let Some(role) = super::relations::Role::from_key(kind) else {
        bail!("Unbekannter Beziehungstyp");
    };
    super::relations::remove(collection, source, role, target)
}

/// Längenbegrenzungen für manuell angelegte Personen.
pub const MAX_NAME_CHARS: usize = 120;
pub const MAX_NOTE_CHARS: usize = 500;

/// Eingaben für eine Person ohne Gesichtsbezug (z. B. verstorbene Großeltern).
#[derive(Clone, Debug, Default)]
pub struct NewPerson {
    pub name: String,
    /// "weiblich", "männlich" oder leer (divers/unbekannt).
    pub gender: String,
    pub birth_year: Option<i32>,
    pub note: String,
}

/// Prüft die Eingaben und liefert den bereinigten Namen.
pub fn validate_new_person(new: &NewPerson) -> Result<String> {
    let name = new.name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        bail!("Bitte einen Namen eingeben");
    }
    if name.chars().count() > MAX_NAME_CHARS {
        bail!("Der Name darf höchstens {MAX_NAME_CHARS} Zeichen lang sein");
    }
    if new.note.chars().count() > MAX_NOTE_CHARS {
        bail!("Die Notiz darf höchstens {MAX_NOTE_CHARS} Zeichen lang sein");
    }
    if !matches!(new.gender.as_str(), "" | "weiblich" | "männlich") {
        bail!("Unbekanntes Geschlecht");
    }
    if let Some(year) = new.birth_year
        && !(1000..=3000).contains(&year)
    {
        bail!("Das Geburtsjahr muss zwischen 1000 und 3000 liegen");
    }
    Ok(name)
}

/// Legt eine Person ohne Gesichter an. Namensdubletten sind erlaubt (die Oberfläche
/// warnt; siehe [`people_named`]). Gesichtszuordnungen bleiben unberührt.
pub fn add_person(collection: &mut Collection, new: &NewPerson) -> Result<i64> {
    collection.writable()?;
    let name = validate_new_person(new)?;
    let tx = collection.db.transaction()?;
    tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
    let id = tx.last_insert_rowid();
    let mut attrs = vec![("name", name)];
    if !new.gender.is_empty() {
        attrs.push(("gender", new.gender.clone()));
    }
    if let Some(year) = new.birth_year {
        attrs.push(("birth_year", year.to_string()));
    }
    let note = new.note.trim();
    if !note.is_empty() {
        attrs.push(("note", note.to_string()));
    }
    for (key, value) in attrs {
        tx.execute(
            "INSERT INTO person_attrs VALUES(?,?,?)",
            params![id, key, value],
        )?;
    }
    tx.commit()?;
    Ok(id)
}

/// Aktive Personen mit diesem Namen (ohne Groß-/Kleinschreibung), für die Dublettenwarnung.
pub fn people_named(people: &[Person], name: &str) -> Vec<i64> {
    let normal =
        |text: &str| super::files::fold(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    let wanted = normal(name);
    people
        .iter()
        .filter(|p| normal(&p.name) == wanted)
        .map(|p| p.id)
        .collect()
}
