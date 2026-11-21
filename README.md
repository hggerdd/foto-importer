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
* **Sammlung wählen:** einmal links in der Seitenleiste; **Wechseln …** öffnet einen
  anderen Ordner.
* **Sammlung:** im Raster suchen und rechts Foto samt
  Metadaten prüfen. **Bestand** enthält Aktualisierung, Metadatenimport und
  Integritätsprüfung. **Filter** öffnet die kombinierten Suchkriterien.
  Die Suchzeile findet auch bestätigte Personennamen. **Gesichter** und
  **Personen** in den Bildmetadaten lassen sich als Suchkriterien übernehmen.
  Unter **Filter** kann man die Anzahl erkannter Gesichter mit Datum,
  Kamera oder weiteren Kriterien kombinieren.
  Für **Nur diese Personen · kein weiteres erkanntes Gesicht** wähle
  die gewünschte Person unter **Alle diese Personen**. Nach einem Wechsel
  der Erkennungsregel startet **Gesichter → Fotos analysieren** die nötige
  Neuanalyse. Auch schwache, unbenannte Treffer zählen bei der Suche.
* **Gesichter:** Fotos analysieren, Gruppen benennen oder ignorieren, Vorschläge
  prüfen; **Erledigt** zeigt Bestätigte und Ignorierte. Abgewählte
  Gruppenmitglieder werden für den Namen abgelehnt. Rücknahme schützt spätere
  Nutzerentscheidungen. Die Statuszeile nennt den Stand und den nächsten Schritt.
  **Automatik & Werkzeuge** zeigt Fotoanalyse, Referenzauswahl nach Bestätigungen
  und Abgleich für Namensvorschläge sowie den Zeitplan und enthält Einrichtung,
  Export, Validierung und die ausdrücklich bestätigte Biometrie-Bereinigung.
  Nach dem Speichern rückt die nächste offene Gruppe an die bisherige
  Bildschirmposition, soweit die Liste genug Platz zum Scrollen bietet.
  Namen am Bild lassen sich anklicken; Details bieten Löschen und Referenzaktionen.
* **Personen:** Die Übersicht bietet Bearbeitung, Fotosuche, Zusammenführung
  und Familienbeziehungen. **Stammbaum**, **Graph** und **Zeitreise** zeigen die
  in der Tabelle gewählte Person. Der Graph zeigt gemeinsame Fotos: dickere Linien
  stehen für mehr gemeinsame Aufnahmen, gestrichelte für eingetragene
  Beziehungen ohne gemeinsames Foto.
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
Bekannte Schema-Stände 1–5 werden beim ersten Schreibzugriff nach einer vorherigen
Datensicherung auf Schema 6 aktualisiert. IDs und Nutzerentscheidungen bleiben
erhalten. Unvollständige Pflicht-Quelltabellen und unbekannte Versionen werden
abgewiesen. Schema 4/5 kann zunächst lesend geöffnet werden.

Arbeitsplatzeinstellungen: `%APPDATA%\FotoImporter\settings.json`, atomar
geschrieben. Für isolierte Tests überschreibt `FOTO_IMPORTER_SETTINGS` den Pfad.
Sammlung: `.myfotomanager`, Anwendungssperre `.myfotomanager.lock`. Fremde Sperren
werden nicht entfernt. Einziger Sicherungsweg ist die Datensicherung
`.myimporter.backup.NNN` (ZIP im Sammlungsordner): von Hand, täglich beim ersten Öffnen
(abschaltbar) und automatisch vor Upgrade, Detektorwechsel, „Neu verknüpfen“ und
„Einträge entfernen“. Ältere Kopien `.myfotomanager.backup` und `.myfotomanager.before-*.bak`
früherer Versionen entstehen nicht mehr und können von Hand gelöscht werden; die
Biometrie-Bereinigung entfernt Gesichtsdaten auch aus ihnen und löscht die
Datensicherungen. Fremde Sicherungskopien außerhalb der Sammlung werden nicht gesucht
oder verändert.

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

[Architektur mit Diagrammen](docs/architektur.md) ·
[ADRs](ADR/README.md) · [Validierung](docs/validierung.md)

Dieses Repository enthält ausschließlich die Rust-Anwendung; der Branch `main` ist der
stabile Stand. Die frühere Python-Version liegt unverändert im Branch `python-legacy`
(Stand 2026-11-15) und lokal in `C:\ttt\foto-importer`; sie ist nur Verhaltensreferenz.
