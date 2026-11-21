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
- **Ein Sicherungsweg:** Die Datensicherung (unten) ist die einzige Sicherung der App. Die
  früheren Kopien `.myfotomanager.backup` (nach jedem Bestands-, Prüf-, Metadaten- und
  Analyselauf ersetzt) und `.myfotomanager.before-detector-*.bak` entstehen nicht mehr;
  vorhandene bleiben liegen, werden nicht automatisch gelöscht und können von Hand entfernt
  werden. „Biometrie entfernen“ bereinigt sie weiterhin. Routineläufe (Bestand, Prüfung,
  Metadaten, Analyse, geprüfte Einzelverschiebung) sichern nicht.
- **Datensicherung der App-Daten** (`core::backup`, Einstellungen → „Datensicherung“): ein ZIP
  `.myimporter.backup.NNN` (mindestens dreistellig, `config::BACKUP_PREFIX`) im
  Sammlungsordner mit `sammlung/.myfotomanager`, `arbeitsplatz/central.sqlite` (eingestellter
  Ort, auch bei ausgeschaltetem Abgleich), `arbeitsplatz/settings.json` und `manifest.json`
  (Format, App-Version und Build, Zeitpunkt, Anlass, Quellpfade, Größe, SHA-256 und
  Schemaversion je Eintrag, fehlende Quellen unter `skipped`). Nicht gesichert, weil
  ableitbar oder wieder ladbar: Fotos, Vorschau- und Gesichtskacheln, Referenzexport
  `.myfotomanager_faces`, ältere Kopien `.myfotomanager.backup` und `*.bak`, `models/`,
  `runtime/`, `logs/`, Sperrdatei, der lokale Cache-Ordner (Startansicht, Vorschauen,
  Personenliste der Zeitreise, Personengraph) und der MatchCache im Speicher.
  Datenbanken werden mit der SQLite-Online-Sicherung kopiert, auf einer eigenen
  Leseverbindung mit einer über alle Schritte offenen Lesetransaktion: die Kopie ist ein
  fester Stand, Schreiber laufen weiter und starten die Kopie nicht neu (Test mit
  gleichzeitigem Schreiber). Die Kopie wird auf Journalmodus DELETE gestellt und ist
  eigenständig. Ablauf: Platz prüfen (Quellen samt `-wal` zweimal plus 64 MB, sonst Abbruch
  ohne Schreiben), Zwischenordner und Zwischen-ZIP `.myimporter.backup.tmp-*` im
  Sammlungsordner, ZIP mit `sync_all`, dann Umbenennen auf die nächste Nummer (höchste
  vorhandene plus eins) ohne Ersetzen (`persist_noclobber`; bei Kollision die folgende
  Nummer). Fehler und Abbruch löschen die Zwischenstände; Reste eines Absturzes älter als
  24 h entfernt der nächste Lauf. Danach werden die ältesten Sicherungen über „Sicherungen
  behalten“ (Arbeitsplatz `backup_keep`, Standard 5, 1 bis 99) gelöscht, nur Dateien genau
  dieses Musters, nie die neue. Deflate-Stufe `BACKUP_COMPRESSION_LEVEL` (1, siehe
  [Validierung](../docs/validierung.md)). ZIP über das schon vorhandene `zip`-Crate (Deflate
  über `flate2`, bereits für das ONNX-Paket im Baum), Prüfsummen über `sha2`, freier Platz
  über `GetDiskFreeSpaceExW` ohne neues Crate: keine neue Abhängigkeit.
- **Automatische Datensicherung vor riskanten Schritten:** vor einer Schema-Migration (in
  `migrations::upgrade`, auf dem Worker, der die Sammlung schreibend öffnet), vor einem
  Detektorwechsel (in `faces::analyze_scoped`, Anlass „Detektorwechsel“), vor „Neu
  verknüpfen“ (`missing::relink`) und vor „Fehlende Einträge entfernen“ (`missing::remove`),
  jeweils im selben Auftrag, mit Fortschritt und Abbruch. Scheitert oder endet die Sicherung
  abgebrochen, findet der Schritt nicht statt und der Auftrag meldet den Fehler.
