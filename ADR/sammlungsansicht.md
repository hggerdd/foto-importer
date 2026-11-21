# Sammlungsansicht

## Entscheidung

- **Aufbau:** Kompakte Leiste (Suche, Filter, Ordnerbaum, Bilddetails, Bestand), darunter
  optional der Ordnerbaum links, die Galerie und rechts der Bildinspektor. Filter und
  Bestandspflege liegen in Dialogen, damit die Galerie ihre Höhe behält. Der Inspektor lässt
  sich ausblenden.
- **Fehlende Dateien:** Der Knopf „Bestand …“ nennt fehlende Dateien („Bestand · N fehlen …“);
  im Bestandsdialog öffnet „Fehlende Dateien (N) …“ eine Liste je Ordner (Varianten-Unterordner
  zählen zum Tagesordner) mit Dateizahl und bestätigten Gesichtern, gelesen auf einem Worker.
  Aktionen: „Erneut suchen“ (Bestand aktualisieren; findet umbenannte Ordner wieder),
  „Mit „…“ neu verknüpfen“ (nur bei eindeutiger Entsprechung) und „Einträge entfernen“ für
  gewählte Ordner nach einer Rückfrage mit Folgen. Regeln in
  [Sammlung und Daten](sammlung-daten.md).
- **Suche:** lesend, auf Foto-Ebene, seitenweise (48 Fotos). Kombinierbar sind Text, Datum,
  Monate, Metadatenregeln (UND/ODER), Personen (alle, mindestens eine, keine, nur diese) und
  Gesichtszustand. XMP-Bewertung und -Farbmarkierung haben beim Anzeigen Vorrang. Neue
  Suchanfragen brechen veraltete SQL-Arbeit ab; das Öffnen der Ansicht startet keine neue,
  solange eine Suche läuft (z. B. die vom Start). Gesamtzahl und Kameraliste lesen `meta`
  nur, wenn eine Regel Spalten daraus braucht (sonst kalt über 30 s auf USB-Platten).
  Außerhalb von „Gesichter“ und „Personen“ liest die App nur Personen und Zählungen, keine
  Gesichtsgruppen samt Vorschlägen.
- **Seite zuerst, Zählung danach:** Der Auftrag „Sammlung durchsuchen“ liest nur die Seite
  (Datumsindex, kalt auf der USB-Platte rund 1 s) und den Stand der Sammlung
  (`collection.id` und die Datenrevision `files` aus `core::revision`, die Trigger bei jeder
  Änderung an Dateien und Projektionsspalten erhöhen). Gesamtzahl, Kameras und Bestand (`status()`) zählt
  danach der eigene Auftrag „Sammlung zählen“ (`counts`); sie lesen alle Treffer und kosteten
  kalt zusammen 20 bis 50 s (Messung 2026-11-21, `E:\Bilder`, siehe
  [Validierung](../docs/validierung.md)). Bis dahin stehen die bisherigen Zahlen mit
  „wird aktualisiert …“ da, und der Hintergrundlauf der Metadaten wartet. Eine Zählung gilt
  weiter, solange der Stand gleich ist und die Suche (ohne Seite) dieselbe ist; das gilt nur
  für Filter, deren Treffer allein an der Projektion hängen (`counts_follow_revision`: kein
  Text, keine Personen, kein Gesichtszustand, keine Regel auf `meta` oder Gesichter). Auf der
  ersten Seite müssen zusätzlich die Foto-IDs mit denen beim Zählen übereinstimmen.
- **Startansicht:** Die erste Seite ohne Filter wird mit ihren Zählungen und dem Stand als
  kleine JSON-Datei (`start-<Hash des Pfads>.json`, über `core::local_cache` mit dessen
  Formatnummer und Größengrenze; ältere Formate gelten als fehlend und werden neu
  geschrieben) im lokalen Cache-Ordner abgelegt
  (`Settings::cache_dir`, `%LOCALAPPDATA%\FotoImporter\cache`, nie in der Sammlung) und beim
  Start (und Sammlungswechsel) sofort gezeigt, bevor die Datenbank geöffnet ist, markiert mit
  „wird aktualisiert …“. Die frisch gelesene Seite ersetzt sie immer; ihre Zählungen gelten
  nur bei gleichem Stand (sonst wird gezählt). Beim Start liest „Personen und Gesichter
  lesen“ erst nach Seite und Zählung, damit beide nicht um die Platte konkurrieren; die
  Ansichten „Gesichter“ und „Personen“ lesen beim Öffnen sofort.
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
  einmal nachgelesen. Nur Antworten für eine andere Sammlung werden verworfen. Dauert ein
  Nachlesen länger als 0,5 s, zeigt der Kopf „aktualisiert …“.
