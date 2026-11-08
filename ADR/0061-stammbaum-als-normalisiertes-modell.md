# ADR-0061: Stammbaum als normalisiertes Modell mit berechneter Verwandtschaft

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-08
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-08
- **Bezieht sich auf:** [0019](0019-personen-und-beziehungsgraph.md), [0042](0042-native-bestandsmigrationen.md), [0058](0058-personengraph-als-netzwerk.md), [0060](0060-beziehungstypen-und-ableitung.md) (teilweise abgelöst: Ableitung, Geschwister/Cousin)

## Kontext

ADR 0060 speicherte auch Geschwister und Cousins und leitete nur zwei Schritte weit
ab. Nutzerkritik: Die Modellierung ist unstrukturiert; die abgeleiteten Angehörigen
sind zu prüfen; nicht jede Beziehung soll dokumentiert werden. Wenn x Kind von y und
y Kind von z ist, ist x Enkel von z, und das soll sich aus dem Baum ergeben, nicht
zusätzlich erfasst werden. Auch Cousins sollen auffindbar sein, ohne dass sie als
Kante vorliegen. Die App soll fehlende Eltern nur vorschlagen, nie selbst ergänzen.

**Prüfung der Ableitung von ADR 0060** (Eingabe, Ist, Soll; als Tests in
`tests/kinship.rs` und `tests/relations.rs` festgehalten):

| Eingabe | Ist (alt) | Soll |
|---|---|---|
| Urenkel: 1→2→4→6→8 | Person 8 hat nur Großeltern; Person 1 nur Enkel | Urgroßeltern, Ururgroßeltern, Urenkel |
| Großonkel (Bruder des Großvaters) | fehlt | Großonkel/Großtante |
| Cousin 2. Grades, Cousin 1. Grades 1x entfernt | fehlen | berechnet und benannt |
| Patchwork: 10 Kind von 1+2, 1 Partner von 3, 3 Mutter von 11 | für 10, 11, 1, 3 keinerlei Ableitung | Stiefmutter, Stiefkind, Stiefgeschwister |
| Partner eines Onkels (Tante durch Heirat) | fehlt | „Tante (angeheiratet)“ |
| Partner des Geschwisterteils | fehlt | Schwager/Schwägerin |
| Mehrere Partner, Kinder je Partner | Stiefkinder und Stiefeltern fehlen | Halbgeschwister plus Stiefbeziehungen |
| 70 Kinder oder Enkel | still auf 50 gekürzt | Kürzung wird mit „… weitere“ gemeldet |
| Eltern {1,2} und {1}: „Geschwister“ | bleibt Geschwister | bleibt Geschwister (Eltern unvollständig); Halbgeschwister erst, wenn beide Seiten einen weiteren Elternteil haben oder der Nutzer „nein“ antwortet |
| Geschwister-Zeile zwischen Onkel und Nichte | erlaubt | abgelehnt |
| Cousin-Zeile zwischen Onkel und Nichte | erlaubt | abgelehnt |
| Gespeicherte Geschwister (4,5), später Eltern, die sie zu Cousins machen | erlaubt | abgelehnt |
| Gespeicherter Cousin, später gemeinsame Eltern | erlaubt | abgelehnt |

Richtig waren: Halbgeschwister bei zwei verschiedenen weiteren Eltern, mehr als zwei
Eltern (erlaubt), Zyklenablehnung (auch bei Zusammenführung), Kanonisierung.
Partner zwischen Onkel und Nichte oder zwischen Vollgeschwistern bleibt erlaubt;
nur Vorfahr/Nachkomme wird abgelehnt (bewusst, siehe Folgen).

## Entscheidung

**Gespeichert werden Basisfakten** (`relations`, Typkatalog in `relations.rs`):

| ID | Typ | Rolle |
|---|---|---|
| 1 | `elternteil_von` | Basisfakt: A ist Elternteil von B (kanonisch; Typ 2 wird beim Lesen gedeutet) |
| 3 | `partner_von` | Basisfakt: aktuelle Partnerschaft |
| 8 | `ehemaliger_partner_von` | Basisfakt: frühere Partnerschaft (neu) |
| 6, 7 | `freund_von`, `bekannter_von` | nicht familiär, symmetrisch |
| 4, 5 | `geschwister_von`, `cousin_von` | nur noch **Behelf** |

