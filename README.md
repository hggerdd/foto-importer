# Foto-Importer · Rust

Native lokale Fotoverwaltung in `C:\ttt\foto-importer-rust`, portiert aus
[Python-Referenz 32fbe476](https://github.com/hggerdd/foto-importer/tree/32fbe4761b47f637a79505d783c94a2969ebc599).
Der aktuelle Stand ist eine **experimentelle Portierung**; vollständige
Funktionsgleichheit ist noch nicht erreicht. Siehe [Funktionsmatrix](docs/paritaet.md).

## Starten

Rust-Toolchain für Windows mit MSVC Build Tools installieren, dann im Projekt:

```powershell
cargo run --locked
```

Release bauen: `cargo build --release --locked`. Die Anwendung benötigt für
Import und Standardbildvorschau keine Python-Installation.

* **Importieren:** Quelle wählen, Datumsmethode einstellen, Tage markieren,
  Ziel und einen gültigen Ordnernamen wählen, Zielplan prüfen, sicher importieren.
* **Sammlung:** Wurzelordner wählen, Bestand aktualisieren, optional Metadaten
  lesen, Suchkriterien anwenden. Ein Foto öffnet den Detailbereich.
* **Personen:** lokale Modelle konfigurieren, Fotos analysieren, Gruppen
  benennen oder ignorieren. Abgewählte Gruppenmitglieder werden für den Namen
  abgelehnt. Rücknahme schützt spätere Nutzerentscheidungen.
* **Aufgaben:** Fortschritt, Abbruch und sitzungsbezogener Verlauf.

ExifTool für Sammlungsexif und RAW-Vorschauen, Adobe DNG Converter für optionale
NEF-Konvertierung separat installieren. Werkzeugpfade in Einstellungen wählen.
ONNX Runtime DLL und Modellgewichte müssen lokal vorliegen. JSON-Manifeste in
`models/` dokumentieren Hashes und Lizenzen. Keine Gewichtsdownloads beim Start.
Eine geänderte Runtime-DLL wird erst nach Neustart geladen.

Originale werden ausschließlich gelesen. Vollständige Kopien werden unter freien
Zielnamen veröffentlicht. Vorhandene Zieldateien bleiben erhalten.
Schema-6-Sammlungen sind die aktuelle Kompatibilitätsgrenze; ältere Versionen
zunächst mit der Python-Referenz aktualisieren. Produktive Sammlungskopien erst
nach Prüfung des [Paritätsberichts](docs/paritaet.md) verwenden.

Arbeitsplatzeinstellungen: `%APPDATA%\FotoImporter\settings.json`, atomar
geschrieben. Für isolierte Tests überschreibt `FOTO_IMPORTER_SETTINGS` den Pfad.
Sammlung: `.myfotomanager`, Anwendungssperre `.myfotomanager.lock`, letzte
abgeschlossene Indexsicherung `.myfotomanager.backup`. Fremde Sperren werden nicht entfernt.

## Entwicklung und Dokumentation

```powershell
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

[Drei UX-Konzepte und Kritik](docs/ux-konzepte.md) ·
[Architektur mit Diagrammen](docs/architektur.md) ·
[ADRs](ADR/README.md) · [Validierung](docs/validierung.md)
