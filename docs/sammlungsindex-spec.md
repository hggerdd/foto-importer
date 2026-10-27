# Spezifikation: Sammlungsindex (`.myfotomanager`)

> Zielgruppe: Coding-Agent (Claude Code / Codex). Dieses Dokument wird **vor** der Gesichtserkennung umgesetzt und ist deren Grundlage (siehe `gesichtserkennung-spec.md`). Vor der Umsetzung die bestehende Codebasis der Import-App lesen und Struktur, Stack und Stil übernehmen. Abweichungen begründen.

---

## 1. Ziel

Die App bekommt einen **eigenen, Lightroom-unabhängigen Index** der Foto-Sammlung (des Zielordners).

- Im **Wurzelordner der Sammlung** liegt eine SQLite-Datei `.myfotomanager`.
- **Schnellscan:** alle Bilddateien in Sekunden bis wenigen Minuten erfassen (Name, relativer Pfad, Größe, Änderungszeit).
- **Metadaten-Anreicherung:** zusätzlich EXIF und MakerNotes lesen (Kamera, Seriennummer, Auslösezähler, Aufnahmezeit, GPS, Objektiv, …), langsam, im Hintergrund.
- **Änderungserkennung, manuell ausgelöst:** neue, geänderte, gelöschte, verschobene und umbenannte Dateien erkennen.
- **Import-Prüfung:** Beim Import von der SD-Karte erkennen, welche Fotos bereits in der Sammlung sind, auch wenn sie umbenannt oder verschoben wurden.
- **Stabile Foto-IDs** als Basis für spätere Funktionen (Gesichter, Suche, Embeddings).

### Nicht-Ziele
- Keine Gesichtserkennung, keine Thumbnails, keine Bildanalyse (spätere Specs).
- Kein automatisches Überwachen des Dateisystems (kein Watcher). Scans startet der Nutzer.
- Kein Schreiben in Bilddateien oder Lightroom-Kataloge. **Die Sammlung wird nur gelesen**, außer der Datei `.myfotomanager` selbst.

---

## 2. Begriffe

| Begriff | Bedeutung |
|---|---|
| Sammlung | Zielordner mit Unterordnern, `root` |
| Datei (`file`) | eine konkrete Datei auf der Platte, z. B. `2025/2025-07-14 Wörthersee/NEF/DSC_4471.NEF` |
| Foto (`photo`) | eine Aufnahme; mehrere Dateien können dazugehören (NEF + JPG + DNG) |
| Identitätsschlüssel | aus Metadaten abgeleitete, umbenennungsfeste ID eines Fotos |
| Scan | ein vom Nutzer gestarteter Lauf über die Sammlung |

---

## 3. Speicherort und Grundregeln

- Datei: `<root>/.myfotomanager` (SQLite, WAL-Modus). Begleitdateien `-wal` und `-shm` entstehen daneben.
- **Alle Pfade relativ zu `root`**, mit `/` als Trenner, **Unicode NFC-normalisiert** (macOS liefert NFD, siehe Abschnitt 10).
- Der Index ist ein **Cache**: Die Dateien sind die Wahrheit. Er muss sich jederzeit aus der Sammlung neu aufbauen lassen, mit Ausnahme von Nutzerdaten, die spätere Module (z. B. Gesichter) anhängen.
- Eine Sammlung wird über eine **Sammlungs-ID (UUID)** in der Datenbank identifiziert, nicht über ihren Pfad. Laufwerksbuchstabe oder Mountpoint dürfen sich ändern.
- Ausgeschlossen vom Scan: `.myfotomanager*`, `.myfotomanager_faces/`, versteckte Ordner, Systemordner (`$RECYCLE.BIN`, `.Trashes`, `@eaDir` von Synology, `System Volume Information`), konfigurierbare Ausschlussmuster.
- Erfasste Endungen (konfigurierbar, case-insensitiv): `nef, nrw, dng, jpg, jpeg, heic, tif, tiff, png, mov, mp4`. `xmp`-Sidecars werden erfasst und der zugehörigen Datei zugeordnet, aber nicht als eigene Fotos gezählt.

---

## 4. Datenmodell

