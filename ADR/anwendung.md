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
- **Metadaten im Hintergrund:** Sind Dateien noch nie gelesen (`meta_state` pending/stale)
  und schreibt nichts anderes, startet die App Aufträge der Art `metadata-bg` („Metadaten im
  Hintergrund lesen“): Portionen zu 512 Dateien in Blöcken zu 32, damit der Haltepunkt
  binnen etwa einer Sekunde greift. Sie haben wie `maintenance` niedrige Priorität
  (zählen nicht als belegt, jeder Nutzerschreibauftrag stoppt sie, danach setzen sie fort)
  und sind pausierbar. Ebenso `index-new`, die Aufnahme importierter Dateien
  (siehe [Import](import.md)); solange sie aussteht, starten `metadata-bg` und `maintenance`
  nicht. Höchstens ein Hintergrundschreiber je Sammlung. Dateien mit Fehler
  werden nicht wiederholt; fehlt ExifTool oder scheitert der Lauf, endet er für die Sitzung
  mit einem Hinweis. Der ausdrückliche Lauf „Metadaten lesen“ nutzt weiter Blöcke zu 128.
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
- **Einstellungen wirken sofort, ohne Speichern-Knopf:** Schalter und Auswahllisten
  speichern beim Ändern; Pfadfelder beim Fokusverlust, mit Enter, beim Verlassen der
  Ansicht und sofort nach „Auswählen …“, nicht bei jedem Tastendruck. Ein Pfad wird vorher
  geprüft (Datei bzw. Ordner mit Modellbeschreibungen vorhanden); ein ungültiger bleibt im
  Feld stehen, erscheint als Fehlerhinweis der Einstellungen und wird nicht gespeichert.
  Folgen nur bei echter Änderung: ein neuer ExifTool-Pfad gibt dem gestoppten
  Hintergrundlauf eine neue Chance, ein neuer Converter liest dessen Version neu, nur eine
  geänderte Modellauswahl schreibt in die Sammlung.
- **Gesichtserkennung einrichten:** Ein Knopf startet nacheinander die vorhandenen
  Aufträge „Laufzeit herunterladen“ (falls die Laufzeit fehlt) und „Modellgewichte
  installieren“ (falls die Standardgewichte fehlen). Der zweite Schritt beginnt nur nach
  erfolgreichem ersten; Fehler oder „Abbrechen“ beenden die Kette. Ist beides vorhanden,
  steht dort „eingerichtet“.
- **Navigation:** links fest, Hauptpunkte Importieren, Sammlung, Gesichter, Personen; unten
  abgesetzt Aufgaben und Einstellungen (`NAV_MAIN`, `NAV_BOTTOM`). Die Zeitreise ist kein
  Hauptpunkt, sondern eine Ansicht der gewählten Person (siehe [Personen](personen.md)).
- **Sammlung einmal wählen:** Die aktive Sammlung steht in der Seitenleiste (Ordnername,
  voller Pfad im Hinweistext) mit dem einzigen Knopf zum Wechseln („Sammlung wechseln …“,
  ohne Sammlung „Sammlung wählen …“). Die Ordnerwahl prüft vor jedem Wechsel
  (`PhotoApp::choose_collection`): Importquelle, Ordner in ihr und Ordner auf einer
  Speicherkarte (ein Pfadteil `DCIM`) werden mit Fehlerhinweis abgelehnt
  (`core::collection::collection_folder_problem`). Ein Ordner, der die Quelle enthält, ist
  erlaubt, denn die Sammlung darf einen Zwischenordner wie `__einzuordnen` als Quelle haben. Eine vorhandene Sammlung öffnet sofort;
  ein Ordner ohne `.myfotomanager` wird erst nach der Rückfrage „Hier eine neue Sammlung
  anlegen?“ zur Sammlung, vorher wird nichts geschrieben. Ansichten ohne Sammlung
  zeigen einen Leerzustand, der dorthin weist. Alles, was beim Wechsel endet oder neu
  gelesen wird (Entwürfe, Filter, Seiten, Auswahl, Rücknahmen, Revisionen), steht in
  `PhotoApp::set_collection`.
- **Gebauter Stand sichtbar:** Unten links steht Git-Branch und Commit des Builds („main ·
  1af9a4e“, „+“ bei ungesicherten Änderungen), damit sich bei Fehlern ein Versionsproblem
  ausschließen lässt. `build.rs` liest beides beim Bauen; ohne Git steht dort „unbekannt“.
- **Oberfläche:** Ein Arbeitsbereich, Kontext rechts, sichtbare
  Aufgaben. Eine Hauptaktion je Kontext, Leerzustände mit Einstieg, konkrete Fehlermeldungen:
  Bekannte Fehler erscheinen als verständlicher Satz, der Originaltext steht im Verlauf.
  Hinweise gehören zur Ansicht, in der die Aktion (bei Aufträgen: der Start) stattfand, und
  erscheinen nur dort. Normale Hinweise verschwinden nach `NOTICE_SECONDS` (15 s), Fehler
  bleiben bis zum Schließen. Ein laufender Auftrag erscheint je Ansicht nur einmal (in der
  Aufgabenansicht als Karte, sonst in der Fußleiste).
  Ergebnisse gelten erst nach erfolgreichem Speichern als gespeichert.

## Grenzen

Ein laufender einzelner Inferenz- oder Betriebssystemaufruf und eine gestartete
Adobe-Konvertierung lassen sich nicht sofort unterbrechen. Automatik (nach Import, 03:00 Uhr)
läuft nur bei geöffneter App.

## Beim Ändern beachten

Neue teure Arbeit an Worker, Abbruch und Fortschritt anbinden. Neuer Zustand, der nur für
eine Sammlung gilt, wird in `set_collection` zurückgesetzt. Ansichten wechseln über
`open_view`, `set_face_mode` und `set_person_mode`, damit die passende Seite gelesen wird
(`page_mode`). Neue Grenzwerte gehören in
`config.rs`, nicht in Widgets. Für jede neue Einstellung den Geltungsbereich festlegen.
Fortschrittsereignisse dürfen Eingabefelder und Namensentwürfe nicht neu aufbauen.
