//! Sofort sichtbare Gesichtsentscheidungen mit Schreibauftrag im Hintergrund (ADR 0068).
//!
//! Wer Gesichter benennt, bestätigt oder ignoriert, sieht die Gruppe sofort verschwinden; das
//! Schreiben in die Sammlung läuft als eigener Auftrag und beliebig viele davon dürfen
//! gleichzeitig vorgemerkt sein (der Schreiber der Sammlung arbeitet sie in Reihenfolge ab,
//! ADR 0008). Die Gesichter gelten, solange ihr Auftrag läuft, als „ausgeblendet“: jede neu
//! gelesene Seite wird dagegen gefiltert. Schlägt ein Auftrag fehl, endet das Ausblenden und
//! die Gesichter erscheinen beim nächsten Lesen wieder (die Datenbank bleibt maßgeblich).
use super::*;

impl PhotoApp {
    /// Gesichter, deren Schreibauftrag noch läuft oder wartet (diese Sammlung).
    pub(super) fn hidden_faces(&self) -> BTreeSet<i64> {
        self.tasks
            .iter()
            .filter(|t| t.collection == self.settings.collection)
            .flat_map(|t| t.hides.iter().copied())
            .collect()
    }

    /// Blockiert ein anderer Schreibauftrag das Vormerken? Vorgemerkte Benennungen selbst
    /// blockieren einander nicht.
    pub(super) fn naming_blocked(&self) -> bool {
        self.tasks.iter().any(|t| {
            t.write
                && !matches!(t.kind, "copy" | "maintenance" | "naming")
                && t.collection == self.settings.collection
        })
    }

    /// Wartet noch ein Benennungsauftrag dieser Sammlung?
    pub(super) fn naming_pending(&self) -> bool {
        self.tasks
            .iter()
            .any(|t| t.kind == "naming" && t.collection == self.settings.collection)
    }

    /// Merkt einen Schreibauftrag vor und blendet `hide` sofort aus der angezeigten Seite aus.
    pub(super) fn queue_hiding(
        &mut self,
        label: &str,
        hide: Vec<i64>,
        work: impl FnOnce(Control) -> Result<Outcome> + Send + 'static,
    ) {
        self.launch_hiding(label, true, "naming", hide, work);
        self.hide_pending_faces();
    }

    /// Entfernt ausgeblendete Gesichter aus der Seite und passt die Zähler an; leere Gruppen
    /// verschwinden. Wiederholbar (idempotent), billig (nur die Gesichter einer Seite).
    pub(super) fn hide_pending_faces(&mut self) {
        let hidden = self.hidden_faces();
        if hidden.is_empty() {
            return;
        }
        let page = &mut self.face_page;
        let before: usize = page.groups.iter().map(Vec::len).sum();
        for group in &mut page.groups {
            group.retain(|f| !hidden.contains(&f.id));
        }
        let removed_groups = page.groups.iter().filter(|g| g.is_empty()).count();
        page.groups.retain(|g| !g.is_empty());
        let removed = before - page.groups.iter().map(Vec::len).sum::<usize>();
        page.total_groups = page.total_groups.saturating_sub(removed_groups);
        match self.face_mode.as_str() {
            "unknown" => {
                page.status.nameable = page.status.nameable.saturating_sub(removed);
                page.status.unknown = page.status.unknown.saturating_sub(removed);
            }
            "review" => page.status.review = page.status.review.saturating_sub(removed),
            _ => {}
        }
    }
}

/// Rückmeldung nach einer gespeicherten Gesichtsentscheidung. Sie nennt, wohin abgelehnte
/// Gesichter gegangen sind, damit sie nicht als verschwunden gelten.
pub(super) fn decision_message(summary: crate::core::faces::DecisionSummary) -> String {
    let mut parts = Vec::new();
    if summary.confirmed > 0 {
        parts.push(format!("{} bestätigt", summary.confirmed));
    }
    if summary.rejected > 0 {
        parts.push(format!(
            "{} abgelehnt, jetzt unter Benennen",
            summary.rejected
        ));
    }
    if summary.ignored > 0 {
        parts.push(format!("{} ignoriert", summary.ignored));
    }
    if parts.is_empty() {
        "Gesichtsentscheidung gespeichert".into()
    } else {
        parts.join(" · ")
    }
}
#[cfg(test)]
mod message_tests {
    use super::decision_message;
    use crate::core::faces::DecisionSummary;
    #[test]
    fn message_names_where_rejected_faces_went() {
        let text = decision_message(DecisionSummary {
            confirmed: 5,
            rejected: 2,
            ignored: 0,
        });
        assert_eq!(text, "5 bestätigt · 2 abgelehnt, jetzt unter Benennen");
        assert_eq!(
            decision_message(DecisionSummary::default()),
            "Gesichtsentscheidung gespeichert"
        );
    }
}
