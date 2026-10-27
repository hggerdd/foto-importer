# ADR-0013: App-, Sammlungs- und Modellkonfiguration getrennt speichern

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0008](0008-lokale-sqlite-sammlung.md), [ADR-0014](0014-lokale-austauschbare-gesichtsmodelle.md)

## Kontext

Fenstergeometrie und letzte Ordner gehören zum Arbeitsplatz. Eine Sammlung hat
eigene Ausschlüsse und aktive Modelle. Eingabenormen und Gewichts-Lizenzen gehören
hingegen zu einem bestimmten Modellartefakt.

## Entscheidung

`config/settings.py` enthält zentrale Laufzeitvorgaben und verwaltet die lokale
Benutzer-JSON-Datei. Sammlungsoptionen liegen in SQLite (`settings`, `face_settings`).
Modellbeschreibungen liegen als JSON-Manifeste unter `models/`. Überschriebene
Matching-Schwellen werden modellbezogen aufbewahrt. Neue Grenzen wie Gruppengröße,
Cache oder Undo-Länge werden nicht verteilt in Widgets festgeschrieben.

## Alternativen

Eine einzige globale Konfiguration würde Modelleinstellungen zwischen Sammlungen
vermischen. Nur Codekonstanten würden Wechsel und Nutzerpräferenzen erschweren.

## Folgen und Grenzen

Beim Umzug einer Sammlung reisen Benutzergeometrie und lokal installierte Gewichte
nicht automatisch mit. Die bestehende Benutzer-JSON wird direkt geschrieben;
atomare oder entprellte Speicherung ist bislang kein garantiertes Verhalten.

## Bei Weiterentwicklung beachten

Für jede neue Einstellung ihren Geltungsbereich festlegen, bisherige Werte und
Fallbacks erhalten. Eine Änderung des Speicherformats benötigt Migration bzw.
Kompatibilitätsbegründung und einen ADR bei geänderter Architektur.

## Belege und Validierung

[Settings](../config/settings.py), [Sammlung](../core/collection_index.py),
[Gesichtseinstellungen](../core/face_store.py), [Modellschwellen](../core/face_service.py),
[Modellmanifeste](../models/README.md), [bisheriger Backlog](../TASKS.md).
