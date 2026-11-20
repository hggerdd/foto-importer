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
- **Namensmerkmale** entstehen nur ab Qualität 0,5. Klar erkannte, aber kleine Gesichter
  (Erkennung ab 0,8) werden aus einer 3200-Pixel-Fassung nachgerechnet.
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
- Je Person höchstens 100 Auswahlen aus höchstens 2048 Kandidaten: Ausreißer- und
  Duplikatfilter (0,92), Serienfilter, Verteilung über Jahre, qualitätsgewichtete Auswahl
  möglichst verschiedener Gesichter. Rund 20 % werden als Validierung zurückgehalten und
  zählen nie im Referenzvergleich. Pins des Nutzers zählen auf das Limit, Ausschlüsse gelten.
  Die Auswahl ist reproduzierbar (Seed 42, BLAKE3 über Gesichts-ID).
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
  `suggest` aus Manifest und Sammlung wertet der Abgleich nicht mehr aus.
- Jede Änderung an Bestätigungen erhöht `generation` (und setzt den Foto-Cursor des
  vollständigen Laufs zurück). Ein Lauf schreibt nur, solange die Generation die vom Beginn
  ist, und schließt nur dann ab; eine Änderung währenddessen lässt den Auftrag für einen
  neuen Lauf nach der Referenzauswahl stehen. Ein abgebrochener Lauf protokolliert keinen
  Abschluss.
- **Inkrementell** (`recognition::rematch`, `MatchCache`): Der Personenscore hängt nur von
  den Referenzen dieser Person ab; Ausschlüsse filtern erst danach. Ein Zwischenspeicher im
  Arbeitsspeicher des Programms (eine Sammlung und ein Modell zugleich, nicht in SQLite)
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
  (0,40, nicht gemessen, begrenzt nur die Liste). Absteigend nach Wert, dann ID. Die Werte
  stammen aus dem Zwischenspeicher, wenn er für Sammlung und Modell bereitliegt (der Leser
  bringt ihn per `sync` auf den Stand und hält die Sperre nur dafür; der Abgleich wartet auf
  sie ohne Rückgabepunkt, gegenseitiges Warten ist ausgeschlossen); sonst rechnet ein
  Durchgang über die Vektoren nur die Spalte dieser Person, ohne Zwischenspeicher aufzubauen.
  Läuft auf dem Leser der Seite (`faces::page_with`, Modus `probable`). Messung in
  [Validierung](../docs/validierung.md).
- Die Referenzabfrage hängt nicht von `ANALYZE`-Statistik ab (nicht korrelierte
  Unterabfragen auf `training_set`, bestätigte Gesichter außen per `CROSS JOIN`).

**Gruppen unbenannter Gesichter** (`faces::prepare_groups`, `grouping`)

- Ein Gesicht tritt der ersten Gruppe bei, in der alle Mitglieder Kosinus 0,6 erreichen und
  kein Mitglied aus demselben Foto stammt; höchstens 28 Mitglieder (`config::GROUP_MAX`).
- Gruppen mit höchstens zwei Gesichtern werden der ähnlichsten Gruppe angeschlossen
  (mittlere Ähnlichkeit ab 0,5, kein Paar unter 0,3).
- Gruppen sind abgeleitet und jederzeit neu erzeugbar; Trigger pflegen die Mitgliedschaft.
  Offene Gesichter werden zu Beginn jedes Referenz- und Abgleichlaufs gruppiert, nicht erst
  an dessen Ende.

**Messung 2026-11-15** (eine Sammlung, SFace, 4.501 bestätigte Gesichter, 1.537 Referenzen,
35 Personen; Person-auslassen-Test, daher obere Schätzung für Fehlannahmen):

| Schwelle | richtige Person erreicht sie | unbekannte Person wird trotzdem zugeordnet |
|---|---|---|
| 0,363 | 99,7 % | 75 % |
| 0,50 | 98,1 % | 43 % |
| 0,60 | 90,8 % | 14 % |
| 0,65 | 81 % | 4,5 % |
| 0,70 | 61 % | 1 % |

Reproduzierbar mit `cargo run --example face_eval -- <Sammlungsordner>` (öffnet nur lesend; zusätzlich Anteile mit 0,05 Abstand zur zweitbesten Person).

## Grenzen

Kein Nachweis gleicher Ergebnisse wie die Python-/OpenCV-Referenz. Die Schwellen für das
Zusammenführen kleiner Gruppen sind nicht gemessen. Der Zwischenspeicher des Abgleichs
verfällt mit dem Programm; der erste Lauf nach dem Start liest alle Vektoren (auf einer
USB-Festplatte nicht gemessen). Wird die Datenbankdatei bei laufendem Programm durch eine
andere ersetzt, erkennt er geänderte Vektoren nur an Gesichts-ID und `rowid`.

## Beim Ändern beachten

Labels hängen am Gesicht, nicht am Vektor. Präsenz, Merkmalsqualität und Namenszuordnung
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
