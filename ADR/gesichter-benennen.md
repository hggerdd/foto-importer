# Gesichter benennen und prüfen

Wie Vorschläge und Gruppen entstehen: siehe [Gesichtserkennung](gesichtserkennung.md).

## Entscheidung

- **Reiter der Ansicht „Gesichter“:** „Benennen“ (Gruppen unbenannter Gesichter, größte
  zuerst), „Vorschläge prüfen“ (`suggested` und `auto` je Person, sicherste Gruppen zuerst;
  Schwelle und Abstand siehe Gesichtserkennung) und „Erledigt“ mit dem Umschalter
  „Bestätigt | Ignoriert“; „Erledigt“ öffnet die zuletzt gewählte der beiden Seiten. Die
  Reiter nennen ihre Anzahl. Personen, Stammbaum, Graph und Zeitreise liegen unter
  „Personen“ (siehe [Personen](personen.md)). Acht
  Gruppen je Seite, höchstens 28 Gesichter je Gruppe; alle Mitglieder sind sichtbar, es gibt
  keine versteckten ausgewählten Gesichter.
- **Gruppenkarte:** Namensfeld mit Enter, Personenauswahl mit Tippfilter, bis zu drei
  Vorschläge mit Porträt (ein Klick wählt und speichert), „Bestätigen“, „Nicht interessiert“.
  Der Vorschlag ist das Mittel über bis zu fünf Gesichter der Gruppe
  (`config::GROUP_SUGGESTION_FACES`).
  Alle Gesichter sind zunächst ausgewählt; ein Klick auf die Kachel wählt sie ab (abgedunkelt
  und durchgestrichen), ein zweiter wieder an. Die Gesichts-ID steht nur im Tooltip. Unter
  einer Kachel steht „nicht X“, wenn für das Gesicht Ablehnungen gespeichert sind (eine
  Abfrage je Seite). Kachelgröße ist per Regler einstellbar, die Spaltenzahl folgt der
  Fensterbreite.
- **Karte in „Vorschläge prüfen“:** statt Namensfeld und Personenauswahl das Referenzporträt
  der vorgeschlagenen Person (`faces::person_portrait`, je Seite einmal geladen), ihr Name,
  die Anzahl und der Wertebereich der Gruppe („Ähnlichkeit 0,62 bis 0,81“). Aktionen:
  „Alle N sind X“ bestätigt die Gruppe, „Keines ist X“ lehnt die Person für alle Gesichter
  ab, „Nicht interessiert“ wie in „Benennen“. Sind Kacheln abgewählt, heißt der erste Knopf
  „N sind X“ und lehnt die abgewählten ab. Wer eine andere Person meint, lehnt ab und benennt
  die Gesichter unter „Benennen“. Über den Gruppen steht ein Personenfilter mit Tippfilter;
  er nennt nur Personen mit offenen Vorschlägen samt Anzahl, „Alle Personen zeigen“ hebt ihn
  auf. Der Filter gilt für die Sitzung und endet mit dem Wechsel der Sammlung.
- **„Ist das X?“ in „Benennen“:** Besteht der erste Vorschlag der Gruppe dieselbe Regel wie
  der Abgleich (`recognition::Limits::sure_suggestion` über `accept`, mit Schwelle und Abstand
  der Sammlung aus `faces::Status`), steht über den Kacheln eine hervorgehobene Zeile mit
  Porträt, „Ist das X?“ und den Knöpfen „Ja“ (wie ein Klick auf den Vorschlag) und „Nein“
  (keines der Gesichter ist X, wie „Keines ist X“). Namensfeld, Auswahl und die übrigen
  Vorschläge bleiben darunter. Ohne sicheren Vorschlag sieht die Karte aus wie sonst.
