# Personenseite: Befund, Varianten, Entscheidung

Stand 2026-11-09. Entwurfsbewertung, keine durchgeführte Nutzerstudie. Bezieht sich auf
ADR [0038](../ADR/0038-gemeinsamer-arbeitsbereich.md), [0045](../ADR/0045-fotofokus-in-der-sammlung.md),
[0060](../ADR/0060-beziehungstypen-und-ableitung.md), [0061](../ADR/0061-stammbaum-als-normalisiertes-modell.md);
Entscheidung: [ADR 0063](../ADR/0063-personentabelle-und-tippfilter.md).

## 1 · Befund (Ist-Zustand vor der Änderung)

Grundlage: [Screenshot des Reiters](screenshots/personen-rueckfragen.png) und der Code
(`people_view.rs::person_manager`, `tree_view.rs`).

| Befund | Beleg |
|---|---|
| Die Personenliste ist ein Scrollbereich mit fester Höhe `max_height(130)`: etwa drei Zeilen sichtbar, bei Hunderten Personen ein Fenster-Schlüsselloch | `ScrollArea::…id_salt("persons").max_height(130.0)` |
| Dieser Bereich liegt **in** einem zweiten Scrollbereich (`person-management`, ganze Seite): Mausrad scrollt je nach Zeigerposition die falsche Ebene | Verschachtelung in `people_view` |
| Kein Sortieren, keine Suche, keine Spalten; je Person nur Name, ID, Gesichtszahl, Geburtsjahr als Fließtext | `person_manager` |
| Auswahl ist unsichtbar: „Person A“ ist ein verstecktes Aufklappfeld weiter unten, die Liste kennt keinen Auswahlzustand | `relation_from` ohne Markierung |
| Fünf Aktionen pro Zeile als Mini-Schaltflächen (`small_button`, 20 Punkt hoch) | Zielgrößen unter den 36 Punkt der übrigen App |
| Bearbeiten öffnet ein schwebendes Fenster, Beziehungen stehen darunter, Rückfragen darüber: Eine Person zu bearbeiten heißt scrollen und Fenster verwalten | `windows.rs`, `person_manager` |
| Alle Personenauswahlen sind `egui::ComboBox` ohne Tippen: Gruppenkarten („Person wählen …“, auf 40 Treffer begrenzt und nur durch das Namensfeld gefiltert), Beziehungsfelder, Stammbaum-Fokus, Verwandtschaftsprüfung, Zusammenführen, Filter der Sammlung. Bei 300 Personen ist das eine endlose Liste | `ComboBox::from_id_salt` an 9 Stellen |
| Das Graph-Seitenfeld hat Suchfeld + eigenen Scrollbereich (110 Punkt) + zweiten Scrollbereich für Details | `relationship_view::side_panel` |
| Der Kopf mit den drei Schritten (Analyse, Lernen, Abgleich) belegt rund 40 % der Höhe, auch in Reitern, die ihn nicht brauchen | Screenshot |
| Zusammenführen ohne Rückfrage | `person_manager` |

Nicht betroffen: die Gesichtsgruppen-Reiter (eine Liste pro Seite, ein Scrollbereich) und
die Einstellungen.

## 2 · Varianten

Gemeinsame Annahme: Die Seite bleibt Reiter „Personen & Beziehungen“ im Arbeitsbereich aus
ADR 0038; Mindestfenster 980 × 680, Seitenleiste 190 Punkt, nutzbare Breite etwa 730 Punkt.

### A · Master-Detail mit sortierbarer Tabelle

