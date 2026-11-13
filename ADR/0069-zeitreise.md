# ADR-0069: Zeitreise, Gesichter einer Person über die Zeit

- **Status:** Akzeptiert (Stufen 1 bis 7 umgesetzt, Grenzen siehe unten)
- **Erfasst am:** 2026-11-13
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-13
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0011](0011-begrenzte-bildvorschauen.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0042](0042-native-bestandsmigrationen.md), [0057](0057-ordnerbaum-in-der-sammlung.md), [0059](0059-arbeitsportionen-neueste-zuerst.md), [0061](0061-stammbaum-als-normalisiertes-modell.md), [0066](0066-schnelles-speichern-von-gesichtsgruppen.md), [0067](0067-kompletter-ordnerbaum.md); Konzept: [docs/konzept-zeitreise.md](../docs/konzept-zeitreise.md)

## Kontext

Der Nutzer will eine Person über Jahre ansehen: Kinder im Wachstum, Geschwister im gleichen
Alter, Erwachsene über Jahrzehnte. Gezeigt werden sollen nur Gesichter in hoher Auflösung,
damit die Bilder vergleichbar sind. Alle Bausteine liegen schon in der Sammlung (Datum,
Rahmen, Landmarks, Güte, Geburtsjahr, Beziehungen). ADR 0042 verbietet neue Tabellen und
Schemaerhöhungen in bestehenden Sammlungen, und die Personenseite ändert parallel Schema und
Migrationen.

## Entscheidung

- **Neue Seite „06 Zeitreise“** (sechster Eintrag der Navigation, `src/ui/app/time_view.rs`),
  Domainlogik in `src/core/timeline.rs`. Die Seite liest nur; Namen, Bestätigungen und
  Originale bleiben unverändert.
- **Keine Schemaänderung.** Alles wird zur Laufzeit aus `faces`, `meta`,
  `metadata_photos` und `files` berechnet (eine Abfrage je Person, rein lesend). Zusätzlich
  ein rein additiver Teilindex `ix_faces_timeline ON faces(person_id, captured_at, id) WHERE
  ignored=0 AND retained=0 AND assignment IN ('user','auto')` nach dem Muster der ADRs
  0066/0067 (`schema.sql`, `migrations::ADDITIVE_INDEXES`, Test).
- **Datum und Datumsqualität.** Reihenfolge: `faces.captured_at`, sonst
  `metadata_photos.dt_original`/`meta.dt_original` (Qualität `exif`), sonst Jahr/Monat aus
  dem Ordnernamen (`ordner`), sonst Änderungszeit der Datei (`datei`, unsicher). Gesichter
  ohne Datum landen im Fach „ohne Datum“ und nicht im Zeitstrahl. Unsichere Quellen werden
  mit gestricheltem Rand markiert; die schlechteste zugelassene Quelle ist wählbar. Datum
  korrigieren ist nicht umgesetzt (offen); Korrekturen würden nie in Dateien geschrieben.
- **Auswahl „hohe Auflösung“.** Kürzere Gesichtsseite in Bildpunkten = Rahmen (normiert)
  mal Bildmaße der Quelldatei (`meta.width/height`, bei Drehung 5 bis 8 vertauscht).
  Mindestwert Standard 160 px, Regler 96 bis 320 (`config::TIMELINE_MIN_PX`, Einstellung
  `timeline_min_px`). Weitere Schwellen: Güte 0,5, Erkennungswert 0,8, Frontalität 0,5.
  Frontalität maßstabsfrei aus den fünf Landmarks (Neigung der Augenlinie, Nasenversatz,
  Verhältnis Augenabstand zu Auge-Mund-Abstand). Nur bestätigte Zuordnungen (`user`), auf
  Wunsch auch sichere automatische; ignorierte, verworfene und von der Person abgelehnte
  Gesichter fehlen.
- **Zeitreise-Wert** (0 bis 1) = 0,35 Auflösung + 0,25 Frontalität + 0,2 Güte + 0,2
  Erkennungswert. Eine Schärfemessung ist nicht gespeichert und wird nicht nachgerechnet; der
  Erkennungswert dient als Ersatz. Der Wert schlägt nur das schönste Gesicht je Abschnitt vor.
- **Zeitabschnitte.** Achse Datum (Jahr oder Monat) oder Alter (Altersjahr, mit Geburtsmonat
  auch Altersmonat). Die Körnung wird nach Datenlage gewählt (Monat bei höchstens 48
  Monaten Spanne oder mindestens 36 brauchbaren Gesichtern je Jahr, sonst Jahr) und ist
  manuell umschaltbar. Ohne Geburtsmonat fällt die Altersachse auf Jahre zurück.
