# Gesichter benennen und prüfen

Wie Vorschläge und Gruppen entstehen: siehe [Gesichtserkennung](gesichtserkennung.md).

## Entscheidung

- **Reiter im Personenbereich:** „Benennen“ (Gruppen unbenannter Gesichter, größte zuerst),
  „Vorschläge prüfen“ (`suggested` und `auto` je Person), „Bestätigt“, „Ignoriert“. Acht
  Gruppen je Seite, höchstens 28 Gesichter je Gruppe; alle Mitglieder sind sichtbar, es gibt
  keine versteckten ausgewählten Gesichter.
- **Gruppenkarte:** Namensfeld mit Enter, Personenauswahl mit Tippfilter, bis zu drei
  Vorschläge mit Porträt (ein Klick wählt und speichert), „Bestätigen“, „Nicht interessiert“.
  Alle Gesichter sind zunächst ausgewählt; Klick wählt einzelne ab. Kachelgröße ist per
  Regler einstellbar, die Spaltenzahl folgt der Fensterbreite.
- **Bestätigen** setzt ausgewählte Gesichter auf `user`. Abgewählte erhalten eine
  gespeicherte Ablehnung für diese Person (`face_rejections`), die jeder spätere Abgleich
  beachtet. Ein neuer Name legt eine Person an; bei mehreren gleichnamigen Personen muss die
  konkrete Person gewählt werden.
- **Nicht interessiert** setzt `ignored=1` für die ganze Gruppe. Vorkommen, Rahmen und
  Vektoren bleiben; bestätigte Identitäten bleiben; maschinelle Zuordnungen entfallen.
  Ignorierte Gesichter sind weder Referenz noch negatives Beispiel. Interesse und Identität
  sind getrennte Entscheidungen.
- **Rücknahme:** bis zu 20 Entscheidungen der laufenden Sitzung, einschließlich Ablehnungen
  und Pins. Eine Rücknahme scheitert, wenn eine spätere Nutzeränderung dieselben Gesichter
  betrifft. Eine dabei angelegte, noch ungenutzte Person wird wieder entfernt.
- **Sofortiges Speichern:** Entschiedene Gesichter verschwinden sofort aus der Seite
  (`queue_hiding`), der Schreibauftrag läuft dahinter in Eingabereihenfolge. Der
  Ausblendstand ergibt sich aus den laufenden Aufträgen; bei einem Fehler erscheinen die
  Gesichter wieder. Nach dem Speichern rückt die nächste Gruppe an die bisherige Position.
- **Seitenabfragen** bilden Teilgruppen aus Zählungen je Person, ohne Fensterfunktionen über
  alle Gesichter. Referenzen und Porträts werden je Seite einmal geladen und leben nur einen
  Lesestand lang.
- **Gesichtskacheln:** Ausschnitte für die Oberfläche liegen als abgeleitete Dateien unter
  `<Sammlung>/.myfotomanager_tiles/` (Schlüssel aus Pfad, Größe, Änderungszeit, Ausschnitt,
  Kantenlänge, Ausrichtung). Sie werden nie ersetzt, vom Indizieren übersprungen und von
  „Biometrie entfernen“ gelöscht.
- **Vorschau unter der Maus:** Das ganze Foto zum Gesicht unter dem Zeiger, abschaltbar,
  höchstens eine Vorschau gleichzeitig.
- **Kopf des Bereichs:** drei Schritte (Fotoanalyse, aus Namen lernen, Namen vorschlagen)
  mit Stand und laufender Phase; Einrichtung, Zeitplan, Export und „Biometrie entfernen“ im
  Dialog „Automatik & Werkzeuge“. In den Reitern Personen, Stammbaum und Graph nur die
  Titelzeile.

## Beschlossen, noch nicht umgesetzt (2026-11-15)

- **Ablehnen wirkt sofort:** Ein abgewähltes Gesicht, das die abgelehnte Person trägt, wird
  in derselben Transaktion unbenannt (auch im Reiter „Bestätigt“), erscheint unter
  „Benennen“ und zeigt dort „nicht X“.
- **„Vorschläge prüfen“** enthält nur sichere Treffer (Schwelle und Abstand siehe
  Gesichtserkennung), sicherste zuerst, mit Personenfilter, Referenzporträt und den Aktionen
  „Alle sind X“ und „Keines ist X“.
- **„Benennen“** berechnet den Vorschlag aus der ganzen Gruppe statt aus drei Gesichtern und
  hebt einen sicheren Vorschlag als „Ist das X?“ hervor.
- **Kopf** schrumpft auf eine Statuszeile mit einer Hauptaktion.
- **Navigation:** Hauptpunkte Importieren, Sammlung, Gesichter, Personen. „Bestätigt“ und
  „Ignoriert“ werden ein Umschalter; Stammbaum, Graph und Zeitreise werden Ansichten der
  gewählten Person.
- Keine IDs als Beschriftung; Tastenkürzel für Bestätigen, Vorschläge, Ablehnen, Rücknahme.

## Grenzen

Gesichter ohne Namensmerkmale (Qualität unter 0,5) erscheinen nicht in diesen Reitern, nur
am Foto. Die Kachelablage wächst unbegrenzt und ist löschbar. Tastatur- und
Screenreader-Bedienung sind nicht langfristig geprüft.

## Beim Ändern beachten

`GROUP_MAX`, Kachelspalten und Seitenabfragen gemeinsam ändern. Jede Stelle, die Gesichter
entscheidet, nutzt `queue_hiding`. Namensentwürfe gelten für genau diese Gesichter und Person
und überleben Fortschrittsereignisse. Gruppenaktionen erzeugen keine zusätzlichen Personen
und löschen keine Vorkommen. Seitenabfragen müssen zu den Teilindizes passen
(`tests/face_save_performance.rs`).