```sql
PRAGMA user_version = 1;    -- Schema-Version, Migrationen versionieren

collection(
  id TEXT PRIMARY KEY,          -- UUID, beim ersten Anlegen erzeugt
  created_at TEXT,
  app_version TEXT
);

settings(key TEXT PRIMARY KEY, value TEXT);   -- Endungen, Ausschlüsse, …

files(
  id INTEGER PRIMARY KEY,
  relpath TEXT NOT NULL UNIQUE,  -- NFC, '/' als Trenner
  relpath_fold TEXT NOT NULL,    -- casefold(relpath) für case-insensitive Dateisysteme
  dir TEXT NOT NULL,             -- relativer Ordner
  name TEXT NOT NULL,
  stem TEXT NOT NULL,            -- Name ohne Endung (für Paarbildung)
  ext TEXT NOT NULL,             -- klein, ohne Punkt
  size INTEGER NOT NULL,
  mtime_ns INTEGER NOT NULL,
  inode TEXT NULL,               -- st_ino bzw. Windows File-ID, wenn verfügbar
  quick_hash TEXT NULL,          -- siehe 6.3
  full_hash TEXT NULL,           -- optional, siehe 6.3
  photo_id INTEGER NULL,
  status TEXT NOT NULL,          -- 'present' | 'missing'
  first_seen_scan INTEGER, last_seen_scan INTEGER,
  meta_state TEXT NOT NULL DEFAULT 'pending',   -- 'pending' | 'done' | 'error' | 'stale'
  meta_error TEXT NULL
);
CREATE INDEX ix_files_name_size ON files(name, size);
CREATE INDEX ix_files_size ON files(size);
CREATE INDEX ix_files_stem_dir ON files(dir, stem);
CREATE INDEX ix_files_photo ON files(photo_id);

meta(
  file_id INTEGER PRIMARY KEY,
  make TEXT, model TEXT, serial TEXT,
  shutter_count INTEGER,         -- Nikon MakerNotes
  dt_original TEXT,              -- wie in EXIF, ohne Umrechnung
  subsec TEXT, offset_time TEXT, -- falls vorhanden
  width INTEGER, height INTEGER, orientation INTEGER,
  lens TEXT, focal REAL, aperture REAL, exposure TEXT, iso INTEGER,
  lat REAL, lon REAL, alt REAL,
  rating INTEGER, label TEXT,    -- aus XMP, falls vorhanden
  json TEXT                      -- vollständige exiftool-Ausgabe
);
CREATE INDEX ix_meta_identity ON meta(serial, shutter_count);
CREATE INDEX ix_meta_time ON meta(dt_original, subsec);

photos(
  id INTEGER PRIMARY KEY,        -- STABIL, wird nie neu vergeben
  identity_key TEXT UNIQUE NULL, -- siehe 6.2; NULL, solange Metadaten fehlen
  status TEXT NOT NULL,          -- 'present' | 'missing'
  created_scan INTEGER
);

scans(
  id INTEGER PRIMARY KEY,
  kind TEXT,                     -- 'quick' | 'meta' | 'verify'
  scope TEXT,                    -- '' = ganze Sammlung, sonst relativer Ordner
  started_at TEXT, finished_at TEXT,
  status TEXT,                   -- 'running' | 'done' | 'cancelled' | 'error'
  stats_json TEXT                -- Zähler: neu, geändert, fehlt, verschoben, …
);

changes(
  id INTEGER PRIMARY KEY,
  scan_id INTEGER,
  kind TEXT,                     -- 'added' | 'modified' | 'missing' | 'moved' | 'returned'
  file_id INTEGER, photo_id INTEGER,
  old_relpath TEXT, new_relpath TEXT,
  at TEXT
);
CREATE INDEX ix_changes_scan ON changes(scan_id);
```

**Prinzipien:**
- `photos.id` ist der **Anker für alle späteren Module.** Er bleibt bei Umbenennen, Verschieben und erneutem Scannen gleich.
- Fehlende Dateien werden **nicht gelöscht**, sondern als `missing` markiert. Erst eine ausdrückliche Nutzeraktion („Fehlende bereinigen“) entfernt sie. Grund: eine nicht eingesteckte Platte oder ein umbenannter Ordner darf keine Daten späterer Module vernichten.
- `changes` ist das Änderungsprotokoll pro Scan. Spätere Module (Gesichter) lesen daraus, was sie neu verarbeiten müssen.

