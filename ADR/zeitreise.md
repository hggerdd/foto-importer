# Zeitreise

Konzept und offene Ideen: [docs/konzept-zeitreise.md](../docs/konzept-zeitreise.md).

## Entscheidung

- **Lesende Ansicht der gewählten Person:** Die Zeitreise hat keinen eigenen Hauptpunkt;
  sie ist unter „Personen“ die Ansicht „Zeitreise“ und öffnet sich für die in der Tabelle
  gewählte Person (auch über „Zeitreise zeigen“ in den Details). Ihre eigene Personenauswahl
  bleibt zum Wechseln und wählt die Person für alle Personenansichten. Ohne gewählte Person
  wird nichts vorgewählt und kein Gesicht gelesen.
- **Personenauswahl sofort, Gesichter nur der gewählten Person:** Die Auswahl zeigt die
  gemeinsame Personenliste aller Personenansichten (siehe [Personen](personen.md), „Eine
  Personenliste für alle Auswahlfelder“); fehlt sie beim Start noch, liest die Zeitreise sie
  einmal über deren kleinen Auftrag (`faces::people_if_changed`, nur Personen-, Namens- und
  Zähltabellen über `ix_faces_user_counts`) und zeigt bis dahin die zuletzt gelesene Liste
  aus dem lokalen Cache (`personen-<Hash>.json`, `core::local_cache`). Erst nach der Wahl liest „Zeitreise
  lesen“ (`timeline::load_person`) Gesichter, Geburt und Entscheidungen nur dieser Person.
  Eine neue Wahl ersetzt den laufenden Auftrag (Abbruch, Revision). Gelesene Personen bleiben
  im Speicher (`config::TIMELINE_CACHED_PEOPLE`, 12); der Wechsel zurück rechnet nur neu.
  Jede gemerkte Person trägt ihren Datenstand (`core::revision`, Bereiche `faces` und
  `people`, in derselben Lesetransaktion gelesen). Die Zeitreise sieht neue Stände über
  jede neu gelesene Gesichterseite, eigene Ladeaufträge und gespeicherte Entscheidungen
  (`TimeUi::observe`); ein neuerer Stand verwirft Gemerktes zu älteren Ständen und liest
  die gezeigte Person neu (sie bleibt bis dahin sichtbar). Ältere
  Antworten kommen nicht mehr hinein. Geänderte Metadaten allein (Bereich `files`) lesen nicht
  neu, damit ein Hintergrundlauf die gezeigte Person nicht laufend neu lädt. Messung 2026-11-21 an `E:\Bilder` (USB-Festplatte): Personenliste kalt 0,15 s,
  Gesichter einer Person kalt 3,3 s (408 Gesichter) bis 10,9 s (3 595), warm 9 bis 54 ms.
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

Auswahl, Vorschlagswert und Export sind nur mit synthetischen Daten geprüft; an einer echten
Sammlung gemessen sind allein die Ladezeiten (oben). Die Gesichtsgröße nutzt die Bildmaße aus den Metadaten
und ist bei RAW mit eingebetteter Vorschau ungenau. Nicht umgesetzt: Datumskorrektur,
Ereignismarken, Vorher/Nachher, Familienvergleich, Kontaktbogen als Bildschirmansicht.

## Beim Ändern beachten

Die Berechnung bleibt lesend; Entscheidungen nur in `face_settings`. Datumskorrekturen werden
nie in Dateien geschrieben. Braucht die Zeitreise eigene Tabellen, ist das eine
Schemamigration (siehe [Sammlung und Daten](sammlung-daten.md)).
