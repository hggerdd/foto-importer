# Gesichter benennen und prüfen

Wie Vorschläge und Gruppen entstehen: siehe [Gesichtserkennung](gesichtserkennung.md).

## Entscheidung

- **Reiter der Ansicht „Gesichter“:** „Benennen“ (Gruppen unbenannter Gesichter, größte
  zuerst), „Vorschläge prüfen“ (`suggested` und `auto` je Person, sicherste Gruppen zuerst;
  Schwelle und Abstand siehe Gesichtserkennung) und „Erledigt“ mit dem Umschalter
  „Bestätigt | Ignoriert“; „Erledigt“ öffnet die zuletzt gewählte der beiden Seiten; dazu
  „Ungeeignet“ (siehe unten). Die Reiter nennen ihre Anzahl. Personen, Stammbaum, Graph,
  Zeitreise und die Gesichter einer Person liegen unter „Personen“ (siehe
  [Personen](personen.md)). Alle Mitglieder einer Gruppe sind sichtbar, es gibt keine
  versteckten ausgewählten Gesichter.
- **Gruppengröße:** Der Regler „Gruppengröße“ in der Werkzeugzeile über den Gruppen neben
  der Kachelgröße (`face_toolbar`, auch auf der Gesichterseite einer Person) wählt 5 bis 100
  Gesichter je Gruppe (`config::GROUP_SIZE_MIN`, `GROUP_SIZE_MAX`, Standard
  `GROUP_SIZE_DEFAULT` = 28; Grenzen nicht gemessen). Gespeichert als
  `Settings::face_group_size`, ein fehlender Eintrag heißt 28, Werte außerhalb werden geklemmt
  (`config::group_size`). Ein Parameter von `faces::page_with` schneidet damit alle Reiter mit
  Gruppen: „Vorschläge prüfen“, „Benennen“, „Bestätigt“, „Ignoriert“, „Ungeeignet“ und alle
  drei Seiten der Personengesichter. Eine Seite hält höchstens 224 Gesichter
  (`config::FACE_PAGE_FACES` = acht Gruppen zu 28) und höchstens acht Gruppen
  (`config::face_groups_per_page`: 5 und 28 → 8, 50 → 4, 100 → 2), damit große Gruppen
  Ladezeit, Kacheln und Ablehnungsabfrage der Seite nicht vervielfachen. Erst beim Loslassen
  des Reglers wird gespeichert und ab Seite 1 neu gelesen (einfach und robust; eine
  umgerechnete Position wäre in „Vorschläge prüfen“ ungenau, weil die Ordnung vom
  Gruppenmittel abhängt). Namensentwürfe und Abwahl hängen an genau ihren Gesichtern und
  ihrer Person (`draft_key`) und gelten nur für eine Gruppe mit denselben Gesichtern weiter;
  die Rücknahme hängt an Gesichts-IDs, nicht an Gruppen. Eine Entscheidung umfasst höchstens
  `GROUP_SIZE_MAX` Gesichter (`name_group`).
- **„Benennen“ und Gruppengröße:** Die gespeicherten Ähnlichkeitsgruppen (höchstens
  `config::CLUSTER_MAX` = 28) bleiben die Einheit. Ist die Gruppengröße kleiner, erscheint eine
  Gruppe in aufeinanderfolgenden Teilen (nach Gesichts-ID); ist sie größer, wird nichts
  zusammengelegt, weil zwei gespeicherte Gruppen einander nicht ähnlich genug sein müssen
  (Complete-Link) und eine gemischte Karte falsche Bestätigungen nahelegte. Große Gruppen
  lohnen sich daher vor allem in „Vorschläge prüfen“ und „Wahrscheinlich X“.