---

## 5. Abläufe

### 5.1 Schnellscan (`kind='quick'`, manuell)

1. Scan-Eintrag anlegen, `status='running'`.
2. Verzeichnisbaum mit **`os.scandir`** iterativ durchlaufen (Stack, keine Rekursion). Symlinks nicht folgen. `DirEntry.stat()` nutzen: unter Windows ohne zusätzlichen Systemaufruf, unter Linux/macOS ein günstiger `stat`.
3. Für jede passende Datei `(relpath, size, mtime_ns, inode)` erzeugen.
4. Mit dem Index abgleichen (Abschnitt 6.1) und in **Batches von z. B. 2.000 Zeilen in einer Transaktion** schreiben (`executemany`).
5. Nach dem Lauf alle Dateien, die in diesem Scan nicht gesehen wurden (`last_seen_scan < scan_id`) und im Scan-Bereich liegen, als **Kandidaten für „fehlt“** behandeln. Vorher die Verschiebungserkennung ausführen (Abschnitt 6.4).
6. Paarbildung aktualisieren (Abschnitt 6.5).
7. Zusammenfassung in `scans.stats_json` schreiben und in der UI anzeigen.

**Scope:** Der Scan kann auf einen Unterordner begrenzt werden, z. B. direkt nach einem Import. Dann darf „fehlt“ nur für Dateien in diesem Unterordner gesetzt werden.

**Abbrechbar:** Bei Abbruch wird der Scan als `cancelled` markiert. Es wird **kein** „fehlt“ gesetzt, weil der Baum nicht vollständig gesehen wurde.

### 5.2 Metadaten-Anreicherung (`kind='meta'`, manuell, optional zeitgesteuert)

- Verarbeitet alle Dateien mit `meta_state IN ('pending','stale')`, Bilder vor Videos, neueste Ordner zuerst.
- **Ein langlebiger exiftool-Prozess** (`-stay_open True -@ -`) oder Batches mit Argumentdatei: `exiftool -j -n -fast2 -G1 …`. Niemals einen Prozess pro Datei starten (etwa 50-mal langsamer).
  - `-fast2` liest nur Dateiköpfe. Prüfen, dass Seriennummer und Auslösezähler aus den Nikon-MakerNotes damit noch geliefert werden; sonst `-fast` verwenden.
  - `-n` liefert Zahlen statt formatierter Texte (z. B. GPS als Dezimalgrad).
- Batchgröße ~200 Dateien, Ergebnisse in einer Transaktion schreiben.
- Danach `identity_key` berechnen und die Datei dem passenden Foto zuordnen (Abschnitt 6.2). Dabei können zwei bisher getrennte Foto-Einträge zusammenfallen. Regel: der **ältere `photos.id` gewinnt**, der andere wird umgehängt und in `changes` protokolliert.
- Fehler pro Datei (`meta_state='error'`, Meldung speichern) brechen den Lauf nicht ab.
- Fortsetzbar: Ein abgebrochener Lauf setzt dort fort, wo er war.

### 5.3 Vollständige Prüfung (`kind='verify'`, manuell, selten)

- Berechnet `full_hash` für alle oder ausgewählte Dateien und vergleicht mit einem bereits gespeicherten Wert.
- Abweichung bei gleicher Größe und Änderungszeit bedeutet einen möglichen Datenfehler (Bitfäule) und wird gesondert gemeldet.

---

## 6. Algorithmen

### 6.1 Abgleich einer Datei beim Schnellscan

```
gefunden (relpath, size, mtime_ns, inode)

wenn relpath im Index:
    wenn size und mtime_ns gleich:         unverändert   → last_seen_scan setzen
    sonst:                                  'modified'    → Werte aktualisieren,
                                                            quick_hash = NULL,
                                                            meta_state = 'stale'
    wenn Status vorher 'missing':           'returned'
sonst:
    vorläufig 'added' (Verschiebungserkennung in 6.4 kann es in 'moved' umwandeln)
```

