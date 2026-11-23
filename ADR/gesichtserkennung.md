# Gesichtserkennung

Bedienung der Reiter: siehe [Gesichter benennen und prüfen](gesichter-benennen.md).

## Entscheidung

**Modelle**

- Vortrainierte Modelle laufen lokal auf der CPU über ONNX Runtime (zwei Inferenzthreads).
  Fotos und Vektoren verlassen den Rechner nicht. Gewichte werden nicht verändert.
- Manifeste unter `models/` beschreiben Rolle, Version, Lizenz, SHA-256 und Eingabe; der
  Hash wird beim Laden geprüft. Standard ist YuNet + SFace, wählbar SCRFD + ArcFace
  (Gewichte nur für nicht kommerzielle Forschung lizenziert).
- Modellschlüssel enthalten ID, Version, Hash und `:rust-ort-v1`. Vektoren verschiedener
  Schlüssel werden nie gemischt oder umgedeutet.

**Analyse** (`faces::analyze_scoped`)

- Je Foto wird die bevorzugte Quelle orientiert und auf 1600 Pixel begrenzt analysiert.
  Rahmen und Landmarks gehören zu dieser Quelle. Aufträge je Foto, Modell und
  Quellfingerabdruck machen den Lauf fortsetzbar, auch für Fotos ohne Gesicht. Ein Fehler an
  einem Foto bricht den Lauf nie ab; fehlt die Quelldatei, wird sie als fehlend markiert
  (kein Analysefehler, siehe [Sammlung und Daten](sammlung-daten.md)). Die automatische
  Analyse liest vorher Metadaten mit denselben Regeln.
- **Präsenz:** Schwelle 0,60 (YuNet) bzw. 0,55 (SCRFD); größere Bilder erhalten zusätzlich
  vier überlappende Ausschnitte. Jeder Treffer bleibt als Vorkommen gespeichert.
- **Namensmerkmale** entstehen nur ab Qualität 0,5 (`config::FACE_QUALITY_MIN`; SQL-Texte nennen
  den Wert wörtlich, ein Test in `config.rs` gleicht jeden mit der Konstante ab). Klar
  erkannte, aber kleine Gesichter (Erkennung ab 0,8) werden aus einer 3200-Pixel-Fassung
  nachgerechnet (`config::FACE_HIRES_EDGE`).
- **Neuanalyse** ordnet Gesichter innerhalb derselben Quelle über die Überdeckung (IoU ab
  0,5) zu. Nicht wiedergefundene bestätigte oder ignorierte Gesichter bleiben als historisch
  (`retained`) erhalten; übrige werden gelöscht. Vor einem Detektorwechsel entsteht eine Sicherung.
- **„Kein Gesicht“** (`faces::mark_not_face`, Reiter „Ungeeignet“) ist eine Nutzerentscheidung
  je Gesicht: `faces.ignored = 2` (`faces::NOT_A_FACE`; 0 aktiv, 1 „Nicht interessiert“), ohne
  Schemaänderung, Zuordnung und Referenzauswahl entfallen, Vorkommen und Foto bleiben. Weil
  alle aktiven Abfragen `ignored=0` und „Ignoriert“ `ignored=1` prüfen, fällt ein solches
  Gesicht aus allen Zählungen, Gruppen, Vorschlägen, dem Abgleich und dem Reiter „Ignoriert“;
  `status.total`, die Gesichter am Foto (`photo_faces`) und die Gesichtszählung der Suche
  (`metadata::CURRENT_FACES`) schließen es aus (Teilindex `ix_faces_not_face`). Die
  Neuanalyse behält `ignored` beim Wiederfinden und hält ein nicht wiedergefundenes als
  historisch, die Entscheidung kehrt also nicht zurück. Rücknahme über `faces::undo`
  (`FaceSnapshot::not_face`).

**Referenzen** (`recognition::rebuild`, `matching::select_references`)

- Nur vom Nutzer bestätigte Gesichter ab Qualität 0,5 können Referenz sein. Automatische
  Zuordnungen lernen nie.
