# Sammlung und Daten

## Entscheidung

- **Eine SQLite-Datei je Sammlung:** `.myfotomanager` im Wurzelordner enthält Inventar,
  Metadaten, Gesichter und Personen. WAL, `busy_timeout`, Verbindungen gehören ihrem Thread.
  Lesende Verbindungen nutzen `mmap_size` 512 MB.
- **Neuanlage nur gewollt und nie auf der Speicherkarte:** Eine neue Sammlung entsteht beim
  ersten schreibenden Öffnen eines Ordners ohne `.myfotomanager`. Die Oberfläche fragt
  vorher nach und lehnt Importquelle und Speicherkarten ab (siehe [Anwendung](anwendung.md)).
  Als zweite Schicht verweigert `Collection::open(.., true)` die Neuanlage in jedem Pfad mit
  einem Teil `DCIM`, bevor Sperre oder Datenbank entstehen; vorhandene Sammlungen dort öffnen
  weiter.
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
  ändern sich nur mit neuer Schemaversion; einzige Ausnahme ist die Ordnerübersicht.
- **Ordnerübersicht `folder_counts` (additiv):** `folder_counts(dir PRIMARY KEY, photos)
  WITHOUT ROWID` zählt die Fotos aus `metadata_photos` je Ordner (`""` = Wurzel; Ordner =
  `relpath` bis zum letzten `/`, gleich `files.dir`). Trigger auf `metadata_photos`
  (Einfügen +1, Löschen −1 und Zeile bei 0 entfernen, Ändern von `relpath` beides) halten sie
  in derselben Transaktion aktuell. Tabelle und Trigger stehen in `schema.sql` und
  `migrations::FOLDER_COUNTS` (Test prüft Gleichheit); `ensure_folder_counts` legt sie beim
  Öffnen mit Schreibrecht ohne Versionssprung an und befüllt sie in derselben Transaktion mit
  einem `GROUP BY`. Fehlt ein Teil, wird alles neu gebaut. Ältere App-Versionen kennen die
  Tabelle nicht, halten sie aber über die Trigger in der Datenbank mit. Der Neuaufbau von
  `metadata_photos` in der Migration setzt die Trigger aus und berechnet die Tabelle danach in
  einer Anweisung neu (je Zeile gefeuert verdoppelten sie seine Dauer). Grund:
  Der Ordnerbaum las sonst auf einer USB-Festplatte Tausende verstreute Seiten (siehe
  [Sammlungsansicht](sammlungsansicht.md)).
- **Foto-Identität:** Dateien verweisen über `files.photo_id` auf ein Foto. Identität kommt
  bevorzugt aus Kameraseriennummer und Auslösezähler, mit festgelegten Rückfällen. Pfade
  beschreiben die Ablage, nicht die Identität. Beim Zusammenführen bleibt der ältere Anker,
  der andere wird weitergeleitet. Verschiebungen werden über Datei-IDs und bekannte Hashes
  nachvollzogen.
- **Indizierung in vier Schritten:** Ordner lesen, Bestand vergleichen, Dateien eintragen
  (Stapel zu 500 je Transaktion, sofort sichtbar), abschließen. Metadaten (ExifTool) und
  BLAKE3-Prüfung sind eigene, fortsetzbare Läufe. Ungelesene Dateien liest zusätzlich ein Hintergrundlauf
  (siehe [Anwendung](anwendung.md)); `CollectionStatus.unread` zählt sie ohne frühere Fehler.
- **Schutz vor Massenverlust:** Nur ein vollständig durchgelaufener Lauf markiert Dateien als
  fehlend; bei Lesefehlern nie, über 20 % nur nach Bestätigung. Ein abgebrochener Lauf behält
  Eingetragenes. Bei Hash-Abweichung bleibt der gespeicherte Vergleichswert erhalten.
- **Projektion für Abfragen:** `metadata_photos` enthält je Foto die bevorzugte Variante und
  wird von Triggern in derselben Transaktion gepflegt; die Originaltabellen bleiben maßgeblich.
  Zählungen im Gesichterbereich nutzen Teilindizes, deren Bedingung wörtlich im SQL steht.
- **Abfragepläne ohne Statistik:** Die App führt kein `ANALYZE` aus; echte Sammlungen haben
  keine `sqlite_stat1`. Heiße Abfragen werden so geschrieben, dass der Plan auch ohne
  Statistik passt: nicht korrelierte `IN`-Unterabfragen statt korrelierter `EXISTS`, deren
  Index der Planer falsch wählen kann, und `CROSS JOIN`, wo die äußere Tabelle feststehen muss.

## Grenzen

Kein Mehrschreiberbetrieb, Netzlaufwerke nur mit Vorsicht. Eine nach Absturz verbliebene
Sperrdatei muss von Hand entfernt werden. Schwache Paarung und ungehashte Datenträgerwechsel
können Nacharbeit brauchen. Beliebig beschädigte Datenbanken werden nicht repariert.
Auf der USB-Festplatte bleiben kalte Lesevorgänge über viele verstreute Gesichtszeilen
teuer (Statistik der Personentabelle, Gruppenladen für das Zusammenführen; Werte in
[Validierung](../docs/validierung.md)).

## Beim Ändern beachten

Neue Varianten- oder Metadatenfelder ändern Projektion und Trigger gemeinsam. Wer
`metadata_photos` anders als über Einfügen, Löschen oder `relpath` ändert oder die
Ordnerformel ändert, passt die `folder_counts`-Trigger an (Test vergleicht Tabelle und
Zählung über `files.dir` nach jeder Änderungsart). Trigger auf
`files` feuern bei jedem `UPDATE` einer gelisteten Spalte; unveränderte Zeilen nur über nicht
gelistete Spalten anfassen. Pläne neuer Gesichtsabfragen ohne `sqlite_stat1` prüfen
(`face_save_bench steps … ohne-statistik`, `EXPLAIN QUERY PLAN`). Migrationstests bilden vorhandene Zwischenstände ab. Neue Module
binden an Foto-IDs und beachten Weiterleitungen.