Die Änderungszeit wird mit einer **Toleranz von 2 s** verglichen (FAT/exFAT speichert nur 2-Sekunden-Schritte). Eine reine Änderung der Änderungszeit ohne Größenänderung bei JPG/NEF ist häufig harmlos (z. B. durch Kopierprogramme), wird aber trotzdem als `modified` gemeldet, damit die Metadaten neu gelesen werden.

### 6.2 Identitätsschlüssel eines Fotos

In dieser Reihenfolge, der erste verfügbare gewinnt:

1. `serial + ':' + shutter_count` (Nikon, eindeutig, umbenennungsfest)
2. `serial + ':' + dt_original + '.' + subsec`
3. `make + model + ':' + dt_original + '.' + subsec + ':' + width + 'x' + height`
4. kein Schlüssel → Foto bleibt nur über Paarbildung (6.5) verbunden

**Achtung:**
- Bei Serienbildern ohne Subsekunden haben mehrere Fotos die gleiche Aufnahmezeit. Stufe 3 nur verwenden, wenn `subsec` vorhanden ist.
- **DNG aus dem Konverter:** prüfen, ob Seriennummer und Auslösezähler übernommen werden (MakerNotes im DNG). Wenn ja, landet das DNG automatisch beim selben Foto wie das NEF.
- **JPG aus der Kamera:** enthält in der Regel dieselben EXIF-Daten wie das NEF und landet ebenfalls beim selben Foto.
- Bearbeitete Exporte (z. B. aus Lightroom) übernehmen oft die EXIF-Daten. Sie würden dann fälschlich als Teil des Fotos gelten. Regel: Dateien in konfigurierbaren Export-Ordnern (z. B. `Export/`) werden **nicht** über den Identitätsschlüssel zugeordnet, sondern als eigene Fotos geführt.

### 6.3 Inhalts-Fingerabdruck

- **`quick_hash`:** BLAKE3 oder xxHash über `size` + die ersten 64 KB + die letzten 64 KB. Sehr schnell, auch über USB oder Netzwerk. Wird nur bei Bedarf berechnet: für Verschiebungs-Kandidaten und für die Import-Prüfung.
- **`full_hash`:** über die ganze Datei, nur im Prüf-Lauf (5.3).
- Eine NEF-Datei ändert sich nicht durch Bearbeitung in Lightroom (die Einstellungen liegen im Katalog bzw. im XMP). Ein `quick_hash` bleibt daher sehr stabil.

### 6.4 Verschiebungs- und Umbenennungserkennung (am Ende des Schnellscans)

Eingaben: Menge **A** = in diesem Scan neu gefundene Pfade, Menge **M** = im Index vorhandene, in diesem Scan nicht gesehene Pfade (im Scan-Bereich).

Für jedes Paar mit gleicher `size` aus A × M, in dieser Reihenfolge:
1. **Gleicher Inode bzw. File-ID** (gleiches Laufwerk) → sicher verschoben oder umbenannt.
2. **Gleicher Name und gleiche Größe** → verschoben, wenn der `quick_hash` übereinstimmt.
3. **Anderer Name, gleiche Größe** → umbenannt, wenn der `quick_hash` übereinstimmt.

Treffer: den bestehenden `files`-Eintrag **wiederverwenden** (`relpath` aktualisieren, ID bleibt, also bleibt auch `photo_id`), `change.kind='moved'` mit altem und neuem Pfad. Mehrdeutigkeit (mehrere Kandidaten mit gleichem Hash) wird nicht automatisch aufgelöst, sondern als `added` + `missing` gemeldet und in der UI zur Entscheidung angeboten.

Übrige Einträge in A → `added`. Übrige in M → `missing`.

**Ordner umbenannt:** Das ist der häufigste Fall (z. B. `2025-07-14` → `2025-07-14 Wörthersee`) und betrifft Hunderte Dateien. Optimierung: Wenn ein ganzer bisheriger Ordner fehlt und ein neuer Ordner mit gleicher Dateiliste (Namen + Größen) auftaucht, alle Dateien auf einmal umhängen, ohne Hashes zu berechnen. In der Zusammenfassung als „Ordner umbenannt“ melden.

