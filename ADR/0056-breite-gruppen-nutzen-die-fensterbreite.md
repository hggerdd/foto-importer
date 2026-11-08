# ADR-0056: Gesichtsgruppen nutzen die Fensterbreite

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-08
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen
- **Bezieht sich auf:** [0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md), [0053](0053-kleine-gruppen-zusammenfuehren.md), [0055](0055-foto-vorschau-unter-der-maus.md)
- **Ändert:** die Obergrenze 14 und die Sieben-Spalten-Regel aus 0026

## Kontext

Gruppenkarten zeigten höchstens sieben Spalten mit Kacheln bis 110 Pixel.
Auf breiten Fenstern blieb rund ein Drittel der Fläche leer, obwohl viele
Gesichter derselben Person vorhanden sind.

## Entscheidung

Die Obergrenze einer Gruppe steigt von 14 auf 28 (`config::GROUP_MAX`). Sie
gilt für die Gruppenbildung, das Zusammenführen kleiner Gruppen (0053) und die
Aufteilung bestätigter oder vorgeschlagener Gesichter einer Person.

Die Kartenansicht wählt Spalten und Kachelgröße nach der verfügbaren Breite:
bei mindestens 96 Pixeln je Kachel 7 bis 14 Spalten, die Kacheln füllen die
Breite (42 bis 150 Pixel). Kleine Gruppen erhalten größere Kacheln. Auf breiten
Fenstern ergeben 28 Gesichter zwei Reihen; auf schmalen bis zu vier. Alle
Mitglieder bleiben sichtbar, ohne gruppeninternes Blättern oder Scrollen (0026).

## Alternativen

Nur die Kacheln zu vergrößern lässt mehr Fläche leer und zeigt nicht mehr
Gesichter je Entscheidung. Eine breitenabhängige Gruppengröße in der Datenbank
würde Gruppen beim Verändern des Fensters umbauen.

## Folgen und Grenzen

Bestehende Gruppen mit höchstens 14 Gesichtern bleiben bestehen; größere
entstehen bei neuen Gesichtern, beim Zusammenführen kleiner Gruppen und für
Personen mit vielen Gesichtern. Größere Gruppen bedeuten mehr Gesichter pro
Bestätigung, abgewählte Ausreißer bleiben möglich. Das Aussehen wurde nur
über den Layouttest geprüft, nicht an privaten Sammlungen.

## Bei Weiterentwicklung beachten

`GROUP_MAX`, Kachelspalten und Seitenabfragen gemeinsam ändern.

## Belege und Validierung

[Konfiguration](../src/config.rs), [Seitenabfragen](../src/core/faces.rs),
[Layout und Test](../src/ui/app/people_view.rs), [Gruppentests](../tests/faces.rs).
