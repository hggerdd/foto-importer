use super::{
    collection::Collection,
    files,
    jobs::{Control, command, wait_child},
};
use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params, types::Value};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
    path::PathBuf,
    time::Duration,
};

pub fn read_metadata(paths: &[PathBuf], executable: &str, control: &Control) -> Result<Vec<Json>> {
    let mut args = tempfile::NamedTempFile::new()?;
    for path in paths {
        let text = path.to_string_lossy();
        if text.contains(['\n', '\r']) {
            bail!("ExifTool kann Dateinamen mit Zeilenumbruch nicht lesen");
        }
        writeln!(args, "{text}")?;
    }
    args.flush()?;
    let mut output = tempfile::tempfile()?;
    let mut error = tempfile::tempfile()?;
    let mut child = command(executable)
        .args([
            "-j",
            "-n",
            "-fast",
            "-G1",
            "-charset",
            "filename=UTF8",
            "-@",
        ])
        .arg(args.path())
        .stdin(std::process::Stdio::null())
        .stdout(output.try_clone()?)
        .stderr(error.try_clone()?)
        .spawn()
        .context("ExifTool fehlt oder ist nicht startbar. Pfad in Einstellungen prüfen.")?;
    let status = wait_child(&mut child, control, Duration::from_secs(120))?;
    if !status.success() && status.code() != Some(1) {
        error.rewind()?;
        let mut message = String::new();
        error.read_to_string(&mut message)?;
        bail!("ExifTool: {message}");
    }
    output.rewind()?;
    Ok(serde_json::from_reader(output)?)
}

fn tags(raw: &Json) -> BTreeMap<String, Json> {
    raw.as_object()
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.rsplit(':').next().unwrap_or(k).to_string(), v.clone()))
                .collect()
        })
        .unwrap_or_default()
}
fn tag<'a>(tags: &'a BTreeMap<String, Json>, names: &[&str]) -> Option<&'a Json> {
    names
        .iter()
        .find_map(|n| tags.get(*n).filter(|v| !v.is_null()))
}
fn text(tags: &BTreeMap<String, Json>, names: &[&str]) -> Option<String> {
    tag(tags, names).map(|v| {
        v.as_str()
            .map(str::to_string)
            .unwrap_or_else(|| v.to_string())
    })
}
fn number(tags: &BTreeMap<String, Json>, names: &[&str]) -> Option<f64> {
    tag(tags, names)
        .and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok()))
        .filter(|v| v.is_finite())
}
pub fn identity(raw: &Json) -> Option<String> {
    let t = tags(raw);
    let serial = text(
        &t,
        &["SerialNumber", "InternalSerialNumber", "CameraSerialNumber"],
    );
    let date = text(&t, &["DateTimeOriginal", "CreateDate"]);
    let sub = text(&t, &["SubSecTimeOriginal"]).unwrap_or_default();
    if let Some(serial) = serial {
        if let Some(count) = text(&t, &["ShutterCount"]) {
            return Some(format!("{serial}:{count}"));
        }
        return date.map(|date| format!("{serial}:{date}.{sub}"));
    }
    if !sub.is_empty() {
        Some(format!(
            "{}{}:{}.{}:{}x{}",
            text(&t, &["Make"])?,
            text(&t, &["Model"])?,
            date?,
            sub,
            text(&t, &["ExifImageWidth", "ImageWidth"])?,
            text(&t, &["ExifImageHeight", "ImageHeight"])?
        ))
    } else {
        None
    }
}