- **Tägliche Datensicherung:** Einstellung „Täglich automatisch sichern“ (Arbeitsplatz
  `daily_backup`, Standard an). Beim Start mit Sammlung und nach jedem Sammlungswechsel
  prüft die Oberfläche einmal, sobald Startseite und Zählung geladen sind und kein
  Nutzerschreibauftrag läuft, ob für diese Sammlung heute schon gesichert wurde
  (`daily_backup_dates`, Datum je Sammlungspfad). Wenn nicht, läuft `backup::automatic` mit
  Anlass „täglich“ als Auftrag `backup-daily`: lesend, niedrige Priorität (gibt Lesevorgängen
  an Haltepunkten nach, belegt keine Knöpfe), abbrechbar, im Diagnose-Log. Das Datum wird
  erst nach Erfolg gespeichert; Abbruch oder Fehler (Fehler als Hinweis) holen sie beim
  nächsten Öffnen nach. Aufbewahrung wie eingestellt. Den Plan (Arbeitsplatzdateien, Anzahl) setzt die
  Oberfläche beim Start und nach jedem Speichern der Einstellungen (`backup::configure`);
  ohne ihn (Tests, Beispiele) wird nur die Sammlungsdatenbank gesichert.
  „Biometrie entfernen“ löscht alle Datensicherungen, weil sie Gesichtsdaten enthalten
  (auch die täglichen; die nächste entsteht frühestens am Folgetag).
  Eine Wiederherstellung in der Oberfläche gibt es nicht; ein Test packt eine Sicherung aus
  und prüft Prüfsummen, `PRAGMA integrity_check` und Inhalt.
- **Schema 7 mit Migration** (`migrations::SCHEMA_VERSION`): Stände 1 bis 6 werden beim ersten
  Schreibzugriff nach einer Datensicherung (`backup::automatic`, Anlass „vor Schema-Migration
  N → 7“) in einer Transaktion auf 7 gebracht; die Versionsnummer ändert sich erst mit dem
  Commit. Stände 1 bis 5 bauen dabei die Projektionen neu auf, Schema 6 erhält nur die
  Datenrevision (Tabelle, Trigger, `projection_revision` entfällt). Quellzeilen, IDs, Labels,
  Vektoren, Pins und Ablehnungen bleiben. Schema 4 bis 6 darf lesend ohne Upgrade geöffnet
  werden. Unvollständige Pflichttabellen und unbekannte Versionen werden abgewiesen. Leser
  migrieren nie.
- **Datenrevision für Caches** (`core::revision`, Schema 7): `data_revision(domain,
  revision)` mit drei Bereichen; Trigger erhöhen den Zähler in derselben Transaktion wie die
  Änderung. `files`: Einfügen, Löschen und Projektionsspalten von `files` und `meta` (dieselben
  Trigger wie `metadata_photos`). `faces`: Gesichter mit Person (Einfügen, Löschen, Ändern von
  Person, Zuordnungsart, Ignorieren, Behalten), `face_rejections`, `training_set`,
  `face_settings`. `people`: `persons`, `person_attrs`, `relations`, `relation_types`.
  Erkennung ohne Person, Merkmale, Gruppen, Aufträge und `face_review` zählen nicht: sie
  entstehen in Massen, und kein Cache hängt daran. `revision::read` liest Identität
  (`collection.id`) und alle Zähler mit einer Abfrage; ein Cache merkt sich den Schlüssel der
  Bereiche, aus denen er gebaut ist, und gilt nur bei Gleichheit. Nutzer: Startansicht und
  Zählungen (`files`), Personengraph (alle drei), Rückfragen „dieselbe Person“ (`faces`,
  `people`), Personendetails (`files`, `faces`), Zeitreise (`faces`, `people`). Ohne Tabelle
  (Schema 6, nur lesend) gibt es keine Revision, und nichts wird wiederverwendet. Kosten je
  Zeile statt Zählung im Schreibcode, weil so auch jeder künftige Schreibweg zählt:
  Messung 2026-11-21 (`examples/revision_bench.rs`, Release, 100.000 synthetische Zeilen,
  Median aus 3 Runden, vorher/nachher): Bestand 23,7/23,2 s, Metadaten 39,9/39,9 s, Analyse
  20,2/21,9 s (Streuung 17,7 bis 24,3 s), Abgleich mit 100.000 geänderten Zuordnungen
  10,8/12,2 s (rund 14 µs je geänderter Zuordnung).
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
  der andere wird weitergeleitet. Verschiebungen und umbenannte Ordner vollzieht der
  Bestandslauf nach, ohne neue Zeilen: Die alte Zeile (Datei-ID, Foto-ID, Gesichter,
  Entscheidungen) übernimmt den neuen Pfad, `faces.source_relpath` folgt. Reihenfolge der
  Zuordnung für eine neue Datei, deren alter Pfad fehlt: (1) gleiche Datei-ID (NTFS-Index,
  auf exFAT bei Ordnerumbenennung ebenfalls stabil) und Größe; (2) gleicher Dateiname
  (gefaltet), Größe und Änderungszeit, geprüft gegen einen gespeicherten Schnellhash, falls
  vorhanden; (3) bekannter BLAKE3-Hash. Bei (2) wird nicht geraten: passen mehrere
  verschwundene Zeilen oder tragen mehrere neue Dateien ohne Bestandszeile denselben Namen und
  dieselbe Größe, bleibt die alte Zeile fehlend und die neue Datei neu; der Lauf meldet
  „N mehrdeutig, nicht zugeordnet“ und die Ansicht „Fehlende Dateien“ bietet, wo eindeutig,
  das Neu-Verknüpfen an.
