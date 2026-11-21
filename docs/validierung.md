# Prüfprotokoll

04.–05.11.2026 · Windows · Rust 1.99.1 · synthetische Fotos und temporäre Datenbanken.

| Prüfung | Ergebnis |
|---|---|
| `cargo test --locked --offline` | 51 Regressionen bestanden; 2 Modelltests separat |
| `cargo test --locked --offline --test models -- --ignored` | 2 bestanden: YuNet/SFace und SCRFD/ArcFace mit installierter ONNX Runtime |
| `cargo fmt --all --check`, `cargo clippy --locked --offline --all-targets -- -D warnings` | Bestanden |
| `cargo build --release --locked --offline` | Windows-EXE erzeugt |
| Nativer Import mit 30 Demofotos | Alle kopiert, keine Fehler; SHA-256-Werte der Originale und Kopien stimmen überein |

Die Regressionen decken unter anderem Importabbruch, Namenskollisionen,
Duplikatvorprüfung, Identität bei Verschiebungen, Altbestandsmigration,
Sperren und abbrechbare Abfragen ab. Dazu kommen Gesichtsgruppen und Rücknahme,
Referenzauswahl, Biometrielöschung aus bekannten Sicherungen, alle acht
EXIF-Ausrichtungen und die Importansicht bei Mindest- und Standardfenstergröße.

Die Modelltests prüfen tatsächliche lokale Ausführung mit synthetischen RGB-Bildern,
Vektorgröße und Normalisierung. ExifTool wurde über ein Testprogramm simuliert.
Ein echter Adobe-Konvertierungslauf, RAW-Tests mit Kameradateien und ein
Qualitätsvergleich der Gesichtserkennung auf einem repräsentativen Bildbestand
stehen aus. Die UI-Prüfung ersetzt keine Screenreader- oder Nutzerstudie.
Produktive Fotos und Sammlungen wurden nicht verändert.

Die [Importansicht](screenshots/import.png) und die
[Sammlungsansicht](screenshots/sammlung.png) zeigen die native Anwendung mit
synthetischen Fotos. Den Beispieldatensatz erzeugt `cargo run --example demo --locked`
unter `tmp/demo`; zum Öffnen `FOTO_IMPORTER_SETTINGS` auf dessen `settings.json` setzen.

Einzelne Abweichungen von Python stehen in der [Funktionsmatrix](paritaet.md).

## ONNX-Runtime-Download am 06.11.2026

`cargo test --locked --test onnx_runtime -- --ignored` hat das offizielle
Windows-x64-Paket von Microsoft heruntergeladen, geprüft, entpackt und
die DLL erfolgreich geladen. Der reguläre Testlauf bleibt offline. Seine
Installationsprüfungen verwenden kleine ZIP-Dateien und decken falsche
Prüfsummen, fehlende Dateien, Abbruch und die Erhaltung früherer Installationen ab.
Alle 53 Offline-Regressionen bestanden. Ein Lauf über die Windows-Oberfläche
hat den Download mit leerem DLL-Pfad gestartet und den neuen Pfad gespeichert.

## Sammlungsansicht am 06.11.2026

Alle 54 Offline-Regressionen bestanden. Die Layoutregression prüft bei
980 × 680 und 1280 × 800, dass Vorschau und Metadaten sichtbar bleiben und
die Dialoge für Filter und Bestand ihre Position nicht verändern.
Das [Windows-Bild der Sammlungsansicht](screenshots/sammlung.png) zeigt
synthetische Fotos und Metadaten im hellen Design.

## Durchsuchbare Gesichtsmetadaten am 06.11.2026

Alle 56 Offline-Regressionen und die strenge Clippy-Prüfung bestanden.
Der Suchtest kombiniert einen Namen mit der Gesichtsanzahl und unterscheidet
bestätigte Zuordnungen, Vorschläge und historische Vorkommen. Er prüft
außerdem, dass null erkannte Gesichter und eine fehlende Analyse
unterschiedliche Treffer liefern.

## Nachrücken der Gesichtsgruppen

Die UI-Regression misst die Bildschirmposition vor und nach der Bestätigung.
Die ursprüngliche Folgegruppe landet auch nach einer Umsortierung wieder an
dieser Position. Ihr Namensentwurf bleibt erhalten. Eine zweite Regression
prüft den Rücksprung von einer leeren Seite zur ersten Seite, wenn keine
Gruppen mehr vorhanden sind.

## Erkennungsablauf im Personenbereich

Die Windows-Sichtprüfung bei 980 × 680 umfasst die drei Schritte, den
Dialog für Automatik und einen Analyseauftrag mit 30 synthetischen Fotos.
Der sichtbare Fortschritt stieg während der Analyse. Die Übersicht zeigte
anschließend 30 analysierte Fotos. Die Statusregression prüft getrennte
Abschlüsse für verschiedene Modelle und einen offenen Auftrag nach Abbruch.

## Beziehungsgraph

Die Graphregression prüft gemeinsame bestätigte Fotos gegen die
Sammlungssuche. Doppelte Vorkommen erhöhen die Fotozahl nicht.
Historische Gesichter, Vorschläge und weitergeleitete Personen tragen
nichts zur Fotozahl bei. Eingetragene Beziehungen ohne gemeinsames
Foto bleiben sichtbar. Ein zweiter Test prüft die Grenze von zwölf
angezeigten Verbindungen.

## Empfindlichere Gesichtserkennung

Lokal geprüft mit dem bereitgestellten Beispielbild. YuNet erkannte vorher
ein Gesicht und mit der neuen Präsenzregel vier. SCRFD stieg von drei auf
fünf. Ein temporärer Sammlungsindex speicherte vier aktuelle YuNet-Treffer.
Zwei davon zählten ohne Namensmerkmale. Die bestehende Bestätigung blieb
erhalten. Die Personensuche fand das Foto, der Filter ohne weiteres
Gesicht schloss es aus. Das Beispielbild gehört nicht zum Repository.

