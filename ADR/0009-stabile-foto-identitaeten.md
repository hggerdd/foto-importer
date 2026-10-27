# ADR-0009: Ein Foto hat eine stabile Identität mit mehreren Dateien

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0008](0008-lokale-sqlite-sammlung.md)

## Kontext

JPEG, NEF, DNG und XMP können dieselbe Aufnahme repräsentieren. Namen und Pfade
ändern sich; Nutzerzuordnungen sollen diese Änderungen überleben.

## Entscheidung

Dateien referenzieren `photos.id` über `files.photo_id`. Kameraidentität bevorzugt
Seriennummer plus Auslösezähler, mit definierten Aufnahmezeit-/Subsekunden-Fallbacks.
Ohne starke Identität helfen kontrollierte Stamm-/Ordner-Paarung und späterer
Metadatenabgleich. Relative normalisierte Pfade beschreiben Ablage, nicht Identität.
Bei Zusammenführung bleibt der ältere Foto-Anker mit Redirect des alten Ankers erhalten.
Verschiebungen werden mit Dateisystem-IDs bzw. vorhandenen Hashhinweisen nachvollzogen.

## Alternativen

Dateinamen als Primärschlüssel würden Kollisionen und Umbenennungen falsch behandeln.
Jede Variante als eigenes Foto zu zählen würde Statistiken und Gesichtsanalyse verdoppeln.

## Folgen und Grenzen

Schwache Paarung kann spätere Korrektur benötigen. Mehrdeutige Verschiebungen werden
nicht beliebig aufgelöst. Ein vor dem Verschieben nie gehashter Datenträgerwechsel
ist nicht immer nachträglich erkennbar. Fehlende Datei und gelöschte Fotoidentität sind verschieden.

## Bei Weiterentwicklung beachten

Neue Module an Foto-IDs anbinden und Redirects berücksichtigen. Keine zweite,
abweichende Paarungslogik einführen; beschnittene Varianten nicht geometrisch gleichsetzen.

## Belege und Validierung

[Identitätsschlüssel](../core/collection_metadata.py), [Index/Paarung](../core/collection_index.py),
[Identitätstests](../tests/core/test_collection_index.py), [Windows-IDs](../core/windows_file_ids.py).