- **Fehlende Dateien beim Lesen:** Metadaten- und Gesichtslauf prüfen jede Datei. Fehlt sie
  (`NotFound`, Windows-Fehler 2/3), wird sie als fehlend markiert (`status='missing'`, Zeile
  `changes` mit `kind='missing'`; Zeilen, Foto-IDs und Entscheidungen bleiben), WARN im
  Diagnose-Log mit Anzahl und Beispielpfad, und der Lauf geht weiter. Fehlende Dateien
  fallen aus allen Warteschlangen (`status='present'`), bis ein Bestandslauf sie wiederfindet.
  Ist die Sammlung selbst nicht mehr erreichbar (Datenbankdatei fehlt), wird nichts markiert.
- **Fehlende Einträge entfernen** (Ansicht „Fehlende Dateien“, `core::missing`): löscht nur
  `files`- und `meta`-Zeilen mit `status='missing'` der gewählten Ordner, nachdem jede Datei
  noch einmal auf der Platte gesucht wurde (wieder Vorhandenes bleibt). Fotos, Gesichter und
  Personenzuordnungen bleiben; die Rückfrage nennt Fotos ohne Datei und bestätigte Gesichter.
  **Neu verknüpfen** übergibt fehlende Zeilen an ihre schon eingetragenen Entsprechungen
  (eindeutig gleicher Name, Größe, Änderungszeit, alle in einem Zielordner) und entfernt die
  doppelten neuen Zeilen; nur solange an deren Fotos noch keine Gesichter hängen.
- **Indizierung in vier Schritten:** Ordner lesen, Bestand vergleichen, Dateien eintragen
  (Stapel zu 500 je Transaktion, sofort sichtbar), abschließen. Metadaten (ExifTool) und
  BLAKE3-Prüfung sind eigene, fortsetzbare Läufe. Ungelesene Dateien liest zusätzlich ein Hintergrundlauf
  (siehe [Anwendung](anwendung.md)); `CollectionStatus.unread` zählt sie ohne frühere Fehler.
- **Schutz vor Massenverlust:** Im Bestandslauf markiert nur ein vollständig durchgelaufener
  Lauf unbesuchte Dateien als fehlend (einzeln geprüfte Dateien im Metadaten- und
  Gesichtslauf siehe oben); bei Lesefehlern nie, über 20 % nur nach Bestätigung. Ein abgebrochener Lauf behält
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

Neue Varianten- oder Metadatenfelder ändern Projektion und Trigger gemeinsam. Neue Tabellen,
aus denen ein Cache liest, bekommen Revisionstrigger ihres Bereichs (`revision::TABLES`,
Test je Tabelle); neue Caches über Sammlungsdaten verwenden `revision::read`, keine eigenen
Zähler oder Fingerabdrücke. Wer
`metadata_photos` anders als über Einfügen, Löschen oder `relpath` ändert oder die
Ordnerformel ändert, passt die `folder_counts`-Trigger an (Test vergleicht Tabelle und
Zählung über `files.dir` nach jeder Änderungsart). Trigger auf
`files` feuern bei jedem `UPDATE` einer gelisteten Spalte; unveränderte Zeilen nur über nicht
gelistete Spalten anfassen. Pläne neuer Gesichtsabfragen ohne `sqlite_stat1` prüfen
(`face_save_bench steps … ohne-statistik`, `EXPLAIN QUERY PLAN`). Migrationstests bilden vorhandene Zwischenstände ab. Neue Module
binden an Foto-IDs und beachten Weiterleitungen. Neue App-eigene Nutzerdaten (Datei oder
Datenbank) in die Datensicherung aufnehmen oder hier begründen, warum nicht; neue riskante
Schritte (Massenlöschung, Umbau) rufen vorher `backup::automatic` und brechen bei Fehler ab.
