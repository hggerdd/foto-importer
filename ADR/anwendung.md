# Anwendung und Nebenläufigkeit

## Entscheidung

- **Technik:** Rust 2024, egui/eframe 0.33 (native OpenGL-Laufzeit, Mindest-Rust 1.88),
  rusqlite mit gebündeltem SQLite. `Cargo.lock` ist versioniert. Kein HTTP-Backend, keine
  Cloud, keine GPU-Inferenz.
- **Schichten:** Domainlogik in `src/core`, Oberfläche und UI-Zustand in `src/ui`,
  zentrale Grenzwerte in `src/config.rs`. Die Oberfläche enthält keine SQL- und keine
  Kopierlogik.
- **Worker:** Jede langsame Arbeit (Datenbank, Dateien, Bilddekodierung, Inferenz, Netz,
  Ordnerdialoge) läuft auf Workern. Nur der UI-Thread besitzt Texturen und UI-Zustand.
  Ergebnisse tragen eine Revision; veraltete Antworten werden verworfen.
- **Vorrang:** Interaktive Arbeit geht vor. Kopier- und DNG-Aufträge warten je Bahn in
  Eingabereihenfolge; Hintergrundarbeit prüft an Haltepunkten (`Control::checkpoint`) auf
  Abbruch und gibt nach. Nutzerschreibaufträge (Benennen, Rücknahme, Einzeländerung)
  warten nicht auf Lesevorgänge und stoppen die Hintergrundwartung vor ihrem Start.
- **Ein Schreiber je Sammlung:** Schreibaufträge der App werden je Sammlung nacheinander
  abgearbeitet; Leser nutzen eigene Verbindungen und stehen nicht in dieser Schlange.
- **Fortschritt:** Jeder lange Auftrag meldet benannte Phasen und `progress(done, total)`.
  `total = 0` bedeutet „Gesamtzahl unbekannt“, dann erscheint kein Prozentwert. Restzeit erst
  nach vier Messpunkten; 100 % erst bei vollständiger Arbeit.
- **Portionen:** Analyse und Metadatenlauf arbeiten neueste Fotos zuerst und in wählbaren
  Portionen (100, 500, 2000, 10000, alle; Standard 500), optional auf einen Ordner begrenzt
  (`core::scope`). Bereits fertige Fotos zählen nicht zur Portion.
- **Einstellungen in drei Bereichen:** Arbeitsplatz in `%APPDATA%\FotoImporter\settings.json`
  (atomar geschrieben, `FOTO_IMPORTER_SETTINGS` überschreibt den Pfad), Sammlungsoptionen in
  der Sammlungsdatenbank (`settings`, `face_settings`), Modellbeschreibungen als Manifeste
  unter `models/`.
- **Oberfläche:** Feste Navigation links, ein Arbeitsbereich, Kontext rechts, sichtbare
  Aufgaben. Eine Hauptaktion je Kontext, Leerzustände mit Einstieg, konkrete Fehlermeldungen.
  Ergebnisse gelten erst nach erfolgreichem Speichern als gespeichert.

## Grenzen

Ein laufender einzelner Inferenz- oder Betriebssystemaufruf und eine gestartete
Adobe-Konvertierung lassen sich nicht sofort unterbrechen. Automatik (nach Import, 03:00 Uhr)
läuft nur bei geöffneter App.

## Beim Ändern beachten

Neue teure Arbeit an Worker, Abbruch und Fortschritt anbinden. Neue Grenzwerte gehören in
`config.rs`, nicht in Widgets. Für jede neue Einstellung den Geltungsbereich festlegen.
Fortschrittsereignisse dürfen Eingabefelder und Namensentwürfe nicht neu aufbauen.
