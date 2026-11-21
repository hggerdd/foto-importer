# Personen und Beziehungen

## Entscheidung

**Personen**

- Personen haben stabile IDs; Name, Geburtsjahr, Geschlecht und Notiz sind Attribute
  (`person_attrs`). Gesichter verweisen auf die ID. Gleichnamige Personen sind erlaubt und
  ausdrücklich auswählbar.
- Es gibt kein Löschen. Zusammenführen hängt Zuordnungen um und setzt `merged_into`; die alte
  ID bleibt als Weiterleitung. Die Oberfläche verlangt eine Bestätigung.
- Personen lassen sich ohne Gesicht anlegen (Name Pflicht, höchstens 120 Zeichen;
  Geburtsjahr 1000 bis 3000; Notiz höchstens 500 Zeichen).

**Beziehungskatalog** (`relations::CATALOG`, einzige Quelle)

| ID | Typ | Rolle |
|---|---|---|
| 1 | Elternteil von | Basisfakt, kanonisch gespeichert (Typ 2 „Kind von“ wird nur gelesen) |
| 3 | Partner von | Basisfakt, symmetrisch |
| 8 | Früherer Partner von | Basisfakt, symmetrisch |
| 6, 7 | Freund von, Bekannter von | nicht familiär, symmetrisch |
| 4, 5 | Geschwister von, Cousin von | nur Behelf, wenn die Eltern unbekannt sind |

- Jede Beziehung steht einmal in `relations`; symmetrische Typen mit kleinerer ID zuerst.
  Eingabe aus beiden Richtungen ergibt denselben Datensatz. Neue Typen sind Zeilen in
  `relation_types` und brauchen keine Schemaänderung.
- Die Beschriftung richtet sich nach dem Geschlecht der Person, die die Rolle trägt;
  unbekannt ergibt die neutrale Form.
- **Prüfung** (`validate_add`, `Validator`): kein Selbstbezug, keine Abstammungszyklen;
  Eltern-Kind, Partner, Geschwister und Cousin schließen sich je Paar aus (Ausnahme Partner
  und Cousin); Freund und Bekannter schließen sich aus; Partner und früherer Partner
  schließen sich aus; kein Partner, Geschwister oder Cousin zwischen Vorfahr und Nachkomme;
  kein Behelf, der dem Baum widerspricht oder bereits aus ihm folgt. Mehr als zwei Eltern
  bleiben erlaubt. Zusammenführen prüft jede umgehängte Kante und lehnt bei Widerspruch ganz ab.

**Berechnete Verwandtschaft** (`kinship`)

- Alles außer den Basisfakten wird berechnet, nie gespeichert: Geschwister, Halbgeschwister,
  Großeltern bis acht Generationen, Onkel/Tante, Nichte/Neffe, Cousins aller Grade,
  Stief- und Schwiegerfamilie (über genau einen aktuellen Partner). Blutsverwandtschaft hat
  Vorrang vor Verschwägerung. `relationship(a,b)` und `relationship(b,a)` sind stets invers.
- Behelfskanten tragen nicht zur Berechnung bei. Folgt dieselbe Beziehung aus dem Baum, wird
  die Behelfszeile in der Anzeige unterdrückt, nicht gelöscht.

**Rückfragen** (`gaps`)

- Die App ergänzt nichts still. Sie fragt: „Sind C und D auch Eltern von B?“ (Geschwister
  mit unvollständigen Eltern), „Ist D auch Elternteil?“ (Partner ohne eigene Kinder) und „Ist
  Y auch mit Z befreundet?“ (Partner eines Freundes). Höchstens 30 Fragen, nie etwas, das die
  Prüfung ablehnen würde, nie bereits Entschiedenes.
- „Ja“ legt die Kante mit voller Prüfung an. „Nein“ wird in `face_settings` gespeichert
  (`family.declined_parents`, `family.declined_friends`) und kehrt nie wieder.
