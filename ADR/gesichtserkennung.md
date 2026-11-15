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
  Quellfingerabdruck machen den Lauf fortsetzbar, auch für Fotos ohne Gesicht.
- **Präsenz:** Schwelle 0,60 (YuNet) bzw. 0,55 (SCRFD); größere Bilder erhalten zusätzlich
  vier überlappende Ausschnitte. Jeder Treffer bleibt als Vorkommen gespeichert.
- **Namensmerkmale** entstehen nur ab Qualität 0,5. Klar erkannte, aber kleine Gesichter
  (Erkennung ab 0,8) werden aus einer 3200-Pixel-Fassung nachgerechnet.
- **Neuanalyse** ordnet Gesichter innerhalb derselben Quelle über die Überdeckung (IoU ab
  0,5) zu. Nicht wiedergefundene bestätigte oder ignorierte Gesichter bleiben als historisch
  (`retained`) erhalten; übrige werden gelöscht. Vor einem Detektorwechsel entsteht eine Sicherung.

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
- Je Foto entscheidet eine gemeinsame Zuordnung, sodass keine Person doppelt vorkommt.
  Ausgeschlossen sind: abgelehnte Personen, im Foto schon bestätigte Personen, Aufnahmen vor
  dem Geburtsjahr, Gesichter unter Qualität 0,5. Gemeinsames Auftreten gibt höchstens 0,03 Bonus.
- Zuordnung `suggested` ab Schwelle `suggest`, `auto` ab `auto`; beide sind Vorschläge und
  werden erst durch Bestätigung zu `user`. Nutzerzuordnungen werden nie überschrieben.
- Jede Änderung an Bestätigungen setzt den Foto-Cursor zurück; der Lauf ist fortsetzbar,
  arbeitet 32 Fotos je Schritt und weicht jeder Nutzeraktion. Ein abgebrochener Lauf
  protokolliert keinen Abschluss.

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

## Beschlossen, noch nicht umgesetzt (2026-11-15)

- Ein Gesicht erhält nur dann einen Vorschlag, wenn der Score mindestens **0,60** beträgt
  **und** mindestens **0,05** vor der zweitbesten Person liegt. Beide Werte sind je Sammlung
  überschreibbar und unter Einstellungen (Erweitert) einstellbar. Der Bonus für gemeinsames
  Auftreten hebt kein Gesicht über die Schwelle.
- Der trainierte Softmax-Klassifikator (`classifier.rs`) entfällt: gemessen Platz 1 96,1 %
  gegenüber 96,9 % des Kosinus-Vergleichs. Bis dahin sortiert er nur die drei
  Vorschlagsschaltflächen und entscheidet nichts.
- Ein Messwerkzeug (`examples/face_eval.rs`) erzeugt die Tabelle oben reproduzierbar.

## Grenzen

Kein Nachweis gleicher Ergebnisse wie die Python-/OpenCV-Referenz. Die Schwellen für das
Zusammenführen kleiner Gruppen sind nicht gemessen. Der Abgleich startet nach jeder
Bestätigung von vorn und holt während einer Benennsitzung nicht auf.

## Beim Ändern beachten

Labels hängen am Gesicht, nicht am Vektor. Präsenz, Merkmalsqualität und Namenszuordnung
getrennt halten. Nie automatische Zuordnungen als Referenz oder Trainingsdaten verwenden, nie
das abgefragte Gesicht als eigene Referenz zählen. Schwellen nur mit Messung ändern.
„Biometrie entfernen“ muss alle Gesichtstabellen, bekannte Sicherungen, den Export und den
Kachelordner erfassen.
