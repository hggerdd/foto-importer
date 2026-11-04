# ADR-0033: Einzelne Gesichter direkt am Bild korrigieren

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-30
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0012](0012-lesende-metadatenansicht.md),
  [0015](0015-fortsetzbare-gesichtspipeline.md),
  [0016](0016-namensvorschlaege-aus-referenzen.md),
  [0021](0021-uninteressierte-gesichtsgruppen.md),
  [0024](0024-fortsetzbarer-gesichtsabgleich.md),
  [0030](0030-gemeinsame-sammlungs-schreibwarteschlange.md),
  [0032](0032-metadaten-ordnerbaum-und-gesichtsrahmen.md)

## Kontext

Objekte wie T-Shirts werden gelegentlich als Gesicht erkannt. Ignorieren eignet
sich für echte, uninteressante Personen; eine Fehldetektion darf dagegen weder
Personensuche noch Gesichtszählung verfälschen. Sichtbare Gesichter sollen außerdem
direkt am Bild benannt werden können.

## Entscheidung

Die bisher lesende Metadatenansicht aus ADR-0012/0032 erhält ausschließlich zwei
explizite Schreibaktionen für einzelne aktuelle Gesichtsvorkommen:

- Ein kleiner Mülleimer unmittelbar hinter jedem Namen bzw. Unbekannt löscht
  dieses Gesicht. Foto, Originaldateien und Personenstammdaten bleiben bestehen.
- Ein Rechtsklick auf die Namensfläche öffnet ein Menü zum Hinzufügen oder Ändern
  des Namens. Ein kleiner Dialog bietet bestehende Personen und freie Eingabe.
  Doppelte Namen werden über die Personen-ID unterscheidbar; die bestehende
  Benennungslogik behandelt die Zuordnung als Nutzerbestätigung.

Beide Aktionen stehen auch in der großen Bildansicht zur Verfügung und werden
nicht als Klick zum Vergrößern weitergereicht. Bei ausgeblendeten Rahmen werden
auch die Aktionsflächen ausgeblendet.

Schreibarbeit läuft auf einem eigenen Worker über den gemeinsamen
Sammlungs-Schreibkontext aus ADR-0030. Das Ziel ist die beim Klick ausgewählte
Sammlung, Foto-ID und Gesichts-ID; vor der Mutation wird das aktuelle Vorkommen
erneut geprüft. Während der Speicherung wird derselbe Gesichtsauftrag nicht
doppelt angenommen. Fehler bleiben sichtbar und lassen die bisherige Darstellung
unverändert. Schließen bricht noch wartende Arbeit kooperativ ab.

Löschen entfernt atomar Vektoren aller Modellräume, Trainings-/Validierungszeilen
einschließlich Pins, Ausschlüsse, Reviews und Ablehnungen. Bestehende Trigger
entfernen Gruppenmitgliedschaften und korrigieren deren Zähler. Betroffene
Referenzpersonen und Modellräume erhalten dauerhafte Nacharbeit; die Verarbeitung
bleibt portionsweise. Bestehende Detektionsjobs werden erhalten, sodass ein
unveränderter normaler Fortsetzungslauf die gelöschte Stelle nicht neu anlegt.

Nach dem Speichern werden ausschließlich Gesichtsmetadaten und Overlays der
aktuell passenden Aufnahme aktualisiert. Bilddaten, Baum, Auswahl und offene
Großansicht bleiben bestehen. Ein inzwischen ausgewähltes anderes Foto wird
nicht durch das alte Ergebnis ersetzt. Die Gesichtsverwaltung invalidiert
Snapshots und Sitzungs-Undo und verarbeitet ausstehende Referenzarbeit weiter.

## Alternativen

Ignorieren behält ein reales Gesichtsvorkommen für spätere Suchbedingungen;
das ist für ein T-Shirt sachlich falsch. Eine virtuelle Person für Fehldetektionen
würde das Erkennungswissen verfälschen. Eine komplette Bild-/Baumaktualisierung
nach jeder Korrektur erzeugt unnötige Ladezeit und verliert den Arbeitskontext.

## Folgen und Grenzen

Keine Schemaänderung, keine neuen Pakete und kein Netztraining. Das Löschen
erzeugt keine neuen Negativbeispiele für den Detektor. Eine erzwungene erneute
Detektion, ein anderer Detektor oder eine geänderte Bildquelle kann die Stelle
wieder erkennen; dauerhafte quellbezogene Unterdrückungsregionen sind nicht Teil
dieser Entscheidung. Frühere explizite Dataset-Exporte bleiben historische
Dateien und werden nicht nachträglich verändert.

Gesichtszuordnungen und Datenbankwerte werden verändert, Originale weiterhin
nur gelesen. Der einzelne Löschklick benötigt keine weitere Bestätigung.
Gelöschte Gesichter werden nicht über das Gruppen-Undo wiederhergestellt.
Bei externer Sperre bleibt die Mutation ohne Erfolg; die App übernimmt keine
fremde Sperre. Andere aktive Schreibarbeit wird zwischen Aufträgen abgewartet.

## Bei Weiterentwicklung beachten

Löschen nicht mit Ignorieren vermischen. Trainingsdaten und Mitgliedschaften
transaktional bereinigen, sämtliche Modellräume beachten und keine verwaisten
Referenzen behalten. Root-/Bildwechsel, alte Großansichtsantworten und
Fehler während der Mutation prüfen. Keine Bilddecodierung nach Einzelkorrekturen.

## Belege und Validierung

- [Löschen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_service.py), [Schreibauftrag](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/metadata_face_actions.py)
- [Namensregeln](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_naming.py), [Metadatenlesen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/metadata_browser.py)
- [Aktionsdialog](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/metadata_face_actions.py), [Trefferflächen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/metadata_preview.py)
- [Ansicht](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/metadata_view.py), [Invalidierung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_controller.py)
- [Transaktions-/Modellraumtest](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_faces.py),
  [Maus-, Sperr- und Bildwechseltests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_metadata_face_actions.py)
- [Bedienung und Bildschirmaufnahmen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/metadata-visualization.md)