- **„Möglicherweise dieselbe Person“** (`duplicates`): Paare, deren Referenzgesichter (dieselbe
  Auswahl wie der Abgleich, `recognition::reference_vectors`) einander sehr ähnlich sind.
  Ähnlichkeit = Median über die Referenzen beider Personen von „Mittel der drei besten
  Kosinuswerte gegen die andere Person“; ab `config::DUPLICATE_PERSON_MIN` (0,55, Messung
  dort), höchstens `DUPLICATE_PERSON_MAX` Fragen, ähnlichste zuerst. Nie gefragt: Paare mit
  gemeinsam bestätigtem Foto, mit eingetragener Beziehung, abgelehnte und zusammengeführte
  Personen. Berechnung auf einem Worker mit Revision, neu nur bei geändertem Datenstand
  (Personen, Beziehungen, bestätigte Gesichter, Referenzen, Modell), nicht beim Wechsel der
  Ansicht. „Zusammenführen …“ öffnet die bestehende Bestätigung in den Details der Person mit
  weniger Gesichtern (Ziel: die andere); erst „Ja, zusammenführen“ schreibt. „Verschieden“
  blendet sofort aus und speichert das Paar in `face_settings` (`people.declined_duplicates`);
  es kehrt nie wieder. Diese Fragen stehen oben unter „Rückfragen“ und in den Details beider
  Personen, zählen im Knopf „Rückfragen“ mit, nicht aber in der Tabellenspalte „Hinweise“.

**Oberfläche**

- **Ansicht „Personen“:** Die Übersicht mit der Personentabelle ist der Hauptinhalt. Der
  Umschalter „Übersicht | Stammbaum | Graph | Zeitreise | Gesichter“ gilt der in der Tabelle
  gewählten Person: Stammbaum, Graph, [Zeitreise](zeitreise.md) und Gesichter öffnen sich für
  sie (`set_person_mode`); neben dem Umschalter steht ihr Name. Wer in einer dieser Ansichten
  eine andere Person wählt, wählt sie für alle. Ohne gewählte Person nennen die drei
  Ansichten den Weg zur Auswahl und arbeiten wie zuvor mit ihrer eigenen Vorgabe
  (Stammbaum: Person mit den meisten Familienbeziehungen; Graph: alle; Zeitreise: keine,
  die Personenauswahl steht sofort bereit). Die Auswahl endet mit dem Wechsel der Sammlung.
- **Übersicht (Personen & Beziehungen):** sortierbare, virtualisierte Tabelle links (Porträt, Name,
  Fotos, Gesichter, Geburt und Alter, Geschlecht, Beziehungen, Familie, erstes und letztes
  Foto, Hinweise; Spalten entfallen bei wenig Breite nach Priorität), Details der gewählten
  Person rechts. Je Spalte genau ein Scrollbereich. Rückfragen stehen hier gesammelt.
- **Personendetails, Beziehungen** (`kinship::overview`): gespeicherte und aus dem Stammbaum
  berechnete Beziehungen in einer Liste, je Beziehung eine Überschrift (Mehrzahl,
  geschlechtsneutral, z. B. „Eltern“), darunter die Personen nach Name; der Eintrag nennt
  die genaue Bezeichnung („Mutter“). Reihenfolge der Gruppen (`kinship::overview_rank`):
  Partner; Blutsverwandte nach Schrittzahl, bei Gleichstand Seitenlinie vor gerader Linie
  (Eltern, Kinder, Geschwister, Halbgeschwister, Großeltern, Enkel, Onkel/Tanten,
  Nichten/Neffen, Urgroßeltern, Cousins …); Stief- und Schwiegerverwandte; direkt erfasste
  Behelfsverbindungen; frühere Partner (nicht mehr Teil der Familie, daher nach ihr); Freunde;
  Bekannte. Gespeicherte Einträge haben „Entfernen“, berechnete sind kursiv und nicht
  bearbeitbar; berechnete Partner, Eltern und Kinder wiederholen die gespeicherten nicht.