Nachberechnung kleiner Gesichter (ADR 0054, 08.11.2026): Echtlauf auf einer Kopie einer privaten Sammlung (Fotos nur gelesen, Datenbankkopie). Unbekannte Gesichter mit Namensmerkmalen: 1 → 58; 31 Fotos in 12 Sekunden. Inhaltliche Treffsicherheit nicht ausgewertet; Lauf über `tests/real_hires.rs` (ignoriert).

Sichtprüfung der Oberfläche (08.11.2026): `cargo run --example demo` und `cargo run --example demo_faces` erzeugen isolierte Kunstsammlungen unter `tmp/`. Mit `FOTO_IMPORTER_SETTINGS`, `FOTO_IMPORTER_VIEW` (collection, people, tasks, settings), `FOTO_IMPORTER_FACE_MODE` (unknown, review, confirmed, ignored, persons) und `FOTO_IMPORTER_CAPTURE` speichert die App nach fünf Sekunden ein Bild des Fensters. Private Sammlungen werden dafür nicht verwendet; Maus-Zustände wie die Hover-Vorschau lassen sich so nicht abbilden. Seit der Trennung von „Gesichter“ und „Personen“ (16.11.2026) gelten zusätzlich `FOTO_IMPORTER_VIEW=faces`, `persons` und `time` sowie `FOTO_IMPORTER_FACE_MODE=tree`, `graph` und `time`; die alten Werte öffnen die neuen Orte (`people` mit einem Gesichtsreiter die Ansicht „Gesichter“, mit `persons`, `tree` oder `graph` die Ansicht „Personen“, `time` dort die Zeitreise; `startup_place`). Sichtprüfung am 16.11.2026 (Debug-Build, Windows,
synthetische Demodaten, 1280 × 800 bei 150 %, dunkles Design): [Benennen](screenshots/gesichter-benennen.png),
[Vorschläge prüfen](screenshots/gesichter-pruefen.png), [Erledigt](screenshots/gesichter-erledigt.png),
[Personen](screenshots/personen.png), [Stammbaum der gewählten Person](screenshots/personen-stammbaum.png),
[Einstellungen](screenshots/einstellungen.png); dazu Import, Zeitreise und Aufgaben ohne Befund.
Behoben: Die Schaltflächen der Seitenleiste wirkten gesperrt (Standardfarben von egui). Die
Demodaten haben keine Analyseaufträge, daher meldet die Statuszeile dort „0 von 90 Fotos
analysiert“. Nicht geprüft: abgewählte Kacheln, „nicht X“, Fehlermeldung mit ✕, Tastenkürzel,
Hell-Design, 980 × 680.

## Personengraph als Netzwerk (ADR 0058)

Layout- und Netzregressionen: Layout mit 0, 1, 2 und 500 Knoten ist endlich,
deterministisch und hält den Mindestabstand ein (500 Knoten mit Zeitgrenze);
verbundene Personen liegen näher als unverbundene; Fotozahlen der Netzabfrage
stimmen mit der Sammlungssuche überein. Sichtprüfung am 08.11.2026 (Debug-Build,
synthetische Demodaten aus `examples/demo_faces.rs`, 40 Personen, 154 Verbindungen,
`FOTO_IMPORTER_FACE_MODE=graph`, `FOTO_IMPORTER_GRAPH_PERSON=1`): [Bild](screenshots/personengraph.png).
Nicht geprüft: Hell-Design, Mausrad/Ziehen/Doppelklick (Screenshot ist statisch),
Bedienung per Screenreader, Sammlungen mit mehreren hundert echten Personen,
Porträts statt Initialen (nicht umgesetzt).

## Stammbaum und berechnete Verwandtschaft (ADR 0061)

Regressionen: Fixture-Familie mit vier Generationen (Halbgeschwister von beiden Eltern,
Patchwork, Cousins beider Seiten, Großonkel, Schwager/Schwägerin, frühere Partner,
unbekannte Geschlechter), Eigenschaftstest mit 40 zufälligen zyklenfreien Bäumen gegen eine
naive Referenzdefinition (Blutsverwandtschaft stimmt überein, `relationship(a,b)` und
`relationship(b,a)` sind zueinander invers), Grenzen (8 Generationen, 100 je Gruppe), Altzeilen und
Zusammenführung mitten im Baum, Rückfragen (erscheinen, Ja legt Kanten an, Nein unterdrückt die
Wiederholung, keine Zyklen oder Widersprüche), Person ohne Foto anlegen (Validierung, `people()`,
Personengraph), Layout (keine Überlappung, deterministisch, endliche Koordinaten, über 1000 Personen
unter der Zeitgrenze). Sichtprüfung am 08.11.2026 (Debug-Build, synthetische Familie aus
`examples/demo_faces.rs`, `FOTO_IMPORTER_FACE_MODE=tree`, `FOTO_IMPORTER_TREE_PERSON=<ID von Mia>`,
`FOTO_IMPORTER_TREE_UP`, `FOTO_IMPORTER_TREE_DOWN`): [Stammbaum](screenshots/stammbaum.png),
[Rückfragen](screenshots/personen-rueckfragen.png). Nicht geprüft: Hell-Design, Bedienung per
Maus (Klick auf Personen, Auswahlfelder, „Person anlegen“), Screenreader, Sammlungen mit
echten Familien, Layout bei Pedigree-Collapse (Ahnenschwund) in der Zeichnung.

## Personentabelle und Tippfilter (ADR 0063)

Regressionen: reine Sortier- und Filterlogik (stabile Mehrspaltensortierung, Umlaute und Umschrift,
leere Werte zuletzt, Präfix vor Wortanfang vor Teiltreffer, Spaltenauswahl nach Breite), eine
Datenbankprüfung der Zählungen (bestätigte Fotos/Gesichter, Datumsbereich, Beziehungen, Familie),
UI-Tests im Kittest-Stil: Tabelle und Detailspalte werden bei 980 × 680, 1280 × 800 und 1920 × 1080
nicht abgeschnitten und überlappen nicht, die Detailspalte läuft nicht über; der Filter reduziert die
Zeilen; Auswahl und Sortierung überstehen neu geladene Daten, eine verschwundene Person verwirft die
Auswahl; ungespeicherte Eingaben im Detail bleiben bei Neuladen erhalten; das Auswahl-Popup bleibt offen.
Für Sichtprüfungen (nur synthetische Daten, `cargo run --example demo_faces`):
`FOTO_IMPORTER_FACE_MODE=persons`, `FOTO_IMPORTER_PEOPLE_SELECT=<ID>`,
`FOTO_IMPORTER_PEOPLE_SORT=<name|photos|faces|birth|gender|relations|family|first|last|hints>[:desc]`,
`FOTO_IMPORTER_PEOPLE_POPUP=<Suchtext>` (hält das Auswahlfeld „von …“ in „Beziehung hinzufügen“ offen).

