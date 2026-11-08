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
