# ADR-0021: Gruppen ignorieren und Gesichtsvorkommen erhalten

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-28
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen
- **Bezieht sich auf:** [ADR-0008](0008-lokale-sqlite-sammlung.md),
  [ADR-0015](0015-fortsetzbare-gesichtspipeline.md),
  [ADR-0016](0016-namensvorschlaege-aus-referenzen.md),
  [ADR-0017](0017-kuratierter-referenzdatensatz.md),
  [ADR-0018](0018-gruppenweise-benennung.md)

## Kontext

Uninteressante Gesichter, etwa Fremde im Hintergrund, sollen mit einer schnellen
Gruppenaktion aus der Benennung verschwinden. Ihre Vorkommen müssen erhalten
bleiben: Eine spätere Suche nach bestimmten Personen ohne weitere Menschen
darf ignorierte Gesichter nicht als Abwesenheit interpretieren.

## Entscheidung

Neben dem Namensfeld jeder Gruppe in Benennen und Prüfen steht
**Nicht interessiert**. Ohne Namenseingabe markiert die Aktion alle aktuellen
Gruppenmitglieder mit dem vorhandenen gesichtsbezogenen Flag ignored=1,
einschließlich abgewählter und nicht sichtbarer Mitglieder. Es gibt keine
virtuelle Person und keine Schemaänderung. Der bestehende sitzungsbezogene
Undo-Verlauf nimmt die Aktion einschließlich Referenzpins zurück.

Fotoverknüpfung, Detektion, Position und Embeddings bleiben erhalten.
Bestätigte Identitäten bleiben bestehen; maschinelle Zuordnungen werden entfernt.
Referenzen werden neu aufgebaut und übrige Vorschläge neu bewertet.
Ignorierte Gesichter liefern weder Identitätsreferenzen noch pauschale negative
Identitätslabels. Namenszuordnung und Interesse sind getrennte Entscheidungen.

## Alternativen

Eine Sammelperson würde verschiedene Identitäten vermischen. Löschen würde
Anwesenheitsinformation verlieren. Einzelaktionen pro Vorschau sind zu langsam.
Das Ignorieren nur ausgewählter Bilder widerspricht der vereinbarten Gruppenaktion.

## Folgen und Grenzen

Die bestehenden Filter entfernen ignorierte Gesichter aus Benennen und Prüfen.
Unter **Alle Gesichter** bleiben sie zugänglich; ausdrückliche Namenszuordnung
aktiviert sie wieder. Undo gilt für bis zu 20 Aktionen der laufenden Sitzung.
Schreibschutz und Worker-/Tk-Grenze bleiben erhalten. Originalfotos ändern sich nicht.

Die Suche nach Personen ohne weitere Menschen wird hier nicht implementiert.
Künftige Anwesenheitsabfragen müssen ignorierte Vorkommen mitzählen und
beibehaltene historische Detektionen von aktuellen Detektionen unterscheiden.
Gesichtserkennung allein beweist nicht die Abwesenheit weiterer Menschen.

## Bei Weiterentwicklung beachten

Interesse nicht als Identitätsbeweis verwenden. Modellwechsel müssen das Flag
weiterhin erhalten. Undo darf spätere ausdrückliche Entscheidungen nicht
überschreiben. Gruppenaktionen dürfen weder zusätzliche Personen erzeugen noch
Vorkommen löschen.

## Belege und Validierung

[Gruppenaktion/Undo](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_naming.py),
[Controller](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_controller.py), [Zeile](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_group_row.py),
[Workspace](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_workspace.py),
[Bedienung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/gesichtserkennung-umsetzung.md).

Regressionen: [Core](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_face_naming.py) prüft Erhaltung,
Referenzausschluss, Rücknahme und Fehlerfälle;
[Tk](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_face_view.py) prüft den Button ohne Namen,
eine 70er-Gruppe einschließlich unsichtbarer/abgewählter Mitglieder, Fokus,
Doppelklickschutz, Zugriff über Alle Gesichter und Undo in Benennen und Prüfen.