Alles andere wird nie gespeichert, sondern berechnet (`src/core/kinship.rs`).
Die Typzeile 8 ist ein Datensatz in `relation_types` (wie 5–7 in ADR 0060, wird mit
`INSERT OR IGNORE` ergänzt, keine Schemaänderung, Schema-6-Sammlungen werden nicht
still nachgerüstet; schreibgeschützt bleibt sie unverändert, der Katalog liegt im Code).
„Frühere Partner“ ist die schlanke Form von Status/Zeitraum: Ohne sie würde ein
beendeter Partner in der Berechnung Stiefeltern und Schwiegerfamilie begründen.
Ein Typ leiblich/Adoptiv/Stief wird **nicht** gespeichert: Er ändert keine
Verwandtschaftsberechnung (Stief ergibt sich aus Partner und Eltern); biologische
Unterscheidung ist kein Ziel.

**Rechner.** `Family::new(edges, genders)` baut aus den Kanten Eltern-, Kinder- und
Partnerindizes. `relationship(a, b)` liefert Bezeichnung und Weg:

1. Partner / früherer Partner (direkte Kante).
2. Blutsverwandtschaft: nächster gemeinsamer Vorfahre X; `a_gen` und `b_gen` sind die Schritte von X
   hinab zu a bzw. b (Tie-Break: kleinste Summe, geringste Differenz, kleinste ID von X). Die
   Bezeichnung ergibt sich nur aus `(a_gen, b_gen)`: (1,0) Kind, (0,n) Eltern/Groß-/Urgroßeltern,
   (1,1) Geschwister, (1,d≥2) Onkel/Großonkel, (n≥2,1) Nichte/Großnichte, sonst
   Cousin `min−1`. Grades, `|a_gen−b_gen|`x entfernt. Geschlecht des Subjekts bestimmt die
   Form, unbekannt ergibt neutrale Formen (Elternteil, Kind, Cousin/Cousine …).
3. Stiefgeschwister: kein gemeinsamer Elternteil, aber ein Elternteil des einen ist (aktueller)
   Partner eines Elternteils des anderen.
4. Direkt erfasste Behelfsverbindung (Geschwister/Cousin), falls der Baum nichts hergibt.
5. Verschwägerung über genau einen Partnerschritt (nur aktuelle Partner, nur nahe
   Blutsverwandte mit Summe ≤ 4): Schwiegereltern/-kinder, Stiefeltern/-kinder, Schwager/Schwägerin,
   sonst „<Blutsbezeichnung> (angeheiratet)“.

Blutsverwandtschaft und Verschwägerung sind getrennt; Blut hat Vorrang. Es wird stets von der
kleineren ID aus gerechnet und umgekehrt (`Relation::inverse`), damit `relationship(a,b)` und
`relationship(b,a)` immer zueinander invers sind. `relatives_of(person)` gruppiert Eltern, Kinder,
Partner, Geschwister, Halbgeschwister, Großeltern, Urgroßeltern, Enkel, Onkel/Tanten, Nichten/Neffen,
Cousins nach Grad, Stief- und Schwiegerfamilie; `cousins_of`/`find` suchen ohne gespeicherte Kanten.
Halbgeschwister: ein gemeinsamer Elternteil und beide Seiten haben einen weiteren, oder der
Nutzer hat „Nein“ zu einem fehlenden Elternteil beantwortet.

**Grenzen (dokumentiert, nicht still).** `MAX_GENERATIONS = 8` je Richtung (Cousin 7. Grades),
darüber „nicht verwandt“. Gruppen zeigen höchstens `MAX_PER_GROUP = 100`; der Rest steht als
„… weitere“. Nicht benannt: Schwippschwager (Partner des Schwagers), Verschwägerung über mehrere
Partnerschritte, Halb-Onkel. Bei Mehrfachverwandtschaft (Ahnenschwund) wird die nächste gewählt.
Zyklen in beschädigten Daten führen nicht zu Endlosschleifen.

**Behelf: Geschwister und Cousin.** Wer die Eltern nicht kennt, soll nicht den ganzen Baum pflegen
müssen. Die Typen bleiben deshalb erfassbar („Behelf“ in der Oberfläche, mit Hinweis). Bestehende
Zeilen gehen nie verloren (AGENTS.md: explizite Entscheidungen). Folgt dieselbe Beziehung aus dem
Baum, wird die Zeile unterdrückt, nicht gelöscht: Sie erscheint nicht doppelt, die Oberfläche weist
freundlich darauf hin und bietet „Behelfsverbindung entfernen“ an (nur auf Klick). Neues Speichern
einer bereits aus dem Baum folgenden Behelfsverbindung wird abgelehnt (`Conflict::Redundant`);
bei der Zusammenführung bleibt sie erhalten. Die App schlägt Geschwister oder Cousin nie vor. In der Rollenliste erscheinen sie nur hinter „Notfall-Behelf anzeigen“ und werden blockiert, sobald sich die Beziehung berechnen lässt (`relations::shortcut_needed`). Behelfskanten tragen nicht zur weiteren Berechnung
bei (keine Nichten über Behelfsgeschwister).

