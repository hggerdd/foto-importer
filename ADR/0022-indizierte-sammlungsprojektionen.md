# ADR-0022: Indizierte und begrenzte Sammlungsabfragen

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-28
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0008](0008-lokale-sqlite-sammlung.md), [0012](0012-lesende-metadatenansicht.md)

## Kontext

100.000 Fotos mit mehreren Gesichtern verlangen begrenzte UI-Abfragen.
Pagination nach vollständigem Laden und wiederholte temporäre Gesamttabellen
verbrauchen unnötig Speicher. Face-first Primärschlüssel unterstützen
modellbezogene Abfragen nicht ausreichend.

## Entscheidung

Schema v4 ergänzt passende Modell-, Referenz-, Review- und Queue-Indizes.
Rückwärtsindizes nach Gesichts-ID verhindern Vollscans bei modellübergreifender
Invalidierung von Mitgliedschaften und wartender Arbeit.
SQL begrenzt Face-Seiten; Vektoren werden zusammen mit gefilterten Gesichtern
abgefragt. Status liefert aggregierte Auftragszahlen.

Eine indizierte metadata_photos-Projektion enthält genau die bisher bevorzugte
Variante pro Foto. SQLite-Trigger aktualisieren ausschließlich betroffene Fotos
in derselben Transaktion. Beim Upgrade erfolgt ein einmaliger Backfill.
Ein Revisionszähler invalidiert einen auf acht Filterzustände begrenzten Cache
kleiner Metadatenaggregate. Originaltabellen bleiben Quelle der Wahrheit.
Leser alter Schemas behalten den bisherigen SQL-Fallback auf dateibasiertem
Temporärspeicher. SQLite-Seitencaches sind pro Verbindung auf 16 MiB konfiguriert.

## Alternativen

Zusätzliche Indizes ohne begrenzte Resultatmengen lösen den Python-Speicherbedarf
nicht. Ein Serverwechsel ist dafür nicht erforderlich. Trigger vermeiden veraltete
Projektionen auch bei bestehenden direkten SQL-Schreibpfaden.

## Folgen und Grenzen

Mehr Index-/Projektionsspeicher und kleine zusätzliche Schreibarbeit.
Freie Teilstringsuche bleibt ein Scan in SQL, lädt jedoch keine Sammlung nach Python.
OFFSET bleibt für bestehende Seitennavigation; tiefe Seiten benötigen Messung.
Schema-Upgrades erfolgen ausschließlich über den bestehenden Schreiber.

## Bei Weiterentwicklung beachten

Neue bevorzugte Varianten oder Metadatenfelder müssen Projektion und Trigger
gemeinsam ändern. Migration erhält IDs, Labels, Pins, Ablehnungen und Vektoren.

## Belege und Validierung

[Schema](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/scaling_schema.py), [Abfragen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_service.py),
[Metadaten](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/metadata_browser.py),
[Migration](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_faces.py),
[Filter und Parallel-Leser](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_metadata_browser.py).

