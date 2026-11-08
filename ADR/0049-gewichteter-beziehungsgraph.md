# ADR-0049: Beziehungsgraph mit gemeinsamen Fotos

- **Status:** Akzeptiert; Darstellung (Ego-Graph, Modalfenster) teilweise abgelöst durch [0058](0058-personengraph-als-netzwerk.md)
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0019](0019-personen-und-beziehungsgraph.md), [0028](0028-kombinierte-bildsuche.md), [0047](0047-gesichter-als-suchmetadaten.md), [0048](0048-erkennungsablauf-im-personenbereich.md)

## Kontext

Die Personenverwaltung kennt eingetragene Familienbeziehungen. Gemeinsame
Fotos sind über die Suche auffindbar. Ein Graph soll beides im Zusammenhang
zeigen und häufig gemeinsam fotografierte Personen sichtbar machen.

## Entscheidung

Die ausgewählte Person steht in der Mitte. Der Graph zeigt bis zu zwölf
direkte Verbindungen nach Anzahl gemeinsamer Fotos. Eingetragene
Beziehungen ergänzen die Verbindungen. Ohne gemeinsames Foto erscheinen
sie gestrichelt. Die durchgezogene Linienstärke wächst mit der Fotozahl,
logarithmisch skaliert auf die aktuelle Ansicht. Die genaue Anzahl steht
im Hinweis am Knoten und in der Verbindungsliste. Die Liste öffnet die zugehörige
Sammlungssuche. Ein Klick auf eine Person macht sie zum Mittelpunkt.

Die lokale Abfrage zählt eindeutige Foto-IDs im Suchindex. Sie verwendet
aktuelle bestätigte Zuordnungen gemäß ADR 0028. Mehrere Vorkommen derselben
Person und Dateivarianten erhöhen die Anzahl nicht. Die Abfrage läuft
lesend und abbrechbar im Hintergrund. Sie liefert nur den angezeigten
Ausschnitt und verwendet keine zusätzlichen Kopien der Fotos oder Vektoren.

## Alternativen

Ein Graph der gesamten Sammlung würde viele überlagerte Linien zeichnen
und eine große Ergebnismenge laden. Eine reine Rangliste würde die genaue
Zahl zeigen und weniger Orientierung beim Wechsel zu einer anderen Person
bieten. Die Rangliste ergänzt den Graphen als bedienbare Verbindungsliste.

## Folgen und Grenzen

Die Linienstärken sind innerhalb einer Ansicht vergleichbar. Ein Wechsel
des Mittelpunkts kann die Skala ändern. Häufige gemeinsame Fotos sagen
nichts über den Verwandtschaftsgrad aus. Eingetragene Verwandtschaft
bleibt eine eigene Information. Eine Schemaänderung ist nicht erforderlich.

## Bei Weiterentwicklung beachten

Die Photo-ID und die Zuordnungsregeln der Sammlungssuche erhalten.
Eingetragene und fotografisch belegte Verbindungen unterscheidbar halten.
Aktualisierungen nach Sammlungs- und Mittelpunktwechsel nur aus der
passenden Abfrage übernehmen.

## Belege und Validierung

[Abfrage](../src/core/relationship_graph.rs),
[Graphansicht](../src/ui/app/relationship_view.rs),
[Regressionen](../tests/relationship_graph.rs).
