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

**Oberfläche**

- **Personen & Beziehungen:** sortierbare, virtualisierte Tabelle links (Porträt, Name,
  Fotos, Gesichter, Geburt und Alter, Geschlecht, Beziehungen, Familie, erstes und letztes
  Foto, Hinweise; Spalten entfallen bei wenig Breite nach Priorität), Details der gewählten
  Person rechts. Je Spalte genau ein Scrollbereich. Rückfragen stehen hier gesammelt.
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
  deterministisch (`graph_layout`). Berechnete Verwandtschaft wird nie als Kante gezeigt.

## Grenzen

Vorhandene widersprüchliche Altdaten werden nicht repariert. Ein früherer Partner muss als
solcher erfasst sein, sonst begründet er Stiefbeziehungen. Bei Mehrfachverwandtschaft gilt
die nächste. Zeichnungen sind nicht mit Screenreader geprüft; Liste und Seitenfeld sind die
Alternative.

## Beim Ändern beachten

Katalog-IDs nie umdeuten. Tiefe und Kappung der Berechnung nur gemeinsam mit
`tests/kinship.rs` ändern. Neue Rückfragemuster brauchen Prüfung, Begrenzung und
Wiederholungsschutz. Neue Tabellenspalten brauchen Breite, Priorität, Sortierwert und Test in
`core::people_table`. Schwere Zählungen gehören auf den Worker (`faces::page`).
