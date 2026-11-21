# Rust Foto-Importer

Vor Änderungen ADR/README.md und den betroffenen Themen-ADR lesen; geänderte Entscheidungen dort umschreiben, keine neuen ADR-Dateien für Details.
Quellprojekt C:/ttt/foto-importer (Python, nur noch als lokaler Ordner; der GitHub-Branch python-legacy wurde gelöscht) ist nur die Verhaltensreferenz, kein Runtime-Backend; dieses Repository enthält ausschließlich Rust (Hauptlinie: main).
Neue Features auf einem neuen codex/-Branch beginnen; fokussierte Schritte committen und pushen.
Domainlogik in src/core, Oberfläche in src/ui, zentrale Grenzen in src/config.rs.
Originale niemals verändern oder löschen; bestehende Zieldateien niemals ersetzen.
SQLite-Identitäten und explizite Nutzerentscheidungen erhalten.
Langsame Arbeit und Prozesse ausschließlich auf Workern; Ergebnisse mit Revisionen prüfen.
Prüfungen: cargo fmt --check; cargo clippy --all-targets -- -D warnings; cargo test.
Echte Modelle/Adobe getrennt von Offline-Regressionen validieren; fehlende Parität dokumentieren.

Unteragenten arbeiten nur im Projektordner: Worktree unter worktrees/<aufgabe>, Build-Ausgaben unter worktrees/.target/<aufgabe>, Zwischendateien unter worktrees/<aufgabe>/tmp (ADR/arbeitsweise.md).