- **Ordnerfilter:** Über der Werkzeugzeile aller Gesichterseiten (`face_folder_bar`, auch auf
  der Gesichterseite einer Person) wählt ein Feld mit Tippfilter (`select::FilterSelect`,
  Einträge aus dem Ordnerbaum der Sammlung samt Fotozahl) „Alle Ordner“ oder einen Ordner;
  der Ordnerbaum der Sammlung bietet dazu im Rechtsklickmenü „Gesichter dieses Ordners
  zuordnen“ (öffnet „Gesichter“ im zuletzt gewählten Reiter mit diesem Filter). Ein Gesicht
  gehört zum Ordner, wenn seine Analysequelle (`faces.source_relpath`) im Ordner oder darunter
  liegt (`faces::FaceFolder`; Bereich `ordner/` ≤ Pfad < `ordner0`, also „Party/jpg“ ja,
  „Party2“ nie). Ein Parameter `folder` von `faces::page_with` filtert alle Seiten mit
  Gruppen: „Vorschläge prüfen“ (samt Zahlen des Personenfilters), „Bestätigt“, „Ignoriert“,
  „Ungeeignet“, „Benennen“ und „Wahrscheinlich X“ (beide Abschnitte), „Bestätigt“ und „Nicht
  X“ einer Person. Vorschläge, Referenzen, Werte, Abschnitte und Ordnung bleiben
  sammlungsweit; der Filter schränkt nur die gezeigten Gesichter ein. In „Benennen“ zeigt jede
  gespeicherte Ähnlichkeitsgruppe nur ihre Gesichter aus dem Ordner (größte im Ordner zuerst),
  leere entfallen, Gruppen werden nie gemischt. Die Reiter und Umschalter nennen die Zahlen
  des Ordners (`FacePage::folder`, `faces::FolderCounts`), die Statuszeile bleibt
  sammlungsweit. Hat der Ordner nicht analysierte Fotos (Analysequelle vorhanden, keine
  fertige Erkennung des aktiven Detektors), steht darunter „N Fotos dieses Ordners noch nicht
  analysiert“ mit „Diesen Ordner analysieren“: dieselbe Fotoanalyse wie „Fotos analysieren“
  mit Ordnerbezug auf genau diesen Ordner, ohne Portion. Der Filter gilt nur für die Sitzung
  und endet mit dem Wechsel der Sammlung (nicht gespeichert, damit beim nächsten Start nicht
  unbemerkt gefiltert ist); solange er gilt, steht „Nur Ordner:“ hervorgehoben mit dem Ordner
  und ✕ (alle Ordner) in der Leiste. Ein Wechsel liest ab Seite 1 neu.
- **Gruppenkarte:** Namensfeld mit Enter, Personenauswahl mit Tippfilter, bis zu drei
  Vorschläge mit Porträt (ein Klick wählt und speichert), „Bestätigen“, „Nicht interessiert“.
  Der Vorschlag ist das Mittel über bis zu fünf Gesichter der Gruppe
  (`config::GROUP_SUGGESTION_FACES`).
  Alle Gesichter sind zunächst ausgewählt; ein Klick auf die Kachel wählt sie ab (abgedunkelt
  und durchgestrichen), ein zweiter wieder an. Die Gesichts-ID steht nur im Tooltip. Unter
  einer Kachel steht „nicht X“, wenn für das Gesicht Ablehnungen gespeichert sind (eine
  Abfrage je Seite). Kachelgröße ist per Regler einstellbar, die Spaltenzahl folgt der
  Fensterbreite (höchstens `GROUP_SIZE_MAX` Spalten); 100er-Gruppen stehen in mehreren Reihen
  und werden gescrollt.
- **Karte in „Vorschläge prüfen“:** statt Namensfeld und Personenauswahl das Referenzporträt
  der vorgeschlagenen Person (`faces::person_portrait`, je Seite einmal geladen), ihr Name,
  die Anzahl und der Wertebereich der Gruppe („Ähnlichkeit 0,62 bis 0,81“). Aktionen:
  „Alle N sind X“ bestätigt die Gruppe, „Keines ist X“ lehnt die Person für alle Gesichter
  ab, „Nicht interessiert“ wie in „Benennen“. Sind Kacheln abgewählt, heißt der erste Knopf
  „N sind X“ und lehnt die abgewählten ab. Wer eine andere Person meint, lehnt ab und benennt
  die Gesichter unter „Benennen“. Über den Gruppen steht ein Personenfilter mit Tippfilter;
  er nennt nur Personen mit offenen Vorschlägen samt Anzahl, „Alle Personen zeigen“ hebt ihn
  auf. Der Filter gilt für die Sitzung und endet mit dem Wechsel der Sammlung.
