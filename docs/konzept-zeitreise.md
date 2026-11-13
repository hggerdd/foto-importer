# Konzept: Seite „06 Zeitreise“ (Personen über die Zeit verfolgen)

Status: Entwurf, kein Code. Stand 2026-11-13. Vor einer Umsetzung entsteht ein ADR.

## Ziel

Eine Person über Jahre ansehen: Wie hat sich ein Kind verändert, wie sehen Geschwister im
gleichen Alter aus, wie hat sich ein Erwachsener über Jahrzehnte gewandelt. Es werden **nur
Gesichter in hoher Auflösung** gezeigt, damit die Bilder vergleichbar und schön sind.
Die Seite liest nur. Originale, Namen und Bestätigungen werden nicht verändert.

## Was schon vorhanden ist

| Baustein | Quelle | Bemerkung |
|---|---|---|
| Aufnahmedatum je Gesicht | `faces.captured_at`, sonst `metadata_photos.dt_original` | Datum kann fehlen oder falsch sein (Kamera-Uhr, Scans) |
| Gesichtsgröße | `faces.bbox_*` und Pixelmaße des Quellbilds (`meta.width/height`) | Daraus folgt die Gesichtsauflösung in Pixeln |
| Güte | `faces.quality`, `det_score`, Landmarks (5 Punkte) | Landmarks erlauben Ausrichtung und Frontalprüfung |
| Hochauflösende Neuberechnung | ADR 0059 (3200 px für kleine, klare Gesichter) | Die Seite nutzt, was dort schon bewertet wurde |
| Geburtsjahr | Personenmerkmal `birth_year` (auch zentral) | Macht aus Datum ein **Alter** |
| Bestätigte Zuordnung | `faces.assignment='user'` | Nur bestätigte (optional auch sichere automatische) Gesichter zählen |
| Ordnernamen | `files.dir` | Als Ereignismarken (Urlaub 2019, Einschulung) |

## Auswahl „hohe Auflösung“

Ein Gesicht ist zeitreisetauglich, wenn alle Bedingungen erfüllt sind (Schwellen in
`config.rs`, einstellbar):

1. kürzere Gesichtsseite im Quellbild mindestens 160 px (Standard; Regler 96 bis 320),
2. `quality` und `det_score` über den Schwellen,
3. ungefähr frontal (Landmark-Geometrie: Augenabstand zu Nase-Mund-Abstand, Neigung),
4. nicht ignoriert, nicht von der Person abgelehnt, bestätigt oder sicher zugeordnet,
5. Datum vorhanden (sonst Fach „ohne Datum“, nicht im Zeitstrahl).

Pro Gesicht wird ein **Zeitreise-Wert** (0 bis 1) aus Auflösung, Schärfe, Frontalität und
Qualität berechnet. Er dient nur zum Vorschlagen des schönsten Gesichts je Zeitraum.

## Ansichten

1. **Zeitstrahl (Hauptansicht).** Waagerechte Achse, zoombar: Jahrzehnte, Jahre, Monate.
   Je Zeitabschnitt erscheint das beste Gesicht als Kachel. Darüber eine **Dichtekurve**
   (Gesichter je Monat), die zeigt, wo viel Material liegt und wo Lücken sind.
   Klick auf eine Kachel öffnet alle Gesichter des Abschnitts und das Originalfoto (wie
   die Vorschau auf der Personenseite).
2. **Altersleiste.** Gleiche Achse, aber in Lebensalter (0, 1, 2 …, Monate im ersten Jahr)
   statt in Datum. Erfordert Geburtsjahr, besser Geburtsdatum. Ideal für Kinder: „mit 3“.
3. **Zeitraffer.** Die Gesichter werden an den Augen ausgerichtet, gleich groß beschnitten
   und nacheinander abgespielt (Wiedergabe, Pause, Geschwindigkeit, Bild für Bild). Ein
   Schieberegler springt in der Zeit. Das ist der „Wie hat sich X verändert“-Moment.
4. **Kontaktbogen nach Jahr.** Raster: eine Zeile je Jahr (oder Altersjahr), darin die
   besten drei bis sechs Gesichter. Guter Gesamtüberblick, druckbar.
5. **Vorher/Nachher.** Zwei Zeitpunkte nebeneinander mit Schieberegler (Überblendung).
   Auch für zwei Personen im selben Alter (Geschwister, Eltern früher, Kind heute).
6. **Familienvergleich im gleichen Alter.** Mehrere Personen in einer Altersleiste
   übereinander, eine Zeile je Person. Zeigt „so sah die große Schwester mit 5 aus“.
   Nutzt die Beziehungen, die schon im Stammbaum liegen (Geschwister, Eltern).
7. **Ereignisse auf der Achse.** Ordnernamen und Fotoserien (Cluster in kurzer Zeit) als
   Marken am Zeitstrahl; optional Orte aus GPS. Hilft beim Wiederfinden.