Sichtprüfung am 09.11.2026 (Debug-Build, synthetische Demodaten, 61 Personen, 1280 × 800, dunkles Design,
`FOTO_IMPORTER_PEOPLE_SORT=photos:desc`, Auswahl #1, Popup mit Suchtext „cl“): [Bild](screenshots/personen-tabelle.png).
Sichtbar: Tabelle mit Porträts, Sortierdreieck, Auswahlzeile, Detailspalte mit gefiltertem Auswahlfeld.
Nicht geprüft: 980 × 680 per Bild (nur per Layouttest), Hell-Design, Mausrad, Screenreader. Die Demodaten
enthalten weder Geburtsjahre noch Aufnahmedaten, daher zeigen diese Spalten dort „–“ (per Test mit Daten abgedeckt).
## Zentrale Beziehungen (ADR 0065)

Regressionen (`tests/central_relations.rs`, offline, `tempfile`): zwei Sammlungen und eine Zentraldatei;
Beziehung in A erscheint nach dem Abgleich in B, zweiter Abgleich ohne Dublette; Löschen in A
schreibt einen Grabstein, B behält seine Kante und meldet sie, nichts kehrt zentral zurück, Neuanlegen
hebt den Grabstein auf; Zyklus über mehrere Personen und Eltern-Kante in Gegenrichtung werden nicht
angelegt, sondern gemeldet (Wahl „zentral übernehmen“ scheitert dort ohne Änderung); die drei Wahlen
„Lokal behalten und zentral ersetzen“, „Zentral übernehmen“, „Keine von beiden“; fehlende Person wird
übersprungen und beim Übernehmen nachgezogen; Entscheidungen beider Sammlungen vereinigt;
Zusammenführen mitten im Baum (zentral und in einer frischen dritten Sammlung); Migration einer
Version-1-Datei mit Personen und Referenzausschnitten (Sicherung `*.v1.bak`, idempotent); neuere Version
abgelehnt ohne Änderung; gesperrte und schreibgeschützte Datei; Parallelität (zwei Sammlungen, vier
Schreiber-Threads); 5.000 Personen und rund 10.000 Beziehungen (Debug-Build: hoch ca. 1,1 s, herunter
inkl. Personen ca. 0,6 s, Leerlauf ca. 0,5 s); Berichtszähler. Zusätzlich ein Gleichheitstest
`Validator` gegen `validate_add` in `src/core/relations.rs`.

Zwei-Sammlungen-Prüfung wiederholen: `cargo test -j 1 --test central_relations -- --nocapture`
(die Zeiten des großen Tests stehen auf der Fehlerausgabe). Sichtprüfung: `cargo run -j 1 --example central_demo`
erzeugt unter `tmp/central-demo` zwei synthetische Sammlungen, eine **temporäre** Zentraldatei im selben Ordner
(`central_path` in den Demo-Einstellungen, nie der echte Bestand im Benutzerprofil) und drei Konflikte in B
(Eltern-Kind gegen zentral Partner, Zyklus, zentral gelöschte Freundschaft). Start:
`FOTO_IMPORTER_SETTINGS=tmp/central-demo/settings.json FOTO_IMPORTER_VIEW=people FOTO_IMPORTER_FACE_MODE=persons
FOTO_IMPORTER_ACTION=central_sync FOTO_IMPORTER_CAPTURE=<Datei> FOTO_IMPORTER_CAPTURE_DELAY=8 cargo run`
(`central_sync` öffnet den Bereich und gleicht einmal ab). Bild vom 09.11.2026 (Debug-Build, 1920 × 1200,
dunkel): [Zentrale Beziehungen](screenshots/zentrale-beziehungen.png): Zähler „Beziehungen: …“, Konfliktgruppe
mit „lokal: Anna ist Mutter von Ben“ / „zentral: Anna und Ben sind Partner“ und den drei Knöpfen. Nicht geprüft:
Klick auf die Knöpfe per Maus (Kernlogik per Test), Hell-Design, 980 × 680, Liste mit vielen Konflikten.

## Indizierung in Stapeln mit Fortschritt (ADR 0064)

Anlass: Ein Ordner mit über 30.000 Bildern zeigte nur „34195 gefunden, 356 s“.
Gemessen wurde mit einem synthetischen Baum (Kunstdaten, niemals private Sammlungen):
30.000 kleine JPG-Dateien in 300 Ordnern, jede zehnte mit NEF-Partner, zusammen 33.000 Dateien.
Windows 11, Release-Build, ein Lauf je Zahl; parallel liefen andere Builds, die Zahlen sind
Größenordnungen, keine Garantien.

| Messung | vorher | nachher |
|---|---|---|
| Verzeichnislauf mit `stat`, ohne Datenbank | 0,6 s | 0,5 s |
| Erster Indexlauf (33.000 Dateien) | **726 s** | **14,4 s** |
| Neuindizierung ohne Änderung | **351 s** | **8,2 s** |

Kleinere Läufe des alten Stands: 3.300 Dateien 30–37 s, 6.600 Dateien 75 s.

Kostentreiber je neuer Datei im alten Lauf (Einzelmessung auf der gefüllten Sammlung, 33.000 Zeilen):

| Teil | Kosten je Datei |
|---|---|
| `File::open` (Lesen) nur für die Datei-ID; der Virenscanner prüft jede erste Öffnung | ≈ 9 ms (`stat`: 0,04 ms) |
| Partner gleicher Dateiname über `casefold(stem)` (Vollscan der Tabelle `files`) | 13 ms |
| Verschiebungskandidaten über Inode+Größe / Größe+Hash (zwei Vollscans) | 2,4 ms / 2,1 ms |
| Einfügen von Foto und Datei einschließlich Triggern | 0,17–0,28 ms |
| Davon Trigger (`browse_files_insert`; 30.000 Einfügungen im Speicher mit / ohne Trigger) | 2,6 s / 0,3 s gesamt |
| Vollständiges `UPDATE` einer unveränderten Zeile / nur `last_seen_scan` | 0,22 ms / 0,005 ms |

Der alte Lauf war **eine einzige** Transaktion über alle Dateien; Trigger sind nicht der
Haupttreiber (≈ 3–5 s von 726 s). Behoben wurde: Attributzugriff statt Lesezugriff für die
Datei-ID (≈ 9 ms → 0,04 ms), Partner- und Verschiebungsabfragen einmal je Lauf im Speicher
(`core::known_files`) statt Vollscans, unveränderte Zeilen nur mit Besuchsmarke
(`UPDATE` ohne Trigger), Stapel-Commits. Die Trigger selbst bleiben (Schema 6, ADR 0042);
sie kosten nach den Behebungen ≈ 0,15 ms je Datei, bei 30.000 Dateien ≈ 4–5 s.

Stapelgröße (11.000 Dateien, erster Lauf): 1 Datei je Commit 30,4 s, 10: 6,5 s, 100: 2,8 s,
**500: 2,4 s**, 2.000: 2,3 s, 10.000: 2,3 s, alles in einer Transaktion: 2,3 s. Ein Commit kostet
≈ 2,7 ms; 500 Dateien halten den Mehraufwand unter 6 %, die ersten Dateien sind nach
wenigen Zehntelsekunden sichtbar, und ein Abbruch verliert höchstens 500 Einträge.

### Benchmark wiederholen

```
cargo run --release --example scan_bench -- 30000 300
```

Erzeugt den Baum in einem temporären Ordner (`SCAN_BENCH_DIR` setzt einen eigenen Ordner),
misst Verzeichnislauf, ersten und zweiten Indexlauf mit Zeiten je Phase, die Einzelkosten
(Abfragen, Öffnen, Einfügen, Trigger) und das Sichern. `SCAN_BENCH_BATCHES=1,100,500,5000`
vergleicht Stapelgrößen. `scan_bench dump <Sammlung>` gibt Dateien und Fotos zum Vergleichen
zweier Läufe aus. Die Zahl „erste Öffnung 9 ms“ hängt vom Virenscanner ab; auf einem
Rechner ohne Echtzeitscan fällt dieser Anteil weg.

### Regressionen

`tests/scan_batches.rs`: gleiche Dateien, Foto-Zuordnungen (NEF/JPG-Paare, Unterordner `nef`,
Export-Kopien, gleichnamige Dateien anderer Ordner) und gleiche Projektion für alle Stapelgrößen von
1 bis zu einer einzigen Transaktion; ein Leser sieht freigegebene Stapel während des Laufs;
Abbruch behält die Stapel, markiert nichts als fehlend und schließt den Scan mit `cancelled`;
der nächste Lauf setzt fort, behält die IDs und liefert dieselben Dateien wie ein
ununterbrochener Lauf (verschwundene Dateien markiert erst der vollständige Lauf als fehlend);
eine unveränderte Neuindizierung ändert weder Zeilen noch Projektion noch Revision; Umbenennen
behält Datei- und Foto-ID. `tests/progress.rs`: Rate/Restzeit erst nach genügend Messpunkten,
Glättung, Neustart der Zählung, „Gesamtzahl unbekannt“ ohne Prozent, Prozent nie 100 vor
Abschluss, deutsche Zahlen- und Zeitformate, Phasen. Zusätzlich wurde auf einem Baum mit 2.200
Dateien ein Lauf des alten und des neuen Programms verglichen: die Tabellen `files` und `photos`
sind identisch (Textvergleich der Ausgabe von `scan_bench dump`).

### Sichtprüfung

Release-Build, synthetischer Baum mit 33.000 Dateien unter `tmp/scan-demo`
(`FOTO_IMPORTER_SETTINGS`, `FOTO_IMPORTER_VIEW`, `FOTO_IMPORTER_ACTION=scan` startet den Lauf
beim Öffnen, `FOTO_IMPORTER_CAPTURE` mit `FOTO_IMPORTER_CAPTURE_DELAY=6`):
[Aufgabenansicht mitten im Lauf](screenshots/indexierung-aufgaben.png) und
[Sammlung während des Laufs](screenshots/indexierung-sammlung.png) (die Galerie zeigt bereits
12.700 der 22.085 eingetragenen Dateien; Fußleiste mit Balken, Prozent, Rate und Restzeit).
Nach dem Lauf nennt die Fußleiste „Metadaten: 0 von 33.000 gelesen“. Nicht geprüft:
Abbrechen per Mausklick in der Oberfläche (nur der Kern-Test), Hell-Design, sehr langsame
Netzlaufwerke, ein Lauf mit echter ExifTool-Anreicherung und die Automatik „Bei automatischen
Läufen zuerst Metadaten lesen“ in der Oberfläche, Rechner ohne Virenscanner.
Beim Start mit einem noch nicht indizierten Ordner melden die ersten lesenden Abfragen
„unable to open database file“ (vorbestehendes Verhalten, nicht Teil dieser Änderung).

## Gesichtsgruppen speichern (ADR 0066)

Synthetische Sammlung (nie private Daten): 90.000 Fotos, 200.000 Gesichter, 40 Personen, 15.000
bestätigt, 20.000 automatisch/vorgeschlagen, 12.030 Gruppen, 5.680 Referenzen, trainiertes Modell.

Wiederholen (Release; die Sammlung liegt unter `tmp/`, die Messung arbeitet auf einer Kopie):

```text
cargo run -j 1 --release --example face_save_bench -- gen tmp/bench-faces
cargo run -j 1 --release --example face_save_bench -- run tmp/bench-faces tmp/bench-faces-work 20
cargo run -j 1 --release --example face_save_bench -- plans tmp/bench-faces
```

Messwerte in ms (Release, ein Rechner, Werte schwanken um etwa 20 %):

| Schritt | Vorher | Nachher |
|---|---|---|
| `name_group` (28 Gesichter) | 20 bis 50 | 13 bis 25 |
| Nachladen `page(unknown)` nach dem Speichern | 5.700 bis 5.950 | 180 bis 260 |
| `page(review)` | 950 | 70 bis 80 |
| `page(confirmed)` | 510 | 75 |
| `page(ignored)` | 5.200 bis 5.700 | 180 |
| `page(persons)` | 930 bis 1.380 | 120 |
| `status()` | 240 bis 300 | 53 |
| `people()` | 150 bis 213 | 1 |
| Vorschläge einer Gruppe | 700 | 40 |
| Öffnen schreibend (erstmalig, baut 3 Zusatzindizes) | 9 | 246 (danach 5) |
| Wartung: Fotos im Abgleich in 20 s | 22 | 2.190 bis 2.576 |
| Abbruch der Wartung bis Rückkehr | 20 bis 170 | 19 |

Kostentreiber vorher: (1) `ranked` je Gesicht mit Neulesen aller Referenzen (24 mal je Seite),
(2) Fensterfunktionen und Zählungen über alle Gesichter der Reiter, (3) `status()` und `people()`
mit Vollscan, (4) kalter Seitencache je lesender Verbindung (1,4 s für das erste Lesen), (5) der
Speicherauftrag wartete als Hintergrundauftrag auf laufende Lesevorgänge. Das Speichern selbst,
das Öffnen und Trigger waren unauffällig. Debug-Build nachher: Nachladen 1.170 bis 1.270 ms,
`name_group` 23 bis 37 ms; ein Debug-Vorher-Wert wurde nicht gemessen. Im Debug-Build kehrt die
Wartung erst nach etwa 2,6 s zurück (das einmalige Laden der Referenzen ist nicht abbrechbar,
Release 19 ms).

Tests: `tests/face_save_performance.rs` (Vorschläge bitgleich zur bisherigen Abfrage für alle
Gesichter, Ablehnungen/Foto-Personen, Seiten und Zähler gleich den bisherigen Fensterabfragen,
Vorschläge nach Bestätigung/Ablehnung/Ignorieren ohne veralteten Zwischenspeicher, Zusatzindizes
idempotent und schreibgeschützt wirkungslos), `src/ui/app/tests.rs` (Nutzerschreibaufträge nicht als
Hintergrundauftrag). Nicht geprüft: echte Modelle, echte Sammlungen, Netzlaufwerke (mmap), ein
Testfall mit absichtlich langem Wartungsauftrag in der Oberfläche (nur der Kern-Abbruch im
Benchmark), Benchmarks der zentralen Synchronisierung.

## Personenseite laden (ADR 0068)

Zwei Messreihen, beide ausschließlich synthetisch (nie private Daten):

```text
cargo run -j 1 --release --example face_save_bench -- gen tmp/bench-faces
cargo run -j 1 --release --example face_save_bench -- run tmp/bench-faces tmp/bench-faces-work 1
cargo run -j 1 --release --example face_thumb_bench -- tmp/bench-thumbs 24 6000 4000 4
```

Gesichtskacheln (24 synthetische 6000x4000-JPEGs mit Rauschen, 160-px-Kachel, Release, 16 Kerne;
Rauschen macht die Dekodierung eher teurer als bei echten Fotos):

| Schritt | Vorher | Nachher |
|---|---|---|
| Kachel ohne Zwischenspeicher, ein Thread (Median) | 255 bis 300 ms | unverändert (Erstansicht) |
| 24 Kacheln erstmals, ein Thread | 6.500 bis 6.900 ms | 6.400 ms |
| 24 Kacheln erstmals, vier Arbeiter | (ein Arbeiter) 6.500 ms | 2.100 bis 2.400 ms |
| Kachel aus dem Zwischenspeicher (Median) | nicht vorhanden (jede Ansicht 255 ms) | 0,4 ms (24 Kacheln: 11 ms) |

Datenbankseite (synthetische Sammlung wie ADR 0066, 90.000 Fotos, 200.000 Gesichter): Der Engpass
der Personenseite lag nicht in SQL. `page(unknown)` 180 bis 280 ms, `page(review)` 70 bis 125 ms,
`page(confirmed)` 75 bis 135 ms, `page(ignored)` 180 bis 240 ms (Messung vor der Änderung). Einziger
sauber vergleichbarer Gewinn: die Zählung „analysierte Fotos" in `status()` von 182 ms (Scan über
alle Fotos) auf 0,1 ms durch `ix_jobs_detect_done`. Die Wiederholung nach der Änderung lief auf
einem stark ausgelasteten Rechner (andere Builds, Datenträger fast voll) und streut zwischen 60 und
500 ms je Seite; sie ist deshalb nicht als Vorher/Nachher-Vergleich belastbar.

