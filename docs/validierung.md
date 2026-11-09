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

Sichtprüfung der Oberfläche (08.11.2026): `cargo run --example demo` und `cargo run --example demo_faces` erzeugen isolierte Kunstsammlungen unter `tmp/`. Mit `FOTO_IMPORTER_SETTINGS`, `FOTO_IMPORTER_VIEW` (collection, people, tasks, settings), `FOTO_IMPORTER_FACE_MODE` (unknown, review, confirmed, ignored, persons) und `FOTO_IMPORTER_CAPTURE` speichert die App nach fünf Sekunden ein Bild des Fensters. Private Sammlungen werden dafür nicht verwendet; Maus-Zustände wie die Hover-Vorschau lassen sich so nicht abbilden.

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
