//! Einblicke zur gewählten Person in den Personendetails: mit wem sie am häufigsten auf
//! Fotos ist. Gezählt wird wie in der Sammlungssuche (eindeutige Fotos, bestätigte
//! Zuordnungen, keine historischen Gesichter, siehe `relationship_graph::for_person`), damit
//! ein Klick auf die Zahl genau diese Fotos findet. Nur lesend; läuft auf einem Worker.
use super::{
    collection::Collection,
    relationship_graph::{self, Connection},
};
use crate::config::PERSON_COMPANIONS;
use anyhow::Result;

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
    Ok(Insights {
        person,
        companions: companions(graph.connections),
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
}
