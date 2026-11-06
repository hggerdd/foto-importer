# ADR-0052: Bildtexturen stabil im Cache halten

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0011](0011-begrenzte-bildvorschauen.md), [0025](0025-sichtbare-gesichtsvorschauen.md), [0045](0045-fotofokus-in-der-sammlung.md)

## Kontext

Galerie, Inspektor, große Vorschau und Gesichtskacheln luden Bilder sichtbar
häufig neu (Platzhalter „Lädt …“, wiederholte Decodierung). Ursachen in
`src/ui/thumbnails.rs` und `src/ui/app.rs`:

- Der Cache-Schlüssel enthielt die Generation, und jedes Suchergebnis rief
  `Thumbnails::clear()` auf. Auch Seitenwechsel, Aktualisierungen nach
  Personen-/Tag-Änderungen und gleiche Treffer verwarfen alle Texturen.
- Das Limit von 32 MiB war kleiner als eine Galerieseite (48 × 400 px) zusammen
  mit Inspektor- (960 px) und Großvorschau (1920 px). Die LRU-Räumung entfernte
  deshalb gerade sichtbare Bilder, die dann sofort neu angefordert wurden.

## Entscheidung

Der Schlüssel besteht aus Wurzel, Pfad, Orientierung, Ausschnitt und Kantenlänge;
die Generation dient nur noch dazu, verspätete Worker-Antworten nach
`clear()` (neue Sammlung/Scan) zu verwerfen. Suchergebnisse leeren den Cache
nicht mehr. Die Räumung überspringt Einträge, die im aktuellen oder vorigen
Frame gezeichnet wurden. Grenzen: 192 MiB dekodierte Pixel und 512 Einträge
(zuvor 32 MiB / 128). Decodierung bleibt auf dem Worker; Originale bleiben
unverändert.

## Alternativen

Ein reines Anheben der Grenzen ließe das Verwerfen bei jeder Suche bestehen.
Texturen pro Seite zu halten wäre komplexer ohne zusätzlichen Nutzen.

## Folgen und Grenzen

Höherer Speicherbedarf (bis 192 MiB Texturen plus Grafikspeicher). Die Grenze
wird nur überschritten, wenn mehr Bilder gleichzeitig sichtbar sind. Der
Pfad identifiziert Inhalt; Originale sind unveränderlich, daher keine
Dateizeit im Schlüssel. Nicht visuell in der laufenden App geprüft.

## Bei Weiterentwicklung beachten

Schlüssel dürfen keine pro Frame oder Revision veränderlichen Werte enthalten.

## Belege und Validierung

[Cache](../src/ui/thumbnails.rs) mit Tests für Schlüsselstabilität und Räumung.
