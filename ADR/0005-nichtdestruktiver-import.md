# ADR-0005: Import kopiert Originale und schützt bestehende Zieldateien

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert

## Kontext

Speicherkarten enthalten nicht ersetzbare Originale. Wiederholte Imports,
Namenskollisionen, ungültige Ordnernamen und Abbrüche gehören zum Normalbetrieb.

## Entscheidung

Ausgewählte Dateien werden nach `<Ziel>/<Ordnername>/<Endung>/` kopiert,
nicht verschoben. Ordnernamen werden vorab validiert. Zieldateien werden exklusiv
angelegt; bei Namenskollisionen wird ein freier Nummernsuffix gesucht. Blöcke
ermöglichen Abbruch und Prioritätswechsel, Dateistatistik wird anschließend übernommen.
Bei behandeltem Fehler/Abbruch entfernt der Job nur seine eigene Teilkopie.

## Alternativen

Verschieben oder Überschreiben wäre für Kameraoriginale riskanter. Ein gesamter
Import als unteilbare Dateisystemtransaktion ist nicht die vorhandene Implementierung.

## Folgen und Grenzen

Fertige Kopien bleiben nach einem Teilfehler erhalten. Während des Schreibens ist
der endgültige Zielpfad bereits sichtbar. Ein Prozessabsturz/Stromausfall kann
Teilkopien hinterlassen; es gibt keinen garantierten atomaren Rename pro Fotokopie.

## Bei Weiterentwicklung beachten

Keine automatische Quelllöschung oder Ersetzung vorhandener Dateien ergänzen.
Änderungen der Ablagestruktur und einer möglichen Staging-Strategie separat
entscheiden; Kollisionen, Abbruch und Wiederholung regressionsprüfen.

## Belege und Validierung

[CopyWorker](../core/copy_worker.py), [Namensprüfung](../core/import_plan.py),
[Vorprüfung](../ui/import_preflight.py), [Kopiertests](../tests/core/test_copy_worker.py),
[Planungstests](../tests/core/test_import_plan.py).
