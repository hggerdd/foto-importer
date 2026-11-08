# ADR-0055: Ganzes Foto als Vorschau unter der Maus im Personenbereich

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-08
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen
- **Bezieht sich auf:** [0011](0011-begrenzte-bildvorschauen.md), [0025](0025-sichtbare-gesichtsvorschauen.md), [0052](0052-stabile-bildtexturen.md)

## Kontext

Gesichtsgruppen zeigen nur Ausschnitte. Zum Benennen hilft der Kontext des
ganzen Fotos, ohne es erst öffnen zu müssen.

## Entscheidung

In den Gesichtsreitern des Personenbereichs (Benennen, Vorschläge, Bestätigt,
Ignoriert) erscheint beim Überfahren einer Gesichtskachel das ganze zugehörige
Foto in einem schwebenden Feld. Es liegt rechts unterhalb des Mauszeigers
und folgt ihm; fehlt der Platz, weicht es nach links bzw. oben aus und bleibt
im Fenster. Es nimmt keine Mausereignisse an.

Pro Frame wird höchstens ein Gesicht erfasst (das unter der Maus); der Wert
wird zu Beginn jedes Frames gelöscht. Verlässt die Maus die Kachel oder das
Fenster, verschwindet die Vorschau sofort. Das Bild wird wie alle Vorschauen
auf dem Worker geladen (800 Pixel, Cache nach 0052) und das Original nie verändert.

Ein Schalter am unteren Rand des Personenbereichs schaltet die Funktion ein
oder aus. Standard ist eingeschaltet. Die Einstellung liegt in `settings.json`
(`hover_preview`); ältere Dateien ohne dieses Feld gelten als eingeschaltet.

## Alternativen

Ein Klick zum Öffnen unterbricht den Benennungsablauf. Ein festes Vorschaufeld
verdeckt Gruppen oder verbraucht Platz.

## Folgen und Grenzen

Schnelles Überfahren vieler Kacheln stößt mehrere Vorschau-Ladeaufträge an;
sie laufen auf dem Worker und werden zwischengespeichert. Das Feld hat feste
Größe (520 × 390); Hochformate lassen seitlich Platz. Das Aussehen wurde
nur im UI-Test auf Zustandsebene geprüft, nicht visuell in der laufenden App.

## Bei Weiterentwicklung beachten

Nie mehr als eine Vorschau gleichzeitig; Zustand pro Frame zurücksetzen.
Weitere Ansichten mit Gesichtskacheln können dieselbe Funktion nutzen.

## Belege und Validierung

[Vorschau und Schalter](../src/ui/app/people_view.rs),
[UI-Test](../src/ui/app/tests.rs).
