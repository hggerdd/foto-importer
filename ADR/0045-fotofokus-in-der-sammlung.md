# ADR-0045: Fotofokus in der Sammlung

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0028](0028-kombinierte-bildsuche.md), [0032](0032-metadaten-ordnerbaum-und-gesichtsrahmen.md), [0033](0033-gesichtskorrekturen-am-bild.md), [0038](0038-gemeinsamer-arbeitsbereich.md)

## Kontext

Der Kopf der Sammlungsseite nimmt im Ausgangsscreenshot rund die halbe Höhe
ein. Der schmale Inspektor zeigt dadurch nur einen kleinen Teil des Fotos und
der zugehörigen Metadaten. Filter und Bestandspflege brauchen ihren Platz
hauptsächlich während der Bedienung.

## Entscheidung

Die Sammlungsseite konkretisiert den gemeinsamen Arbeitsbereich aus ADR 0038.
Eine kompakte Leiste bietet Sammlungswahl, Suche, Ordner, Filter und Bestand.
Filter und Bestandspflege erscheinen in Dialogen. Galerie und Bilddetails
nutzen die verbleibende Höhe. Die Vorschau bleibt beim Scrollen der
Metadaten sichtbar. Personenaktionen erhalten im selben Detailbereich einen
eigenen Reiter. Der Detailbereich lässt sich für die Rastersichtung ausblenden.
Seit [ADR-0047](0047-gesichter-als-suchmetadaten.md) stehen Gesichtsanzahl und
bestätigte Personen zusätzlich im Metadatenreiter und können von dort als
Suchkriterien übernommen werden.

Seitennavigation und Ordnerwahl verwenden die angewendeten Kriterien.
Änderungen am Suchentwurf benötigen weiterhin eine ausdrückliche Anwendung.
Vorschauen nutzen den bestehenden Worker und Cache. Gesichtsrahmen beziehen
sich auf die tatsächlich dargestellte Bildvariante.

## Alternativen

Eine Vollbreitengalerie mit Detailfenster, ein Einzelbild mit Filmstreifen und
drei feste Spalten sind im [Entwurfsvergleich](../docs/ux-sammlung.md) gegenübergestellt.

## Folgen und Grenzen

Die geöffnete Vorschau beansprucht bei normaler Fensterbreite knapp die
Hälfte der Arbeitsfläche. Bestandspflege und erweiterte Kriterien sind mit
einem zusätzlichen Klick erreichbar. Die Vorschau wird nur für die
aktuelle Auswahl geladen. Die Entscheidung setzt keine Schemaänderung voraus.

## Bei Weiterentwicklung beachten

Die Bedienleisten dürfen der Galerie auch bei vielen Kriterien oder Ordnern
keine zusätzliche Höhe entziehen. Vorschau und Metadaten bei 980 × 680 und
normaler Fenstergröße gemeinsam prüfen.

## Belege und Validierung

[Sammlungsansicht](../src/ui/app/collection_view.rs),
[Layoutregression](../src/ui/app/tests.rs), [Entwurfsvergleich](../docs/ux-sammlung.md).
