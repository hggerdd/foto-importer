# ADR-0042: Native Bestandsmigrationen mit Sicherung

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung nach Prüfung der veröffentlichten SQL-Strukturen
- **Löst ab:** [0037](0037-sammlungskompatibilitaet.md)
- **Bezieht sich auf:** [0008](0008-lokale-sqlite-sammlung.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0029](0029-schneller-gesichtsstart-und-schema-reparatur.md)

## Kontext

Die zunächst gewählte Schema-6-Grenze verhindert die verlangte eigenständige
Rust-Migration älterer Sammlungen. Die Referenzmigrationen 1–6 erweitern Tabellen
und abgeleitete Strukturen; bestehende Nutzerspalten müssen nicht umgedeutet werden.

## Entscheidung

Ein Schreiber prüft die je Version 1–5 erforderlichen Quelltabellen und Spalten.
Fehlende Nutzerdaten werden nicht erfunden. Vor jeder Migration entsteht eine
SQLite-Sicherung `.myfotomanager.before-rust-vN-UUID.bak`.
Eine Transaktion ergänzt fehlende Tabellen und Indizes, installiert ausschließlich
bekannte App-Trigger neu und baut Foto-/Quellprojektionen in SQL neu auf.
Quellzeilen, IDs, Labels, Vektoren, Pins, Ablehnungen und bestehende Queues bleiben.
Die Versionsnummer wird erst mit erfolgreichem Commit 6. Schema 4/5 darf lesend
ohne Upgrade geöffnet werden; 1–3 benötigt zuerst Bestand aktualisieren.
Unvollständige abgeleitete v5-Strukturen werden mit demselben geprüften Pfad ergänzt.

## Alternativen

Ein Python-Upgrader würde weiter Python als Voraussetzung verlangen. Eine neue
Datenbank mit neu vergebenen IDs gefährdet Nutzerentscheidungen. Blindes Ergänzen
fehlender Quelltabellen könnte Beschädigung als leere Nutzerdaten verschleiern.

## Folgen und Grenzen

Einmalige SQL-Projektionsarbeit kann bei großen Sammlungen dauern; sie läuft auf
einem Worker. Unbekannte spätere Versionen und fehlende Pflicht-Quellspalten bleiben
sichtbare Fehler. Die Testfixtures modellieren die veröffentlichten Strukturen,
sie sind keine Behauptung einer Untersuchung jeder historischen Produktivdatei.
Biometrisches Löschen bereinigt auch die neuen Migrationssicherungen.

## Bei Weiterentwicklung beachten

Neue veröffentlichte Schemaänderungen benötigen eine neue Version. Leser führen
keine Migration aus. Sperren, Backup vor Upgrade und atomaren Commit erhalten.

## Belege und Validierung

[Migrationsservice](../src/core/migrations.rs), [Öffnen](../src/core/collection.rs),
[Versionen 1–5, Teil-v5 und Sicherungsbereinigung](../tests/migrations.rs).
