# ADR-0035: Native Rust-Desktop-Anwendung

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung auf Nutzeranweisung
- **Bezieht sich auf:** [0002](0002-modulare-desktop-anwendung.md), [0003](0003-ui-thread-und-worker.md), [0020](0020-reproduzierbare-validierung.md)
- **Löst ab:** Technologie-/Laufzeitentscheidung 0002 und Tk-spezifischen Teil von 0003 ausschließlich in diesem Rust-Repository.

## Kontext

Der Projektverantwortliche verlangt Rust, eine bessere Oberfläche und eine
wartbare lokale Anwendung. Das Python-Projekt bleibt als Referenz unverändert.

## Entscheidung

Rust 2024, egui/eframe 0.33.3 mit nativer OpenGL-Laufzeit, rusqlite mit gebündeltem
SQLite. Cargo.lock wird versioniert. UI und Core bleiben getrennt, langsame Arbeit
auf Workern. Nur der UI-Thread besitzt Texturen und UI-Zustand. Revisionen verwerfen
alte Resultate. Cargo-Tests ersetzen pytest für Rust; reale Modelltests bleiben opt-in.
Die bekannte 0.33-Linie hält die Mindest-Rustversion 1.88; das ist eine bewusste
Versionierung, keine Behauptung, sie sei die neueste Version.

## Alternativen

Tauri erfordert eine zusätzliche Web-Laufzeit und Frontendsprache. Slint bietet
deklarative UI, würde aber ebenfalls eine weitere UI-Sprache einführen. GTK ist
unter Windows aufwendiger auszuliefern. Egui bietet Rust-Durchgängigkeit, verlangt
aber explizite Gestaltung und eigene Prüfung der Zugänglichkeit.

## Folgen und Grenzen

Kein Python-Backend und kein HTTP-Server. EXIFTool, Adobe und ONNX-Gewichte bleiben
separate lokale Werkzeuge. Native Controls und vollständige Screenreader-Parität
sind nicht zugesichert. Keine pauschale Performancegarantie durch Sprachwechsel.

## Bei Weiterentwicklung beachten

Kopierschutz, stabile IDs und Nutzerlabels aus den Bestands-ADRs erhalten.
Nicht implementierte Abläufe ausdrücklich in der Funktionsmatrix kennzeichnen.

## Belege und Validierung

[Manifest](../Cargo.toml), [Start](../src/main.rs), [Worker](../src/core/jobs.rs),
[Architektur](../docs/architektur.md). Prüfprotokoll unter docs/validierung.md.