Sofortiges Bestätigen: `queue_hiding` blendet die Gruppe im selben Aufruf aus (Test: zwei
vorgemerkte Aufträge, Seite sofort ohne die Gesichter, unter 100 ms ohne auf die Aufträge zu
warten; Ausblenden endet mit dem Auftrag auch bei Fehler). Die Dauer bis zum Neuzeichnen in der
echten Oberfläche wurde nicht gemessen.

Tests: Kachel-Cache in `src/core/previews.rs` (erstes gleich zweites Ergebnis, Invalidierung durch
Änderung von Quelle/Ausschnitt, beschädigte Kachel, Original unverändert), Löschen der Kacheln in
`tests/maintenance.rs`, neuer Zusatzindex im bestehenden Idempotenztest (`tests/face_save_performance.rs`),
Ausblenden in `src/ui/app/tests.rs`. Nicht geprüft: echte Fotos/RAW, echte Modelle, Netzlaufwerke,
echte Oberfläche mit mehreren Aufträgen nacheinander, Verhalten bei vollem Datenträger.

## Ordnerbaum aus `folder_counts` (2026-11-17)

Anlass: Auf `E:\Bilder` (USB-Festplatte, 98.082 Dateien, 64.139 Fotos, 697 Ordner) brauchte der
Ordnerbaum über `ix_files_dir_cover` kalt 16,6 s (warm 0,02 s), nur über `metadata_photos` kalt
33,7 s: Tausende verstreute Seitenzugriffe. Lesend geprüft (ohne Schreibzugriff): Die Ordnerformel
`rtrim(rtrim(relpath,replace(relpath,'/','')),'/')` stimmt bei allen 98.082 Dateien und allen
64.139 bevorzugten Varianten mit `files.dir` überein; keine verwaiste Projektionszeile.