- Die Auswahlgröße je Person folgt allein der Zahl `n` ihrer bestätigten Gesichter mit
  Merkmalen und sättigt (`config::reference_limit`): `ceil(800·(1−exp(−n/800)))`, höchstens
  `n`, ohne feste Untergrenze (30 → 30, 100 → 95, 500 → 372, 1000 → 571, 5000 → 799).
  Ausgewählt wird aus höchstens 2048 Kandidaten (`config::REFERENCE_CANDIDATES_MAX`, je Jahr
  nach Qualität im Wechsel; Pins kommen hinzu): Ausreißer- und Duplikatfilter (0,92),
  Serienfilter, Verteilung über Jahre, qualitätsgewichtete Auswahl möglichst verschiedener
  Gesichter. Pins zählen auf die Grenze, gehen aber nie verloren (die Grenze wächst bei Bedarf
  auf ihre Zahl); Ausschlüsse gelten.
- **Validierung** (`config::reference_validation`): 20 % der Auswahl, höchstens 40, und erst ab
  50 Ausgewählten; seltene Personen nutzen alle Gesichter als Referenz. Validierungsgesichter
  zählen nie im Abgleich und dienen nur „Validierung auswerten“ (`maintenance::calibrate`).
  Die Auswahl ist reproduzierbar (Seed 42, BLAKE3 über Gesichts-ID).
- **Regelwechsel:** `config::REFERENCE_POLICY` benennt die geltende Regel; die Sammlung merkt sie
  sich je Modell (`rust.reference_policy:<Modell>`). Weicht sie beim Öffnen mit Schreibrecht ab
  (`recognition::renew_outdated_references`), werden alle Personen mit Auswahl als geändert
  markiert und der Abgleich vorgemerkt; die nächste Wartung wählt neu aus. Pins, Ausschlüsse
  und Zuordnungen bleiben; der Zwischenspeicher des Abgleichs rechnet die Spalten der Personen
  mit geänderten Referenzen neu.
- Nach einer Bestätigung werden nur geänderte Personen neu ausgewählt.

**Abgleich** (`recognition::rematch`)

- Personenscore = Mittel der bis zu drei besten Kosinuswerte gegen die Referenzen der Person.
- Ausgeschlossen sind: abgelehnte Personen, im Foto schon bestätigte Personen, Aufnahmen vor
  dem Geburtsjahr, Gesichter unter Qualität 0,5.
- Ein Gesicht erhält nur dann einen Vorschlag, wenn die beste zulässige Person mindestens
  **0,60** erreicht **und** mindestens **0,05** vor der zweitbesten liegt
  (`recognition::Limits`, Vorgaben in `config.rs`). Ab **0,70** heißt die Zuordnung `auto`,
  darunter `suggested`; beide sind Vorschläge und werden erst durch Bestätigung zu `user`.
  Nutzerzuordnungen werden nie überschrieben. Gemeinsames Auftreten gibt keinen Bonus.
- Je Foto kommt keine Person doppelt vor: Schlagen zwei Gesichter dieselbe Person vor,
  erhält sie das ähnlichere; das andere bleibt unbenannt.
- Die drei Werte sind je Sammlung und Modell überschreibbar
  (`rust.thresholds.<Modell>.review`, `.margin`, `.sure`). `set_review_threshold` setzt die
  Schwelle (0,50 bis 0,75) und startet den Abgleich neu; der Regler „Strenge der
  Namensvorschläge“ unter Einstellungen (Erweitert) ruft sie auf. Die früheren Werte `auto` und
  `suggest` aus dem Manifest werden weder ausgewertet noch in die Sammlung geschrieben.
- Jede Änderung an Bestätigungen erhöht `generation` (und setzt den Foto-Cursor des
  vollständigen Laufs zurück). Ein Lauf schreibt nur, solange die Generation die vom Beginn
  ist, und schließt nur dann ab; eine Änderung währenddessen lässt den Auftrag für einen
  neuen Lauf nach der Referenzauswahl stehen. Ein abgebrochener Lauf protokolliert keinen
  Abschluss.
