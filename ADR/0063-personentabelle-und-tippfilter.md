# ADR-0063: Personentabelle mit Detailspalte und Tippfilter-Auswahlfeldern

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-09
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-09
- **Bezieht sich auf:** [0038](0038-gemeinsamer-arbeitsbereich.md) und [0045](0045-fotofokus-in-der-sammlung.md) (ergänzt: Liste links, Inspektor rechts auch für Personen), [0048](0048-erkennungsablauf-im-personenbereich.md) (die drei Schritte sind in den Reitern Personen, Stammbaum und Graph auf die Titelzeile verkürzt), [0058](0058-personengraph-als-netzwerk.md) (Personensuche im Graph), [0060](0060-beziehungstypen-und-ableitung.md) und [0061](0061-stammbaum-als-normalisiertes-modell.md) (ersetzt nur die Bedienoberfläche der Beziehungseingabe „Person A – Rolle – Person B“ im Reiter und die Platzierung der Rückfragen; Modell, Regeln und Speicherung bleiben)

## Kontext

Die Personenseite bestand aus einer Liste in einem 130 Punkt hohen Scrollbereich innerhalb eines
zweiten Scrollbereichs, ohne Sortierung, Auswahl oder Attribute, mit Fenster zum Bearbeiten und
Aufklappfeldern ohne Tippen für jede Personenauswahl. Befund und Variantenvergleich:
[docs/ux-personenseite.md](../docs/ux-personenseite.md).

## Entscheidung

1. **Master-Detail.** Reiter „Personen & Beziehungen“: links eine sortierbare, virtualisierte Tabelle
   (Mini-Porträt, Name, Fotos, Gesichter, Geburtsjahr · Alter, Geschlecht, Beziehungen,
   Familie, erstes/letztes Foto, Hinweise) mit Suchzeile; rechts die Eigenschaften der gewählten Person
   (Porträt, Name, Geschlecht, Geburtsjahr, Notiz, Beziehungen, Beziehung hinzufügen, Rückfragen zur Person,
   Fotos/Graph/Stammbaum, Zusammenführen mit Bestätigung). Je Spalte genau ein Scrollbereich.
2. **Reine Logik im Kern.** `core::people_table` (Spalten, Zeilenmodell, stabile Mehrspaltensortierung mit
   leeren Werten zuletzt, Filter, sichtbare Spalten nach Breite) und `core::choice_filter` (Normalisierung
   über `files::fold`, Akzent- und Umlautvarianten, Rang Präfix vor Wortanfang vor Teiltreffer, stabil).
3. **Daten vom Worker.** `faces::page` liefert im Reiter `persons` Zeilen und Porträts: eine gruppierte
   Abfrage über `ix_faces_person` (`load_stats`) plus bereits geladene Kanten, Stammbaum und Rückfragen.
   Die UI übernimmt das Ergebnis nur bei passender Revision; die Auswahl ist eine Personen-ID und überlebt
   Neuladen; ungespeicherte Eingaben im Detail bleiben bis zum Speichern oder Zurücksetzen erhalten.
   Für Persons-, Stammbaum- und Graph-Reiter entfällt die Gruppenabfrage der Gesichtsreiter.
4. **Tippfilter-Auswahlfeld** `ui::select::FilterSelect`: Popup mit sofort fokussiertem Suchfeld, 36-Punkt-Zeilen,
   höchstens 60 Treffer mit „… N weitere“, ein Scrollbereich, Pfeiltasten/Enter/Esc. Es ersetzt alle
   Personenauswahlen und die Beziehungsrollen. Feste, kurze Fachlisten (Arbeitsportion, Modellwahl,
   Metadaten-Kriterien, Gesichtszustand) bleiben `ComboBox`; das Geschlecht steht als Schaltflächengruppe.
5. **Rückfragen** werden typunabhängig dargestellt (`question_text` mit allgemeinem Rückfalltext für neue
   `SuggestionKind`-Werte). Personen werden weiterhin ausschließlich über `faces::add_person` angelegt.

## Alternativen

Kartenzeilen mit Filterleiste und eine schmale Liste mit Detailreitern, siehe Vergleich. Eine fertige
Tabellenbibliothek (`egui_extras`) wurde nicht eingebunden, um keine Abhängigkeit für eine kleine
Zeichenroutine aufzunehmen; Spalten, Sortierung und Auswahl stehen ohnehin in eigener, getesteter Logik.

## Folgen und Grenzen

* Nutzerentscheidungen und IDs bleiben unverändert; „Zusammenführen“ verlangt nun eine Bestätigung.
  Eine Person löschen gibt es weiterhin nicht.
* Bei 980 Punkt Breite zeigt die Tabelle nur vier Spalten; die übrigen sind erst in größeren Fenstern sichtbar.
* Mini-Porträts verwenden das Gesicht mit der besten Qualität (keine Vektoren in der Tabellenabfrage).
* Entwurf ohne Nutzerstudie; Screenreader und Hell-Design nicht geprüft.

## Bei Weiterentwicklung beachten

Keine zweite Scrollebene in einer Spalte einführen. Neue Personenauswahlen verwenden `FilterSelect`.
Neue Spalten brauchen Breite, Priorität, Sortierwert und Test in `core::people_table`.
Schwere Zählungen gehören in `faces::page` (Worker), nicht in die Darstellung.

## Belege und Validierung

[Tabellenmodell](../src/core/people_table.rs), [Filter](../src/core/choice_filter.rs),
[Auswahlfeld](../src/ui/select.rs), [Seite](../src/ui/app/people_table.rs),
[Datenbanktest](../tests/people_table.rs), Layout- und Auswahltests in
[tests.rs](../src/ui/app/tests.rs), Sichtprüfung in [validierung.md](../docs/validierung.md).
