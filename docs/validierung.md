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
