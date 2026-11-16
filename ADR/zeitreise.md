# Zeitreise

Konzept und offene Ideen: [docs/konzept-zeitreise.md](../docs/konzept-zeitreise.md).

## Entscheidung

- **Lesende Ansicht der gewählten Person:** Die Zeitreise hat keinen eigenen Hauptpunkt;
  sie ist unter „Personen“ die Ansicht „Zeitreise“ und öffnet sich für die in der Tabelle
  gewählte Person (auch über „Zeitreise zeigen“ in den Details). Ihre eigene Personenauswahl
  bleibt zum Wechseln und wählt die Person für alle Personenansichten.
- **Inhalt:** Gesichter einer Person über die Zeit (`core/timeline.rs`,
  `ui/app/time_view.rs`) mit den Reitern Zeitstrahl, Lücken, Zeitraffer & Export. Alles wird
  zur Laufzeit berechnet; es gibt keine Schemaänderung, nur den additiven Teilindex
  `ix_faces_timeline`.
- **Datum und Datumsqualität:** Aufnahmezeit des Gesichts, sonst EXIF, sonst Jahr und Monat
  aus dem Ordnernamen, sonst Änderungszeit der Datei. Unsichere Quellen sind gestrichelt
  markiert; die schlechteste zugelassene Quelle ist wählbar. Gesichter ohne Datum stehen in
  einem eigenen Fach.
- **Geeignete Gesichter:** kürzere Gesichtsseite ab 160 Pixel im Quellbild (Regler 96 bis
  320), Qualität ab 0,5, Erkennung ab 0,8, Frontalität ab 0,5. Nur bestätigte Zuordnungen,
  auf Wunsch auch sichere automatische. Ignorierte, historische und für die Person
  abgelehnte Gesichter fehlen.
- **Vorschlag je Abschnitt:** Wert = 0,35 Auflösung + 0,25 Frontalität + 0,2 Qualität + 0,2
  Erkennung. Er schlägt nur das Gesicht je Abschnitt vor.
- **Abschnitte:** Achse Datum (Jahr oder Monat) oder Alter. Die Körnung wird nach Datenlage
  gewählt und ist umschaltbar. Der optionale Geburtsmonat steht lokal in `person_attrs`
  (`birth_month`); ohne ihn rechnet die Altersachse in Jahren.
- **Nutzerentscheidungen** stehen in `face_settings`: festgenagelte Gesichter je Abschnitt
  (`timeline.pins:<person>`) und ausgeblendete Gesichter (`timeline.hidden:<person>`). Ist ein
  festgenageltes Gesicht nicht mehr geeignet, gilt die automatische Wahl; die Entscheidung
  bleibt gespeichert.
- **Lücken:** leere Abschnitte zwischen zwei belegten, mit der Zahl ungeeigneter Gesichter
  im Zeitraum und einem Sprung in die Sammlungssuche.
- **Export** nur als neue Dateien in einen gewählten Ordner, nie ersetzend: PNG-Collage,
  PNG-Kontaktbogen, Zeitraffer als animiertes GIF und als PNG-Bildfolge.
- **Worker:** Abfrage und Berechnung laufen auf Workern mit Revisionsprüfung; Kacheln kommen
  aus dem Vorschau-Cache. Die Augenausrichtung entsteht beim Zeichnen, nicht durch
  Pixelarbeit im UI-Thread.

## Grenzen

Nur mit synthetischen Daten gemessen. Die Gesichtsgröße nutzt die Bildmaße aus den Metadaten
und ist bei RAW mit eingebetteter Vorschau ungenau. Nicht umgesetzt: Datumskorrektur,
Ereignismarken, Vorher/Nachher, Familienvergleich, Kontaktbogen als Bildschirmansicht.

## Beim Ändern beachten

Die Berechnung bleibt lesend; Entscheidungen nur in `face_settings`. Datumskorrekturen werden
nie in Dateien geschrieben. Braucht die Zeitreise eigene Tabellen, ist das eine
Schemamigration (siehe [Sammlung und Daten](sammlung-daten.md)).