```text
┌ Suche … ───────────────┐ ┌ Rückfragen · 2 ┐ ┌ ＋ Person ┐
├ ○ Name ▲  Fotos  Geburt│ │ ┌────────┐ Anna Müller       │
│ ◉ Anna M.   36  1980·46│ │ │Porträt │ #1 · 36 Fotos …   │
│ ○ Ben       24     –   │ │ └────────┘ [Fotos][Graph]…   │
│ ○ Clara     12  1991·35│ │ Eigenschaften  [Name      ]  │
│   … (virtualisiert)    │ │ Beziehungen    …             │
│                        │ │ Zusammenführen [Person ▾]    │
└────────────────────────┘ └──────────────────────────────┘
   ein Scrollbereich            ein Scrollbereich
```

* Maus: Kopf sortiert per Klick, Zeile 40 Punkt, Hover und Auswahl klar; Klickweg zur Person: 1 Klick.
* Dichte: 4 bis 10 Spalten je nach Breite; Skalierung: virtualisiert, 1000 Personen kein Problem.
* 980 × 680: Tabelle 430 Punkt (4 Spalten), Detail 300 Punkt; 1920 × 1080: alle 10 Spalten.
* Barrierefreiheit: Zeilen und Köpfe mit AccessKit-Beschriftung, Pfeiltasten.
* Aufwand: mittel (eigene Tabelle, `egui_extras` ist nicht eingebunden). Passt zu ADR 0045 (Liste + breiter Inspektor).

### B · Kartenzeilen mit Filterleiste

```text
[ Suche ][ Nur Rückfragen ][ weiblich ▾ ][ Jahrgang ▾ ][ Sortierung ▾ ]
┌──────────────────────────────┐ ┌ Detail ───────────────┐
│ (Foto) Anna Müller   ★ 36    │ │                       │
│        * 1980 · 2 Eltern     │ │                       │
├──────────────────────────────┤
│ (Foto) Ben …                 │
```

* Maus: große Ziele (56 Punkt), schöne Porträts; Sortierung aber nur über ein Auswahlfeld, kein direkter Klick auf Spalten.
* Dichte: niedrig, etwa 6 Personen auf 680 Punkt Höhe; Hunderte Personen heißt viel Scrollen. Vergleichen mehrerer Werte (Fotos, Alter) ist schwer.
* Aufwand: ähnlich A, plus mehrere Filter-Auswahlfelder. Erfüllt den Wunsch „Tabelle mit Sortierfunktion und weiteren Attributen“ nur unvollständig.

### C · Schmale Liste + Detail mit Reitern

```text
┌ Liste (Name) ┐ ┌ [Eigenschaften][Beziehungen][Fotos] ─────┐
│ Anna         │ │                                           │
│ Ben          │ │  breiter Detailbereich                    │
```

* Maus: Liste sehr effizient; zusätzlicher Klick je Reiter (Beziehungen ist die häufigste Arbeit). Keine Attribute in der Liste, daher kaum Sortierung sinnvoll.
* 980 × 680: gut, 1920 × 1080: viel ungenutzter Platz im Detail.
* Aufwand: gering bis mittel, neue Reiterleiste in der Reiterleiste (zwei Ebenen).

### Bewertung

| Kriterium | A Tabelle + Detail | B Kartenzeilen | C Liste + Reiter |
|---|---|---|---|
| Mausbedienung (Klickwege, Ziele) | sehr gut, Kopf sortiert direkt | gut, Sortierung indirekt | gut, Zusatzklicks |
| Informationsdichte | hoch | niedrig | niedrig in der Liste |
| Hunderte Personen | gut (sortieren, suchen, virtualisiert) | mäßig | gut (suchen), kein Sortieren |
| 980 × 680 | Spalten entfallen nach Priorität | gut | gut |
| 1920 × 1080 | zeigt alle Spalten | Karten verschwenden Breite | Detail verschenkt Breite |
| Barrierefreiheit | tabellarische Semantik, Tastatur | Karten einzeln benennen | zwei Fokusebenen |
| Wartbarkeit | eine reine Sortier-/Filterlogik, ein Zeichenpfad | mehr Filterzustand | zwei Navigationsebenen |
| Konsistenz (ADR 0038/0045: Liste links, Inspektor rechts) | hoch | mittel | mittel |