- **Geburtsmonat.** Der Nutzer hat entschieden, dass Monatsgenauigkeit genügt, kein Tag.
  Das Personenmerkmal `birth_year` bleibt unverändert (auch zentral). Der optionale
  Monat steht lokal in `person_attrs` unter `birth_month` (1 bis 12). Die zentrale
  Synchronisation (`central::FIELDS`) wird nicht geändert; **der Monat gilt vorerst nur lokal
  in der Sammlung** und wandert nicht in den zentralen Bestand. Das Alter ist auf Monate
  genau, nicht auf Tage.
- **Nutzerentscheidungen** in `face_settings`: `timeline.pins:<person>`
  (`Abschnittsschlüssel=Gesichts-ID;…`, Schlüssel wie `dy:2019`, `dm:<Monatsindex>`,
  `ay:3`, `am:40`) und `timeline.hidden:<person>` (IDs, durch Komma getrennt). Gesichts-IDs
  bleiben bei Neuanalyse stabil; ein festgenageltes Gesicht, das nicht mehr geeignet ist,
  fällt für die Anzeige auf die automatische Wahl zurück, die Entscheidung bleibt gespeichert.
  Ausblenden hebt eine Festlegung desselben Gesichts auf.
- **Lückenhinweise:** aufeinanderfolgende leere Abschnitte (Jahr: ab einem, Monat: ab drei)
  zwischen zwei Abschnitten mit Gesicht; dazu die Zahl der Gesichter der Person im
  Zeitraum, die nur zu klein, unscharf oder nicht frontal sind, und ein Sprung in die
  Sammlungssuche mit diesem Zeitraum.
- **Worker.** Abfrage und Berechnung (`timeline::load`, `timeline::compute`) laufen nur auf
  Workern mit Revisionsprüfung (Art `timeline`, Antwort `Outcome::Timeline`). Kacheln nutzen
  den vorhandenen Vorschau-Cache (`Thumbnails::image`, ADR 0011/0052) und laden nur
  sichtbare Zeilen. Ausrichtung an den Augen im Zeitraffer und im Vergleich geschieht beim
  Zeichnen durch eine Ähnlichkeitsabbildung des Ausschnitts (gedrehtes Viereck), nicht
  durch Pixelarbeit auf dem UI-Thread.
- **Export** nur als neue Dateien in einen vom Nutzer gewählten Ordner, nie ersetzend:
  PNG-Collage und PNG-Kontaktbogen sowie der Zeitraffer als **animiertes GIF** und als
  **PNG-Bildfolge**. Gewählt, weil `image` (Features `gif`, `png`) schon Abhängigkeit ist; ein
  Videocodec oder PDF-Schreiber wäre eine neue schwere Abhängigkeit. GIF hat 256 Farben,
  daher bleibt die PNG-Bildfolge für beste Qualität. Bestehende Dateinamen werden nie
  überschrieben (Zähler im Namen).

## Alternativen

Eine abgeleitete Tabelle `face_timeline` wäre schneller, braucht aber Migration und Trigger
(ADR 0042) und kollidiert mit der parallelen Schemaarbeit; bei einigen Tausend Gesichtern je
Person genügt die Laufzeitberechnung (gemessen siehe unten). Den Geburtsmonat zentral
mitzuführen ändert das Zentralschema und bleibt einer eigenen Entscheidung vorbehalten.

## Folgen und Grenzen

- Messwerte, synthetisch, 100.000 Gesichter einer Person: siehe Abschnitt „Messwerte“.
- Gemessen und gesehen wurde nur mit synthetischen Daten und einfachen generierten
  Gesichtern; nicht mit echten Sammlungen, Netzlaufwerken oder echten Gesichtsmodellen.
- Die Gesichtsgröße verwendet die Bildmaße der Quelldatei aus den Metadaten. Stimmen sie nicht
  mit der Analysegrundlage überein (RAW mit eingebetteter Vorschau), ist die Pixelzahl
  ungenau, und die Augenausrichtung wird verworfen, wenn die Augen außerhalb des Rahmens
  lägen.
- Die Ausschnitte kommen aus dem Vorschau-Cache (höchstens 1600 px Analysekante); für die
  Anzeige reicht das, die Auflösungsschwelle bezieht sich auf das Quellbild.
- Datumskorrektur, Ereignismarken (Ordner/Serien/GPS) und die Verlaufskurve der Ähnlichkeit
  aus dem Konzept sind nicht umgesetzt.

## Bei Weiterentwicklung beachten

Berechnung bleibt lesend; Entscheidungen nur in `face_settings`; Originale und bestehende
Zieldateien werden nie verändert. Wird ein Schema für die Zeitreise nötig, braucht es ein
eigenes ADR mit Migration (ADR 0042).

## Belege

[Kern](../src/core/timeline.rs), [Tests](../tests/timeline.rs),
[Seite](../src/ui/app/time_view.rs), [Demo](../examples/timeline_demo.rs).
