# ADR-0005: Import kopiert Originale und schützt bestehende Zieldateien

- **Status:** Im Rust-Port teilweise abgelöst durch [ADR-0036](0036-vollstaendige-kopien.md); historische Python-Entscheidung erhalten
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

[CopyWorker](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/copy_worker.py), [Namensprüfung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/import_plan.py),
[Vorprüfung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/import_preflight.py), [Kopiertests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_copy_worker.py),
[Planungstests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_import_plan.py).


> Rust-Port: direkte Veröffentlichung ersetzt durch [ADR-0036](0036-vollstaendige-kopien.md); sonstiger Originalschutz bleibt verbindlich.
