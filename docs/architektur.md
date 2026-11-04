# Architektur

Die Python-Referenz ist Commit `32fbe4761b47f637a79505d783c94a2969ebc599`.
Rust enthält Domainservices und eine native egui-Oberfläche. Python ist für
den Betrieb nicht erforderlich. Entwicklung und Prüfung erfolgen unter Windows.

```mermaid
flowchart TB
    User[Fotografin / Fotograf] --> UI[egui Arbeitsbereich]
    UI --> Commands[Revisionierte Aufträge]
    Commands --> Readers[Lesende Worker]
    Commands --> Writer[Ein aktiver UI-Schreibauftrag]
    Commands --> Copy[Import / DNG Worker]
    Readers --> Query[Begrenzte SQL-Suche]
    Readers --> Preview[Orientierte Vorschauen · begrenzter Cache]
    Writer --> Index[Inventar / Identitäten / BLAKE3]
    Writer --> Faces[Personen / Gesichter / Referenzen]
    Faces --> ONNX[Lokale CPU-Inferenz · ORT]
    Writer --> Lock[Exklusive Host/PID-Sperrdatei]
    Query --> DB[(SQLite .myfotomanager · Schema 6)]
    Index --> DB
    Faces --> DB
    Index --> Exif[ExifTool · ausschließlich lesend]
    Copy --> Stage[Temporäre vollständige Kopie]
    Stage --> Target[Freier Zielname ohne Überschreiben]
    Copy --> Adobe[Optionaler Adobe DNG Converter]
    Preview --> Originals[Originaldateien · nur lesen]
    ONNX --> Originals
```

```mermaid
sequenceDiagram
    actor Mensch
    participant UI
    participant Worker
    participant Quelle
    participant Ziel
    Mensch->>UI: Quelle und Tage wählen
    UI->>Worker: Scan mit Revision
    Worker->>Quelle: Dateien und Datumswerte lesen
    Worker-->>UI: Vollständige Dateiliste + Warnungen
    Mensch->>UI: Zielplan prüfen und importieren
    UI->>Worker: Unveränderlichen Importplan übergeben
    loop Je ausgewählter Datei
        Worker->>Quelle: Blöcke lesend öffnen
        Worker->>Ziel: Eigene temporäre Datei schreiben
        Worker->>Worker: Abbruch prüfen, Daten synchronisieren
        Worker->>Ziel: Vollständige Datei exklusiv veröffentlichen
        Worker-->>UI: Aggregierter Fortschritt
    end
    Worker-->>UI: Erfolg und einzelne Fehler
```

## Grenzen und Zuständigkeiten

* `src/config.rs`: Arbeitsplatzkonfiguration und zentrale Ressourcenlimits.
* `src/core/files.rs`: Quellscan, Namensprüfung, Kopien und Adobe-Prozesse.
* `src/core/jobs.rs`: Worker-Lebenszyklus, Abbruch und Fortschritt.
* `src/core/collection.rs`: DB-Sperre, Inventar und Integritätsprüfung.
* `src/core/metadata.rs`: ExifTool und parametrisierte Suchabfragen.
* `src/core/faces.rs`: Personenentscheidungen, Gruppen, Beziehungen und Referenzen.
* `src/core/inference.rs`: geprüfte Modellmanifeste und lokale ONNX-Ausführung.
* `src/core/previews.rs`: Vorschauen ohne dauerhaften Vollbildspeicher.
* `src/ui/`: Darstellung und UI-Zustand; keine SQL- oder Kopierlogik.

Schema, Trigger und Projektionsregeln stammen aus der Referenz. Die vorhandenen
ADRs sind Constraints, keine Behauptung vollständiger Funktionsparität.
Der überprüfte Implementierungsstand steht in [paritaet.md](paritaet.md).
