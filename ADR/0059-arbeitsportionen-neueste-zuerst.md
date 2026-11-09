# ADR-0059: Analyse und Metadaten in Portionen, neueste zuerst

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-08
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen
- **Bezieht sich auf:** [0010](0010-gestufte-bestandspruefung.md), [0015](0015-fortsetzbare-gesichtspipeline.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md); ergänzt durch [0064](0064-gestufte-indizierung-mit-fortschritt.md)

## Kontext

Eine Sammlung kann mehr als 100.000 Fotos enthalten, die nach und nach
durchgegangen werden, beginnend mit den neuesten. Analyse und Metadaten-
Anreicherung arbeiteten bisher immer alle Fotos in Datenbank-Reihenfolge ab,
ohne Mengenbegrenzung und ohne Bezug zum Aufnahmedatum. Bei dieser Größe
dauert ein solcher Lauf Stunden bis Tage. Nach jeder Analyse wurde zudem die
ganze Datenbank gesichert.

## Entscheidung

Beide Läufe legen ihre Reihenfolge einmal vorab fest (`core::scope`):
**neueste zuerst** nach Aufnahmedatum (`meta.dt_original`), solange es fehlt
nach Dateidatum (`files.mtime_ns`), das bei kopierten Dateien vom Aufnahmedatum
abweichen kann. Bei den Metadaten, für die noch kein Aufnahmedatum vorliegt,
gilt das Dateidatum; Dateien mit früherem Fehler folgen zuletzt.

Ein **Arbeitsumfang** (`Scope`) begrenzt einen Lauf auf eine Portion
(Standard 500, wählbar 100, 500, 2000, 10000 oder alle) und optional auf einen
Ordner der Sammlung. Fertige Fotos werden übersprungen und zählen nicht zur
Portion; ein Lauf bearbeitet also die nächsten noch offenen neuesten Fotos.
Der Fortschritt zählt die Portion. Die Einstellung steht im Schritt
„Fotoanalyse“ des Personenbereichs und gilt auch für den Metadatenlauf, die
Import-Automatik und die nächtliche Automatik. Die Datenbanksicherung nach der
Analyse entfällt, wenn nichts Neues bearbeitet wurde.

## Alternativen

Eine feste Obergrenze ohne Reihenfolge würde alte Fotos zuerst bearbeiten.
Eine Reihenfolge nur nach Foto-ID folgt der Scan-Reihenfolge, nicht der Zeit.
Eine Sortierung bei jeder Seite einer Abfrage skaliert bei 100.000 Fotos nicht;
deshalb wird die ID-Liste einmal je Lauf aufgebaut und in Blöcken gelesen.

## Folgen und Grenzen

Die Reihenfolge nach Dateidatum ist nur ein Ersatz, solange Metadaten fehlen.
Die ID-Liste für 100.000 Fotos belegt wenige Megabyte und wird je Lauf neu
berechnet. Wie lange eine Portion dauert, ist nicht gemessen; die Portionsgröße
sollte nach einem Probelauf gewählt werden. Fotos, deren Datei sich ändert,
gelten wieder als offen und werden in ihrer Zeitreihenfolge nachgeholt.

## Bei Weiterentwicklung beachten

Neue lange Läufe sollen denselben `Scope` nutzen. Reihenfolge und Portion
getrennt vom Fertig-Test halten (fertige Fotos zählen nicht).

## Belege und Validierung

[Reihenfolge und Scope](../src/core/scope.rs), [Analyse](../src/core/faces.rs),
[Metadaten](../src/core/metadata.rs), [Tests](../tests/scope.rs),
[Echtlauf auf Sammlungskopie](../tests/real_hires.rs) (ignoriert).
