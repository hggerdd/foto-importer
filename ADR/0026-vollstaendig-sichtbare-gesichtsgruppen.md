# ADR-0026: Vollständig sichtbare Gesichtsgruppen mit höchstens 14 Bildern

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-28
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen
- **Bezieht sich auf:** [0023](0023-gesichtsgruppen-mit-begrenztem-aufwand.md), [0025](0025-sichtbare-gesichtsvorschauen.md)
- **Löst ab:** [ADR-0023](0023-gesichtsgruppen-mit-begrenztem-aufwand.md)

## Kontext

Bei mehr als sieben Gesichtern war die Auswahl nicht vollständig sichtbar.
Größere Gruppen enthielten zusätzlich Mitglieder hinter Vorschauseiten.
Damit konnte eine Gruppenzuordnung ein ungeprüftes anderes Gesicht einschließen.
Gewünscht sind höchstens zwei Reihen, maximal 14 Mitglieder und größere Gruppen zuerst.

## Entscheidung

Benennen und Prüfen zeigen ausschließlich Gruppen mit höchstens 14 Gesichtern.
Alle Mitglieder erhalten eine Kachel: maximal sieben Spalten und zwei Reihen,
ohne gruppeninterne Seiten oder Scrollfläche. Bei schmaleren Fenstern schrumpfen
die Kacheln. Fokus auf ein Namensfeld holt die ganze Gruppe ins Blickfeld,
soweit die Fensterhöhe ausreicht. Das Blättern zwischen Gruppenseiten bleibt.

Benennung, Bestätigung und Nicht interessiert beziehen sich genau auf die
dargestellten Mitglieder. Die Zeile lehnt übergroße Eingaben ausdrücklich ab,
statt Vorschauen still abzuschneiden. Abgewählte Ausreißer bleiben abwählbar;
Inline-Namen, Tastaturbedienung, Undo und die Sichtbereichsladung aus ADR-0025 bleiben.

Unbekannte Gruppen werden anhand ihrer gespeicherten Mitgliederzahl absteigend
sortiert; Gruppen-ID ist der stabile Gleichstandsentscheid. Schema v5 ersetzt
den Seitenindex entsprechend. Prüfvorschläge werden pro Person lückenlos in
14er-Gruppen aufgeteilt, unabhängig von Lücken zwischen Gesichts-IDs. Auch dort
steht die Gruppengröße vor der Personen-/Teilgruppenreihenfolge. Die Sortierung
gilt für die gesamte verfügbare Menge vor der Seitenauswahl, nicht nur pro Seite.

Die technische Basis aus ADR-0023 bleibt bestehen: modellbezogene gespeicherte
Gruppen, Dirty-Queue, 128er-Arbeitsschritte, begrenzte Signaturkandidaten und exakte
Complete-Link-/Foto-Eindeutigkeitsprüfung. Geänderte Gruppengrenzen invalidieren
über den vorhandenen Konfigurationsschlüssel nur abgeleitete Gruppen. Alte
256er-Gruppen werden neu aufgeteilt; Gesichter, Embeddings, Nutzerlabels und
Ignorierentscheidungen bleiben erhalten.

## Alternativen

14 Vorschauen aus einer größeren Gruppe würden weiterhin ungesehene Mitglieder
mit auswählen. Internes Blättern oder Scrollen widerspricht dem gewünschten
Prüfablauf. Ausschließlich die aktuelle Seite zu sortieren würde Einzelbilder
vor größeren Gruppen auf späteren Seiten belassen.

## Folgen und Grenzen

Große Mengen derselben Person verteilen sich auf mehr Gruppen und Seiten.
Hintergrundarbeit kann neue Gruppen ergänzen oder ihre Reihenfolge verändern.
Eine laufende UI-Aktion nutzt weiterhin ausschließlich ihre dargestellte Auswahl.
Bei sehr geringer Fensterhöhe bleibt vertikales Scrollen der Arbeitsfläche nötig;
es gibt jedoch keine versteckten zusätzlichen Mitglieder innerhalb einer Gruppe.

## Bei Weiterentwicklung beachten

Gruppenobergrenze und Vorschaukacheln gemeinsam ändern. Keine übergroße Auswahl
durch Trunkierung verdecken. Größenreihenfolge, beide Zeilen, Größenänderung und
Upgrades mit bestehenden Nutzerentscheidungen regressionsprüfen.

## Belege und Validierung

[Grenzen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/config/settings.py), [Gruppenseiten](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_groups.py),
[Prüfgruppen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_queries.py), [Schema v5](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/scaling_schema.py),
[Zeile](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_group_row.py), [Fokus](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_workspace.py),
[Gruppen-/Upgrade-Tests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_collection_scaling.py),
[echte Tk-Tests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_face_view.py),
[synthetische Vorschau](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/screenshots/face-group-naming.png).