**Widersprüche** (`validate_add`): wie ADR 0060 (Selbstbezug, Zyklen, Paarregeln,
Vorfahr/Nachkomme), zusätzlich: Behelf gegen den Baum (Geschwister/Cousin zwischen Onkel und
Nichte), und eine neue Eltern-Kante darf keine gespeicherte Behelfsverbindung neu widersprüchlich
machen (schon vorher widersprüchliche Altdaten blockieren nichts). Partner und früherer Partner
desselben Paares schließen sich aus.

**Rückfragen statt stillem Ergänzen** (`src/core/gaps.rs`, reine Funktion
`suggestions(edges, decisions)`). Zwei Muster, begrenzt (30) und geordnet:
(a) A und B gelten als Geschwister (gemeinsame Eltern oder Behelf), A hat Eltern C, D, B nicht: „Sind
C und D auch Eltern von B?“; (b) C hat genau einen Partner D ohne eigene Kinder und Kinder, die nur C
als Elternteil haben: „Ist D auch Elternteil?“ Nie vorgeschlagen werden Paare, die `validate_add`
ablehnt (Zyklus, Selbstbezug, Widerspruch), bereits Entschiedenes, und Personen mit zwei oder mehr
Eltern. Antworten: „Ja“ (legt Kanten mit voller Prüfung an), „Nur X“ bzw. „Nein – Halbgeschwister“
(speichert die Ablehnung), „Später“ (nur für diese Sitzung zurückgestellt). Der Nutzer entscheidet
immer; die Mechanik dupliziert nichts. Alle Vorschläge stehen an einer zentralen Stelle, oben im Reiter „Personen & Beziehungen“; der Reiter „Stammbaum“ verweist mit dem Zähler darauf. **Speicherung der Ablehnung:** als Paare (Kind:Elternteil) im
vorhandenen Schlüssel-Wert-Speicher `face_settings` (Schlüssel `family.declined_parents`).
Begründung: keine Schemaänderung (ADR 0042), geht mit der Sammlung mit, ist unabhängig von
Personenattributen und überlebt Zusammenführungen unschädlich (nicht mehr passende Paare werden
ignoriert). Antwort „Ja“ braucht keine Speicherung: Die Kante ist die Entscheidung.

**Person hinzufügen.** `faces::add_person` legt `persons` und `person_attrs` (Name, Geschlecht,
Geburtsjahr, Notiz) ohne Gesichter an: Name Pflicht (Leerraum bereinigt, höchstens 120 Zeichen),
Geschlecht als Auswahl (weiblich/männlich/leer = divers/unbekannt, kompatibel zu den Freitextwerten
von `Gender::parse`), Geburtsjahr 1000–3000, Notiz höchstens 500 Zeichen. Namensdubletten sind erlaubt;
die Oberfläche warnt. Danach wird „Beziehung hinzufügen“ (Kind von / Elternteil von / Partner von /
Freund von) angeboten. Die Person erscheint in Auswahllisten, Stammbaum und Personengraph (Punkt ohne Fotos).

**Eingabe.** „X ist Kind von Y“, „X ist Elternteil von Y“, „X und Y sind Partner“,
„X ist Freund/Bekannter von Y“ aus beiden Richtungen (unverändert aus ADR 0060); an der Person
steht eine Schnellzeile; Behelf-Typen stehen zuletzt mit Hinweis.

