# Sammlung und Daten

## Entscheidung

- **Eine SQLite-Datei je Sammlung:** `.myfotomanager` im Wurzelordner enthält Inventar,
  Metadaten, Gesichter und Personen. WAL, `busy_timeout`, Verbindungen gehören ihrem Thread.
  Lesende Verbindungen nutzen `mmap_size` 512 MB.
- **Sperre:** Die Host/PID-Sperrdatei `.myfotomanager.lock` erlaubt einen schreibenden
  Prozess; eine weitere Instanz öffnet lesend. Fremde Sperren werden nie entfernt.
- **Sicherungen:** `.myfotomanager.backup` nach abgeschlossenen Indexoperationen;
  benannte Sicherungen vor Schema-Upgrade und Detektorwechsel.
- **Schema 6 mit Migration:** Stände 1 bis 5 werden beim ersten Schreibzugriff nach einer
  Sicherung in einer Transaktion auf 6 gebracht; die Versionsnummer ändert sich erst mit dem
  Commit. Quellzeilen, IDs, Labels, Vektoren, Pins und Ablehnungen bleiben. Schema 4/5 darf
  lesend ohne Upgrade geöffnet werden. Unvollständige Pflichttabellen und unbekannte Versionen
  werden abgewiesen. Leser migrieren nie.
- **Additive Indizes ohne Versionssprung:** Reine Zusatzindizes auf abgeleiteten Daten stehen
  in `schema.sql` und `migrations::ADDITIVE_INDEXES` (Test prüft Gleichheit) und entstehen beim
  Öffnen mit Schreibrecht per `CREATE INDEX IF NOT EXISTS`. Tabellen, Spalten und Trigger
  ändern sich nur mit neuer Schemaversion.
- **Foto-Identität:** Dateien verweisen über `files.photo_id` auf ein Foto. Identität kommt
  bevorzugt aus Kameraseriennummer und Auslösezähler, mit festgelegten Rückfällen. Pfade
  beschreiben die Ablage, nicht die Identität. Beim Zusammenführen bleibt der ältere Anker,
  der andere wird weitergeleitet. Verschiebungen werden über Datei-IDs und bekannte Hashes
  nachvollzogen.
- **Indizierung in vier Schritten:** Ordner lesen, Bestand vergleichen, Dateien eintragen
  (Stapel zu 500 je Transaktion, sofort sichtbar), abschließen. Metadaten (ExifTool) und
  BLAKE3-Prüfung sind eigene, fortsetzbare Läufe.
- **Schutz vor Massenverlust:** Nur ein vollständig durchgelaufener Lauf markiert Dateien als
  fehlend; bei Lesefehlern nie, über 20 % nur nach Bestätigung. Ein abgebrochener Lauf behält
  Eingetragenes. Bei Hash-Abweichung bleibt der gespeicherte Vergleichswert erhalten.
- **Projektion für Abfragen:** `metadata_photos` enthält je Foto die bevorzugte Variante und
  wird von Triggern in derselben Transaktion gepflegt; die Originaltabellen bleiben maßgeblich.
  Zählungen im Gesichterbereich nutzen Teilindizes, deren Bedingung wörtlich im SQL steht.

## Grenzen

Kein Mehrschreiberbetrieb, Netzlaufwerke nur mit Vorsicht. Eine nach Absturz verbliebene
Sperrdatei muss von Hand entfernt werden. Schwache Paarung und ungehashte Datenträgerwechsel
können Nacharbeit brauchen. Beliebig beschädigte Datenbanken werden nicht repariert.

## Beim Ändern beachten

Neue Varianten- oder Metadatenfelder ändern Projektion und Trigger gemeinsam. Trigger auf
`files` feuern bei jedem `UPDATE` einer gelisteten Spalte; unveränderte Zeilen nur über nicht
gelistete Spalten anfassen. Migrationstests bilden vorhandene Zwischenstände ab. Neue Module
binden an Foto-IDs und beachten Weiterleitungen.
