# ADR-0068: Schnelle Personenseite: Gesichtskachel-Cache, parallele Vorschau, sofortiges Bestätigen

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-13
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0004](0004-vorrang-interaktiver-arbeit.md), [0008](0008-lokale-sqlite-sammlung.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0042](0042-native-bestandsmigrationen.md), [0066](0066-schnelles-speichern-von-gesichtsgruppen.md)

## Kontext

Auf großen Sammlungen (reale Größenordnung: rund 14.000 Gesichter in „Benennen", 3.400 in
„Vorschläge prüfen") lud die Personenseite zu langsam. Gemessen wurde mit der synthetischen
Sammlung aus ADR 0066 (`examples/face_save_bench.rs`, 90.000 Fotos, 200.000 Gesichter) und mit
synthetischen 24-Megapixel-JPEGs (`examples/face_thumb_bench.rs`); nie private Daten.

1. **Datenbankseite** ist nach ADR 0066 nicht mehr der Engpass: `page(unknown)` 180 bis 280 ms,
   `page(review)` 100 ms, `page(confirmed)` 130 ms, Reiterwechsel gleich teuer, weil jede Seite
   `status()` neu liest. Der größte Einzelposten darin war „analysierte Fotos" (Scan über alle
   Fotos, 100 bis 180 ms).
2. **Gesichtskacheln sind der Engpass.** Jede Kachel (160 px) dekodierte das **ganze Foto**
   (Median 250 bis 280 ms bei 6000x4000, RAW zusätzlich ExifTool-Aufruf) und es gab genau
   **einen** Vorschau-Thread. Eine Seite mit bis zu 8 Gruppen zu je 28 Gesichtern kostete
   so Sekunden bis Minuten, und jeder Seiten- oder Reiterwechsel begann von vorn (der
   Texturspeicher im Arbeitsspeicher überlebt keinen Neustart).
3. **Bestätigen** blockierte alle weiteren Entscheidungen, bis der Schreibauftrag fertig war und
   die Seite neu gelesen wurde.

## Entscheidung

- **Gesichtskachel-Zwischenspeicher** (`previews::cached_preview`): Ausschnitt-Anfragen mit
  Sammlungswurzel (nur die Oberfläche; der Trainingsexport nutzt weiter `preview` unverändert)
  werden unter `<Sammlung>/.myfotomanager_tiles/<xx>/<blake3>.tile` abgelegt. Schlüssel:
  relativer Pfad, Dateigröße, Änderungszeit, Ausschnitt, Kantenlänge, Ausrichtung. Ändert sich
  die Quelle, entsteht eine neue Kachel. Format JPEG Q90 (Ausschnitte über den Bildrand mit
  Transparenz: PNG). Auch frisch berechnete Kacheln laufen durch dieselbe Kodierung, damit
  erstes und späteres Anzeigen identisch sind. Schreiben atomar (`persist_noclobber`), nie
  eine vorhandene Datei ersetzen, Fehler (nur lesbarer Datenträger, Verknüpfung statt Ordner)
  werden ignoriert; beschädigte Kacheln werden gelöscht und neu berechnet. Das ist eine **reine
  Ableitung ohne Schemaänderung**, keine Tabelle, kein Versionssprung, keine Migration;
  der Ordner beginnt mit Punkt und wird vom Indizieren übersprungen. „Biometrie löschen"
  entfernt ihn mit (Gesichtsbilder). Originale werden nie verändert.
- **Parallele Vorschau-Arbeiter** (`ui::thumbnails`): `clamp(Kerne, 2, THUMBNAIL_WORKERS=4)`
  statt einem Thread; kurze Anfragewarteschlange, damit sichtbare Kacheln Vorrang behalten. Alles
  bleibt auf Workern, die Oberfläche blockiert nie.
- **Zusatzindex** `ix_jobs_detect_done` auf `face_jobs(model_id,photo_id) WHERE stage='detect'
  AND status='done'` für „analysierte Fotos" in `status()`. Rein additiv wie in ADR 0066
  (`schema.sql` und `migrations::ADDITIVE_INDEXES`, `CREATE INDEX IF NOT EXISTS` beim
  schreibenden Öffnen, schreibgeschütztes Öffnen verändert nichts, kein Versionssprung).
