//! Nutzerentscheidungen zu Gesichtern: benennen, zuordnen, ablehnen, kein Gesicht,
//! löschen und rückgängig machen.
use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub struct FaceSnapshot {
    pub id: i64,
    pub person: Option<i64>,
    pub assignment: Option<String>,
    pub confidence: Option<f64>,
    pub ignored: bool,
    pub rejections: Vec<i64>,
    pub pins: Vec<(i64, String)>,
    /// „Kein Gesicht“ ([`NOT_A_FACE`]); dann ist auch `ignored` gesetzt.
    #[serde(default)]
    pub not_face: bool,
}
impl FaceSnapshot {
    /// Gespeicherter Wert von `faces.ignored`.
    fn ignored_value(&self) -> i64 {
        if self.not_face {
            NOT_A_FACE
        } else {
            i64::from(self.ignored)
        }
    }
}
#[derive(Clone)]
pub struct Undo {
    pub before: Vec<FaceSnapshot>,
    pub after: Vec<FaceSnapshot>,
    pub created: Option<i64>,
}
fn snapshot(db: &rusqlite::Connection, id: i64) -> Result<FaceSnapshot> {
    let mut value = db.query_row(
        "SELECT person_id,assignment,confidence,ignored FROM faces WHERE id=? AND retained=0",
        [id],
        |r| {
            let ignored: i64 = r.get(3)?;
            Ok(FaceSnapshot {
                id,
                person: r.get(0)?,
                assignment: r.get(1)?,
                confidence: r.get(2)?,
                ignored: ignored != 0,
                rejections: Vec::new(),
                pins: Vec::new(),
                not_face: ignored == NOT_A_FACE,
            })
        },
    )?;
    value.rejections = db
        .prepare("SELECT person_id FROM face_rejections WHERE face_id=? ORDER BY person_id")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    value.pins=db.prepare("SELECT person_id,embedder_id FROM training_set WHERE face_id=? AND pinned=1 ORDER BY person_id,embedder_id")?.query_map([id],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(value)
}
pub fn name_group(
    collection: &mut Collection,
    ids: &[i64],
    selected: &BTreeSet<i64>,
    person: Option<i64>,
    name: &str,
    ignore: bool,
) -> Result<Undo> {
    collection.writable()?;
    // Dieselbe Obergrenze wie die größte Anzeigegruppe (Regler „Gruppengröße“).
    let max = crate::config::GROUP_SIZE_MAX;
    if ids.is_empty() || ids.len() > max {
        bail!("Gruppe muss 1 bis {max} sichtbare Gesichter enthalten");
    }
    // Ohne Auswahl ist nur „keines davon ist diese Person“ sinnvoll: Dafür muss die Person
    // schon bestehen, sonst entstünde eine Person ohne Gesicht.
    if !ignore && selected.is_empty() && person.is_none() {
        bail!("Bitte mindestens ein Gesicht auswählen");
    }
    let tx = collection.db.transaction()?;
    let before = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    let mut created = None;
    let person = if ignore {
        None
    } else if let Some(person) = person {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM persons WHERE id=? AND merged_into IS NULL)",
            [person],
            |r| r.get(0),
        )?;
        if !exists {
            bail!("Person ist nicht mehr verfügbar");
        }
        Some(person)
    } else {
        let name = name.trim();
        if name.is_empty() {
            bail!("Bitte einen Namen eingeben");
        }
        let matches = tx
            .prepare("SELECT person_id FROM person_attrs WHERE key='name' AND casefold(value)=?")?
            .query_map([crate::core::files::fold(name)], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        match matches.as_slice() {
            [id] => Some(*id),
            [] => {
                tx.execute("INSERT INTO persons DEFAULT VALUES", [])?;
                let id = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO person_attrs VALUES(?,'name',?)",
                    params![id, name],
                )?;
                created = Some(id);
                Some(id)
            }
            _ => bail!("Mehrere Personen heißen so. Bitte die konkrete Personen-ID wählen."),
        }
    };
    for id in ids {
        if ignore {
            tx.execute("UPDATE faces SET ignored=1,person_id=CASE WHEN assignment='user' THEN person_id ELSE NULL END,assignment=CASE WHEN assignment='user' THEN assignment ELSE NULL END WHERE id=?",[id])?;
        } else if selected.contains(id) {
            tx.execute("UPDATE faces SET person_id=?,assignment='user',confidence=1,ignored=0,assigned_at=datetime('now') WHERE id=?",params![person,id])?;
            tx.execute(
                "DELETE FROM face_rejections WHERE face_id=? AND person_id=?",
                params![id, person],
            )?;
        } else {
            tx.execute(
                "INSERT OR IGNORE INTO face_rejections VALUES(?,?)",
                params![id, person],
            )?;
            // Die Ablehnung wirkt sofort: Trägt das Gesicht genau diese Person (vorgeschlagen
            // oder bestätigt), wird es unbenannt und kehrt über `face_group_dirty` in eine
            // Gruppe unter „Benennen“ zurück, statt auf den nächsten Abgleich zu warten.
            tx.execute(
                "UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL,assigned_at=NULL WHERE id=? AND person_id IS ?",
                params![id, person],
            )?;
        }
        tx.execute("DELETE FROM training_set WHERE face_id=?", [id])?;
    }
    let after = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    tx.commit()?;
    let undo = Undo {
        before,
        after,
        created,
    };
    if undo.summary().rejected > 0 {
        regroup_released(collection, ids);
    }
    Ok(undo)
}
/// Hebt die gespeicherte Ablehnung „nicht `person`“ für diese Gesichter auf (Personenseite
/// „Nicht X“, Kreuz bzw. „Ablehnung aufheben“). Zuordnung, Ignorieren und Referenzauswahl
/// bleiben unberührt; das Gesicht ist danach wieder offen für `person` und kann vorgeschlagen
/// werden. Weil keine Zuordnung wechselt, löst kein Trigger den Abgleich aus: Er wird hier für
/// die betroffenen Modelle vorgemerkt (wie beim Löschen eines Gesichts). Rücknahme über
/// [`undo`] stellt die Ablehnung exakt wieder her. Gesichter ohne diese Ablehnung bleiben
/// unverändert.
pub fn lift_rejection(collection: &mut Collection, ids: &[i64], person: i64) -> Result<Undo> {
    collection.writable()?;
    let max = crate::config::GROUP_SIZE_MAX;
    if ids.is_empty() || ids.len() > max {
        bail!("Gruppe muss 1 bis {max} sichtbare Gesichter enthalten");
    }
    let tx = collection.db.transaction()?;
    let before = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    for id in ids {
        let removed = tx.execute(
            "DELETE FROM face_rejections WHERE face_id=? AND person_id=?",
            params![id, person],
        )?;
        if removed > 0 {
            mark_rematch(&tx, *id)?;
        }
    }
    let after = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    tx.commit()?;
    Ok(Undo {
        before,
        after,
        created: None,
    })
}
/// Merkt den Abgleich für die Modelle dieses Gesichts vor (neue Generation), wenn sich nur
/// seine Ablehnungen ändern; Zuordnungswechsel erledigt der Trigger `user_face_maintenance`.
fn mark_rematch(db: &rusqlite::Connection, id: i64) -> Result<()> {
    db.execute(
        "INSERT INTO face_maintenance SELECT embedder_id,0,1 FROM face_embeddings WHERE face_id=? ON CONFLICT(embedder_id) DO UPDATE SET after_photo=0,generation=generation+1",
        [id],
    )?;
    Ok(())
}
/// „Kein Gesicht“ (z. B. ein Muster auf der Tapete): eine dauerhafte Nutzerentscheidung
/// je Gesicht ([`NOT_A_FACE`]). Das Vorkommen bleibt gespeichert, damit eine erneute Analyse
/// es wiederfindet und die Entscheidung behält; jede Zuordnung und Referenz entfällt. Das
/// Foto bleibt unberührt. Rücknahme über [`undo`].
pub fn mark_not_face(collection: &mut Collection, ids: &[i64]) -> Result<Undo> {
    collection.writable()?;
    if ids.is_empty() {
        bail!("Bitte mindestens ein Gesicht auswählen");
    }
    let tx = collection.db.transaction()?;
    let before = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    for id in ids {
        tx.execute(
            "UPDATE faces SET ignored=?,person_id=NULL,assignment=NULL,confidence=NULL,assigned_at=NULL WHERE id=?",
            params![NOT_A_FACE, id],
        )?;
        tx.execute("DELETE FROM training_set WHERE face_id=?", [id])?;
    }
    let after = ids
        .iter()
        .map(|id| snapshot(&tx, *id))
        .collect::<Result<Vec<_>>>()?;
    tx.commit()?;
    Ok(Undo {
        before,
        after,
        created: None,
    })
}
/// Gerade freigegebene Gesichter sofort gruppieren, damit sie unter „Benennen“ erscheinen.
/// Begrenzt auf wenige Schritte; was liegen bleibt, erledigt die Hintergrundwartung.
/// Gruppen sind abgeleitet, ein Fehler hier darf die gespeicherte Entscheidung nicht gefährden.
fn regroup_released(collection: &mut Collection, ids: &[i64]) {
    let Ok(Some(key)) = setting(collection, EMBEDDER_KEY) else {
        return;
    };
    let marks = vec!["?"; ids.len()].join(",");
    let pending = |collection: &Collection| -> bool {
        let mut values: Vec<rusqlite::types::Value> = vec![key.clone().into()];
        values.extend(ids.iter().map(|id| rusqlite::types::Value::from(*id)));
        collection
            .db
            .query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM face_group_dirty WHERE embedder_id=? AND face_id IN ({marks}))"),
                rusqlite::params_from_iter(values),
                |r| r.get(0),
            )
            .unwrap_or(false)
    };
    let control = Control::default();
    for _ in 0..4 {
        if !pending(collection)
            || !matches!(prepare_groups(collection, &key, &control), Ok(n) if n > 0)
        {
            break;
        }
    }
}
/// Was eine Entscheidung bewirkt hat, für die Rückmeldung an den Nutzer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DecisionSummary {
    pub confirmed: usize,
    pub rejected: usize,
    pub ignored: usize,
    /// Als „Kein Gesicht“ markiert (zählt nicht als ignoriert).
    pub not_face: usize,
    /// Ablehnung aufgehoben, ohne dass das Gesicht bestätigt wurde (Personenseite „Nicht X“).
    pub lifted: usize,
}
impl Undo {
    pub fn summary(&self) -> DecisionSummary {
        let mut summary = DecisionSummary::default();
        for (before, after) in self.before.iter().zip(&self.after) {
            let user = after.assignment.as_deref() == Some("user");
            if user
                && !after.ignored
                && (before.assignment.as_deref() != Some("user")
                    || before.person != after.person
                    || before.ignored)
            {
                summary.confirmed += 1;
            } else if before
                .rejections
                .iter()
                .any(|person| !after.rejections.contains(person))
            {
                summary.lifted += 1;
            }
            if after
                .rejections
                .iter()
                .any(|person| !before.rejections.contains(person))
            {
                summary.rejected += 1;
            }
            if after.not_face && !before.not_face {
                summary.not_face += 1;
            } else if after.ignored && !before.ignored {
                summary.ignored += 1;
            }
        }
        summary
    }
}
pub fn undo(collection: &mut Collection, undo: &Undo) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    // Ein Vorschlag des Abgleichs ist keine Nutzeränderung: Hat er seit der Entscheidung eine
    // maschinelle Zuordnung gesetzt oder entfernt (etwa nach einer aufgehobenen Ablehnung),
    // hindert das die Rücknahme nicht; sie stellt den Stand davor wieder her.
    let machine = |s: &FaceSnapshot| s.assignment.as_deref() != Some("user");
    let mut rematch = Vec::new();
    for expected in &undo.after {
        let current = snapshot(&tx, expected.id)?;
        let assignment_changed =
            current.person != expected.person || current.assignment != expected.assignment;
        if (assignment_changed && !(machine(&current) && machine(expected)))
            || current.ignored_value() != expected.ignored_value()
            || current.rejections != expected.rejections
            || current.pins != expected.pins
        {
            bail!("Spätere Nutzeränderung verhindert Rücknahme");
        }
        if let Some(before) = undo.before.iter().find(|b| b.id == expected.id)
            && before.rejections != current.rejections
        {
            rematch.push(expected.id);
        }
    }
    // Nur geänderte Ablehnungen lösen keinen Trigger aus; der Abgleich beachtet sie neu.
    for id in rematch {
        mark_rematch(&tx, id)?;
    }
    for before in &undo.before {
        tx.execute(
            "UPDATE faces SET person_id=?,assignment=?,confidence=?,ignored=? WHERE id=?",
            params![
                before.person,
                before.assignment,
                before.confidence,
                before.ignored_value(),
                before.id
            ],
        )?;
        tx.execute("DELETE FROM face_rejections WHERE face_id=?", [before.id])?;
        for person in &before.rejections {
            tx.execute(
                "INSERT INTO face_rejections VALUES(?,?)",
                params![before.id, person],
            )?;
        }
        tx.execute("DELETE FROM training_set WHERE face_id=?", [before.id])?;
        for (person, key) in &before.pins {
            tx.execute(
                "INSERT INTO training_set VALUES(?,?,?,'ref',datetime('now'),'pin',1)",
                params![person, before.id, key],
            )?;
        }
    }
    if let Some(id) = undo.created {
        let used:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM faces WHERE person_id=?) OR EXISTS(SELECT 1 FROM relations WHERE from_person=? OR to_person=?)",params![id,id,id],|r|r.get(0))?;
        if !used {
            tx.execute("DELETE FROM person_attrs WHERE person_id=?", [id])?;
            tx.execute("DELETE FROM persons WHERE id=?", [id])?;
        }
    }
    tx.commit()?;
    Ok(())
}
pub(super) fn delete_face_rows(db: &rusqlite::Connection, id: i64) -> Result<()> {
    for table in [
        "face_embeddings",
        "face_rejections",
        "training_set",
        "training_exclusions",
        "face_review",
    ] {
        db.execute(&format!("DELETE FROM {table} WHERE face_id=?"), [id])?;
    }
    db.execute("DELETE FROM faces WHERE id=?", [id])?;
    Ok(())
}
pub fn delete_face(collection: &mut Collection, photo: i64, id: i64) -> Result<()> {
    collection.writable()?;
    let tx = collection.db.transaction()?;
    let valid: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM faces WHERE id=? AND photo_id=? AND retained=0)",
        params![id, photo],
        |r| r.get(0),
    )?;
    if !valid {
        bail!("Gesicht gehört nicht mehr zum ausgewählten Foto");
    }
    tx.execute("INSERT OR IGNORE INTO face_reference_dirty SELECT e.embedder_id,f.person_id FROM face_embeddings e JOIN faces f ON f.id=e.face_id WHERE f.id=? AND f.person_id IS NOT NULL",[id])?;
    mark_rematch(&tx, id)?;
    delete_face_rows(&tx, id)?;
    tx.commit()?;
    Ok(())
}
