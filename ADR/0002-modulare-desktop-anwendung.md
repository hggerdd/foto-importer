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

[Start](../main.py), [Abhängigkeiten](../pyproject.toml), [Lockfile](../uv.lock),
[Hauptfenster](../ui/main_window.py), [FaceService](../core/face_service.py),
[UI-Tests](../tests/ui/test_main_window.py).