- **Tasten** gelten für die oberste Gruppe der Seite, nur ohne Fokus in einem Feld und
  solange die Knöpfe aktiv wären: `J` bestätigt den Vorschlag und `N` lehnt ihn ab (in
  „Vorschläge prüfen“ und in „Benennen“, dort nur bei einem sicheren Vorschlag), `1` bis `3` übernehmen einen der Vorschläge in „Benennen“,
  `Entf` heißt „Nicht interessiert“ (beide Reiter). „Bestätigt“ und „Ignoriert“ haben keine
  Tasten. Die Hinweistexte der Knöpfe der obersten Gruppe nennen die Taste. Nach einer
  Entscheidung per Taste erhält das Namensfeld der nächsten Gruppe keinen Fokus, damit die
  Tasten weiter gelten; nach einem Klick oder Enter erhält es ihn wie bisher.
- **Einzeln benennen:** Jede Gesichtskachel (Gruppenkarten in „Benennen“, „Vorschläge
  prüfen“ und „Erledigt“, Referenzporträt der Karte und von „Ist das X?“, Porträt in den
  Personendetails, Kacheln, Abschnittsansicht und große Vorschau der Zeitreise) hat ein
  Rechtsklickmenü „Einzeln benennen …“; der Hinweistext nennt „Rechtsklick: einzeln
  benennen“. Der Linksklick bleibt unverändert (Auswahl bzw. Fokus). Der Eintrag öffnet
  denselben Dialog wie in den Bilddetails der Sammlung; er ist ein globales Fenster, hängt an
  keiner Ansicht und liest seine Vorschläge mit eigener Revision. „Übernehmen“ setzt nur
  dieses Gesicht auf `user` (eine bestehende Ablehnung dieser Person entfällt), andere
  Gesichter der Gruppe erhalten keine Ablehnung. Der Dialog schließt sofort, das Gesicht
  verschwindet über `queue_hiding`; danach werden Gesichter, Personen und beim nächsten Zeigen
  die Zeitreise neu gelesen. Strg+Z nimmt die Benennung zurück. Personentabelle (Zeilen sind
  Personen), Vorschlagschips (Klick wählt die Person) und der Graph haben keinen Eintrag.
- **Bestätigen** setzt ausgewählte Gesichter auf `user`. Abgewählte erhalten eine
  gespeicherte Ablehnung für diese Person (`face_rejections`), die jeder spätere Abgleich
  beachtet. Ein neuer Name legt eine Person an; bei mehreren gleichnamigen Personen muss die
  konkrete Person gewählt werden.
- **Ablehnen wirkt sofort:** Trägt ein abgewähltes Gesicht die abgelehnte Person
  (vorgeschlagen oder bestätigt), wird es in derselben Transaktion unbenannt und gleich
  wieder gruppiert; es erscheint unter „Benennen“. Die Statusmeldung nennt das
  („2 abgelehnt, jetzt unter Benennen“). Ohne jede Auswahl gilt „keines davon ist diese
  Person“; das setzt eine bestehende Person voraus, damit keine Person ohne Gesicht entsteht.
- **Nicht interessiert** setzt `ignored=1` für die ganze Gruppe. Vorkommen, Rahmen und
  Vektoren bleiben; bestätigte Identitäten bleiben; maschinelle Zuordnungen entfallen.
  Ignorierte Gesichter sind weder Referenz noch negatives Beispiel. Interesse und Identität
  sind getrennte Entscheidungen.
- **Rücknahme:** bis zu 20 Entscheidungen der laufenden Sitzung, einschließlich Ablehnungen
  und Pins. Eine Rücknahme scheitert, wenn eine spätere Nutzeränderung dieselben Gesichter
  betrifft. Eine dabei angelegte, noch ungenutzte Person wird wieder entfernt. Knopf und
  Strg+Z lösen dieselbe Rücknahme aus; Strg+Z nur ohne Textfeldfokus und wenn der Knopf
  aktiv wäre.
- **Sofortiges Speichern:** Entschiedene Gesichter verschwinden sofort aus der Seite
  (`queue_hiding`), der Schreibauftrag läuft dahinter in Eingabereihenfolge. Der
  Ausblendstand ergibt sich aus den laufenden Aufträgen; bei einem Fehler erscheinen die
  Gesichter wieder. Nach dem Speichern rückt die nächste Gruppe an die bisherige Position.
