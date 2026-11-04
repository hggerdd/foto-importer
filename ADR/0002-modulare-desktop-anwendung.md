# ADR-0002: Modulare Python-/Tkinter-Desktop-Anwendung

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0003](0003-ui-thread-und-worker.md)

## Kontext

Die Fotoverwaltung arbeitet direkt mit lokalen Ordnern und externen Werkzeugen.
Import, Sammlung und Gesichtserkennung benötigen eine gemeinsame Desktop-Oberfläche.

## Entscheidung

Python ab 3.13 und Tkinter/ttk bilden die Anwendung. `main.py` startet sie;
`ui/` enthält Widgets und Controller, `core/` Daten-/Dateilogik und Worker,
`config/` zentrale Vorgaben. Abhängigkeiten werden mit `uv` und `uv.lock` verwaltet.
Die Oberfläche ruft lokale Services auf, ohne HTTP-Backend oder Browser-Laufzeit.

## Alternativen

Ein Web-Frontend mit separatem Server oder ein Wechsel des GUI-Frameworks wäre
eine andere Architektur mit zusätzlichem Betriebs-/Migrationsaufwand. Ein monolithisches
Fenstermodul würde wiederverwendbare Dateilogik unnötig an Widgets koppeln.

## Folgen und Grenzen

Installation und Betrieb bleiben lokal. Tkinter verlangt disziplinierte Thread-
und Widget-Verwaltung. Die Trennung ist eine Leitlinie, keine feste Modul-Längengrenze;
einige aktuelle Services sind umfangreich.

## Bei Weiterentwicklung beachten

Domainlogik in `core/` halten, Widgets dünn halten und Zyklen zwischen UI und Core
vermeiden. Ein Framework-/Backendwechsel benötigt einen ablösenden ADR.

## Belege und Validierung

[Start](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/main.py), [Abhängigkeiten](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/pyproject.toml), [Lockfile](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/uv.lock),
[Hauptfenster](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/main_window.py), [FaceService](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_service.py),
[UI-Tests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_main_window.py).


> Rust-Port: Technologieentscheidung abgelöst durch [ADR-0035](0035-rust-desktop.md). Der historische Python-Entscheidungstext bleibt erhalten.
