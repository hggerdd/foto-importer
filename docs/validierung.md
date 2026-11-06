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
