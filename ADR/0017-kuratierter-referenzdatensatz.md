# ADR-0017: Referenzen balancieren und aus SQLite reproduzierbar exportieren

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0008](0008-lokale-sqlite-sammlung.md), [ADR-0016](0016-namensvorschlaege-aus-referenzen.md)

## Kontext

Ein häufig fotografiertes Familienmitglied und nahezu identische Serien würden
die Referenzen dominieren. Maximale Diversität allein bevorzugt zudem falsch beschriftete Ausreißer.

## Entscheidung

Pro Person und Embedder werden bestätigte hochwertige Gesichter begrenzt ausgewählt:
Ausreißerprüfung, Duplikat-/Serienfilter, Jahresverteilung und qualitätsgewichtetes
Farthest-Point-Sampling. Seed und IDs stabilisieren die Auswahl; Nutzerpins zählen
auf das Limit. Ein Validierungssplit bleibt vom Referenzvergleich getrennt.
Nach Nutzeraktionen wird vollständig deterministisch neu aufgebaut, statt einen
zweiten inkrementellen Auswahlalgorithmus zu pflegen.

## Alternativen

Alle Gesichter oder nur neueste Serien wären unausgewogen. Inkrementeller Austausch
wäre möglich, bedeutet aber zusätzliche Zustände und potenziell abweichende Resultate.

## Folgen und Grenzen

SQLite bleibt Quelle der Wahrheit. `.myfotomanager_faces/` ist ein regenerierbarer,
atomar synchronisierter Export mit Personen-IDs und nicht ausgerichteten Ausschnitten.
Ordneränderungen werden nicht zurückgelesen. Pins haben Vorrang vor dem Duplikatfilter;
kleine Datensätze erlauben keinen belastbaren getrennten Validierungssplit.
Ein Vollneuaufbau kann bei großen Sammlungen länger dauern.

## Bei Weiterentwicklung beachten

Keine Namen als Identität exportieren oder Auto-Labels in Referenzen übernehmen.
Grenzen, Pins, Ausschlüsse, Reproduzierbarkeit und biometrische Löschung inklusive
App-Sicherungen erhalten. Verbesserte inkrementelle Auswahl als neue Entscheidung bewerten.

## Belege und Validierung

[Auswahl](../core/face_algorithms.py), [Export/Kalibrierung](../core/face_dataset.py),
[Aufbau/Löschung](../core/face_service.py), [Datensatztests](../tests/core/test_faces.py),
[Abweichungen von der Spezifikation](../docs/gesichtserkennung-umsetzung.md).
