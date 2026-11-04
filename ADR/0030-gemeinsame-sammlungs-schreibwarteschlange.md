# ADR-0030: Schreibaufträge einer Anwendung pro Sammlung koordinieren

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-29
- **Art:** Neue Entscheidung nach einem intermittierenden Sperrfehler
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0008](0008-lokale-sqlite-sammlung.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0029](0029-schneller-gesichtsstart-und-schema-reparatur.md)

## Kontext

Index- und Gesichtscontroller hatten voneinander unabhängige Busy-Flags. Ihre
Worker konnten gleichzeitig einen Sammlungs-Schreiber öffnen. Die vorhandene
Dateisperre machte den zweiten Zugriff nur lesbar; der Schreibversuch meldete
daraufhin „andere Instanz“, obwohl beide Aufträge aus derselben Anwendung kamen.
Der Index-Status belegte unnötig ebenfalls einen Schreibzugriff. Bei der aktuellen
Diagnose war keine dauerhafte Sperrdatei vorhanden; der interne Konflikt lässt
sich mit überlappenden Controllern reproduzieren.

## Entscheidung

Alle schreibenden UI-Aufträge von Index- und Gesichtscontroller verwenden eine
prozessweite FIFO-Warteschlange je aufgelöstem, auf Windows normalisiertem
Sammlungsordner. Ein Kontext reserviert den Schreibplatz, öffnet anschließend
einen eigenen CollectionIndex auf dem Worker und gibt Verbindung und Platz auch
bei Fehler oder Abbruch frei. SQLite-Verbindungen werden nicht zwischen Threads
geteilt. Verschiedene Sammlungen werden nicht gegenseitig blockiert.

Warten geschieht ausschließlich auf Workern, mit kooperativer Abbruchprüfung.
Ein wartender Auftrag wird erst nach Freigabe ausgeführt; Abbruch während des
Wartens führt die Operation nicht aus. Die Oberfläche zeigt den Wartezustand und
setzt ihn bei Beginn, Abschluss oder Abbruch zurück. Normale Hintergrundschritte
ohne Wartezustand überschreiben keine Rückmeldung einer Gruppenaktion.

Reine Index-Statusabfragen öffnen ausdrücklich lesend und lösen kein Upgrade aus.
Das Anschlussverhalten aus ADR 0029 bleibt erhalten: Gesichtsstatus kann parallel
zu Hintergrundarbeit gelesen werden. Die UI unterscheidet eine absichtlich lesende
Statusabfrage von einer wegen einer fremden Sperre schreibgeschützten Operation.

Die vorhandene Host/PID-Dateisperre bleibt maßgeblich für andere Prozesse. Der
Queue-Kontext fordert nach dem Öffnen ausdrücklich Schreibfähigkeit; eine fremde
Sperre wird weder gelöscht noch umgangen. Das bisherige direkte CollectionIndex-
API und dessen Nur-Lese-Fallback bleiben kompatibel. Es werden keine produktiven
Sperrdateien zur Behebung des internen Konflikts entfernt.

## Alternativen

Mehrere Schreiber mit SQLite-busy_timeout allein würden die fachliche
Anwendungssperre umgehen. Das Löschen einer aktiven Dateisperre wäre unsicher.
Unbegrenzte Wiederholungen nach PermissionError können einen echten fremden
Schreiber nicht von eigener Arbeit unterscheiden und schlecht abbrechbar sein.
Ein globaler Mutex für alle Sammlungen würde unabhängige Ordner unnötig sperren.

## Folgen und Grenzen

Ein langer laufender Schreibauftrag wird nicht präemptiert. Nachfolgende
Änderungen warten sichtbar; ein laufender Indexauftrag kann über dessen
Abbrechen-Aktion beendet werden. Wartende Plätze werden auch bei Ausnahmen
freigegeben. Die Warteschlange gilt für die beiden UI-Schreiberpfade und wird
nicht persistent gespeichert. Neue UI-Schreiber müssen denselben Kontext nutzen.
Externe und verwaiste Sperren bleiben durch die bestehende Sperrverwaltung geregelt;
Netzwerk-/Mehrprozess-Schreibbetrieb wird nicht neu freigegeben.

## Bei Weiterentwicklung beachten

Keine Schreibfähigkeit aus der PID allein ableiten und keine aktive fremde Sperre
übernehmen. Lesen nicht unnötig in die Schreibwarteschlange stellen. FIFO,
Sammlungsunabhängigkeit, Abbruch und externe Prozesssperren prüfen. SQL und
Warten bleiben außerhalb des UI-Threads.

## Belege und Validierung

[Schreibkontext](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/collection_writer.py),
[Indexcontroller](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/collection_controller.py),
[Gesichtscontroller](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_controller.py),
[FIFO/Abbruch und echter zweiter Prozess](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_collection_writer.py),
[überlappende Controller und lesender Status](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_collection_controller.py),
[Gruppenaktionen/Status](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_face_view.py).

[UI-Sichtprüfung eines wartenden Auftrags](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/screenshots/collection-writer-wait.png).