Synthetisch (Release, SSD, 100.000 Fotos in 60.000 Ordnern, Werte schwanken um etwa 20 %):

```text
cargo run --release --example folder_bench -- tmp/bench-folders 100000 20000
```

| Schritt | Zeit |
|---|---|
| Join über `ix_files_directory_name` | 630 bis 770 ms |
| Join über `ix_files_dir_cover` | 85 bis 115 ms |
| nur `metadata_photos` | 155 bis 190 ms |
| `folder_counts` lesen (529 Seiten) | 12 bis 19 ms |
| Erstbefüllung beim schreibenden Öffnen | 160 bis 180 ms |
| Schnellscan 10.000 neue Dateien, mit / ohne Trigger | 1,13 / 1,10 s (+3 bis 9 %) |
| 10.000 Metadaten-Updates (Meta einfügen, `meta_state='done'`), mit / ohne | 2,6 / 1,9 bis 2,1 s (+25 bis 38 %) |
| Neuaufbau `metadata_photos` (Migration), Trigger je Zeile / ausgesetzt | 1,87 s / 0,77 s |

Ein Metadaten-Update ersetzt die Projektionszeile mehrmals (Meta- und Datei-Trigger), jede
Ersetzung kostet ein −1 und ein +1; das sind etwa 50 bis 70 µs je Datei, klein gegen das
Lesen mit ExifTool. Mit 697 statt 60.000 Ordnern ist die Tabelle auf `E:\Bilder` nur wenige
Seiten groß. Die Migration setzt die Trigger aus und zählt danach in einer Anweisung.

