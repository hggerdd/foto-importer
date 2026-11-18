//! Einblicke zur gewählten Person in den Personendetails: mit wem sie am häufigsten auf
//! Fotos ist und wie sich ihre Fotos über die Jahre verteilen. Gezählt wird wie in der
//! Sammlungssuche (eindeutige Fotos, bestätigte Zuordnungen, keine historischen Gesichter,
//! siehe `relationship_graph::for_person`), damit ein Klick auf eine Zahl genau diese Fotos
//! findet. Nur lesend; läuft auf einem Worker.
use super::{
    collection::Collection,
    metadata::PHOTO_FORMATS,
    relationship_graph::{self, Connection},
};
use crate::config::{PERSON_COMPANIONS, PERSON_YEARS};
use anyhow::Result;
use std::collections::BTreeMap;

// Die Nachbarschaft liefert höchstens `NEIGHBOR_LIMIT` Personen, Fotokanten zuerst.
const _: () = assert!(PERSON_COMPANIONS <= relationship_graph::NEIGHBOR_LIMIT);

/// Eine Person, mit der die gewählte Person Fotos teilt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Companion {
    pub person: i64,
    pub name: String,
    /// Gemeinsame Fotos (wie die Sammlungssuche „alle diese Personen“).
    pub photos: usize,
}

/// Alles, was die Detailspalte zur Person zusätzlich liest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Insights {
    pub person: i64,
    /// Meiste gemeinsame Fotos zuerst (Gleichstand: Name, dann ID), höchstens
    /// [`PERSON_COMPANIONS`].
    pub companions: Vec<Companion>,
    /// Fotos je Aufnahmejahr (nur Jahre mit Fotos, aufsteigend). Das Jahr stammt aus
    /// `dt_original` wie der Datumsfilter der Suche.
    pub years: BTreeMap<i32, usize>,
    /// Fotos ohne verwertbares Aufnahmejahr (fehlend oder außerhalb von `PERSON_YEARS`).
    pub undated: usize,
}

/// Ein Balken des Jahresdiagramms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct YearBar {
    pub year: i32,
    pub photos: usize,
    /// Alter in diesem Jahr (Jahr minus Geburtsjahr), falls das Geburtsjahr bekannt ist.
    pub age: Option<i32>,
}

/// Lückenlose Balken vom ersten bis zum letzten Jahr mit Fotos; Jahre ohne Fotos zählen 0,
/// damit Lücken sichtbar bleiben.
pub fn year_bars(years: &BTreeMap<i32, usize>, birth: Option<i32>) -> Vec<YearBar> {
    let (Some(&first), Some(&last)) = (years.keys().next(), years.keys().next_back()) else {
        return Vec::new();
    };
    (first..=last)
        .map(|year| YearBar {
            year,
            photos: years.get(&year).copied().unwrap_or(0),
            age: birth.and_then(|b| super::people_table::age(b, year)),
        })
        .collect()
}

/// Fotos je Aufnahmejahr; Jahre außerhalb von [`PERSON_YEARS`] gelten als ohne Datum.
pub fn load_years(collection: &Collection, person: i64) -> Result<(BTreeMap<i32, usize>, usize)> {
    let query = format!(
        "SELECT CASE WHEN substr(p.dt_original,1,4) GLOB '[0-9][0-9][0-9][0-9]'
                     THEN CAST(substr(p.dt_original,1,4) AS INTEGER) END AS year, count(*)
         FROM (SELECT DISTINCT f.photo_id FROM faces f INDEXED BY ix_faces_person
               WHERE f.person_id=?1 AND f.retained=0 AND f.assignment='user') x
         JOIN metadata_photos p ON p.photo_id=x.photo_id
         WHERE {PHOTO_FORMATS} GROUP BY year"
    );
    let mut years = BTreeMap::new();
    let mut undated = 0;
    let mut statement = collection.db.prepare(&query)?;
    let mut rows = statement.query([person])?;
    while let Some(row) = rows.next()? {
        let year: Option<i32> = row.get(0)?;
        let count = row.get::<_, i64>(1)? as usize;
        match year.filter(|y| PERSON_YEARS.contains(y)) {
            Some(year) => *years.entry(year).or_default() += count,
            None => undated += count,
        }
    }
    Ok((years, undated))
}

/// Personen mit gemeinsamen Fotos aus der Nachbarschaft; rein eingetragene Beziehungen
/// ohne gemeinsames Foto fehlen. Die Reihenfolge der Nachbarschaft bleibt erhalten.
pub fn companions(connections: Vec<Connection>) -> Vec<Companion> {
    connections
        .into_iter()
        .filter(|c| c.photos > 0)
        .take(PERSON_COMPANIONS)
        .map(|c| Companion {
            person: c.person,
            name: c.name,
            photos: c.photos,
        })
        .collect()
}

pub fn load(collection: &Collection, person: i64) -> Result<Insights> {
    let graph = relationship_graph::for_person(collection, person)?;
    let (years, undated) = load_years(collection, person)?;
    Ok(Insights {
        person,
        companions: companions(graph.connections),
        years,
        undated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connection(person: i64, photos: usize) -> Connection {
        Connection {
            person,
            name: format!("P{person}"),
            photos,
            recorded: None,
        }
    }

    #[test]
    fn companions_keep_the_order_drop_photo_less_links_and_stop_at_the_limit() {
        let mut list: Vec<Connection> = (0..PERSON_COMPANIONS as i64 + 2)
            .map(|i| connection(i + 2, 20 - i as usize))
            .collect();
        list.push(connection(99, 0));
        let picked = companions(list);
        assert_eq!(picked.len(), PERSON_COMPANIONS);
        assert_eq!(picked[0].person, 2);
        assert!(picked.windows(2).all(|w| w[0].photos >= w[1].photos));
        assert!(picked.iter().all(|c| c.photos > 0));
        assert!(companions(vec![connection(5, 0)]).is_empty());
    }

    #[test]
    fn year_bars_fill_gaps_and_add_the_age_only_with_a_birth_year() {
        let years = BTreeMap::from([(2010, 4), (2013, 1)]);
        let bars = year_bars(&years, Some(2008));
        let flat: Vec<(i32, usize, Option<i32>)> =
            bars.iter().map(|b| (b.year, b.photos, b.age)).collect();
        assert_eq!(
            flat,
            [
                (2010, 4, Some(2)),
                (2011, 0, Some(3)),
                (2012, 0, Some(4)),
                (2013, 1, Some(5))
            ]
        );
        assert!(year_bars(&years, None).iter().all(|b| b.age.is_none()));
        // Ein Geburtsjahr nach dem Foto ergibt kein Alter.
        assert_eq!(year_bars(&years, Some(2012))[0].age, None);
        assert!(year_bars(&BTreeMap::new(), Some(2000)).is_empty());
    }
}