- **Reiter „Ungeeignet“** (`faces::page`, Modus `unsuitable`): genau die offenen Gesichter
  ohne Gruppe des aktiven Modells (`faces::UNSUITABLE`), also die Zahl
  `Status::unsuitable` = offen minus gruppiert, die „Benennen“ als Hinweis nennt; der Hinweis
  hat den Knopf „Ungeeignete Gesichter ansehen“. Neueste Fotos zuerst, Gesichter eines Fotos
  nebeneinander, Gruppen der gewählten Größe; keine Namensvorschläge. Hier beginnen alle
  Kacheln abgewählt, ein Klick wählt aus (hervorgehoben, nicht durchgestrichen); „Alle
  auswählen“ und „Auswahl aufheben“ gelten für die Karte. „N benennen“ (Name oder Person)
  benennt nur die ausgewählten (`name_group` mit genau diesen IDs), die übrigen erhalten keine
  Ablehnung. „Kein Gesicht“ speichert für die ausgewählten die Nutzerentscheidung
  `faces::mark_not_face` (siehe Gesichtserkennung). Beides blendet sofort aus und ist mit
  Strg+Z zurückzunehmen; nach einer Entscheidung hat die verkleinerte Karte eine neue,
  leere Auswahl.
- **Rechtsklickmenü der Gruppenkacheln:** „Einzeln benennen …“ und „Foto in der Sammlung
  zeigen“; Letzteres öffnet die Sammlung mit der Suche nach Ordner und Dateiname ohne Endung
  (trifft auch RAW/DNG desselben Fotos), das erste Ergebnis ist gewählt.
- **Kachelsymbole:** Jede Gruppenkachel trägt oben rechts dauerhaft dezente, unter der Maus
  hervorgehobene Symbole (eine gemeinsame Funktion `tile_icons` für alle Gruppenkarten),
  von links: durchgestrichener Kreis („nicht interessiert“), Kreuz, Haken, Fragezeichen. Der
  Kreis steht auf allen Seiten außer „Ignoriert“; Doppelklick ignoriert nur dieses Gesicht
  (`Decision::OneIgnore`, derselbe Weg wie „Nicht interessiert“: maschinelle Zuordnung
  entfällt, bestätigte Identität bleibt, keine Ablehnung). Haken und Kreuz gibt es nur, wenn die Gruppe eine
  Zielperson X hat (`tile_target`): in „Vorschläge prüfen“ und „Wahrscheinlich X“ die
  vorgeschlagene Person, in „Benennen“ der sichere Vorschlag von „Ist das X?“. Auf Seiten
  bestätigter Gesichter („Bestätigt“ und „Bestätigt“ einer Person) ist X die Person der
  Gruppe, dort gibt es nur das Kreuz. Auf „Nicht X“ einer Person (`TileTarget::lift`) tragen
  die Gesichter die Ablehnung schon: Der Haken heißt „das ist doch X“ (Ablehnung entfällt, `user`
  wie oben), das Kreuz „Ablehnung aufheben“ (`Decision::OneLift`, nur die Ablehnung, siehe
  [Personen](personen.md)). „Ungeeignet“, „Ignoriert“ und Gruppen ohne Zielperson
  haben weder Haken noch Kreuz. **Doppelklick** auf den Haken bestätigt genau dieses Gesicht
  als X (`user`), Doppelklick auf das Kreuz lehnt X für genau dieses Gesicht ab (gespeicherte
  Ablehnung; trug es X, wird es unbenannt) – derselbe Weg wie „Bestätigen“ bzw. „Keines ist
  X“ (`decide_group` mit `Decision::OneIs`/`OneIsNot`, `name_group` mit nur dieser ID); die
  übrigen Gesichter bleiben unberührt. Beide Klicks müssen dasselbe Symbol treffen. Ein
  Klick auf das Fragezeichen öffnet „Einzeln benennen“ wie das Rechtsklickmenü. Ein Klick
  auf ein Symbol schaltet die Auswahl der Kachel nie um; über einem Symbol ruht die
  Fotovorschau. Die Gruppe bleibt stehen, solange Gesichter übrig sind, und behält Abwahl
  und Namensentwurf; Strg+Z nimmt die Entscheidung zurück. Die Symbole lassen sich in der
  Fußzeile neben der Fotovorschau abschalten („Symbole auf den Gesichtern“,
  `Settings::tile_icons`, Standard an); Rechtsklickmenü und Gruppenknöpfe bleiben.