Tests: `tests/folders.rs` (Tabelle gleich Join über `files.dir` und gleich Zählung über
`relpath` nach Einfügen, Metadaten einfügen/ändern/löschen, Verschieben, Fehlen, Löschen,
Zusammenführen, direktem `relpath`-Update und Neuaufbau einer beschädigten Tabelle; Rückfall
ohne Tabelle beim lesenden Öffnen, Anlage beim schreibenden Öffnen, Neuaufbau bei fehlendem
Trigger), `src/ui/app/tests.rs` (Bestandsänderung bricht laufendes Lesen nicht ab, genau ein
Nachlesen). Nicht gemessen: kalter Lesestart von `folder_counts` auf `E:\Bilder` (die Sammlung
wurde nicht schreibend geöffnet), Dauer der Erstbefüllung auf der USB-Festplatte (erwartet in
der Größenordnung des kalten Scans über `metadata_photos`, einmalig).

## Gesichter bei 100.000 Gesichtern (2026-11-18)

Synthetisch (Release, SSD, 45.000 Fotos, 100.000 Gesichter, 40 Personen, 7.500 bestätigt,
5.589 Trainingszeilen, 6.002 Gruppen):

```text
cargo run --release --example face_save_bench -- gen tmp/bench100k 45000
cargo run --release --example face_save_bench -- steps tmp/bench100k tmp/steps ohne-statistik
cargo run --release --example face_save_bench -- run tmp/bench100k tmp/bench100k-work 900
```

| Schritt | vorher | nachher |
|---|---|---|
| Vollständiger Abgleich (`rematch`), Ruhestand | 131 bis 139 s | 6,6 s |
| Vollständiger Abgleich, erster Lauf mit vielen Änderungen | 139 s | 16 s |
| `References::load` (ohne Statistik) | 75 ms | 53 ms |
| `page(unknown)` / `page(persons)` | 160 bis 240 / 76 bis 142 ms | 184 / 119 ms |
| Speichern einer Gruppe (`name_group`) | 12 bis 17 ms | unverändert |
| Wartung nach vier Bestätigungen (`run`): Abgleich / gesamt ohne Altlast | 137 s / 139 s | 16,8 s / 19,3 s |
| `merge_small_groups` | 0,86 bis 1,5 s | unverändert |
| `prepare_groups`, 2.000 Gesichter | 2,8 bis 6,6 s | unverändert |
| Personentabelle, Zeitreise (größte Person), Porträts | 15 / 11 / 90 ms | unverändert |

Die erste `run`-Messung vorher brauchte zusätzlich 163 s für `prepare_groups`, weil der
Generator 80.000 veraltete `face_group_dirty`-Zeilen hinterließ (jetzt behoben); das
entspricht dem Gruppieren nach einer großen Analyse (rund 2 ms je Gesicht), nicht dem Alltag.

Ergebnisgleichheit: Nach zwei vollständigen Läufen stimmen Person, Art und Ähnlichkeit aller
100.000 Gesichter mit dem bisherigen Code überein (bitgleich, `tmp/assign_old.txt` gegen die
neue Arbeitskopie). Regressionen: `recognition::tests::block_ranking_is_bit_identical_to_the_single_comparison`
(Blockrechnung gegen den Einzelvergleich, mit Ausschlüssen, eigenem Gesicht, Gleichständen und
abweichender Länge), `tests/faces.rs` (`rematch_writes_only_changed_assignments`,
`a_change_during_the_rematch_keeps_the_restart`).