- **Ordnerübersicht in der Sammlung:** Der Baum liest die Tabelle `folder_counts` (Fotos je
  Ordner, von Triggern gepflegt, siehe [Sammlung und Daten](sammlung-daten.md)); das sind
  wenige Seiten. Lokale Caches außerhalb der Sammlung (auf der Platte: Startansicht,
  Vorschau-Cache, Personenliste der Zeitreise, Personengraph; im Speicher: MatchCache des
  Abgleichs) sind nur Beschleunigung: Ohne sie ist jede Ansicht vollständig und richtig, nur langsamer; der
  Ordnerbaum selbst wird nicht lokal gespeichert. Fehlt die Tabelle (ältere Sammlung, nur lesend geöffnet),
  zählt der Baum über `ix_files_dir_cover` (Messung 2026-11-17, `E:\Bilder`,
  USB-Festplatte, 98 082 Dateien: kalt 16,6 s, warm 0,02 s; nur über `metadata_photos` kalt
  33,7 s — die verstreuten Seitenzugriffe begründen die Tabelle).
- **Vorschauen:** Dekodierung nur auf Workern (zwei bis vier Threads), nur für sichtbare
  Kacheln. Die Warteschlange (`ui::preview_queue`) hat zwei Bahnen: Das Foto unter der Maus
  geht allen Kacheln vor, eine neue Mausanfrage ersetzt die wartende, und ein eigener
  Worker (`THUMBNAIL_HOVER_WORKERS`) nimmt nur Mausanfragen, damit das Foto nie hinter einer
  laufenden Kachelrunde wartet. Wartende Kacheln, die zuletzt gezeichnet wurden, kommen
  zuerst; was `THUMBNAIL_STALE_FRAMES` Frames nicht gezeichnet wurde (weggescrollt, Maus
  weiter), fällt heraus und wird beim nächsten Zeichnen neu angefragt. Die Zeit von der
  Mausanfrage bis zur Textur steht als `vorschau maus gezeigt` im Diagnose-Log. Der Texturcache ist nach Wurzel, Pfad, Orientierung, Ausschnitt und Kantenlänge
  verschlüsselt, begrenzt auf 192 MiB und 512 Einträge, und räumt nichts, was im aktuellen
  oder vorigen Bild gezeichnet wurde. Vorschauen sind keine RAW-Entwicklung.
- **Lokaler Vorschau-Cache** (`core::thumb_cache`): Ganze Vorschauen der Galerie (400) und
  des Inspektors (960) liegen als JPEG (Qualität 85) unter `<cache_dir>\vorschau`, nie in der
  Sammlung; ein Treffer liest eine kleine lokale Datei statt das Original von der
  Sammlungsplatte zu dekodieren. Schlüssel: kanonischer Pfad, Größe und Änderungszeit des
  Originals, Ausrichtung, Kantenlänge (ein geändertes Original ergibt einen neuen Eintrag);
  vorher gelten dieselben Pfadprüfungen wie beim Dekodieren. Transparente Bilder werden nicht
  gespeichert. Grenze `THUMB_CACHE_BYTES` (1 GiB); beim Start und nach je 256 neuen Einträgen
  werden die am längsten unbenutzten gelöscht (ein Treffer frischt die Änderungszeit höchstens
  täglich auf), bis neun Zehntel übrig sind. Gelöscht werden nur `*.jpg` in den
  Unterordnern und alte Zwischendateien. Nicht in der Datensicherung (ableitbar).

## Grenzen

Freie Teilstringsuche ist ein Scan in SQL. Tiefe Seiten nutzen `OFFSET`. Ändert sich der
Stand, bleibt die Zählung kalt teuer (verstreute Seiten auf der USB-Platte); erst danach
stimmen Gesamtzahl und Blätterer. Die Startansicht hilft nur der Suche ohne Filter. Rahmen
unterschiedlich beschnittener Varianten werden nicht gleichgesetzt.

## Beim Ändern beachten

Die Leisten dürfen der Galerie keine Höhe nehmen; Vorschau und Metadaten bei 980 × 680
prüfen. Cache-Schlüssel dürfen keine je Bild oder Revision veränderlichen Werte enthalten.
Neue Suchfelder legen fest, ob ihre Zählung der Datenrevision `files` folgt
(`counts_follow_revision`); im Zweifel nicht. Änderungen, die Treffer verändern, ohne über die
Trigger die Revision `files` zu erhöhen, dürfen keine wiederverwendbare Zählung erlauben.
Neue Suchfelder legen ihre Variantenpriorität fest. Bestätigte Zuordnung, Vorschlag und
fehlende Detektion bleiben unterscheidbar.