- **Sofortiges Bestätigen** (`ui/app/optimistic.rs`): „Gesichtsgruppe speichern" und
  „Gesicht benennen" rufen `queue_hiding` auf. Die Gesichter verschwinden sofort aus der
  angezeigten Seite (samt Zählern); der Schreibauftrag läuft im Hintergrund. Beliebig viele
  Aufträge dürfen vorgemerkt sein; der Schreiber der Sammlung (Bahn `writer:`, ADR 0008) arbeitet
  sie nacheinander in Eingabereihenfolge ab (SQLite kennt nur einen Schreiber). Solange ein Auftrag
  läuft, bleiben seine Gesichter in jeder neu gelesenen Seite ausgeblendet; der Ausblendstand
  ergibt sich aus den laufenden Aufträgen (`Running::hides`) und kann daher nie hängen bleiben.
  Nach dem letzten vorgemerkten Auftrag wird neu gelesen; ein Fehler endet das Ausblenden, die
  Gesichter erscheinen wieder (die Datenbank bleibt maßgeblich) und der Fehler wird gemeldet.
  Andere Schreibaufträge (Indizieren, Einzeländerungen) sperren das Vormerken weiter. Die
  Funktion ist wiederverwendbar für jede Stelle, die Gesichter entscheidet.

## Alternativen

Kacheln in einer SQLite-Tabelle: erforderte Schema 7 und Schreibzugriffe aus dem Vorschau-Thread
neben dem einzigen Schreiber (ADR 0042, 0008); der Dateiordner ist einfacher, robust gegen
Nebenläufigkeit und leicht zu löschen. Kacheln beim Analysieren vorab erzeugen: bestehende
Sammlungen blieben leer, 200.000 Kacheln nachträglich kosten Stunden. Kleinere JPEG-Decodierung per
DCT-Skalierung: braucht eine zusätzliche Bibliothek, nicht gemessen. Echte parallele Datenbank-
Schreiber: mit SQLite nicht möglich.

## Folgen und Grenzen

Erstansicht einer Gruppe kostet weiter eine Foto-Dekodierung je Kachel (2,4 statt 6,5 s für
24 Kacheln bei vier Threads); jede weitere Ansicht (Seitenwechsel zurück, Reiterwechsel, nächster
Programmstart) liest nur Kacheln (0,4 ms je Kachel statt 265 ms). Speicherbedarf rund 5 bis 8 KB
je angezeigter Kachel (unbegrenzt wachsend, löschbar: Ordner entfernen). Verlustbehaftete Kacheln
(JPEG Q90) sind nur für die Anzeige. Wird ein Auftrag abgebrochen oder stürzt die App ab, bevor
er geschrieben hat, sind die Gesichter beim nächsten Start wieder da. Zähler in den Reiternamen
werden lokal angepasst und beim Neulesen korrigiert. Nicht gemessen: echte Fotos, RAW, Netzlaufwerke
und die echte Oberfläche; die Zeiten stammen aus synthetischen JPEGs mit Rauschen (eher
Obergrenze).

## Bei Weiterentwicklung beachten

Der Kachelordner darf nie Originale enthalten und muss von „Biometrie löschen" erfasst bleiben.
Neue Aufrufer von Gesichtsausschnitten sollen `cached_preview` nutzen, außer sie benötigen
unverlustete Bilder (Trainingsexport). Weitere Entscheidungsfunktionen für Gesichter nutzen
`queue_hiding`, nie einen eigenen Ausblendzustand.

## Belege und Validierung

[Kachel-Benchmark](../examples/face_thumb_bench.rs), [Tests Kachel-Cache](../src/core/previews.rs)
(Gleichheit erstes/zweites Mal, Invalidierung, beschädigte Kachel, Original unverändert),
[Test sofortiges Bestätigen](../src/ui/app/tests.rs), Löschtest in
[tests/maintenance.rs](../tests/maintenance.rs), Index-Test in
[tests/face_save_performance.rs](../tests/face_save_performance.rs),
[Messreihe](../docs/validierung.md#personenseite-laden-adr-0068).