- **„Ist das X?“ in „Benennen“:** Besteht der erste Vorschlag der Gruppe dieselbe Regel wie
  der Abgleich (`recognition::Limits::sure_suggestion` über `accept`, mit Schwelle und Abstand
  der Sammlung aus `faces::Status`), steht über den Kacheln eine hervorgehobene Zeile mit
  Porträt, „Ist das X?“ und den Knöpfen „Ja“ (wie ein Klick auf den Vorschlag) und „Nein“
  (keines der Gesichter ist X, wie „Keines ist X“). Namensfeld, Auswahl und die übrigen
  Vorschläge bleiben darunter. Ohne sicheren Vorschlag sieht die Karte aus wie sonst.
- **Tasten** gelten für die oberste Gruppe der Seite, nur ohne Fokus in einem Feld und
  solange die Knöpfe aktiv wären: `J` bestätigt den Vorschlag und `N` lehnt ihn ab (in
  „Vorschläge prüfen“ und in „Benennen“, dort nur bei einem sicheren Vorschlag), `1` bis `3` übernehmen einen der Vorschläge in „Benennen“,
  `Entf` heißt „Nicht interessiert“ (beide Reiter; `J`, `N`, `Entf` ebenso in „Wahrscheinlich
  X“ der Personenseite). „Bestätigt“, „Ignoriert“ und „Ungeeignet“ haben keine Tasten. Die Hinweistexte der Knöpfe der obersten Gruppe nennen die Taste. Nach einer
  Entscheidung per Taste erhält das Namensfeld der nächsten Gruppe keinen Fokus, damit die
  Tasten weiter gelten; nach einem Klick oder Enter erhält es ihn wie bisher.