- **Inkrementell** (`recognition::rematch`, `MatchCache`): Der Personenscore hängt nur von
  den Referenzen dieser Person ab; Ausschlüsse filtern erst danach. Ein Zwischenspeicher im
  Arbeitsspeicher des Programms (eine Sammlung und ein Modell zugleich, erkannt an Pfad,
  `collection.id` und Modell, nicht in SQLite)
  hält je verglichenem Gesicht den Vektor und den Score gegen jede Person mit Referenzen,
  dazu den Referenzstand, gegen den gerechnet wurde. Jeder Lauf liest Referenzen,
  Gesichter (ohne Vektoren), Ablehnungen und Geburtsjahre neu; nur die Spalten von Personen
  mit geänderten Referenzen werden für alle Gesichter neu gerechnet, neue Gesichter und
  ersetzte Vektoren (erkannt an der neuen `rowid` der Merkmalszeile, die
  `INSERT OR REPLACE` vergibt) einzeln geladen. Rangfolge, Ausschlüsse, Schwellen und die
  Entscheidung je Foto entstehen in jedem Lauf aus dem aktuellen Stand; das Ergebnis ist
  bitgleich zum vollständigen Lauf. Ein leerer oder fremder Zwischenspeicher (Programmstart,
  andere Sammlung, anderes Modell) wird zuerst aufgebaut: alle Vektoren lesen, alle Spalten
  rechnen; ein Abbruch behält die schon gerechneten Gesichter. Würde er
  `config::MATCH_CACHE_MAX_BYTES` überschreiten, läuft stattdessen der vollständige Lauf.
  Rechnen verteilt sich auf bis zu `config::MATCH_THREADS` Threads des Wartungs-Workers;
  geschrieben werden nur geänderte Zuordnungen in kurzen Transaktionen.
- **Vollständiger Lauf** (`recognition::rematch_full`, Rückfall und Maßstab): fortsetzbar
  über den Foto-Cursor, 32 Fotos je Schritt, Gesichter, Vektoren und Ablehnungen mit zwei
  Abfragen je Schritt, Schreiben nur geänderter Zuordnungen je Schritt.
- **Rechenweg:** Referenzen liegen in Blöcken zu acht (`recognition::References`); jedes
  Skalarprodukt summiert in derselben Reihenfolge wie `inference::cosine` und ist bitgleich.
  Die besten zwei Personen werden ohne vollständiges Sortieren mit derselben strengen
  Ordnung bestimmt (Score, Name, ID). Messungen in [Validierung](../docs/validierung.md).
- **Wahrscheinliche Gesichter einer Person** (`recognition::person_candidates`, Personenseite
  „Wahrscheinlich X“): derselbe Personenscore wie im Abgleich für alle Gesichter, die der
  Abgleich vergleicht (nicht vom Nutzer benannt, auch Vorschläge anderer Personen), ohne
  seine Ausschlüsse für diese Person (abgelehnt, im Foto bestätigt, vor dem Geburtsjahr);
  Schwelle und Abstand gelten nicht, nur die Untergrenze `config::PERSON_CANDIDATE_MIN`
  (0,40, nicht gemessen, begrenzt nur die Liste). Je Gesicht zusätzlich die beste Person
  wie im Abgleich (`best_two` über alle Personen mit Referenzen, Ordnung Score, Name, ID,
  ohne die Ausschlüsse des Gesichts für andere Personen). Reihenfolge: erst Gesichter, deren
  beste Person X ist, dann die übrigen (mit der besseren Person und ihrem Wert); je
  Abschnitt absteigend nach dem Wert für X, bei Gleichstand nach Gesichts-ID. Ein
  Gleichstand zwischen X und einer anderen Person entscheidet wie im Abgleich der Name, dann
  die ID. Die Werte stammen aus dem Zwischenspeicher, wenn er für Sammlung und Modell
  bereitliegt (der Leser bringt ihn per `sync` auf den Stand und hält die Sperre nur dafür;
  der Abgleich wartet auf sie ohne Rückgabepunkt, gegenseitiges Warten ist ausgeschlossen);
  sonst rechnet ein Durchgang über die Vektoren die Spalte dieser Person und nur für
  Gesichter ab der Untergrenze alle Spalten (Blockrechnung wie oben), ohne Zwischenspeicher
  aufzubauen. Beide Wege sind bitgleich (`person_candidates_with` wählt ausdrücklich).
  Läuft auf dem Leser der Seite (`faces::page_with`, Modus `probable`). Messung in
  [Validierung](../docs/validierung.md).
