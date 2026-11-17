# Sammlungsansicht

## Entscheidung

- **Aufbau:** Kompakte Leiste (Suche, Filter, Ordnerbaum, Bilddetails, Bestand), darunter
  optional der Ordnerbaum links, die Galerie und rechts der Bildinspektor. Filter und
  Bestandspflege liegen in Dialogen, damit die Galerie ihre Höhe behält. Der Inspektor lässt
  sich ausblenden.
- **Suche:** lesend, auf Foto-Ebene, seitenweise (48 Fotos). Kombinierbar sind Text, Datum,
  Monate, Metadatenregeln (UND/ODER), Personen (alle, mindestens eine, keine, nur diese) und
  Gesichtszustand. XMP-Bewertung und -Farbmarkierung haben beim Anzeigen Vorrang. Neue
  Suchanfragen brechen veraltete SQL-Arbeit ab.
- **Filter ohne Entwurf:** Jede Änderung im Filterdialog fragt die Sammlung sofort neu ab
  (auf dem Worker); Datums- und Wertfelder bei Fokusverlust, Enter oder Schließen des
  Dialogs. Es gibt keinen Anwenden-Knopf und keinen Zustand „Entwurf“; der Knopf zeigt
  „Filter · aktiv“ oder „Filter …“. Metadatenkriterien ohne Wert zählen erst, wenn ein Wert
  dasteht. Der Suchtext der Leiste gilt weiterhin erst mit „Suchen“ oder Enter; Filter
  übernehmen den zuletzt gesuchten Text. Neuanfragen nach Bestandsänderungen wiederholen die
  zuletzt angefragte Suche, damit eine noch laufende Filteränderung nicht verloren geht.
- **Gesichter als Metadaten:** Gesichtsanzahl und bestätigte Personen stehen im
  Metadatenreiter und lassen sich als Suchkriterium übernehmen. Die Suchzeile findet
  bestätigte Personennamen. Ignorierte Gesichter zählen als anwesend; historische
  (`retained`) Vorkommen zählen nicht.
- **Bildinspektor:** Vorschau bleibt beim Scrollen der Metadaten sichtbar. Gesichtsrahmen
  beziehen sich auf die orientierte Quelldatei. Am Bild lässt sich ein einzelnes Gesicht
  benennen (Dialog mit Vorschlägen), eine Fehldetektion löschen und eine Referenz festlegen
  oder ausschließen. Löschen ist nicht Ignorieren: Es entfernt das Vorkommen samt
  Trainingsdaten in einer Transaktion.
- **Metadaten lesen am Foto:** Der Knopf „Metadaten lesen“ im Inspektor liest die Dateien
  des gewählten Fotos neu (auch bei vorhandenen Werten) als Nutzerschreibauftrag mit
  derselben Speicherlogik wie der Sammlungslauf (`metadata::store_results`); danach lädt der
  Inspektor die Werte neu. Er ist gesperrt, solange ein Auftrag die Sammlung belegt.
- **Ordnerbaum:** Einklappbares Seitenfeld, wird in einem Workerauftrag vollständig gelesen
  und im Speicher gehalten; Auf- und Zuklappen greift nicht auf die Datenbank zu. Ein Klick
  setzt den Ordnerfilter. Zählung und Sortierung folgen denselben Varianten- und Präfixregeln
  wie die Suche (Test vergleicht beide). Ändert sich der Bestand (Fotos, Dateien) während
  eines Lesens, wird es **nicht abgebrochen**: das Ergebnis wird gezeigt und danach genau
  einmal nachgelesen. Nur Antworten für eine andere Sammlung werden verworfen.
- **Ordnerbaum-Cache:** Das letzte Ergebnis je Sammlung liegt als JSON im
  Einstellungsordner (`cache/folders-<Hash des normalisierten Sammlungspfads>.json`, mit
  Pfad, Foto- und Dateizahl, Zeitpunkt), nie in der Sammlung. Ist der Baum leer, wird der
  Cache beim Start des Lesens im UI-Thread geladen (kleine Datei auf der lokalen Platte) und
  sofort gezeigt; der Kopf zeigt „aktualisiert …“, bis das Lesen ihn ersetzt. Der Worker
  schreibt den Cache nach jedem erfolgreichen Lesen. Fehlender, defekter oder fremder Cache
  zählt als nicht vorhanden. Die Abfrage selbst bleibt die über `ix_files_dir_cover`
  (Messung 2026-11-17, `E:\Bilder`, USB-Festplatte, 98 082 Dateien: kalt 16,6 s, warm
  0,02 s; die Variante nur über `metadata_photos` kalt 33,7 s).
- **Vorschauen:** Dekodierung nur auf Workern (zwei bis vier Threads), nur für sichtbare
  Kacheln. Der Texturcache ist nach Wurzel, Pfad, Orientierung, Ausschnitt und Kantenlänge
  verschlüsselt, begrenzt auf 192 MiB und 512 Einträge, und räumt nichts, was im aktuellen
  oder vorigen Bild gezeichnet wurde. Vorschauen sind keine RAW-Entwicklung.

## Grenzen

Freie Teilstringsuche ist ein Scan in SQL. Tiefe Seiten nutzen `OFFSET`. Rahmen
unterschiedlich beschnittener Varianten werden nicht gleichgesetzt.

## Beim Ändern beachten

Die Leisten dürfen der Galerie keine Höhe nehmen; Vorschau und Metadaten bei 980 × 680
prüfen. Cache-Schlüssel dürfen keine je Bild oder Revision veränderlichen Werte enthalten.
Neue Suchfelder legen ihre Variantenpriorität fest. Bestätigte Zuordnung, Vorschlag und
fehlende Detektion bleiben unterscheidbar.