## 3 · Entscheidung

**Variante A**, ergänzt um zwei Elemente aus B: ein rundes Mini-Porträt je Zeile und eine
Suchzeile über der Tabelle (Tippen filtert sofort). Aus C stammt die Gliederung des
Detailbereichs in Abschnitte (Eigenschaften, Beziehungen, Rückfragen, Zusammenführen); statt
Reitern stehen sie untereinander in einem Scrollbereich, weil die Beziehungen der häufigste
Arbeitsschritt sind und kein Klick zusätzlich kosten sollen.

Folgen für das Layout:

* Genau ein Scrollbereich je Spalte (Tabelle, Detail), keine Verschachtelung.
* Kopf mit den drei Erkennungsschritten ist in den Reitern Personen, Stammbaum und Graph
  auf die Titelzeile verkürzt (läuft ein Auftrag, erscheint seine Fortschrittszeile).
* Rückfragen stehen nicht mehr über der Liste, sondern im Detailbereich: bei der
  betroffenen Person und gesammelt über „Rückfragen · N“.

### Spalten

Name (mit Porträt), Fotos (mit bestätigtem Gesicht), Gesichter (bestätigt), Geburtsjahr · Alter,
Geschlecht, Beziehungen (gespeichert), Familie (Eltern · Kinder · Partner aus dem Stammbaum),
erstes und letztes Foto, Hinweise (offene Rückfragen). Bei wenig Breite entfallen Spalten nach
Priorität (zuerst Familie, Erstes Foto, Letztes Foto, Geschlecht …); Name, Fotos und
Geburt · Alter bleiben. Klick auf den Kopf sortiert (Zahlen und Daten zuerst absteigend, erneuter
Klick kehrt um), Umschalt+Klick fügt ein weiteres Kriterium hinzu (Rangnummer im Kopf). Leere Werte
stehen in jeder Richtung zuletzt; Name und ID sind die feste Nebenordnung.

### Tippfilter-Auswahlfeld

Klick öffnet ein Popup, das Suchfeld hat sofort den Fokus. Treffer: Präfix vor Wortanfang vor
Teiltreffer, stabil; ohne Groß-/Kleinschreibung, `mü`, `mu` und `mue` finden Müller. Zeilen 36 Punkt,
Hover, Auswahl markiert; höchstens 60 Treffer mit „… N weitere“; Pfeiltasten, Enter, Esc.
Listen mit weniger als 8 Einträgen (Beziehungsrollen mit wenigen Optionen) zeigen kein Suchfeld,
sind aber dieselbe Komponente.

Umgestellt: Gruppenkarten, Beziehungsrolle und Gegenperson, Zusammenführen, Stammbaum-Fokus und
Verwandtschaftsprüfung, Personenfilter der Sammlung, Graph-Personensuche (ersetzt Suchfeld + 110-Punkt-Liste).
Bleiben als `ComboBox` (feste Fachlisten ohne Personenbezug): Arbeitsportion,
Modellwahl in den Einstellungen, Metadaten-Kriterien/Operatoren und Gesichtszustand der Sammlungssuche.
Das Geschlecht steht als drei Schaltflächen (kürzer als jedes Aufklappfeld).

## 4 · Grenzen

* Löschen einer Person gibt es weiterhin nicht (ID-Stabilität, ADR 0019); Zusammenführen behält
  die alte ID als Weiterleitung und verlangt jetzt eine Bestätigung.
* Nicht geprüft: Screenreader, echte große Sammlungen, Hell-Design, Bedienung per Touch.
* Mehrspaltensortierung ist nur per Umschalt+Klick erreichbar (Hinweis im Tooltip).
* Porträts sind je Person das Gesicht mit der besten Qualität, nicht das Medoid (der Graph
  verwendet das Medoid); so bleibt das Laden der Tabelle ohne Vektoren.