- **Personenstatistik** (`recognition::person_scores`, Ansicht siehe [Personen](personen.md)):
  derselbe Personenscore, nur lesend. Für bestätigte Gesichter zählen Referenzen vom selben
  Aufnahmetag nicht (`References::scores_without`, derselbe Rechenweg wie `scores`, das nur
  das eigene Gesicht auslässt). Zwischenspeicher und Durchgang über die Vektoren sind
  bitgleich (`cached_and_streamed_scores_are_identical`).
- **Abgelehnte Gesichter einer Person** (`recognition::person_face_scores`, Personenseite
  „Nicht X“): derselbe Wert wie die abgelehnten der Statistik, nur die Spalte dieser Person,
  Vektoren nur der gezeigten Gesichter. Wird eine Ablehnung aufgehoben
  (`faces::lift_rejection`), ändert sich keine Zuordnung und kein Trigger greift; der Abgleich
  wird deshalb dort (und bei der Rücknahme geänderter Ablehnungen) für das Modell des Gesichts
  vorgemerkt.
- Die Referenzabfrage hängt nicht von `ANALYZE`-Statistik ab (nicht korrelierte
  Unterabfragen auf `training_set`, bestätigte Gesichter außen per `CROSS JOIN`).

**Gruppen unbenannter Gesichter** (`faces::prepare_groups`, `grouping`)

- Ein Gesicht tritt der ersten Gruppe bei, in der alle Mitglieder Kosinus 0,6 erreichen
  (`config::GROUP_JOIN_MIN`) und
  kein Mitglied aus demselben Foto stammt; höchstens 28 Mitglieder (`config::CLUSTER_MAX`).
  Das ist eine Grenze der Gruppenbildung, nicht der Anzeige: Die Gruppengröße der Oberfläche
  (`config::GROUP_SIZE_*`, siehe Gesichter benennen) ändert keine gespeicherte Gruppe.
- Gruppen mit höchstens zwei Gesichtern werden der ähnlichsten Gruppe angeschlossen
  (mittlere Ähnlichkeit ab 0,5, kein Paar unter 0,3; `config::GROUP_SMALL`, `GROUP_MERGE_*`).
- Gruppen sind abgeleitet und jederzeit neu erzeugbar; Trigger pflegen die Mitgliedschaft.
  Offene Gesichter werden zu Beginn jedes Referenz- und Abgleichlaufs gruppiert, nicht erst
  an dessen Ende.

**Messung 2026-11-21: Referenzgrenze** (`examples/reference_limit_eval.rs`, nur lesend; eine
Sammlung, SFace, 17.023 bestätigte Gesichter, 50 Personen). Je Person sind rund 20 % der
Aufnahmetage (BLAKE3 über Person und Tag) eine feste Testmenge von 2.931 Gesichtern, die in
keiner Variante Kandidat ist; die Auswahl läuft im Speicher. „richtig“: eigene Person wird mit
Abstand 0,05 vorgeschlagen; „falsch“: eine andere; „unbekannt“: Person-auslassen-Test (obere
Schätzung). Gruppen nach bestätigten Gesichtern der Person (Testgesichter: <30: 48, 30–100: 82,
100–500: 569, >500: 2.232).

| Variante | Refs | richtig 0,60 / 0,70 (>500) | falsch 0,60 (>500 / 100–500) | richtig 0,60 / 0,70 (<30; 30–100) | gesamt richtig 0,60 / 0,70 | gesamt falsch 0,60 | unbekannt 0,60 / 0,70 |
|---|---|---|---|---|---|---|---|
| fest 100 (bis 2026-11-21) | 2.083 | 63 % / 29 % | 5,4 % / 0 % | 83 % / 58 %; 77 % / 67 % | 68 % / 38 % | 4,1 % | 17,9 % / 4,8 % |
| fest 400 | 4.755 | 86 % / 71 % | 1,5 % / 0 % | 81 % / 58 %; 77 % / 67 % | 87 % / 72 % | 1,1 % | 16,7 % / 5,3 % |
| sättigend 600 | 5.358 | 88 % / 76 % | 0,7 % / 0,2 % | wie oben | 88 % / 75 % | 0,5 % | 16,2 % / 5,2 % |
| sättigend 800 | 5.862 | 90 % / 78 % | 0,2 % / 0,5 % | wie oben | 89 % / 77 % | 0,3 % | 16,0 % / 5,3 % |
| sättigend 1200 | 6.488 | 91 % / 80 % | 0,2 % / 1,4 % | wie oben | 90 % / 79 % | 0,4 % | 15,5 % / 5,3 % |
| alle (ohne Kandidatengrenze) | 7.973 | 93 % / 83 % | 0,1 % / 1,6 % | wie oben | 92 % / 81 % | 0,4 % | 15,1 % / 5,4 % |
| **gewählt:** sättigend 800, Validierung neu | 6.612 | 90 % / 79 % | 0,1 % / 0,5 % | 83 % / 63 %; 77 % / 70 % | 90 % / 78 % | 0,2 % | 15,7 % / 5,4 % |

