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
    Commands --> Copy[Importdienst · Kopier- und DNG-Warteschlangen]
    Readers --> Query[Begrenzte SQL-Suche]
    Readers --> Preview[Orientierte Vorschauen · begrenzter Cache]
    Writer --> Index[Inventar / Identitäten / BLAKE3]
    Writer --> Faces[Personen / Gesichter / Referenzen]
    Writer --> Migrations[Sicherung und Schema-Migration 1–5]
    Faces --> ONNX[Lokale CPU-Inferenz · ORT]
    Writer --> Lock[Exklusive Host/PID-Sperrdatei]
    Query --> DB[(SQLite .myfotomanager · Schema 6)]
    Index --> DB
    Faces --> DB
    Migrations --> DB
    Index --> Exif[ExifTool · ausschließlich lesend]
    Copy --> Stage[Temporäre vollständige Kopie]
    Stage --> Target[Freier Zielname ohne Überschreiben]
    Target --> Adobe[Optionale Konvertierung der Zielkopie]
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

Beim Öffnen einer bestehenden Sammlung bleibt die Suche lesend. Erst ein
Schreibauftrag übernimmt die exklusive Sammlungssperre und migriert ältere
Schemas nach einer Sicherung. Hintergrundaufgaben geben interaktiven Aufträgen
Vorrang; neue Suchabfragen können veraltete SQL-Arbeit abbrechen. Nach dem Kopieren
nutzt eine optionale DNG-Konvertierung ihre eigene Warteschlange.

## Grenzen und Zuständigkeiten

* `src/config.rs`: Arbeitsplatzkonfiguration und zentrale Ressourcenlimits.
* `src/core/files.rs`: Quellscan, Namensprüfung, Kopien und Adobe-Prozesse.
* `src/core/import_service.rs`: Vorprüfung, Importablauf und Ergebnisberichte.
* `src/core/dng_updates.rs`: Tagesprüfung der Adobe-Version und lokaler Versionsvergleich.
* `src/core/jobs.rs`: Worker-Lebenszyklus, Abbruch und Fortschritt.
* `src/core/collection.rs`: DB-Sperre, Inventar und Integritätsprüfung.
* `src/core/migrations.rs`: Sicherung, Schemawechsel und Kompatibilitätsprüfung.
* `src/core/metadata.rs`: ExifTool und parametrisierte Suchabfragen.
* `src/core/faces.rs`: Gesichtsaufträge, Gruppen und bestätigte Zuordnungen.
* `src/core/matching.rs` und `recognition.rs`: Referenzauswahl, Vorschläge und Zuordnung.
* `src/core/maintenance.rs`: Personenpflege, Datensatzexport, Kalibrierung und Biometrielöschung.
* `src/core/inference.rs`: geprüfte Modellmanifeste und lokale ONNX-Ausführung.
* `src/core/previews.rs`: Vorschauen ohne dauerhaften Vollbildspeicher.
* `src/ui/`: Darstellung und UI-Zustand; keine SQL- oder Kopierlogik.

Schema, Trigger und Projektionsregeln stammen aus der Referenz. Die vorhandenen
ADRs sind Constraints, keine Behauptung vollständiger Funktionsparität.
Der überprüfte Implementierungsstand steht in [paritaet.md](paritaet.md).