- **Personendetails, Zusatzangaben** (`person_insights`): ein Leseauftrag je Person und
  Datenstand (`face_page_serial`) auf einem Worker mit Revision, nie je Bild. Fehler bleiben
  still; der Abschnitt bleibt leer. Nur für Personen mit Fotos.
  - **Häufig zusammen mit:** bis zu acht Personen (`config::PERSON_COMPANIONS`) mit den meisten
    gemeinsamen Fotos, gezählt wie der Graph und die Sammlungssuche
    (`relationship_graph::for_person`). Der Name wählt die Person, die Zahl öffnet die Sammlung
    mit „alle diese Personen“ = beide.
  - **Fotos je Jahr:** ein Balken je Aufnahmejahr (`dt_original`, gleiche Zählung) vom ersten
    bis zum letzten Jahr, Lückenjahre leer; mit Geburtsjahr nennt der Hinweistext das Alter
    (Jahr minus Geburtsjahr, daher „etwa“), die Ränder zeigen Jahr und Alter. Ein Klick öffnet
    die Sammlung mit der Person und dem Datumsfilter dieses Jahres. Jahre außerhalb
    `config::PERSON_YEARS` und fehlende Daten zählen als „ohne Datum“.
- **Gesichter einer Person** (`person_faces_view`, „Gesichter prüfen“ unter dem Namen in den
  Personendetails oder der Umschalter): oben eine Personenauswahl (gilt wie die Tabellenwahl
  für alle Ansichten), darunter „Wahrscheinlich X · N | Bestätigt · N“. Beide Seiten nutzen
  die Gruppenkarten, Seiten zu acht Gruppen, Rücknahme, sofortiges Ausblenden und den Leser
  der Ansicht „Gesichter“ (`faces::page_with`, Revision `face_revision`).
  - **Wahrscheinlich X** (Modus `probable`, `recognition::person_candidates`, siehe
    Gesichtserkennung): zwei Abschnitte, jeder nach Personenscore für X absteigend und für
    sich in Gruppen zu 28 geschnitten (keine Gruppe mischt sie). Zuerst die Gesichter, deren
    beste Person X ist; dann unter der Überschrift „Eher jemand anderes – X ist nicht am
    wahrscheinlichsten“ die, bei denen eine andere Person vorn liegt. Dort nennt jede Kachel
    „eher: Y (0,62)“ und die Kopfzeile der Karte die anderen Personen mit Anzahl. Der Reiter
    zeigt „Wahrscheinlich X · N (+M eher andere)“. Karte, Knöpfe und Tasten wie „Vorschläge
    prüfen“ für diese Person („Alle N sind X“, abgewählte werden abgelehnt; „Keines ist X“;
    „Nicht interessiert“). Nach jeder Entscheidung liest die Seite neu; bestätigte Gesichter
    ändern die Referenzen erst mit der Wartung.
  - **Bestätigt** (Modus `person-confirmed`): bestätigte Gesichter der Person. Falsche
    abwählen, dann „N abgewählte sind nicht X“: nur diese erhalten die Ablehnung und werden
    unbenannt (unter „Benennen“); die übrigen bleiben samt Referenzauswahl unberührt.
    „Keines ist X“ gilt für die ganze Karte.
  - Wechsel von Person, Seite oder Ansicht verwirft die angezeigten Karten bis zum neuen
    Lesen, damit keine Karte einer anderen Seite entschieden wird.
- **Tippfilter-Auswahlfeld** (`select::FilterSelect`) für jede Personenauswahl: Suchfeld mit
  Fokus, Präfix vor Wortanfang vor Teiltreffer, Umlaute tolerant, Pfeiltasten und Enter.
- **Stammbaum:** Fokus-Person mit Partnern, Vorfahren (bis sechs Generationen), Geschwistern
  und Nachkommen, höchstens 300 Personen. Das Layout ist eine reine, deterministische
  Funktion (`tree_layout`). Textalternative „Als Liste anzeigen“; Verwandtschaftsprüfung für
  zwei beliebige Personen.