- **Seitenabfragen** bilden Teilgruppen aus Zählungen je Person, ohne Fensterfunktionen über
  alle Gesichter. Referenzen und Porträts werden je Seite einmal geladen und leben nur einen
  Lesestand lang.
- **Reihenfolge in „Vorschläge prüfen“:** Je Person werden die Vorschläge nach Ähnlichkeit
  absteigend in Gruppen zu höchstens 28 geschnitten; alle Gruppen sind nach ihrer mittleren
  Ähnlichkeit absteigend geordnet (Gleichstand: Person, dann Teil). Das Mittel statt des
  besten Werts, damit ein einzelner hoher Treffer keine schwache Gruppe nach vorn zieht.
  Dazu liest die Seite alle offenen Vorschläge mit einer Abfrage über `ix_faces_review`
  (`faces::REVIEW_ROWS`) und ordnet im Speicher. `faces::page_filtered` beschränkt
  „Vorschläge prüfen“, „Bestätigt“ und „Ignoriert“ auf eine Person; „Bestätigt“ und
  „Ignoriert“ bleiben nach Größe geordnet.
- **Gesichtskacheln:** Ausschnitte für die Oberfläche liegen als abgeleitete Dateien unter
  `<Sammlung>/.myfotomanager_tiles/` (Schlüssel aus Pfad, Größe, Änderungszeit, Ausschnitt,
  Kantenlänge, Ausrichtung). Sie werden nie ersetzt, vom Indizieren übersprungen und von
  „Biometrie entfernen“ gelöscht.
- **Vorschau unter der Maus:** Das ganze Foto zum Gesicht unter dem Zeiger, abschaltbar,
  höchstens eine Vorschau gleichzeitig.
- **Kopf der Ansicht:** eine Statuszeile mit höchstens einer Hauptaktion
  (`recognition_view::status_line`, reine Funktion): was gerade läuft, sonst „Einrichten …“
  bei fehlender Laufzeit oder fehlenden Modellen, „N von M Fotos analysiert“ mit „Fotos
  analysieren“ oder „Weiter analysieren“, „Fortsetzen“ bei angehaltenem Vorschlagen, sonst
  „Alles auf dem Stand“ ohne Knopf. Daneben der Einstieg in den Dialog „Automatik &
  Werkzeuge“: dort die drei Schritte (Fotoanalyse mit Portion und Ordnerbezug, aus Namen
  lernen, Namen vorschlagen) mit Stand und Aktionen, darunter Einrichtung, Zeitplan, Export
  und „Biometrie entfernen“.
- **Laufender Gesichtsauftrag nur einmal:** Fortschritt und Abbruch zeigt die Fußleiste;
  die Statuszeile nennt den Auftrag nur in Worten. Zeigt die Fußleiste einen anderen
  Auftrag, steht der Fortschritt mit Abbruch in der Statuszeile.

## Grenzen

Gesichter ohne Namensmerkmale (Qualität unter 0,5) erscheinen nicht in diesen Reitern, nur
am Foto; „Benennen“ nennt ihre Anzahl. Die Kachelablage wächst unbegrenzt und ist löschbar. Tastatur- und
Screenreader-Bedienung sind nicht langfristig geprüft.

## Beim Ändern beachten

`GROUP_MAX`, Kachelspalten und Seitenabfragen gemeinsam ändern. Neue Zustände der
Erkennung brauchen einen Zweig und einen Test in `status_line`; die Zeile behält höchstens
eine Hauptaktion. Jede Stelle, die Gesichter
entscheidet, nutzt `queue_hiding`. Namensentwürfe gelten für genau diese Gesichter und Person
und überleben Fortschrittsereignisse. Gruppenaktionen erzeugen keine zusätzlichen Personen
und löschen keine Vorkommen. Seitenabfragen müssen zu den Teilindizes passen
(`tests/face_save_performance.rs`).
