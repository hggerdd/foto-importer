# Prüfprotokoll

2026-11-04, Windows, Rust 1.99.1, lokale synthetische Dateien und temporäre DBs.

* `cargo check --offline`: bestanden.
* `cargo test --offline`: 18 Regressionen bestanden (Import 5, Sammlung/Suche 7,
  Gesichter/Beziehungen 6), keine Modellgewichte oder Adobe erforderlich.
* Native App gestartet; Startansicht und AccessKit-Baum geprüft.
* Strenge Clippy-, Release-, Modell- und weitere UI-Prüfungen folgen im Verlauf.

Dies belegt weder vollständige Python-Parität noch eine Erkennungspräzision.
Produktive/private Fotos oder Sammlungen wurden nicht geändert.
