# Rust Foto-Importer

Vor Änderungen ADR/README.md und betroffene akzeptierte ADRs lesen.
Quellprojekt C:/ttt/foto-importer ist die Verhaltensreferenz, kein Runtime-Backend.
Neue Features auf einem neuen codex/-Branch beginnen; fokussierte Schritte committen und pushen.
Domainlogik in src/core, Oberfläche in src/ui, zentrale Grenzen in src/config.rs.
Originale niemals verändern oder löschen; bestehende Zieldateien niemals ersetzen.
SQLite-Identitäten und explizite Nutzerentscheidungen erhalten.
Langsame Arbeit und Prozesse ausschließlich auf Workern; Ergebnisse mit Revisionen prüfen.
Prüfungen: cargo fmt --check; cargo clippy --all-targets -- -D warnings; cargo test.
Echte Modelle/Adobe getrennt von Offline-Regressionen validieren; fehlende Parität dokumentieren.

