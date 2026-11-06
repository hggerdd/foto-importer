# ADR-0047: Gesichter als durchsuchbare Fotometadaten

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0028](0028-kombinierte-bildsuche.md), [0032](0032-metadaten-ordnerbaum-und-gesichtsrahmen.md), [0045](0045-fotofokus-in-der-sammlung.md)

## Kontext

Die Bilddetails zeigen Aufnahmeinformationen direkt. Personen waren erst im
separaten Reiter sichtbar. Die gewöhnliche Suchzeile konnte Pfade und
Kameraangaben durchsuchen. Für Namen musste man den Filterdialog öffnen.

## Entscheidung

Gesichtsanzahl und bestätigte Personen stehen im Metadatenreiter. Ein Klick
übernimmt einen Wert als Entwurf für die kombinierte Suche. Einzelne
Vorkommen und ihre Bearbeitung bleiben im Personenreiter. Die Anzeige ist
unabhängig von den Gesichtsrahmen im Bild.

Die Suchzeile akzeptiert auch Teilstücke von Personennamen. Sie sucht in
aktuellen Gesichtern anhand gespeicherter Zuordnungen. Bestätigte Namen sind
der Standard. Die bestehende Option für unbestätigte Vorschläge gilt auch
für die Suchzeile. Namenswerte bleiben gebundene SQL-Parameter.

Die Anzahl erkannter Gesichter ist ein numerisches Metadatenkriterium.
Gezählt werden aktuelle Vorkommen einschließlich unbekannter und
ignorierter Gesichter. Ein Zahlenvergleich setzt einen abgeschlossenen
Detektionsauftrag des aktiven Modells voraus. Ohne diesen Nachweis fehlt
die aktuelle Anzahl. Ein bereits analysiertes Foto kann dagegen den Wert
null Gesichter haben.

## Alternativen

Ein Namensfilter ausschließlich im Personenreiter wäre beim Sichten schwerer
zu erreichen. Eine Übernahme in EXIF oder XMP würde Daten doppelt speichern
und Personenmetadaten beim Weitergeben einer Datei offenlegen können.

## Folgen und Grenzen

Die Suche verwendet den vorhandenen Sammlungsindex und ändert weder das
Datenbankschema noch Fotodateien. Der Name bezeichnet eine Zuordnung in
dieser Sammlung. Die Anzahl beschreibt erkannte Vorkommen und keine
vollständige Zählung aller Menschen auf dem Foto.

## Bei Weiterentwicklung beachten

Gespeicherte historische Vorkommen bleiben bei Treffer und Anzahl außen vor.
Bestätigte Zuordnungen, unbestätigte Vorschläge und fehlende Detektion
weiterhin unterscheiden. Neue Filter auf Fotoebene kombinierbar halten.

## Belege und Validierung

[Suchabfrage](../src/core/metadata.rs), [Bilddetails](../src/ui/app/collection_view.rs),
[Suchregression](../tests/collection.rs).