- **Graph:** alle Personen als Netzwerk, höchstens 600 Personen und 6000 Kanten. Zwei
  einzeln schaltbare Ebenen: eingetragene Beziehungen (nach Typ, nie nur über Farbe
  unterschieden) und gemeinsame Fotos (Linienstärke logarithmisch, Schwelle „ab Fotos“).
  Gemeinsame Fotos zählen wie die Sammlungssuche: eindeutige Fotos, bestätigte Zuordnungen,
  keine historischen Gesichter. Berechnung auf einem Worker mit Revision; Layout
  deterministisch (`graph_layout`). Die Paare Foto–Person werden einmal gelesen; Fotos je
  Person und gemeinsame Fotos je Paar zählt Rust. Porträts wie in der Personentabelle
  (bestes bestätigtes Gesicht, ohne Merkmalsvektoren).
  - **Lokaler Cache und Fingerabdruck** (`relationship_graph::signature`, `refresh`): Der
    zuletzt berechnete Graph liegt mit seinem Fingerabdruck im lokalen Cache
    (`graph-<Hash>.json`) und erscheint beim ersten Öffnen sofort („wird aktualisiert …“).
    Der Worker prüft den Fingerabdruck (Stand der Sammlung, Personen, Personenangaben,
    Beziehungen, Beziehungstypen, Anzahl und Summen bestätigter Gesichter über die Teilindizes
    `ix_faces_user_counts` und `ix_faces_reference`) und rechnet nur bei Abweichung neu; so
    auch nach Schreibaufträgen, die den Graphen nicht berühren. „Aktualisieren“ rechnet immer
    neu. Messung 2026-11-21 an `E:\Bilder`: bisher kalt rund 8 s ohne Konkurrenz (Paare 3,2 s,
    Porträts mit Vektoren 4,5 s), im Log 43 s neben anderen Aufträgen; Fingerabdruck warm
    unter 20 ms. Berechnete Verwandtschaft wird nie als Kante gezeigt
  (Ausnahme wie bisher: Geschwister aus gemeinsamen Eltern).
  - **Layout-Gewicht** (`relationship_graph::layout_weight`): gemeinsame Fotos `1 + ln(n)`,
    eingetragene Beziehung `+1,5`, mal Nähefaktor der engsten Beziehung des Paares
    (`config::GRAPH_*`): Eltern/Kind und Partner 2,5, Geschwister und Cousin 1,6, Freunde und
    reine Fotokanten 1,0, Bekannte und frühere Partner 0,35. So rücken Familie unabhängig von
    der Fotozahl zusammen und Bekannte auseinander. Nur gezeichnete Kanten zählen; sonstige
    berechnete Verwandtschaft beeinflusst das Layout nicht.
  - **Verbindung hinzufügen:** Umschalter in der Werkzeugleiste. Erster Klick merkt eine
    Person, erneuter Klick hebt das auf, Klick auf eine zweite öffnet am Klickpunkt ein Menü
    mit allen Typen in beiden Richtungen („A ist Mutter von B“, „B ist Vater von A“, Behelf
    gekennzeichnet). Was `validate_add` auf dem geladenen Bestand ablehnt, ist gesperrt und
    nennt den Grund. Gespeichert wird über denselben Weg wie in der Übersicht
    (`save_relation` → `central::add_relation_synced`, Schreibauftrag auf dem Worker, Hinweis,
    Graph neu berechnet); der Modus bleibt für das nächste Paar an. „Abbrechen“, Esc und Klick
    auf den Hintergrund verwerfen das Paar, ein weiteres Esc beendet den Modus. Ziehen und Zoom
    bleiben; die normale Auswahl per Klick gilt nur außerhalb des Modus.

## Grenzen

Die Schwelle für „dieselbe Person“ ist an einer Sammlung mit SFace gemessen; Duplikate mit
nur wenigen Gesichtern erreichen sie oft nicht (drei Gesichter im Median 0,54). Vorhandene
widersprüchliche Altdaten werden nicht repariert. Ein früherer Partner muss als
solcher erfasst sein, sonst begründet er Stiefbeziehungen. Bei Mehrfachverwandtschaft gilt
die nächste. Zeichnungen sind nicht mit Screenreader geprüft; Liste und Seitenfeld sind die
Alternative.

## Beim Ändern beachten

Katalog-IDs nie umdeuten. Tiefe und Kappung der Berechnung nur gemeinsam mit
`tests/kinship.rs` ändern. Neue Rückfragemuster brauchen Prüfung, Begrenzung und
Wiederholungsschutz. Neue Tabellenspalten brauchen Breite, Priorität, Sortierwert und Test in
`core::people_table`. Schwere Zählungen gehören auf den Worker (`faces::page`).