### 6.5 Paarbildung (Datei → Foto)

1. Hat die Datei einen `identity_key` → Foto mit diesem Schlüssel (anlegen, falls nicht vorhanden).
2. Sonst: Dateien mit gleichem `stem` im **gleichen Ordner oder in benachbarten Ordnern derselben Ebene** (z. B. `…/NEF/DSC_4471.NEF` und `…/JPG/DSC_4471.JPG`, die Import-App legt Endungen in eigene Unterordner) und ähnlicher Änderungszeit → gleiches Foto.
3. Sonst: neues Foto.

Die Ordnerstruktur der Import-App (ein Unterordner pro Endung) ist konfigurierbar, nicht fest kodiert.

---

## 7. Import-Prüfung (Schnittstelle für die Import-App)

Für jede Datei auf der SD-Karte wird ein Status bestimmt:

| Status | Bedingung |
|---|---|
| `imported` | `identity_key` der Kartendatei existiert in `photos` (Status `present`) |
| `imported_likely` | gleicher Name + gleiche Größe im Index, aber Metadaten der Sammlung noch nicht gelesen |
| `imported_missing` | Foto bekannt, aber alle Dateien `missing` (z. B. gelöscht) |
| `new` | sonst |

- Für die Kartendateien die Metadaten direkt von der Karte lesen (exiftool im Batch, `-fast2`). Bei einigen hundert Fotos dauert das wenige Sekunden.
- **Nur Name + Größe ist nicht eindeutig:** Der Nikon-Zähler läuft bei `DSC_9999` über, und mehrere Kameras erzeugen gleiche Namen. Daher immer den Identitätsschlüssel bevorzugen; Name + Größe ist nur der Schnellweg.
- Die Änderungszeit der Kartendatei **nicht** zum Vergleich nutzen (FAT-Genauigkeit, Zeitzonen, Kopierprogramme).
- API: `check_import(files_on_card) -> list[(card_path, status, photo_id | None, collection_relpath | None)]`
- In der Tagesansicht: pro Tag „x von y bereits importiert“, bereits importierte Fotos ausgegraut, standardmäßig nicht erneut kopieren.
- **Nach dem Kopieren** automatisch einen Schnellscan nur über die Zielordner starten. So ist der Index sofort aktuell, ohne die ganze Sammlung zu scannen. Das ist kein Watcher, sondern Teil der Import-Aktion.

---

## 8. Bedienung (UI)

**Ansicht „Sammlung“ → Bereich „Index“:**
- Status: Anzahl Dateien und Fotos, letzter Schnellscan, Anteil mit Metadaten, Anzahl fehlender Dateien.
- Buttons: **„Änderungen suchen“** (Schnellscan der ganzen Sammlung), „Ordner prüfen …“ (Scope), **„Metadaten ergänzen“**, „Vollständige Prüfung …“.
- Fortschritt: Phase, gezählte Dateien, Dateien pro Sekunde, geschätzte Restzeit, Abbrechen-Button.
- **Ergebnisbericht nach jedem Scan**, gruppiert: neu, geändert, verschoben/umbenannt (mit alt → neu), umbenannte Ordner, fehlend, wieder aufgetaucht, mehrdeutig (Entscheidung nötig).
- „Fehlende bereinigen …“ mit Bestätigung und Hinweis, welche späteren Daten (z. B. Gesichter) daran hängen.
- Option für die Metadaten-Anreicherung: „Automatisch nachts um 03:00“ (nur solange die App läuft). Standard: aus.

**Erster Start auf einer bestehenden Sammlung:** Dialog „Sammlung indizieren?“ mit Hinweis, dass nur `.myfotomanager` im Wurzelordner angelegt wird und keine Fotos verändert werden.

---

## 9. Performance-Vorgaben

