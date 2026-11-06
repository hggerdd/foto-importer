# ADR-0043: Portables Windows-Release-Paket

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0013](0013-konfigurationsbereiche.md), [0035](0035-rust-desktop.md), [0039](0039-rust-onnx-adapter.md)

## Kontext

Ein Start aus Explorer darf Ressourcen nicht vom zufälligen Arbeitsverzeichnis
abhängig machen. Separate Gewichts- und Werkzeuglizenzen verhindern ein pauschales
Veröffentlichen aller lokal installierten Binärdateien im Git-Repository.

## Entscheidung

Eine Windows-EXE mit nativer Oberfläche wird über Cargo Release gebaut. Ein
reproduzierbares PowerShell-Skript erzeugt ein neues, datiertes lokales ZIP mit
EXE, Modellmanifesten, Dokumentation und ADRs. Es überschreibt keine alten Pakete.
ExifTool, Adobe, ONNX Runtime und Gewichte werden nicht automatisch gebündelt.
Die Runtime ist seit [ADR-0044](0044-onnx-runtime-download.md) auf Klick in der App installierbar.
Ressourcen werden zuerst neben der EXE gesucht, beim Entwickeln ersatzweise im
Projektordner. Konfigurierte absolute Pfade behalten Vorrang. App-Einstellungen
bleiben im Benutzerprofil; Fenstergröße wird am Monitor begrenzt wiederhergestellt.

## Alternativen

Ein Installer würde Signatur-, Update- und Verteilungsentscheidungen hinzufügen.
Ein Paket mit sämtlichen Gewichten könnte Nutzungsbeschränkungen unbeabsichtigt
weitergeben. Arbeitsverzeichnis-relative Ressourcen sind beim Explorer-Start fragil.

## Folgen und Grenzen

Import und Standardbildvorschau benötigen kein Python. Optionale Funktionen
benötigen ihre dokumentierten lokalen Werkzeuge. Noch keine Codesignatur,
automatische App-Aktualisierung oder getestete Verteilung auf einem frischen PC.
Der lokale Entwicklungsordner enthält separat kopierte vorhandene Modelle/DLLs;
diese bleiben außerhalb Git und des Standard-ZIP.

## Bei Weiterentwicklung beachten

Lizenzhinweise, Prüfsummen und reproduzierbare Builds erhalten. Signierung oder
Installer als eigene Entscheidung bewerten. Keine Modelle beim Start herunterladen.

## Belege und Validierung

[Buildskript](../scripts/build-release.ps1), [Ressourcen](../src/config.rs),
[Start](../src/main.rs), [Prüfprotokoll](../docs/validierung.md).
