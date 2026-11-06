# ADR-0048: Erkennungsablauf im Personenbereich sichtbar machen

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0016](0016-namensvorschlaege-aus-referenzen.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0027](0027-sichtbarer-erkennungsstand.md), [0031](0031-inkrementelle-gesichtsgruppen-ansicht.md), [0038](0038-gemeinsamer-arbeitsbereich.md)

## Kontext

Ein gemeinsamer Knopf für Lernen und Vorschläge lässt offen, wann bestätigte
Namen als Beispiele dienen und wann der Abgleich tatsächlich läuft. Statuskarten
und aufgeklappte Wartung verdrängten die Gruppen aus dem sichtbaren Bereich.

## Entscheidung

Drei kompakte Schritte bleiben oberhalb der Gruppen sichtbar. Fotoanalyse
erkennt Gesichter und erzeugt Merkmale mit vortrainierten Modellen. Lernen
wählt geeignete bestätigte Vorkommen als Referenzen aus. Der Abgleich
verwendet diese Referenzen für Vorschläge aus vorhandenen Merkmalen.
Vortrainierte Gewichte werden für diesen Arbeitsablauf unverändert verwendet.

Der Bereich zeigt bestätigte Gesichter, ausgewählte Referenzen und Personen
mit Referenzen getrennt. Die laufende Phase stammt vom Worker. Eine feste
Statuszeile zeigt im Leerlauf die aktivierte Fotoanalyse-Automatik.
Ein Dialog nimmt Einrichtung, Zeitplan, Export und Wartung auf.

Offene Arbeit und Pause haben Vorrang vor einem früheren Abschluss.
Fortsetzen verwendet den vorhandenen Auftrag inklusive offener Referenzen
und gespeichertem Cursor. Der vollständige Abgleich speichert seinen
Abschluss mit dem aktiven Embedder-Schlüssel. Der alte, keinem Modell
zugeordnete Zeitstempel wird für die Anzeige nicht verwendet.

## Alternativen

Ein Einrichtungsassistent wäre bei der ersten Nutzung übersichtlich, würde
aber bei jeder späteren Bestätigung den laufenden Stand verstecken.
Ein großes Dashboard wäre für die Diagnose nützlich und würde
den Platz für die laufende Gruppenarbeit verringern.

## Folgen und Grenzen

Der Status zählt gespeicherte Daten in der gewählten Sammlung. Ein Abschluss
beschreibt den Rechenlauf, die Vorschläge bleiben bis zur Bestätigung
vorläufig. Die kompakten Karten ergänzen die vorhandenen Aufgaben.
Die Auswahl, Speicherung und Sortierung der Gesichtsgruppen bleiben gemäß
ADR 0031 und 0026 erhalten.

## Bei Weiterentwicklung beachten

Ein abgebrochener Abgleich darf keinen Abschluss protokollieren.
Phasenwechsel aktualisieren Fortschritt, ohne Eingabefelder neu aufzubauen.
Zeitplanung und Lernzustand auf die aktive Sammlung und Modellwahl beziehen.

## Belege und Validierung

[Personenübersicht](../src/ui/app/recognition_view.rs),
[Abgleich und Abschluss](../src/core/recognition.rs),
[Statusregression](../tests/maintenance.rs).
