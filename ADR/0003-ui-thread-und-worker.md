# ADR-0003: Tk-Zugriffe nur im UI-Thread

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0002](0002-modulare-desktop-anwendung.md)

## Kontext

Scans, Bilddekodierung, SQLite, Kopieren und Inferenz können lange dauern.
Tk-Zugriffe aus Worker-Threads verursachen Fehler bis zur Widget-Finalisierung.

## Entscheidung

Langsame Arbeit läuft in Threads bzw. externen Prozessen. `UiDispatcher` übergibt
Ergebnisse über eine geschützte Queue; nur der Tk-Thread liest Tk-Variablen,
erzeugt `ImageTk`-Bilder und verändert Widgets. Fortschritt darf zusammengefasst
werden; Abschluss und Fehler bleiben zustellbar. Versionsnummern/Revisionen
verwerfen veraltete Scan-, Vorschau- und Filterergebnisse.

## Alternativen

Synchrone Arbeit im Eventloop blockiert Eingaben. Direkte Worker-Aufrufe von
Tkinter oder unbeschränkte `after`-Meldungen würden Thread- und Lastprobleme erzeugen.

## Folgen und Grenzen

Controller müssen Lebenszyklus, Abbruch und verspätete Ergebnisse behandeln.
Auch Referenzen auf Tk-Variablen dürfen nicht erst bei Worker-Garbage-Collection
freigegeben werden. Die Queue ersetzt keine fachliche Transaktion.

## Bei Weiterentwicklung beachten

UI-Werte vor Worker-Start in normale Python-Werte übernehmen. Beim Schließen
Callbacks abkoppeln und Tcl-Traces/Bilder im UI-Thread lösen. Neue asynchrone
Ansichten gegen veraltete Antworten und Schließen testen.

## Belege und Validierung

[Dispatcher](../ui/dispatcher.py), [Controller](../ui/controllers.py),
[Metadatenansicht](../ui/metadata_view.py), [Gruppenzeile](../ui/face_group_row.py),
[Controller-Tests](../tests/ui/test_controllers.py), [Gesichtertests](../tests/ui/test_face_view.py).