pub fn enrich(collection: &mut Collection, executable: &str, control: &Control) -> Result<String> {
    collection.writable()?;
    let total: usize = collection.db.query_row(
        "SELECT count(*) FROM files WHERE status='present' AND meta_state!='done'",
        [],
        |r| r.get(0),
    )?;
    let mut done = 0;
    let mut cursor = 0;
    loop {
        control.checkpoint()?;
        let rows=collection.db.prepare("SELECT id,relpath,photo_id FROM files WHERE status='present' AND meta_state!='done' AND id>? ORDER BY id LIMIT 128")?
            .query_map([cursor],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.is_empty() {
            break;
        }
        let paths: Vec<_> = rows
            .iter()
            .map(|r| collection.safe_path(&r.1))
            .collect::<Result<_>>()?;
        let raw = read_metadata(&paths, executable, control)?;
        // SourceFile matching prevents a missing ExifTool result shifting other files.
        let raw: BTreeMap<_, _> = raw
            .into_iter()
            .filter_map(|v| Some((files::fold(v.get("SourceFile")?.as_str()?), v)))
            .collect();
        let tx = collection.db.transaction()?;
        for ((id, rel, photo), path) in rows.iter().zip(&paths) {
            control.checkpoint()?;
            cursor = *id;
            let key = files::fold(&path.to_string_lossy().replace('\\', "/"));
            let found = raw
                .iter()
                .find(|(k, _)| k.replace('\\', "/") == key)
                .map(|(_, v)| v);
            let Some(raw) = found else {
                tx.execute("UPDATE files SET meta_state='error',meta_error='Kein ExifTool-Ergebnis' WHERE id=?",[id])?;
                continue;
            };
            let t = tags(raw);
            if let Some(error) = text(&t, &["Error"]) {
                tx.execute(
                    "UPDATE files SET meta_state='error',meta_error=? WHERE id=?",
                    params![error, id],
                )?;
                continue;
            }
            tx.execute("INSERT OR REPLACE INTO meta(file_id,make,model,serial,shutter_count,dt_original,subsec,offset_time,width,height,orientation,lens,focal,aperture,exposure,iso,lat,lon,alt,rating,label,json) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",params![id,text(&t,&["Make"]),text(&t,&["Model"]),text(&t,&["SerialNumber","InternalSerialNumber","CameraSerialNumber"]),number(&t,&["ShutterCount"]),text(&t,&["DateTimeOriginal","CreateDate"]),text(&t,&["SubSecTimeOriginal"]),text(&t,&["OffsetTimeOriginal"]),number(&t,&["ExifImageWidth","ImageWidth"]),number(&t,&["ExifImageHeight","ImageHeight"]),number(&t,&["Orientation"]),text(&t,&["LensID","LensModel","Lens"]),number(&t,&["FocalLength"]),number(&t,&["FNumber"]),text(&t,&["ExposureTime"]),number(&t,&["ISO"]),number(&t,&["GPSLatitude"]),number(&t,&["GPSLongitude"]),number(&t,&["GPSAltitude"]),number(&t,&["Rating"]),text(&t,&["Label"]),raw.to_string()])?;
            tx.execute(
                "UPDATE files SET meta_state='done',meta_error=NULL WHERE id=?",
                [id],
            )?;
            if let Some(key) =
                identity(raw).filter(|_| !rel.split('/').any(|p| p.eq_ignore_ascii_case("export")))
            {
                let current_key: Option<String> =
                    tx.query_row("SELECT identity_key FROM photos WHERE id=?", [photo], |r| {
                        r.get(0)
                    })?;
                let existing: Option<i64> = tx
                    .query_row("SELECT id FROM photos WHERE identity_key=?", [&key], |r| {
                        r.get(0)
                    })
                    .optional()?;
                if current_key.as_ref().is_some_and(|v| v != &key) {
                    let new_photo = if let Some(p) = existing {
                        p
                    } else {
                        tx.execute(
                            "INSERT INTO photos(identity_key,status) VALUES(?,'present')",
                            [&key],
                        )?;
                        tx.last_insert_rowid()
                    };
                    tx.execute(
                        "UPDATE files SET photo_id=? WHERE id=?",
                        params![new_photo, id],
                    )?;
                } else if let Some(other) = existing.filter(|p| p != photo) {
                    let winner = other.min(*photo);
                    let loser = other.max(*photo);
                    tx.execute(
                        "UPDATE photos SET identity_key=NULL,status='missing' WHERE id=?",
                        [loser],
                    )?;
                    tx.execute(
                        "UPDATE files SET photo_id=? WHERE photo_id=?",
                        params![winner, loser],
                    )?;
                    tx.execute(
                        "UPDATE faces SET photo_id=? WHERE photo_id=?",
                        params![winner, loser],
                    )?;
                    tx.execute(
                        "DELETE FROM face_jobs WHERE photo_id IN (?,?)",
                        params![winner, loser],
                    )?;
                    tx.execute(
                        "INSERT OR REPLACE INTO settings VALUES(?,?)",
                        params![format!("photo.redirect.{loser}"), winner.to_string()],
                    )?;
                    tx.execute(
                        "UPDATE photos SET identity_key=? WHERE id=?",
                        params![key, winner],
                    )?;
                } else {
                    tx.execute(
                        "UPDATE photos SET identity_key=? WHERE id=?",
                        params![key, photo],
                    )?;
                }
            }
            done += 1;
        }
        tx.commit()?;
        control.progress(done, total);
    }
    collection.backup()?;
    Ok(format!("{done} Dateien mit Metadaten angereichert"))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub field: String,
    pub operator: String,
    pub value: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Search {
    pub text: String,
    pub start: String,
    pub end: String,
    pub months: Vec<u32>,
    pub people_all: Vec<i64>,
    pub people_any: Vec<i64>,
    pub people_none: Vec<i64>,
    pub only_people: bool,
    pub suggestions: bool,
    pub face_state: String,
    pub rules: Vec<Rule>,
    pub any_rule: bool,
    pub oldest: bool,
    pub page: usize,
    pub folder: Option<String>,
}
pub const FIELDS: &[(&str, &str)] = &[
    ("camera", "Kamera"),
    ("lens", "Objektiv"),
    ("make", "Hersteller"),
    ("serial", "Seriennummer"),
    ("iso", "ISO"),
    ("aperture", "Blende"),
    ("exposure", "Belichtung"),
    ("focal", "Brennweite"),
    ("width", "Breite"),
    ("height", "Höhe"),
    ("rating", "Sterne"),
    ("label", "Farbmarkierung"),
    ("path", "Pfad"),
    ("format", "Format"),
    ("latitude", "GPS-Breite"),
    ("longitude", "GPS-Länge"),
    ("gps", "GPS vorhanden"),
    ("metadata", "Metadatenstatus"),
    ("shutter_count", "Auslösungen"),
    ("face_count", "Anzahl erkannter Gesichter"),
];
const CURRENT_FACES: &str = "f.photo_id=p.photo_id AND f.retained=0";
pub(super) const PHOTO_FORMATS: &str = "p.ext IN ('jpg','jpeg','png','gif','bmp','tif','tiff','nef','nrw','dng','heic','arw','cr2','cr3','orf')";
const DETECTION_DONE: &str = "EXISTS(SELECT 1 FROM face_jobs j WHERE j.photo_id=p.photo_id AND j.stage='detect' AND j.model_id=? AND j.status='done')";

fn compile_face_count(rule: &Rule, detector: &str) -> Result<(String, Vec<Value>)> {
    let mut args = vec![detector.to_owned().into()];
    let comparison = match rule.operator.as_str() {
        "present" => return Ok((DETECTION_DONE.into(), args)),
        "empty" => return Ok((format!("NOT {DETECTION_DONE}"), args)),
        "is" => "=",
        "not" => "!=",
        "min" => ">=",
        "max" => "<=",
        _ => bail!("Gesichter mit einer Anzahl vergleichen"),
    };
    let count = rule
        .value
        .trim()
        .parse::<u32>()
        .context("Bitte eine ganze, nichtnegative Anzahl eingeben")?;
    args.push(i64::from(count).into());
    Ok((
        format!(
            "{DETECTION_DONE} AND (SELECT count(*) FROM faces f INDEXED BY ix_faces_photo WHERE {CURRENT_FACES}){comparison}?"
        ),
        args,
    ))
}

pub fn compile_rule(rule: &Rule) -> Result<(String, Vec<Value>)> {
    let (expression, numeric) = match rule.field.as_str() {
        "camera" => ("p.camera", false),
        "lens" => ("p.lens", false),
        "make" => ("m.make", false),
        "serial" => ("m.serial", false),
        "path" => ("p.relpath", false),
        "metadata" => ("p.meta_state", false),
        "iso" => ("p.iso", true),
        "aperture" => ("m.aperture", true),
        "exposure" => ("CAST(m.exposure AS REAL)", true),
        "focal" => ("m.focal", true),
        "width" => ("m.width", true),
        "height" => ("m.height", true),
        "shutter_count" => ("m.shutter_count", true),
        "latitude" => ("m.lat", true),
        "longitude" => ("m.lon", true),
        "rating" => (
            "coalesce((SELECT sm.rating FROM files sf JOIN meta sm ON sm.file_id=sf.id WHERE sf.photo_id=p.photo_id AND sf.ext='xmp' AND sf.status='present' AND sm.rating IS NOT NULL ORDER BY sf.id LIMIT 1),m.rating)",
            true,
        ),
        "label" => (
            "coalesce((SELECT sm.label FROM files sf JOIN meta sm ON sm.file_id=sf.id WHERE sf.photo_id=p.photo_id AND sf.ext='xmp' AND sf.status='present' AND sm.label IS NOT NULL ORDER BY sf.id LIMIT 1),m.label)",
            false,
        ),
        "gps" => {
            let expr = "(m.lat IS NOT NULL AND m.lon IS NOT NULL)";
            return match rule.operator.as_str() {
                "present" => Ok((expr.into(), vec![])),
                "empty" => Ok((format!("NOT {expr}"), vec![])),
                _ => bail!("GPS mit vorhanden oder fehlt vergleichen"),
            };
        }
        "format" => {
            if !["is", "not"].contains(&rule.operator.as_str())
                || rule.value.is_empty()
                || !rule.value.chars().all(|c| c.is_ascii_alphanumeric())
            {
                bail!("Format mit ist / ist nicht und Dateiendung vergleichen");
            }
            return Ok((
                format!(
                    "{}EXISTS(SELECT 1 FROM files v WHERE v.photo_id=p.photo_id AND v.status='present' AND v.ext=?)",
                    if rule.operator == "not" { "NOT " } else { "" }
                ),
                vec![rule.value.to_lowercase().into()],
            ));
        }
        _ => bail!("Unbekanntes Suchfeld"),
    };
    let op = rule.operator.as_str();
    if op == "empty" || op == "present" {
        return Ok((
            format!(
                "{}({expression} IS NULL OR trim(CAST({expression} AS TEXT))='')",
                if op == "present" { "NOT " } else { "" }
            ),
            vec![],
        ));
    }
    if rule.value.trim().is_empty() {
        bail!("Suchwert fehlt");
    }
    if numeric {
        let value: f64 = rule
            .value
            .replace(',', ".")
            .parse()
            .context("Bitte eine Zahl eingeben")?;
        if !value.is_finite() {
            bail!("Zahl muss endlich sein");
        }
        let op = match op {
            "is" => "=",
            "not" => "!=",
            "min" => ">=",
            "max" => "<=",
            _ => bail!("Ungültiger Zahlenvergleich"),
        };
        Ok((format!("{expression}{op}?"), vec![value.into()]))
    } else {
        let clause = match op {
            "is" => format!("casefold({expression})=? AND {expression} IS NOT NULL"),
            "not" => format!("casefold({expression})!=? AND {expression} IS NOT NULL"),
            "contains" => format!("instr(casefold({expression}),?)>0"),
            _ => bail!("Ungültiger Textvergleich"),
        };
        Ok((clause, vec![files::fold(&rule.value).into()]))
    }
}

pub fn predicate(search: &Search, detector: &str) -> Result<(String, Vec<Value>)> {
    if search.people_all.len() + search.people_any.len() + search.people_none.len() > 100
        || search.rules.len() > 40
    {
        bail!("Zu viele Kriterien");
    }
    let mut clauses = vec![PHOTO_FORMATS.to_owned()];
    let mut args = Vec::new();
    let state = if search.suggestions {
        "f.assignment IN ('user','auto','suggested')"
    } else {
        "f.assignment='user'"
    };
    let current = CURRENT_FACES;
    if !search.text.trim().is_empty() {
        clauses.push(format!(
            "(instr(casefold(p.relpath),?)>0 OR instr(casefold(p.camera),?)>0 OR instr(casefold(p.lens),?)>0
              OR EXISTS(SELECT 1 FROM faces f INDEXED BY ix_faces_photo
                  JOIN persons person ON person.id=f.person_id AND person.merged_into IS NULL
                  JOIN person_attrs name ON name.person_id=person.id AND name.key='name'
                  WHERE {current} AND {state} AND instr(casefold(name.value),?)>0))"
        ));
        args.extend(vec![Value::Text(files::fold(search.text.trim())); 4]);
    }
    for (text, lower) in [(&search.start, true), (&search.end, false)] {
        if !text.is_empty() {
            let day = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .context("Datum als JJJJ-MM-TT eingeben")?;
            let day = if lower {
                day
            } else {
                day.succ_opt().context("Enddatum zu groß")?
            };
            clauses.push(format!("p.dt_original{}?", if lower { ">=" } else { "<" }));
            args.push(day.format("%Y:%m:%d").to_string().into());
        }
    }
    if !search.start.is_empty() && !search.end.is_empty() && search.start > search.end {
        bail!("Beginn liegt nach dem Ende");
    }
    if !search.months.is_empty() {
        if search.months.iter().any(|m| !(1..=12).contains(m)) {
            bail!("Ungültiger Monat");
        }
        clauses.push(format!(
            "CAST(substr(p.dt_original,6,2) AS INTEGER) IN ({})",
            placeholders(search.months.len())
        ));
        args.extend(search.months.iter().map(|m| Value::Integer(*m as i64)));
    }
    let included: std::collections::BTreeSet<_> = search
        .people_all
        .iter()
        .chain(&search.people_any)
        .copied()
        .collect();
    if search
        .people_all
        .iter()
        .chain(&search.people_any)
        .chain(&search.people_none)
        .any(|p| *p <= 0)
    {
        bail!("Ungültige Personen-ID");
    }
    if search
        .people_all
        .iter()
        .any(|p| search.people_none.contains(p))
        || (!search.people_any.is_empty()
            && search
                .people_any
                .iter()
                .all(|p| search.people_none.contains(p)))
    {
        bail!("Person gleichzeitig erforderlich und ausgeschlossen");
    }
    if search.only_people && included.is_empty() {
        bail!("Bitte zuerst Personen auswählen");
    }
    if ["none", "unanalysed"].contains(&search.face_state.as_str()) && !included.is_empty() {
        bail!("Personensuche widerspricht Gesichtsfilter");
    }
    for person in &search.people_all {
        clauses.push(format!("EXISTS(SELECT 1 FROM faces f INDEXED BY ix_faces_photo WHERE {current} AND {state} AND f.person_id=?)"));
        args.push((*person).into());
    }
    for (people, negative) in [(&search.people_any, false), (&search.people_none, true)] {
        if !people.is_empty() {
            clauses.push(format!("{}EXISTS(SELECT 1 FROM faces f INDEXED BY ix_faces_photo WHERE {current} AND {state} AND f.person_id IN ({}))",if negative{"NOT "}else{""},placeholders(people.len())));
            args.extend(people.iter().map(|p| Value::Integer(*p)));
        }
    }
    let analysed = DETECTION_DONE;
    if !search.people_none.is_empty()
        || search.only_people
        || ["none", "unknown", "with_faces"].contains(&search.face_state.as_str())
    {
        clauses.push(analysed.into());
        args.push(detector.to_string().into());
    }
    if search.only_people {
        clauses.push(format!("NOT EXISTS(SELECT 1 FROM faces f INDEXED BY ix_faces_photo WHERE {current} AND (f.person_id IS NULL OR f.assignment IS NULL OR NOT ({state}) OR f.person_id NOT IN ({})))",placeholders(included.len())));
        args.extend(included.iter().map(|p| Value::Integer(*p)));
        if search.people_any.is_empty() {
            clauses.push(format!(
                "(SELECT count(*) FROM faces f INDEXED BY ix_faces_photo WHERE {current})=?"
            ));
            args.push((included.len() as i64).into());
        }
    }
    match search.face_state.as_str(){""|"any"=>{},"none"=>clauses.push(format!("NOT EXISTS(SELECT 1 FROM faces f INDEXED BY ix_faces_photo WHERE {current})")),"with_faces"=>clauses.push(format!("EXISTS(SELECT 1 FROM faces f INDEXED BY ix_faces_photo WHERE {current})")),"unknown"=>clauses.push(format!("EXISTS(SELECT 1 FROM faces f INDEXED BY ix_faces_photo WHERE {current} AND (f.person_id IS NULL OR f.assignment IS NULL OR NOT ({state})))")),"unanalysed"=>{clauses.push(format!("NOT {analysed}"));args.push(detector.to_string().into());},_=>bail!("Unbekannter Gesichtsfilter")}
    let mut rules = Vec::new();
    for rule in &search.rules {
        let (sql, values) = if rule.field == "face_count" {
            compile_face_count(rule, detector)?
        } else {
            compile_rule(rule)?
        };
        rules.push(format!("({sql})"));
        args.extend(values);
    }
    if !rules.is_empty() {
        clauses.push(format!(
            "({})",
            rules.join(if search.any_rule { " OR " } else { " AND " })
        ));
    }
    if let Some(folder) = &search.folder {
        clauses.push("substr(p.relpath,1,?)=?".into());
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        args.push((prefix.chars().count() as i64).into());
        args.push(prefix.into());
    }
    Ok((clauses.join(" AND "), args))
}
fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

#[derive(Clone, Debug)]
pub struct Photo {
    pub id: i64,
    pub file_id: i64,
    pub path: String,
    pub name: String,
    pub date: String,
    pub camera: String,
    pub lens: String,
    pub iso: Option<f64>,
    pub size: u64,
    pub source: String,
    pub orientation: u32,
}
#[derive(Default)]
pub struct SearchPage {
    pub photos: Vec<Photo>,
    pub total: usize,
    pub cameras: Vec<(String, usize)>,
}
pub fn search(collection: &Collection, filters: &Search, detector: &str) -> Result<SearchPage> {
    let (sql, args) = predicate(filters, detector)?;
    collection.db.execute_batch("BEGIN")?;
    let result = (|| -> Result<SearchPage> {
        let from =
            format!("FROM metadata_photos p LEFT JOIN meta m ON m.file_id=p.file_id WHERE {sql}");
        let total = collection.db.query_row(
            &format!("SELECT count(*) {from}"),
            rusqlite::params_from_iter(&args),
            |r| r.get(0),
        )?;
        let cameras=collection.db.prepare(&format!("SELECT coalesce(p.camera,'Unbekannt'),count(*) {from} GROUP BY p.camera ORDER BY count(*) DESC LIMIT 8"))?.query_map(rusqlite::params_from_iter(&args),|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut page_args = args.clone();
        let has_sources: bool = collection.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='face_sources')",
            [],
            |r| r.get(0),
        )?;
        let source = if has_sources {
            "coalesce((SELECT sf.relpath FROM face_sources s JOIN files sf ON sf.id=s.file_id WHERE s.photo_id=p.photo_id),p.relpath)"
        } else {
            "p.relpath"
        };
        let orientation = if has_sources {
            "coalesce((SELECT sm.orientation FROM face_sources s JOIN meta sm ON sm.file_id=s.file_id WHERE s.photo_id=p.photo_id),m.orientation,1)"
        } else {
            "coalesce(m.orientation,1)"
        };
        page_args.push((crate::config::PAGE_SIZE as i64).into());
        page_args.push(
            ((filters.page.saturating_mul(crate::config::PAGE_SIZE)).min(i64::MAX as usize) as i64)
                .into(),
        );
        let query = format!(
            "SELECT p.photo_id,p.file_id,p.relpath,p.name,coalesce(p.dt_original,''),coalesce(p.camera,''),coalesce(p.lens,''),CAST(p.iso AS REAL),p.size,{source},{orientation} {from} ORDER BY p.dt_original {},p.photo_id LIMIT ? OFFSET ?",
            if filters.oldest { "ASC" } else { "DESC" }
        );
        let photos = collection
            .db
            .prepare(&query)?
            .query_map(rusqlite::params_from_iter(&page_args), |r| {
                Ok(Photo {
                    id: r.get(0)?,
                    file_id: r.get(1)?,
                    path: r.get(2)?,
                    name: r.get(3)?,
                    date: r.get(4)?,
                    camera: r.get(5)?,
                    lens: r.get(6)?,
                    iso: r.get(7)?,
                    size: r.get::<_, i64>(8)? as u64,
                    source: r.get(9)?,
                    orientation: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(SearchPage {
            photos,
            total,
            cameras,
        })
    })();
    collection.db.execute_batch("ROLLBACK")?;
    result
}

pub fn detail(collection: &Collection, photo: i64) -> Result<Json> {
    let rows=collection.db.prepare("SELECT f.relpath,m.json FROM files f LEFT JOIN meta m ON m.file_id=f.id WHERE f.photo_id=? AND f.status='present' ORDER BY f.ext='xmp' DESC,f.id")?
        .query_map([photo],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<String>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(serde_json::json!(rows.into_iter().map(|(path,raw)|serde_json::json!({"Datei":path,"Metadaten":raw.and_then(|v|serde_json::from_str::<Json>(&v).ok())})).collect::<Vec<_>>()))
}
