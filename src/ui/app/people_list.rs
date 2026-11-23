//! Eine Personenliste für alle Auswahlfelder (ADR Personen, „Personenliste“).
//!
//! Alle Personenauswahlen lesen `face_page.people`. Die Liste hängt am Datenstand der Sammlung
//! (`faces::PEOPLE_DOMAINS`, Datenrevision mit Triggern auf `persons` und `person_attrs`) und
//! wird an genau einer Stelle aktuell gehalten: Nach jedem Schreibauftrag der offenen Sammlung
//! prüft ein kleiner Leseauftrag den Stand und liest die Liste nur bei Abweichung neu. So wirken
//! Anlegen (Namensentwurf, „Einzeln benennen“, Kachel, „Person anlegen“), Umbenennen,
//! Zusammenführen, Rücknahme und Abgleich mit dem zentralen Bestand ohne eigene Aufrufe, und die
//! neue Person ist wählbar, ohne auf das langsamere Lesen der Gesichterseite zu warten (das
//! weitere Entscheidungen abbrechen oder aufschieben).
use super::*;
use crate::core::revision::Key;

#[derive(Default)]
pub(super) struct PeopleList {
    /// Datenstand der Liste in `face_page.people`; `None`, solange unbekannt oder ohne
    /// Revision.
    key: Option<Key>,
    /// Eine Liste wurde übernommen (aus dem Leseauftrag oder einer Gesichterseite).
    loaded: bool,
    /// Ein Schreibauftrag dieser Sammlung endete: Stand prüfen, sobald kein Lesen läuft.
    stale: bool,
    revision: u64,
}

impl PeopleList {
    /// Prüfung vormerken (nach Schreibaufträgen, beim Öffnen einer Ansicht ohne Liste).
    pub(super) fn invalidate(&mut self) {
        self.stale = true;
    }
    /// Neue Sammlung: nichts Bekanntes gilt weiter; laufende Antworten verfallen.
    pub(super) fn reset(&mut self) {
        self.key = None;
        self.loaded = false;
        self.stale = false;
        self.revision += 1;
    }
    /// Ist ein Ergebnis mit dieser Nummer das zuletzt angeforderte?
    pub(super) fn current(&self, revision: u64) -> bool {
        revision == self.revision
    }
}

impl PhotoApp {
    /// Startet die Prüfung des Personenstands, wenn sie fällig ist und keine läuft. Eine
    /// während des Lesens fällig gewordene Prüfung folgt danach, damit kein Schreibauftrag
    /// verloren geht.
    pub(super) fn sync_people_list(&mut self) {
        if !self.people_list.stale
            || self.closing
            || self.settings.collection.is_empty()
            || self.tasks.iter().any(|t| t.kind == JobKind::People)
        {
            return;
        }
        self.people_list.stale = false;
        self.people_list.revision += 1;
        let revision = self.people_list.revision;
        let known = self.people_list.key.clone();
        let root = PathBuf::from(&self.settings.collection);
        let (cache, collection) = (self.cache_dir.clone(), self.settings.collection.clone());
        self.launch(
            "Personenliste prüfen",
            false,
            JobKind::People,
            move |control| {
                let c = Collection::open(&root, false)?;
                c.cancel_with(&control);
                let (revisions, people) = faces::people_if_changed(&c, known.as_ref())?;
                // Für den nächsten Start (Zeitreise vor dem ersten Lesen der Gesichterseite).
                if let Some(people) = &people {
                    crate::core::timeline::store_people(&cache, &collection, people);
                }
                Ok(Outcome::People(revision, revisions, people))
            },
        );
    }

    /// Übernimmt eine frisch gelesene Personenliste, sofern sie nicht älter ist als die
    /// bekannte. `None`: Stand unverändert.
    pub(super) fn accept_people_list(
        &mut self,
        revisions: Option<crate::core::revision::Revisions>,
        people: Option<Vec<faces::Person>>,
    ) {
        let key = revisions.as_ref().map(|r| r.key(faces::PEOPLE_DOMAINS));
        let Some(people) = people else {
            return;
        };
        if let (Some(new), Some(known)) = (&key, &self.people_list.key)
            && new.older_than(known)
        {
            return;
        }
        log::info!(target: "personen", "personenliste_neu personen={}", people.len());
        self.face_page.people = people;
        self.people_list.key = key;
        self.people_list.loaded = true;
        // Abhängige Ansichten prüfen sich am neuen Stand (sie rechnen nur bei Abweichung).
        self.relationship_graph.stale = true;
        if let Some(revisions) = &revisions {
            self.time_ui.observe(revisions);
        }
    }

    /// Eine neu gelesene Gesichterseite bringt ihre Personenliste mit. Ist die bereits gezeigte
    /// neuer (der kleine Leseauftrag war schneller als die Seite), bleibt sie.
    pub(super) fn merge_page_people(&mut self, page: &mut FacePage) {
        let key = page
            .revisions
            .as_ref()
            .map(|r| r.key(faces::PEOPLE_DOMAINS));
        if let (Some(new), Some(known)) = (&key, &self.people_list.key)
            && new.older_than(known)
        {
            page.people = std::mem::take(&mut self.face_page.people);
            return;
        }
        self.people_list.key = key;
        self.people_list.loaded = true;
    }

    /// Eine Ansicht braucht die Liste, bevor die Gesichterseite gelesen ist (Zeitreise beim
    /// Start): einmal lesen.
    pub(super) fn need_people_list(&mut self) {
        if !self.people_list.loaded && self.face_page.people.is_empty() {
            self.people_list.loaded = true;
            self.people_list.invalidate();
        }
    }
}
