# ADR-0011: Vollständige Importgalerie mit begrenztem Darstellungsaufwand

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0003](0003-ui-thread-und-worker.md), [ADR-0004](0004-vorrang-interaktiver-arbeit.md)

## Kontext

Ein Aufnahmetag kann Tausende Bilder enthalten. Die frühere begrenzte Vorschau
verbarg Bilder; eine vollständige Menge an Tk-Widgets wäre dagegen zu teuer.

## Entscheidung

Die Importgalerie bildet den gesamten Tag als virtuelle Scrollfläche ab, erstellt
Canvas-Elemente/Bilder aber nur für den sichtbaren Bereich und einen kleinen Vorlauf.
Ein Vorschauworker hält höchstens einen aktiven und einen ersetzbaren wartenden
Auftrag. Ein begrenzter LRU-Cache speichert kleine, abgelöste Pillow-Bilder;
Größe und Dateizeitinformationen invalidieren veränderte Quellen. EXIF-Orientierung
und verkleinerte JPEG-Dekodierung werden beim Laden berücksichtigt.

## Alternativen

Ein festes erstes N Bilder würde weiter Aufnahmen verstecken. Alle Vollbilder oder
alle Widgets gleichzeitig zu laden würde Speicher und Eventloop belasten.

## Folgen und Grenzen

Scrollen kann zunächst Platzhalter zeigen. Der erste Decode bleibt I/O-/CPU-Arbeit;
Caching macht ihn nicht grundsätzlich schneller. Vorschauen sind keine RAW-Entwicklung.

## Bei Weiterentwicklung beachten

Die vollständige Auswahl darf nicht von der geladenen Vorschau abhängen. Widget-/Cache-
Mengen begrenzen und alte Anfragen beim Wechsel verwerfen. Neue Raster mit großen
Sammlungen prüfen, nicht nur mit acht Bildern.

## Belege und Validierung

[Galerie](../ui/preview_widget.py), [Worker](../core/preview_worker.py),
[Cache](../core/thumbnails.py), [Galerietests](../tests/ui/test_preview_widget.py),
[Workertests](../tests/core/test_preview_worker.py), Historie: `f2fb5ee`, `2497e7d`.
