# ADR-0025: Gesichtsvorschauen nur im Sichtbereich laden

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-28
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0011](0011-begrenzte-bildvorschauen.md), [0018](0018-gruppenweise-benennung.md), [0023](0023-gesichtsgruppen-mit-begrenztem-aufwand.md)
- **Löst ab:** UI-Ladeentscheidung aus ADR-0018; Gruppenbenennung bleibt erhalten.

## Kontext

Seitenbegrenzung verhindert zwar hunderttausende Tk-Bilder, lädt aber weiterhin
unsichtbare Gruppen. Ein reines Anzahl-Limit erlaubt zudem zu große Zoom-Caches.

## Entscheidung

Inline-Namen, Autovervollständigung, Enter, Tab, Ausreißerauswahl, Gruppenvorschauen
und sitzungsbezogenes Undo bleiben bestehen. Ein eigener Vorschauworker lädt
ausschließlich sichtbare Gesichtskacheln. Scrollen ersetzt wartende Arbeit.
Veraltete Ergebnisse werden verworfen, unsichtbare Tk-Bilder und Snapshot-Bilder
freigegeben. Verborgene Fenster/Ansichten starten keinen Decode.

Importvorschauen besitzen einen LRU-Cache von höchstens 128 Einträgen und 32 MiB
dekodierten Pixeln; Gesichtsvorschauen höchstens 256 Einträge und 16 MiB.
Tk-Bilder existieren nur für sichtbare Kacheln. JPEG-Decoding nutzt vor
Orientierung/RGB-Konvertierung die verkleinerte Decoderauflösung.
Andere Formate können beim Decode weiterhin vorübergehend Vollbilder benötigen.

Personen-Fotolisten werden ausschließlich beim Öffnen des Dialogs und in
100er-Seiten gelesen. Der Hauptsnapshot enthält keine Fotohistorie.
SQLite und Bild-Decoding bleiben im Worker, Tk-Bilder auf dem UI-Thread.

## Alternativen

Vorabladen der gesamten Seite ist einfacher, erfüllt aber den gewünschten
Sichtbereichsvertrag nicht. Ein Cache nur nach Anzahl begrenzt große Zoomstufen
nicht ausreichend. Neue Widgets pro Sammlungseintrag bleiben ausgeschlossen.

## Folgen und Grenzen

Scrollen zeigt vorübergehend Platzhalter. Kleine Bild-Caches sind bewusst
zusätzlich zum Sichtbereich erlaubt. Die Importgalerie hält weiterhin die
Dateiliste als leichte Pfadmetadaten; sie dekodiert sie nicht vollständig.

## Bei Weiterentwicklung beachten

Bildbesitz auch bei Schließen, Sammlungswechsel und verspäteten Antworten
freigeben. Auswahl/Undo beziehen sich auf Gruppenmitglieder, nicht auf
momentan dekodierte Bilder.

## Belege und Validierung

[Face-Worker](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_thumbnails.py), [Cache](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/image_cache.py),
[Workspace](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_workspace.py), [Personendialog](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_view.py),
[Viewport-Test](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_face_view.py),
[Cachegrenzen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_collection_scaling.py).