Echte Sammlung `E:\Bilder` (USB-Festplatte, 36.590 Gesichter, 8.594 bestätigt, 2.399
Trainingszeilen, keine `sqlite_stat1`), nur lesend mit Python geprüft: Die bisherige
Referenzabfrage lief über `ix_training_model` und prüfte je bestätigtem Gesicht alle
Referenzzeilen: 1,8 s warm, 4,7 s kalt, bei jedem Laden von „Benennen“ und jedem Abgleich.
Die neue Abfrage liefert dieselben 1.916 Zeilen in 19 ms warm, 1,2 s kalt. Kalt teuer bleiben
auf der USB-Festplatte die Statistik der Personentabelle (2,2 s, warm 40 ms) und das Laden der
Gruppen in `merge_small_groups` (2,9 s, warm 115 ms). Nicht gemessen: der neue Abgleich auf
`E:\Bilder` (er schreibt), die laufende App.

## Inkrementeller Abgleich bei 100.000 Gesichtern (2026-11-18)

Synthetisch wie oben (Release, SSD, 45.000 Fotos, 100.000 Gesichter, 40 Personen, 128
Dimensionen, 4 Rechenthreads). Je Runde eine Entscheidung über `name_group` (Gruppe
bestätigt, Ablehnung von zwei Gesichtern, eine Bestätigung entfernt), Referenzauswahl,
dann der inkrementelle Abgleich; der vollständige Lauf (`rematch_full`) läuft auf einer
Kopie desselben Stands (`VACUUM INTO`), danach werden Person, Art und Wert aller Gesichter
bitgenau verglichen:

```text
cargo run --release --example face_save_bench -- gen tmp/bench100k 45000
cargo run --release --example face_save_bench -- inkrementell tmp/bench100k tmp/inc ohne-statistik 9
cargo run --release --example face_save_bench -- inkrementell tmp/bench100k tmp/inc 6
```

| Schritt | vorher (vollständig) | nachher (inkrementell) |
|---|---|---|
| Abgleich je Entscheidung, ohne Statistik, 9 Runden | 5,7 bis 7,3 s, Median 6,45 s | 266 bis 569 ms, Median 489 ms |
| Abgleich je Entscheidung, mit `ANALYZE`, 6 Runden | Median 6,9 s | Median 505 ms |
| Lauf ohne Änderung | 6,6 s | 336 ms |
| Erster Lauf nach Programmstart (Zwischenspeicher aufbauen) | – | 2,7 s |

Ergebnisgleichheit: alle 15 Runden ohne Abweichung (100.000 Gesichter je Vergleich). Ein
inkrementeller Lauf nach einer Bestätigung teilt sich grob auf in Lesen von Referenzen und
Gesichtern (rund 140 ms), eine neu gerechnete Personenspalte (rund 100 ms), Entscheidungen
je Foto (rund 110 ms) und Schreiben von 700 bis 950 geänderten Vorschlägen samt
Gruppentriggern (rund 150 ms). Der Zwischenspeicher belegt hier rund 70 MB. Regressionen:
`tests/faces.rs` `incremental_rematch_equals_the_full_rematch` (60 zufällige Änderungen
nach Bestätigung, Ablehnung, entfernter Bestätigung, Referenzausschluss, Referenzauswahl mit
Validierungsteil und Ausreißer, neuem und ersetztem Vektor, Geburtsjahr, Umbenennung mit
Namensgleichstand, Ignorieren, Qualität und historischen Gesichtern; nach jeder Änderung
gegen den vollständigen Lauf auf einer Kopie), `incremental_rematch_notices_a_replaced_vector`,
`block_ranking_is_bit_identical_to_the_single_comparison` (auch die besten zwei ohne
Sortieren). Nicht gemessen: der erste Lauf auf der USB-Festplatte von `E:\Bilder` und der
Abgleich in der laufenden App.

## Personenseite „Gesichter“ und Reiter „Ungeeignet“ (2026-11-19)

Gemessen mit einer Kopie von `E:\Bilder\.myfotomanager` (lesend per SQLite-Sicherung
kopiert, die Sammlung selbst blieb unberührt; danach gelöscht), Release, SFace, 36.590
Gesichter, Person mit den meisten bestätigten Gesichtern („leo“):

```text
cargo run --release --example person_faces_bench -- <Kopie>
cargo run --release --example person_faces_bench -- <Kopie> --abgleich
```

| Schritt | Zeit |
|---|---|
| „Wahrscheinlich leo“ ohne Zwischenspeicher (ein Durchgang über alle Vektoren), erster Lauf | 682 ms |
| dasselbe, Folgeläufe | 99 ms |
| dasselbe aus dem Zwischenspeicher des Abgleichs | 70 bis 79 ms |
| ganze Seite „probable“ (8 Gruppen, mit Zählungen, Personen, Porträt) | 130 bis 150 ms |
| Abgleich, der den Zwischenspeicher aufbaut (nur auf der Kopie) | 496 ms |
| Seite „Ungeeignet“ (7.417 Gesichter, Seite 1) | 100 bis 108 ms |

8.132 unbestätigte Gesichter erreichen für diese Person den Mindestwert 0,40
(`config::PERSON_CANDIDATE_MIN`); beide Rechenwege liefern dieselbe Liste. Regressionen:
`tests/face_pages.rs` (Auswahl und Seiten der ungeeigneten Gesichter, „Kein Gesicht“ in
Zählungen, Gruppen, Abgleich und nach erneuter Analyse, Rücknahme; Rangfolge und
28er-Gruppen der wahrscheinlichen Gesichter mit Ausschlüssen, gleiches Ergebnis vor und
nach dem Abgleich; Bestätigen und Entfernen auf der Personenseite) und in
`src/ui/app/tests.rs` die Karten beider Seiten. Nicht geprüft: Sichtprüfung in der laufenden
App, Zeiten auf der USB-Festplatte selbst.

## Umbenannte Ordner und fehlende Dateien (2026-11-20)

