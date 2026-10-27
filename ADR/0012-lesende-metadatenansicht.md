# ADR-0012: Metadaten auf Foto-Ebene lesend und seitenweise darstellen

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0003](0003-ui-thread-und-worker.md), [ADR-0009](0009-stabile-foto-identitaeten.md)

## Kontext

Kennzahlen, Filter und Detailvorschau sollen dieselbe Sammlung konsistent zeigen,
ohne RAW/JPEG-Varianten mehrfach zu zählen oder jede EXIF-JSON-Struktur in die GUI zu laden.

## Entscheidung

Ein lesender Browser fragt Foto-Zusammenfassungen innerhalb eines SQLite-Snapshots
ab. Filter, sortierte Seiten und Diagramme beziehen sich auf Foto-Ebene; Seiten
umfassen standardmäßig 200 Datensätze. Detail- und Listenabfragen laufen in getrennten
Workern. JPEG-Paare bzw. eingebettete RAW-Vorschauen dienen zur Anzeige. XMP-Bewertung
und -Farbmarkierung haben beim Anzeigen Vorrang, ohne Dateien zu verändern.

## Alternativen

Ein Dateiraster ohne Fotoaggregation würde Paare mehrfach zählen. Sofortiges Laden
sämtlicher Detailmetadaten wäre unnötig teuer. Editierbare Sterne würden neue Schreibsemantik erfordern.

## Folgen und Grenzen

Fehlende Metadaten bleiben sichtbar unbekannt. Die Ansicht ersetzt nicht die
Metadatenanreicherung; eine abgebildete Bewertung ist kein Bearbeitungswerkzeug.
Zusammenfassungen und Seiten benötigen dennoch Datenbankarbeit.

## Bei Weiterentwicklung beachten

Filterkonsistenz, Snapshot und Paarzählung erhalten. Schreibfunktionen oder andere
Aggregationsregeln ausdrücklich entscheiden und neue veraltete Ergebniszustellungen verhindern.

## Belege und Validierung

[Abfragen](../core/metadata_browser.py), [Ansicht](../ui/metadata_view.py),
[Abfragetests](../tests/core/test_metadata_browser.py), [UI-Tests](../tests/ui/test_metadata_view.py),
Historie: `1251e0a`, `06e639e`.