Ohne Kandidatengrenze war die Auswahl bei gleicher Größe schlechter (sättigend 800: 76 % statt
78 % bei 0,70, >500), weil die Jahresverteilung verloren geht. Über 800 hinaus steigen die
Verwechslungen mittlerer Personen mit häufigen. Ein zusätzlicher Mittelwertvektor je Person
(Prototyp) half seltenen Personen bei 0,70 (<30: 71 % statt 63 %), ließ aber unbekannte
Personen häufiger zugeordnet werden (30–100 bei 0,70: 12,2 % statt 8,5 %); nicht übernommen.
Gespiegelte oder gedrehte Ausschnitte als zusätzliche Referenzen sind nicht gemessen. Die
Gruppen unter 100 Gesichtern haben wenige Testgesichter; ihre Unterschiede sind unsicher.
Laufzeit (Release, 4 Threads): Auswahl aller 50 Personen 0,9 s; Abgleich aller offenen
Gesichter hochgerechnet 0,9 s (fest 100) bzw. 2,6 s (gewählt), einfache Skalarprodukte.

**Messung 2026-11-15** (Schwellen; 4.501 bestätigte Gesichter, 1.537 Referenzen, 35 Personen,
Person-auslassen-Test): Die richtige Person erreicht 0,60 in 90,8 %, 0,70 in 61 %; eine
unbekannte Person überschreitet 0,60 in 14 %, 0,70 in 1 %. Die Schwellen 0,60 / 0,70 / 0,05
bleiben; die neue Messung zeigt bei 0,70 rund 5 % unbekannte Zuordnungen (vorher 4,8 % mit
fest 100), unabhängig von der Referenzgrenze. Reproduzierbar mit
`cargo run --example face_eval -- <Sammlungsordner>`.

## Grenzen

Kein Nachweis gleicher Ergebnisse wie die Python-/OpenCV-Referenz. Die Schwellen für das
Zusammenführen kleiner Gruppen sind nicht gemessen. Der Zwischenspeicher des Abgleichs
verfällt mit dem Programm; der erste Lauf nach dem Start liest alle Vektoren (auf einer
USB-Festplatte nicht gemessen). Wird die Datenbankdatei bei laufendem Programm durch eine
andere ersetzt, erkennt er geänderte Vektoren nur an Gesichts-ID und `rowid`.

## Beim Ändern beachten

Ändert sich Referenzgrenze, Kandidatengrenze oder Validierung, `config::REFERENCE_POLICY`
erhöhen und mit `reference_limit_eval` messen (`--check` prüft, dass die Auswahl im Speicher
der gespeicherten gleicht). Labels hängen am Gesicht, nicht am Vektor. Präsenz, Merkmalsqualität und Namenszuordnung
getrennt halten. Neue Abfragen auf aktive Gesichter prüfen `ignored=0`, nicht
`ignored<>1`, sonst kehren „Kein Gesicht“-Entscheidungen zurück. Nie automatische Zuordnungen als Referenz oder Trainingsdaten verwenden, nie
das abgefragte Gesicht als eigene Referenz zählen. Schwellen nur mit Messung ändern.
Änderungen an Rangfolge oder Abgleich gegen den Einzelvergleich prüfen
(`block_ranking_is_bit_identical_to_the_single_comparison`), den inkrementellen gegen den
vollständigen Lauf (`incremental_rematch_equals_the_full_rematch`,
`face_save_bench inkrementell … ohne-statistik`) und mit
`face_save_bench steps … ohne-statistik` messen. Was den Score einer Person von anderen
Personen abhängig machte, bräche den inkrementellen Abgleich.
„Biometrie entfernen“ muss alle Gesichtstabellen, bekannte Sicherungen, den Export und den
Kachelordner erfassen.