8. **Verlaufskurve der Ähnlichkeit** (Zusatz). Abstand der Gesichts-Embeddings zum
   Mittelwert der Person über die Zeit. Zeigt, in welchen Jahren sich ein Gesicht stark
   ändert (Kindheit, Pubertät) und wo die Erkennung unsicher wird. Als Hilfsansicht für
   Fachleute, nicht als Standard; bei Kindern ausdrücklich nicht als „Ähnlichkeit“ in Prozent
   beschriften.
9. **Lückenhinweise.** „Zwischen 2012 und 2014 fehlt ein gutes Bild.“ Mit Link auf
   Fotos dieses Zeitraums, die ein Gesicht der Person enthalten, aber zu klein sind.
10. **Export (neue Dateien, nie Originale ersetzen):** Collage als PNG, Kontaktbogen als
    PDF, Zeitraffer als Video oder animiertes WebP in einen vom Nutzer gewählten Ordner.

## Bedienung

- Kopfzeile: Person wählen (Tippfilter wie `FilterSelect`), Zeitraum, Schwelle „Auflösung“,
  Umschalter Datum/Alter, Quelle (nur bestätigt / auch automatisch sichere).
- Je Zeitabschnitt kann der Nutzer die automatische Wahl **festnageln oder ersetzen**
  (Stern auf einem anderen Gesicht) oder ein Gesicht **ausblenden**. Das ist eine
  ausdrückliche Nutzerentscheidung und bleibt bei Neuberechnung erhalten.
- Alle Zeitabschnitte laden einzeln und sichtbar (nur Kacheln im Sichtbereich), Bilder aus dem
  vorhandenen Vorschau-Cache, nie am UI-Thread.
- Tastatur: Pfeile bewegen den Zeitstrahl, Leertaste spielt den Zeitraffer, Plus/Minus zoomen.

## Daten und Leistung

- Kern: eine Abfrage „Gesichter der Person mit Datum, Größe, Güte“ (wenige Tausend Zeilen je
  Person). Nötig ist ein **Teilindex** `faces(person_id, captured_at)` für bestätigte,
  nicht ignorierte Gesichter (rein additiv, wie bei ADR 0066). Alternativ eine abgeleitete
  Tabelle `face_timeline` (Gesicht, Datum, Pixelgröße, Zeitreise-Wert), die beim Analysieren
  befüllt wird; sie ist neu berechenbar und braucht deshalb nur eine Migration, keinen
  Update-Lauf für bestehende Entscheidungen.
- Datumsqualität als eigene Angabe (`exif`, `datei`, `ordner`, `unbekannt`), damit
  unsichere Daten in der Ansicht markiert werden (gestrichelter Rand).
- Ausrichtung und Zuschnitt werden beim Zeichnen aus Landmarks berechnet und als kleine
  Vorschaubilder zwischengespeichert (Cache-Ordner der Sammlung, nicht in der Datenbank).
- Neu rechnen nur für Gesichter, die noch keine hochaufgelöste Variante haben (vorhandene
  Pipeline aus ADR 0059, auf Worker).

## Risiken und offene Fragen

- **Datum:** Fotos ohne oder mit falschem Aufnahmedatum verfälschen die Achse. Vorschlag:
  Datumsquelle sichtbar machen und Korrektur nur als Anzeigewert je Foto erlauben, nie in
  die Datei schreiben.
- **Falsche Zuordnung:** Ein falsch bestätigtes Gesicht fällt in einer Zeitreise sofort auf.
  Vorschlag: Rechtsklick „gehört nicht zu dieser Person“ (führt zum bestehenden Ablehnen).
- **Kinder:** Embeddings erkennen Kleinkinder schlechter; Ansicht 8 deshalb nur
  Hilfsansicht.
- **Datenschutz:** Exporte erzeugen neue Dateien nur im vom Nutzer gewählten Ordner;
  nichts verlässt den Rechner.
- Fragen an den Nutzer: Genügt das Geburtsjahr, oder soll ein Geburtsdatum (Tag) möglich
  sein? Standard-Schwelle 160 px richtig? Zeitraffer als Video wichtig oder reicht
  Bildschirmwiedergabe? Soll die Seite auch Haustiere/andere Objekte kennen (nein, nur
  Personen)?

## Vorschlag für die Umsetzung in Stufen

1. Kern: Abfrage, Zeitreise-Wert, Teilindex, Tests mit synthetischen Daten.
2. Seite 06 mit Zeitstrahl, Dichtekurve, Detailansicht beim Klick.
3. Altersleiste und Kontaktbogen nach Jahr.
4. Zeitraffer mit Ausrichtung.
5. Vorher/Nachher und Familienvergleich.
6. Festnageln/Ausblenden (Nutzerentscheidungen) und Lückenhinweise.
7. Export.

Jede Stufe auf eigenem `codex/`-Branch mit ADR (nächste freie Nummer), Test und
Sichtprüfung per Fensteraufnahme.