- **Einzeln benennen:** Jede Gesichtskachel (Gruppenkarten in „Benennen“, „Vorschläge
  prüfen“ und „Erledigt“, Referenzporträt der Karte und von „Ist das X?“, Porträt in den
  Personendetails, Kacheln, Abschnittsansicht und große Vorschau der Zeitreise) hat ein
  Rechtsklickmenü „Einzeln benennen …“; der Hinweistext nennt „Rechtsklick: einzeln
  benennen“ (Gruppenkacheln: „… oder Foto in der Sammlung zeigen“, dazu die Kachelsymbole).
  Der Linksklick bleibt unverändert (Auswahl bzw. Fokus). Der Eintrag öffnet
  denselben Dialog wie in den Bilddetails der Sammlung; er ist ein globales Fenster, hängt an
  keiner Ansicht und liest seine Vorschläge mit eigener Revision. „Übernehmen“ setzt nur
  dieses Gesicht auf `user` (eine bestehende Ablehnung dieser Person entfällt), andere
  Gesichter der Gruppe erhalten keine Ablehnung. Der Dialog schließt sofort, das Gesicht
  verschwindet über `queue_hiding`; danach werden Gesichter und Personen neu gelesen, und die
  Zeitreise folgt dem neuen Datenstand (siehe [Zeitreise](zeitreise.md)). Strg+Z nimmt die Benennung zurück. Personentabelle (Zeilen sind
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
  betrifft. Ein späterer Vorschlag des Abgleichs (`suggested`/`auto` statt vorher keiner oder
  ein anderer) ist keine Nutzeränderung und hindert sie nicht; die Rücknahme stellt den Stand
  davor her und merkt den Abgleich vor, wenn sich Ablehnungen ändern. Eine dabei angelegte, noch ungenutzte Person wird wieder entfernt. Knopf und
  Strg+Z lösen dieselbe Rücknahme aus; Strg+Z nur ohne Textfeldfokus und wenn der Knopf
  aktiv wäre.
- **Sofortiges Speichern:** Entschiedene Gesichter verschwinden sofort aus der Seite
  (`queue_hiding`), der Schreibauftrag läuft dahinter in Eingabereihenfolge. Der
  Ausblendstand ergibt sich aus den laufenden Aufträgen; bei einem Fehler erscheinen die
  Gesichter wieder. Nach dem Speichern rückt die nächste Gruppe an die bisherige Position;
  bleiben Gesichter der Gruppe übrig (Kachelsymbol, abgewählte entfernen), bleibt sie stehen.
- **Seitenabfragen** bilden Teilgruppen aus Zählungen je Person, ohne Fensterfunktionen über
  alle Gesichter. Referenzen und Porträts werden je Seite einmal geladen und leben nur einen
  Lesestand lang.
- **Reihenfolge in „Vorschläge prüfen“:** Je Person werden die Vorschläge nach Ähnlichkeit
  absteigend in Gruppen der gewählten Größe geschnitten; alle Gruppen sind nach ihrer mittleren
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
  höchstens eine Vorschau gleichzeitig. Sie hat Vorrang vor allen Kacheln der Seite und
  nutzt die Kantenlänge des Inspektors (`HOVER_PREVIEW_EDGE` = 960), damit der lokale
  Vorschau-Cache greift (siehe [Sammlungsansicht](sammlungsansicht.md)). Ein Rahmen markiert
  genau das Gesicht unter dem Zeiger (ohne Namen, keine anderen Gesichter): 2 Punkte
  Akzentfarbe mit dunklem Strich innen und außen, 4 Punkte Abstand, an den Bildrand geklemmt,
  erst wenn das Foto gezeichnet ist. Gemeinsame Stelle für alle Gesichterseiten
  (`hover_photo_preview`). Der gespeicherte Rahmen ist auf das orientierte Analysebild der
  Quelldatei normiert (0..1); die Vorschau lädt dieselbe Datei (`face.source`) mit derselben
  Ladefunktion und Ausrichtung wie die Kachel, deshalb gilt der normierte Rahmen unabhängig
  von der Kantenlänge direkt auf der gezeichneten Bildfläche. Dieselbe Umrechnung
  (`faces_view::face_frame`) nutzen die Rahmen der Bilddetails. Eine andere Datei des Fotos
  (etwa JPG statt DNG) wird für die Vorschau nicht verwendet, damit der Rahmen nie auf einem
  anders beschnittenen Bild liegt.
- **Kopf der Ansicht:** eine Statuszeile mit höchstens einer Hauptaktion
  (`recognition_view::status_line`, reine Funktion): was gerade läuft, sonst „Einrichten …“
  bei fehlender Laufzeit oder fehlenden Modellen, „N von M Fotos analysiert“ mit „Fotos
  analysieren“ oder „Weiter analysieren“, „Fortsetzen“ bei angehaltenem Vorschlagen, sonst
  „Alles auf dem Stand“ ohne Knopf. Daneben der Einstieg in den Dialog „Automatik &
  Werkzeuge“: dort die Schritte Fotoanalyse (mit Portion und Ordnerbezug), aus Namen
  lernen, Namen vorschlagen und „Genauer vergleichen (ArcFace)“ mit Stand und Aktionen,
  darunter Einrichtung, Zeitplan, Export und „Biometrie entfernen“. Der vierte Schritt nennt
  das aktive Vergleichsmodell, bietet ohne Gewichte „Genaueres Modell einrichten …“ mit
  Lizenzhinweis (nur nicht kommerziell), sonst den Stand „N von M Gesichtern genauer
  berechnet“ mit geschätzter Dauer (`config::STRONG_SECONDS_PER_PHOTO`), „Gesichter genauer
  berechnen (über Nacht)“, „Auf ArcFace umschalten“ (erst ab 98 % Abdeckung) bzw. „Zurück zu
  SFace“ (siehe [Gesichtserkennung](gesichtserkennung.md)). Den Stand liest ein Worker nur,
  solange der Dialog offen ist, höchstens alle zehn Sekunden. Regler „Strenge der
  Namensvorschläge“, Statistikachse und die Untergrenze von „Wahrscheinlich X“ folgen der Skala
  des aktiven Modells.
- **Laufender Gesichtsauftrag nur einmal:** Fortschritt und Abbruch zeigt die Fußleiste;
  die Statuszeile nennt den Auftrag nur in Worten. Zeigt die Fußleiste einen anderen
  Auftrag, steht der Fortschritt mit Abbruch in der Statuszeile.

## Grenzen

Gesichter ohne Namensmerkmale (Qualität unter 0,5) erscheinen nur unter „Ungeeignet“ und am
Foto; noch nicht gruppierte Gesichter stehen bis zur Gruppierung ebenfalls dort. Die
Kachelablage wächst unbegrenzt und ist löschbar. Kachel und Mausvorschau übergeben die
Ausrichtung 1 als Rückfall; fehlt einer Datei die eingebettete Ausrichtung, während die
Metadaten (z. B. aus XMP) eine andere nennen, sind Kachel, Vorschau und Rahmen anders
gedreht als das Analysebild und damit falsch (nicht gemessen, vermutlich selten). Tastatur- und
Screenreader-Bedienung sind nicht langfristig geprüft.

## Beim Ändern beachten

Gruppengrenzen (`GROUP_SIZE_*`, `FACE_PAGE_FACES`), Kachelspalten und Seitenabfragen
gemeinsam ändern; jede Seitenabfrage nimmt die Größe aus `page_with`, keine eigene Zahl.
`CLUSTER_MAX` getrennt davon nur mit Neuaufbau der Gruppen ändern. Neue Zustände der
Erkennung brauchen einen Zweig und einen Test in `status_line`; die Zeile behält höchstens
eine Hauptaktion. Jede Stelle, die Gesichter
entscheidet, nutzt `queue_hiding`. Namensentwürfe gelten für genau diese Gesichter und Person
und überleben Fortschrittsereignisse. Gruppenaktionen erzeugen keine zusätzlichen Personen
und löschen keine Vorkommen. Seitenabfragen müssen zu den Teilindizes passen
(`tests/face_save_performance.rs`). Eine neue Seite mit Gesichtsgruppen beachtet den
Ordnerfilter (`FaceFolder::predicate` in SQL oder die Gesichts-IDs des Ordners für
sammlungsweit berechnete Listen) und braucht einen Fall in
`tests/face_pages.rs::folder_filter_limits_every_face_page_to_the_folder_and_its_subfolders`.
Die Bedingung geht über `files.relpath` (Index) und `ix_faces_photo`; kein `LIKE` auf Pfaden
der Gesichter. Die Gruppenabfrage von „Benennen“ im Ordner hat eine feste Join-Reihenfolge
(`faces::unknown_plan`, Test): Messung 2026-11-23 an `E:\Bilder` nur lesend (66 462
Gesichter, 74 696 Fotos; dieselben Abfragen über Python-sqlite3, nicht die App), Ordner mit
6 284 Gesichtern: Ordnerzahlen kalt 1,2 s, Gruppen ohne feste Reihenfolge kalt 3,4 s, mit ihr
0,03 s warm; Ordner mit 237 Gesichtern alle Abfragen zusammen unter 0,4 s kalt.
