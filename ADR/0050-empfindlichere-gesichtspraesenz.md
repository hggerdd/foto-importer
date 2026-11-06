# ADR-0050: Empfindlichere Gesichtserkennung für die Präsenzsuche

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0015](0015-fortsetzbare-gesichtspipeline.md), [0028](0028-kombinierte-bildsuche.md), [0039](0039-rust-onnx-adapter.md), [0047](0047-gesichter-als-suchmetadaten.md)

## Kontext

Eine Personensuche ohne weitere Gesichter braucht auch unscharfe, kleine
und schlecht beleuchtete Vorkommen. Die frühere Erkennung nutzte das ganze
Bild mit einem Schwellenwert von 0,80. Auf einem bereitgestellten
Gruppenfoto erkannte der Standarddetektor YuNet damit nur ein Gesicht.
Wenn die Merkmalsberechnung für ein Gesicht fehlschlug, wurde außerdem
der ganze Fotoauftrag als fehlgeschlagen behandelt.

## Entscheidung

YuNet verwendet für Präsenz 0,60, SCRFD 0,55. Ein Durchlauf prüft das
gesamte Bild. Bilder ab 720 Punkten an der längeren und 320 Punkten an
der kürzeren Seite erhalten zusätzlich vier überlappende Ausschnitte.
Kästen werden in Bildkoordinaten zurückgerechnet, an den Bildgrenzen
begrenzt und über alle Durchläufe nach Überlappung zusammengeführt.
Die Zahl der Durchläufe bleibt begrenzt. Der Worker prüft Abbruch
zwischen Durchläufen und Gesichtern.

Ein Präsenztreffer bleibt gespeichert, auch wenn seine Qualität für
Namensmerkmale zu niedrig ist oder deren Berechnung scheitert.
Die Namensmerkmale benötigen weiterhin mindestens 0,80 Detektionsscore
und ausreichende Größe. Frühere Vorschläge entfallen, wenn ein Vorkommen
keine brauchbaren Merkmale mehr hat. Nutzerbestätigungen werden wie
bisher über passende Kästen derselben Quelle wiedererkannt.

Der Detektorschlüssel trägt zusätzlich `:presence-v2`. Für die
strenge Suche sind nur abgeschlossene Analysen mit dieser Erkennung
gültig. Alte Ergebnisse bleiben bis zur erneuten Analyse verfügbar,
gelten aber nicht als Nachweis für „kein weiteres Gesicht“. Die
Merkmalsmodelle behalten ihre Schlüssel.

## Alternativen

Eine niedrigere Schwelle allein verbessert kleine Gesichter im
verkleinerten Gesamtbild nur begrenzt. Eine globale höhere Eingabegröße
passt nicht zu jeder ONNX-Datei. Eine separate Körpererkennung würde
eine andere Suchsemantik und ein weiteres Modell erfordern.

## Folgen und Grenzen

Die erneute Analyse benötigt mehr CPU-Zeit. Die höhere Empfindlichkeit
kann zusätzliche Fehldetektionen erzeugen. Der Filter bedeutet
„kein weiteres erkanntes Gesicht“. Abgewandte oder vollständig
verdeckte Personen können trotz erneuter Analyse unerkannt bleiben.
Für das bereitgestellte Beispiel stieg die Zahl sichtbarer Gesichtstreffer
von eins auf vier mit YuNet und von drei auf fünf mit SCRFD.
Das Foto bleibt außerhalb des Repositorys.

## Bei Weiterentwicklung beachten

Präsenz, Qualität der Merkmale und Namenszuordnung getrennt behandeln.
Bei einer geänderten Detektionsregel auch die Gültigkeit früherer
Negativtreffer prüfen. Vergleiche auf repräsentativen Fotos durchführen,
bevor allgemeine Aussagen über Trefferquoten getroffen werden.

## Belege und Validierung

[Detektionsdurchläufe](../src/core/inference.rs),
[Speicherung](../src/core/faces.rs), [Suchregression](../tests/collection.rs).