Nur lesend an `E:\Bilder` (exFAT, USB-Festplatte, 98 082 Dateien): Nach der Umbenennung
`260822_Hochzeit_Tag_2` → `260822_Geburtstag_Martin` (177 Dateien) haben die Dateien am neuen
Ort dieselbe Datei-ID (`GetFileInformationByHandle`, Stichprobe 2 Dateien), Größe und
Änderungszeit wie die gespeicherten Zeilen; Hashes sind nicht gespeichert. Ein
Bestandslauf ordnet sie damit über Datei-ID und Größe den alten Zeilen zu. Nicht ausgeführt:
der Bestandslauf selbst auf `E:\Bilder` (Sammlung blieb unverändert). Regressionen in
`tests/missing_files.rs` (Umbenennung mit Datei-ID und ohne über Name/Größe/Zeit,
mehrdeutige Fälle, Metadatenblock mit fehlender und mit unlesbarer Datei, Entfernen und
Neu-Verknüpfen) und in `src/ui/app/tests.rs` (nicht verbundene Quelle beim Start, Dialog
„Fehlende Dateien“). Der Gesichtslauf mit fehlender Datei ist ohne Modelle nicht offline
getestet.

## Datensicherung (2026-11-20)

`cargo run --release --example backup_bench -- tmp/backup-bench 800`: synthetische
Sammlungsdatenbank 804 MB (Gesichtsvektoren 128 × float32, Metadaten-JSON, Einstellungszeilen)
auf der lokalen SSD. `backup::create` mit Stufe 1: 11,0 s und 11,1 s (Online-Kopie 3,6 bis
4,0 s, Packen 7,2 bis 7,4 s), ZIP 480 MB (60 %). Nur Packen der Datei: Stufe 1 7,1 s / 480 MB,
Stufe 6 16,8 s / 416 MB, Stufe 9 23,9 s / 395 MB. Zwischenbedarf auf dem Laufwerk der
Sammlung: Kopie der Datenbank plus ZIP. Nicht gemessen: echte Sammlung auf der
USB-Festplatte (`E:\Bilder`, rund 824 MB; dort bestimmt der Datenträger die Dauer, also
Lesen 824 MB plus Schreiben von etwa 1,3 GB) und der Packgrad echter Gesichtsdaten.
Regressionen in `tests/backup.rs` (Auspacken mit Prüfsummen, `PRAGMA integrity_check` und
Inhalt aller drei Teile, Nummern ohne Ersetzen, Aufräumen nur passender Namen, Abbruch ohne
Reste, gleichzeitiger Schreiber), `tests/migrations.rs` (Sicherung mit altem Schemastand vor
der Migration, Löschen bei „Biometrie entfernen“) und `tests/missing_files.rs` (Sicherung vor
„Einträge entfernen“).

## Erster Blick auf die Sammlung (2026-11-21)

Diagnose-Logs `%APPDATA%\FotoImporter\logs` (Build `main·6b1c543` und älter, Sammlung
`E:\Bilder`, USB-Festplatte): Der Startauftrag „Sammlung durchsuchen“ dauerte 14,7 s, 27,4 s
und 34,1 s, bevor die Galerie Zeilen hatte; parallel lief „Personen und Gesichter lesen“
(20,9 s bis 32,1 s) auf derselben Platte. Galerievorschauen dekodierten jedes Original von
`E:` (JPEG um 1 s, DNG über ExifTool 1,3 bis 2,1 s je Foto im Log), ohne Speicher über den
Programmlauf hinaus.

Nur lesend an `E:\Bilder\.myfotomanager` (1,39 GB, 99 134 Dateien, 57 512 Fotos; Python
`sqlite3`, `mode=ro`, `mmap_size` 512 MB wie die App; die laufende App hielt die Datei offen).
Erste Messung nach längerer Pause (kalt, soweit der Dateicache es erlaubt), zweite direkt
danach (warm):

| Abfrage | kalt | warm |
|---|---|---|
| Erste Seite (48 Fotos, Datumsindex, `face_sources`, `meta`) | 1,1 s | 5 bis 13 ms |
| Gesamtzahl mit Formatfilter (Scan `metadata_photos`) | 24,9 s | 24 ms |
| Kameras (Index `ix_browse_camera` plus Zeilen) | 16,2 s | 22 ms |
| `status()` Fotos (`ix_browse_year`) | 6,8 s | 3 ms |
| `status()` Dateien vorhanden (Scan `files`) | 6,5 s | 22 ms |
| `status()` übrige drei Zählungen | je 15 ms (Seiten schon warm) | |
| Ordnerübersicht `folder_counts` | 0,2 s | 0,5 ms |

Die Zählungen lesen verstreute Seiten (Trigger-Pflege über Jahre); kalt bestimmt die
Kopfbewegung die Dauer. Daraus: Seite zuerst, Zählung danach und nur bei geändertem Stand
(ADR Sammlungsansicht). Geschätzt, nicht in der App auf `E:` gemessen: Galerie nach rund
1,5 s (Öffnen 0,2 bis 0,3 s laut Log plus Seite 1,1 s) statt 15 bis 34 s; mit gespeicherter
Startansicht sofort mit dem ersten Frame. Bei unverändertem Stand entfällt die Zählung ganz.

Startansicht laden (Test `the_page_comes_first_and_the_saved_start_view_shows_before_any_read`,
Debug-Build, lokale SSD): 0,26 ms für drei Fotos; eine volle Seite ist rund 20 KB JSON.

`cargo run --release --example thumb_cache_bench -- <Ordner> <Cache> 12`, Originale nur
gelesen von `E:\Bilder`, Cache auf der lokalen SSD, ein Thread: 2012er JPEGs ohne Treffer
218 ms, mit Treffer 1,0 ms je Foto; 24-MP-JPEGs (2023) ohne Treffer 283 ms, mit Treffer
1,8 ms; Eintrag im Mittel 23 KB. Nicht gemessen: RAW/DNG-Vorschauen (ExifTool) mit Cache und
die ganze App gegen `E:\Bilder` (wird dort nicht gestartet). Regressionen:
`core::start_view` (Wiederverwendung nur bei gleichem Stand, gleicher Suche und gleicher
erster Seite; Filter auf `meta` und Gesichter nie), `core::thumb_cache` (Schlüssel folgt dem
Original, kaputte Einträge, Aufräumen nur eigener Dateien), `src/ui/app/tests.rs` (Seite vor
Zählung, Startansicht vor jedem Lesen, Personen erst nach der Sammlung).