- **Schnellscan, erstmalig:** auf lokaler SSD ≥ 20.000 Dateien/s; auf USB-Festplatte ≥ 3.000 Dateien/s; auf NAS (SMB) begrenzt durch das Netz, Ziel ≥ 1.000 Dateien/s.
- **Schnellscan, unverändert:** ähnlich schnell, da nur Größe und Änderungszeit verglichen werden. Den Index dafür vorab einmal komplett in ein Dict `relpath → (size, mtime_ns, id)` laden. Bei 200.000 Dateien sind das wenige zehn MB im Speicher.
- **Metadaten:** ≥ 50 Dateien/s mit `-fast2` und `-stay_open` auf lokaler Platte.
- SQLite: `journal_mode=WAL`, `synchronous=NORMAL`, `temp_store=MEMORY`, `cache_size=-65536`, `busy_timeout=5000`. Indizes erst nach dem ersten Massenimport anlegen.
- **Parallelität im Schnellscan:** Auf einer lokalen Platte ein Thread. Auf NAS optional mehrere Threads, je einer pro Top-Level-Ordner, weil dort die Latenz dominiert. Datenbank-Schreibzugriffe immer über **einen** Writer-Thread.
- Die UI bleibt während aller Läufe bedienbar. Läufe laufen in einem Hintergrund-Thread oder -Prozess und melden den Fortschritt über eine Queue.

---

## 10. Schwierige Themen

1. **Unicode-Normalisierung:** macOS liefert Dateinamen in NFD (`ö` = `o` + Umlautzeichen), Windows und Linux in der Regel NFC. Ohne Normalisierung erscheinen alle Umlaut-Ordner bei einem Plattformwechsel als „fehlt“ und „neu“. Immer NFC speichern und vergleichen.
2. **Groß/Kleinschreibung:** Windows und macOS ignorieren sie meist, Linux nicht. `relpath_fold` für Vergleiche nutzen. Eine reine Umbenennung `dsc_1.nef` → `DSC_1.NEF` ist eine Umbenennung, kein Löschen und Neuanlegen.
3. **Zeitstempel:** FAT/exFAT speichern 2 s Auflösung und Ortszeit. NAS-Freigaben können Zeiten verschieben. Toleranz verwenden; Änderungszeit nie als Identitätsmerkmal nutzen.
4. **Nikon-Zähler-Überlauf** und mehrere Kameras: siehe Abschnitt 7. Name allein ist nie eindeutig.
5. **Nicht eingehängte Laufwerke:** Wenn `root` fehlt oder leer ist, **Scan abbrechen** statt alles als „fehlt“ zu markieren. Zusätzlich Schutzregel: Würde ein Scan mehr als z. B. 20 % der Dateien als fehlend markieren, vorher beim Nutzer nachfragen.
6. **SQLite auf NAS/SMB:** Dateisperren über das Netz sind unzuverlässig. Nur ein Prozess darf schreiben. Die App erkennt einen Netzwerkpfad und warnt bzw. bietet an, die Datenbank lokal zu halten (Pfad zur Sammlungs-ID in den lokalen App-Einstellungen).
7. **Gleichzeitige Nutzung:** Die App sperrt die Sammlung beim Öffnen über eine Lock-Datei `.myfotomanager.lock` mit Rechnername und Prozess-ID. Eine zweite Instanz öffnet die Sammlung dann nur lesend.
8. **Dateien in Bearbeitung:** Eine Datei, die während des Scans noch kopiert wird, hat eine wechselnde Größe. Dateien, deren Änderungszeit weniger als ~10 s zurückliegt, beim Abgleich als „unsicher“ markieren und beim nächsten Scan erneut prüfen.
9. **Sehr viele Dateien in einem Ordner** (z. B. 20.000 in `DCIM`): `scandir` streamt; nicht vorab alle Namen in einer Liste sammeln.
10. **Rechte und Fehler:** Unlesbare Ordner oder Dateien protokollieren und überspringen, nie den Scan abbrechen.
11. **Backups:** Die Datei `.myfotomanager` liegt in der Sammlung und wird mitgesichert. Das ist gewünscht, aber eine Sicherung während eines Schreibvorgangs kann inkonsistent sein. Daher regelmäßig (z. B. nach jedem Scan) eine konsistente Kopie per SQLite-Backup-API nach `.myfotomanager.bak` schreiben.

---

## 11. Schnittstelle für spätere Module (z. B. Gesichter)

