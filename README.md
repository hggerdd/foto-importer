# Foto-Importer · Rust

Native lokale Fotoverwaltung in `C:\ttt\foto-importer-rust`, portiert aus
[Python-Referenz 32fbe476](https://github.com/hggerdd/foto-importer/tree/32fbe4761b47f637a79505d783c94a2969ebc599).
Die zentralen Abläufe sind nativ implementiert und mit Regressionen abgesichert.
Vollständige Gleichwertigkeit für jede reale Kamera-/Modellkonstellation ist damit
noch nicht nachgewiesen. Der genaue Stand steht in der [Funktionsmatrix](docs/paritaet.md).

## Starten

Rust-Toolchain für Windows mit MSVC Build Tools installieren, dann im Projekt:

```powershell
cargo run --locked
```

Release bauen: `cargo build --release --locked`. Die Anwendung benötigt für
Import und Standardbildvorschau keine Python-Installation.
Direktstart: `target\release\foto-importer.exe`. Ein portables ZIP entsteht mit
`powershell -File scripts\build-release.ps1`; alte Pakete bleiben erhalten.

* **Importieren:** Quelle wählen, Datumsmethode einstellen, Tage markieren,
  Ziel und einen gültigen Ordnernamen wählen, Zielplan prüfen, sicher importieren.
* **Sammlung:** Wurzelordner wählen, im Raster suchen und rechts Foto samt
  Metadaten prüfen. **Bestand** enthält Aktualisierung, Metadatenimport und
  Integritätsprüfung. **Filter** öffnet die kombinierten Suchkriterien.
  Die Suchzeile findet auch bestätigte Personennamen. **Gesichter** und
  **Personen** in den Bildmetadaten lassen sich als Suchkriterien übernehmen.
  Unter **Filter** kann man die Anzahl erkannter Gesichter mit Datum,
  Kamera oder weiteren Kriterien kombinieren.
* **Personen:** lokale Modelle konfigurieren, Fotos analysieren, Gruppen
  benennen oder ignorieren. Abgewählte Gruppenmitglieder werden für den Namen
  abgelehnt. Rücknahme schützt spätere Nutzerentscheidungen.
  Namen am Bild lassen sich anklicken; Details bieten Löschen und Referenzaktionen.
  Unter Personen & Beziehungen stehen Bearbeitung, Fotosuche, Zusammenführung
  und Familienbeziehungen bereit. Erkennungswissen verwalten enthält Export,
  Validierung, opt-in Automatik und ausdrücklich bestätigte Biometrie-Bereinigung.
* **Aufgaben:** Fortschritt, Abbruch und sitzungsbezogener Verlauf.

ExifTool für Sammlungsexif und RAW-Vorschauen, Adobe DNG Converter für optionale
NEF-Konvertierung separat installieren. Werkzeugpfade in Einstellungen wählen.
Für die Gesichtsanalyse unter **Personen** oder **Einstellungen** auf
**ONNX Runtime herunterladen** klicken. Die App prüft das Microsoft-Paket
für Windows x64, legt es im Benutzerprofil ab und speichert den DLL-Pfad.
Falls zuvor eine andere Runtime geladen war, ist ein Neustart nötig.
Anschließend **YuNet und SFace installieren** wählen. Die App verwendet passende
lokale Dateien oder lädt die Gewichte von OpenCV Zoo, prüft sie und speichert
den Modellordner im Benutzerprofil. [Modellanleitung](models/README.md)

Originale werden ausschließlich gelesen. Vollständige Kopien werden unter freien
Zielnamen veröffentlicht. Vorhandene Zieldateien bleiben erhalten.
Bekannte Schema-Stände 1–5 werden beim ersten Schreibzugriff mit einer vorherigen
SQLite-Sicherung auf Schema 6 aktualisiert. IDs und Nutzerentscheidungen bleiben
erhalten. Unvollständige Pflicht-Quelltabellen und unbekannte Versionen werden
abgewiesen. Schema 4/5 kann zunächst lesend geöffnet werden.

Arbeitsplatzeinstellungen: `%APPDATA%\FotoImporter\settings.json`, atomar
geschrieben. Für isolierte Tests überschreibt `FOTO_IMPORTER_SETTINGS` den Pfad.
Sammlung: `.myfotomanager`, Anwendungssperre `.myfotomanager.lock`, letzte
abgeschlossene Indexsicherung `.myfotomanager.backup`. Fremde Sperren werden nicht entfernt.
Vor Upgrades und Detektorwechseln entstehen zusätzliche benannte Sicherungen.
Biometrie-Bereinigung entfernt Gesichtsdaten auch aus diesen bekannten Sicherungen;
fremde Sicherungskopien außerhalb der Sammlung werden nicht gesucht oder verändert.

Rust-Adapter tragen eigene Modellschlüssel. Bestehende Python-Vektoren bleiben
erhalten; für neue Rust-Vorschläge ist eine lokale Analyse erforderlich. Fotos und
Vektoren werden nicht hochgeladen. Die automatische Netzwerkanfrage prüft
täglich den Adobe-DNG-Downloadlink per HEAD und speichert auch Fehler zwischen.
Runtime- und Modellinstallation verbinden sich erst nach Klick mit GitHub.

## Entwicklung und Dokumentation

```powershell
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

[Drei UX-Konzepte und Kritik](docs/ux-konzepte.md) ·
[Architektur mit Diagrammen](docs/architektur.md) ·
[ADRs](ADR/README.md) · [Validierung](docs/validierung.md)

Der stabile Rust-Stand liegt im Branch `rust-main`. Die Python-Version
verwendet im gemeinsamen GitHub-Repository den Branch `main`.
