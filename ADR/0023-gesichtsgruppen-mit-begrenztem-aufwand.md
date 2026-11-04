# ADR-0023: Gesichtsgruppen mit begrenztem Aufwand

- **Status:** Abgelöst durch [ADR-0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md)
- **Erfasst am / Entscheidungsdatum:** 2026-10-28
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0018](0018-gruppenweise-benennung.md), [0022](0022-indizierte-sammlungsprojektionen.md)
- **Löst ab:** Gruppierungsentscheidung aus ADR-0018; dessen Inline-Benennung gilt mit [0025](0025-sichtbare-gesichtsvorschauen.md) weiter.

## Kontext

Globale Complete-Link-Gruppierung bei jedem Öffnen und Blättern ist für
mehrere hunderttausend Gesichter zu teuer. Die Gruppierung ist eine Arbeitshilfe,
kein Identitätsbeweis.

## Entscheidung

SQLite speichert modellbezogene Gruppen und eine dauerhafte Dirty-Queue.
Jeder Arbeitsschritt verarbeitet höchstens 128 Gesichter. Ein deterministischer
12-Bit-Projektionsschlüssel liefert höchstens 32 Kandidatengruppen aus derselben
oder benachbarten Signatur. Kandidaten werden weiterhin exakt gegen alle aktiven
Mitglieder geprüft: Kosinusschwelle und höchstens ein Gesicht je Foto gelten.

Jede Gruppe enthält höchstens 256 Einträge. Größere Mengen derselben Person
erscheinen in mehreren Gruppen. Prüfvorschläge werden ebenso in begrenzten
ID-Bereichen gezeigt. Seiten laden höchstens acht Gruppen, und volle
Gesichtsmetadaten werden nur für deren Vorschaukandidaten gelesen.
Ein aktiver Gruppenkatalog mit transaktionalen Mitgliederzählern liefert die
Seitennavigation, ohne dafür alle Gesichtsmitgliedschaften neu zu aggregieren.

Schwellen-/Qualitätsänderungen invalidieren die abgeleiteten Gruppen.
Embedding-Schreibvorgänge und Änderungen an Identität, Interesse, Qualität oder
Beibehaltung invalidieren das betroffene Mitglied transaktional. Eine Rücknahme
wird gegen inzwischen hinzugekommene Mitglieder erneut exakt geprüft. Ungeeignete
Queue-Einträge werden in begrenzten Schritten entfernt statt wiederholt überlesen.
Unterbrechungen erhalten bereits fertiggestellte Gruppen. Gruppierung kann nach
dem ersten Teilresultat im Hintergrund weiterlaufen; fehlende Vektoren werden
nicht als bereits gruppierte Gesichter ausgegeben.

## Alternativen

Globales exaktes Vergleichen wächst quadratisch. Ein neuer ANN-Dienst würde
Installation und Modellsemantik erweitern. Begrenzte Kandidaten sind hier
zulässig, weil fehlende Gruppentreffer zusätzliche Gruppen erzeugen und
niemals selbst eine Personenidentität bestätigen.

## Folgen und Grenzen

Ähnliche Gesichter können auf mehrere Gruppen verteilt sein. Dies senkt den
Benennungskomfort, lockert aber keine Identitäts-/Complete-Link-Sicherheitsregel.
Die Gruppierung ersetzt keine Gesichtserkennungs-Evaluation.
Repräsentanten können nach Änderungen veraltet sein; sie schlagen ausschließlich
Kandidaten vor. Exakte Mitgliederprüfung entscheidet weiterhin.

## Bei Weiterentwicklung beachten

Modellräume nie mischen, ursprüngliche Gesichter/Labels nicht löschen und neue
Suchverfahren gegen falsche Zusammenführungen prüfen. Leere Gruppen sind
regenerierbarer Zustand; Löschen aller Biometrie entfernt auch Repräsentanten.

## Belege und Validierung

[Gruppen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_groups.py), [Seiten](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_queries.py),
[Schema/Invalidierung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/scaling_schema.py),
[Gruppen-, Wiederanlauf- und Begrenzungstests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_collection_scaling.py).