Spätere Module verwenden ausschließlich:
- `photos.id` als stabilen Anker. Eigene Tabellen referenzieren `photos.id`, nie `files.id` oder Pfade.
- `files` mit `photo_id`, um die beste Quelldatei für ein Foto zu wählen (Vorschau aus NEF, sonst JPG).
- `changes` seit einem gespeicherten `scan_id`, um nur Neues und Geändertes zu verarbeiten: `added`, `modified`, `returned` → verarbeiten; `missing` → Daten behalten, aber ausblenden; `moved` → nichts zu tun.
- Eine Python-API statt direkter SQL-Zugriffe, z. B.:

```python
class CollectionIndex:
    def open(root: Path) -> "CollectionIndex": ...
    def quick_scan(scope: str = "", progress=None, cancel=None) -> ScanResult: ...
    def enrich_metadata(limit: int | None = None, progress=None, cancel=None) -> ScanResult: ...
    def verify(scope: str = "", progress=None, cancel=None) -> ScanResult: ...
    def check_import(card_files: list[Path]) -> list[ImportStatus]: ...
    def changes_since(scan_id: int) -> Iterator[Change]: ...
    def photo_files(photo_id: int) -> list[FileRecord]: ...
    def purge_missing(older_than_scans: int = 0) -> int: ...
```

- Spätere Module legen ihre Tabellen **in derselben Datenbank** an, mit eigenem Präfix (z. B. `face_`) und eigener Migrationsversion in `settings` (z. B. `schema.faces = 1`).

---

## 12. Umsetzungsphasen

1. **Grundlage:** Schema und Migrationen, `CollectionIndex.open`, Schnellscan ohne Verschiebungserkennung, Ergebnisbericht, UI-Button „Änderungen suchen“.
2. **Änderungserkennung:** `changes`, Verschiebungs- und Umbenennungserkennung inklusive Ordner-Umbenennung, Schutzregeln (5, 8).
3. **Metadaten:** exiftool-Batchlauf, `meta`-Tabelle, Identitätsschlüssel, Paarbildung.
4. **Import-Prüfung:** `check_import`, Anzeige in der Tagesansicht, Schnellscan der Zielordner nach dem Kopieren.
5. **Prüfung und Komfort:** `verify`, Bereinigen fehlender Dateien, zeitgesteuerte Metadaten-Anreicherung, Backup-Kopie.

---

## 13. Tests und Akzeptanzkriterien

Testfälle mit einer generierten Beispielsammlung (temporäres Verzeichnis, kleine Dummy-Dateien plus einige echte NEF/JPG mit EXIF):

- Erstscan erfasst alle Dateien, ignoriert ausgeschlossene Ordner und erkennt NEF/JPG-Paare.
- Unveränderter zweiter Scan meldet **null** Änderungen.
- Datei umbenannt → `moved`, gleiche `files.id` und `photo_id`.
- Ordner umbenannt (100 Dateien) → eine Meldung „Ordner umbenannt“, keine Hash-Berechnung.
- Datei gelöscht → `missing`, Foto bleibt erhalten; wiederhergestellt → `returned`, gleiche IDs.
- Datei inhaltlich geändert (gleiche Größe, neue Änderungszeit) → `modified`, `meta_state='stale'`.
- Dateiname in NFD und NFC → gleiche Datei.
- `root` nicht erreichbar oder leer → Scan bricht ab, nichts wird als fehlend markiert.
- Abgebrochener Scan → keine `missing`-Markierungen.
- Import-Prüfung: Kartendatei, die in der Sammlung umbenannt wurde → `imported` (über Identitätsschlüssel). Zwei verschiedene Fotos mit gleichem Namen `DSC_0001.NEF` → korrekt als unterschiedliche Fotos erkannt.
- Performance-Test mit 100.000 Dummy-Dateien: Erstscan und Wiederholungsscan innerhalb der Vorgaben aus Abschnitt 9.
- Neuaufbau: Datenbank löschen, Schnellscan und Metadatenlauf → gleiche Fotos werden wieder denselben Identitätsschlüsseln zugeordnet (IDs dürfen sich dabei ändern; das ist der Grund, warum spätere Module ihre Daten nie ohne Rückfrage verwerfen).