**Stammbaumansicht** (`src/core/tree_layout.rs`, `src/ui/app/tree_view.rs`): eigener Reiter
„Stammbaum“ im Personenbereich. Begründung: Die Baumfläche braucht die volle Breite und Höhe und
einen eigenen Fokus (Person, Generationen), während „Personen & Beziehungen“ ein Formular- und
Listenreiter ist; der Graph (ADR 0058) bleibt eigenständig. Rückfragen und „Person hinzufügen“ liegen
im Reiter „Personen & Beziehungen“ (Zähler im Reiternamen); die Verwandtschaftsprüfung („Anna ist
Großmutter von Leo“, „Ben und Dora sind Cousins 1. Grades“, Weg als Text) im Stammbaum-Reiter.
Das Layout ist eine reine, deterministische Funktion: gewählt werden nur Fokus-Person, deren Partner
und frühere Partner, Vorfahren (1–6 Generationen), Geschwister auf der Ebene der Person und Nachkommen
mit ihren Partnern (0–6 Generationen), höchstens 300 Personen (Rest gemeldet). Reihenfolge je Ebene aus
der Verwandtschaft (Eltern über ihren Kindern, Paare als Block), Lage iterativ nahe an den Mittelwerten
der Nachbarn, jede Ebene wird auf Mindestabstände projiziert (isotonische Regression), daher keine
Überlappung. Zeichnung: Kästchen mit Initialen, Geschlechtsfarbe, Name, Geburtsjahr; Familienlinien
verbinden Eltern mit ihren Kindern; frühere Partner gestrichelt; Klick wechselt den Fokus. Als
Textalternative gibt es „Als Liste anzeigen“ (gruppierte Verwandte) und Tooltips mit dem Satz zur
Beziehung. Die Berechnung läuft im UI-Thread, weil sie auf die Umgebung begrenzt ist
(Millisekunden für hunderte Personen im Test); der Stammbaum selbst (`Family`) wird mit den
Gesichtsdaten auf dem Worker aufgebaut.

**Personengraph.** Unverändert: er liest `relations`/`relation_types`; gespeicherte Eltern-,
Partner-, Freund- und Bekanntenkanten erscheinen wie bisher, der neue Typ 8 neutral über den Namen
in der Tabelle; berechnete Verwandtschaft wird nie als Kante dargestellt.

## Alternativen

Alle Verwandtschaftsgrade zu speichern (ADR 0060) erzeugt Redundanz und Widersprüche und zwingt zur
Pflege jeder Kante. Geschwister/Cousin ganz zu streichen würde bestehende Entscheidungen entwerten und
Nutzer ohne Elternkenntnis blockieren. Stillschweigendes Ergänzen fehlender Eltern (verworfen, Nutzerwunsch):
falsch bei Halbgeschwistern. Ablehnungen in `person_attrs` oder einer neuen Tabelle: Attribute sind
personenbezogen, eine Tabelle wäre eine stille Schemaänderung. Layout mit vorhandenen Graphbibliotheken:
neue Abhängigkeit für wenige Dutzend Knoten nicht gerechtfertigt.

## Folgen und Grenzen

Weniger zu erfassen, mehr zu erkennen. Bestehende Daten bleiben vollständig; nur die Anzeige
unterdrückt Redundantes. Bei unvollständigen Eltern gilt „im Zweifel Geschwister“. Ein ehemaliger Partner
muss als solcher erfasst werden, sonst gilt er als aktuell (Stiefbeziehungen). Partner zwischen
Vollgeschwistern wird nicht abgelehnt. Behelfskanten speisen die Berechnung nicht. Der Stammbaum zeigt
nur direkte Linien samt Geschwistern (Onkel, Cousins über die Prüfung und Liste). Zeichnung nicht mit
Screenreader oder Nutzern geprüft; die Textliste ist die Alternative. Bisher keine Auslagerung der
Layoutberechnung auf einen Worker.

## Bei Weiterentwicklung beachten

IDs im Katalog nie umdeuten; Typ 8 ist neu. Tiefe und Kappung nur gemeinsam mit den Tests
(`tests/kinship.rs`, Eigenschaftstest gegen naive Referenz) ändern. Neue Rückfragemuster brauchen
„nie ohne Antwort“, Prüfung per `validate_add`, Begrenzung und Wiederholungsschutz. Wird die
Layoutberechnung teurer (mehr als 300 Personen), auf einen Worker mit Revisionen verlagern.

## Belege und Validierung

[Rechner](../src/core/kinship.rs), [Rückfragen](../src/core/gaps.rs),
[Layout](../src/core/tree_layout.rs), [Katalog und Prüfung](../src/core/relations.rs),
[Person anlegen](../src/core/faces.rs), [Oberfläche](../src/ui/app/tree_view.rs),
Tests: [Verwandtschaft](../tests/kinship.rs), [Rückfragen](../tests/gaps.rs),
[Layout](../tests/tree_layout.rs), [Personen](../tests/people.rs),
[Beziehungen, Altzeilen, Zusammenführung](../tests/relations.rs). Sichtprüfung mit synthetischen Daten
(`examples/demo_faces.rs`) siehe [Prüfprotokoll](../docs/validierung.md). Echte Sammlungen wurden nicht geprüft.
