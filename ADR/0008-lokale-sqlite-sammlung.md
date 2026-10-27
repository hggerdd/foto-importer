# ADR-0008: SQLite-Datenbank im Sammlungsordner

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert

## Kontext

Inventar, Metadaten, Foto-Identitäten und spätere Nutzerzuordnungen sollen gemeinsam
mit einer Sammlung transportierbar und ohne Datenbankserver verfügbar sein.

## Entscheidung

Im Sammlungswurzelordner liegt `.myfotomanager`. SQLite mit WAL, `busy_timeout`,
gebündelten Schreibvorgängen und versionierten Migrationen über `PRAGMA user_version`
speichert Inventar und Gesichtsdaten gemeinsam. Aktuelle Schemaentwicklung: v1
Inventar, v2 Gesichtstabellen, v3 Referenzausschlüsse. Verbindungen gehören ihrem
erzeugenden Thread. Eine Host/PID-Lockdatei erlaubt einen Anwendungsschreiber;
eine weitere Instanz öffnet lesend. SQLite-Backups sichern abgeschlossene Indexoperationen.

## Alternativen

Ein zentraler Server oder eine getrennte Gesichtsdatenbank würde Betrieb bzw.
Konsistenz komplexer machen. SQLite-Zeilen sperren nicht automatisch die gesamten
fachlichen Abläufe; deshalb besteht zusätzlich die Anwendungssperre.

## Folgen und Grenzen

Gleichzeitige Bearbeitung ist begrenzt. Ein abgestürzter Prozess kann eine Lockdatei
zurücklassen. UNC/SMB verlangt Vorsicht; eine separate lokale DB für NAS-Sammlungen
ist nicht implementiert. Automatisches Entfernen fremder Sperren findet nicht statt.

## Bei Weiterentwicklung beachten

Migrationen mit vorhandenen Daten prüfen, keine Tabellen stillschweigend neu anlegen
oder Identitäten zurücksetzen. Netzwerk-/Mehrschreiberbetrieb erfordert eine neue Entscheidung.

## Belege und Validierung

[CollectionIndex](../core/collection_index.py), [Migrationen](../core/face_store.py),
[Index-Tests](../tests/core/test_collection_index.py), [Migrationstests](../tests/core/test_faces.py),
Historie: `556ea81`, `f503ce2`, `b737634`.
