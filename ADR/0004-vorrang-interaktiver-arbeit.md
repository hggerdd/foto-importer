# ADR-0004: Interaktive Analyse hat Vorrang vor Hintergrundaufträgen

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0003](0003-ui-thread-und-worker.md)

## Kontext

Kopieren und DNG-Konvertierung sollen weitere Auswahl und Vorschau nicht ausbremsen.
Unbegrenzte parallele Jobs überlasten Datenträger und CPU.

## Entscheidung

Ein gemeinsamer `TaskScheduler` reserviert Vordergrundarbeit sofort. Kopier- und
DNG-Jobs warten pro Lane in FIFO-Reihenfolge; standardmäßig läuft je einer.
Sie prüfen kooperative Checkpoints, einschließlich während Kopierblöcken.
Nach Vordergrundarbeit gilt eine kurze Ruhezeit. Gesichtsanalysen geben ebenfalls
an Checkpoints nach. Unter Windows werden Hintergrundthreads bzw. Adobe-Prozesse
zusätzlich niedriger priorisiert.

## Alternativen

Unbegrenzte Threads oder nur Betriebssystemprioritäten würden keine verlässliche
fachliche Reihenfolge schaffen. Eine globale Prozess-Absenkung würde auch die GUI treffen.

## Folgen und Grenzen

Mehr Aufträge können warten als gleichzeitig arbeiten. Laufende OS-I/O und eine
bereits gestartete Adobe-Konvertierung sind nicht sofort präemptierbar.
Die Prioritätsabsenkung ist plattformabhängig, die kooperative Steuerung bleibt bestehen.

## Bei Weiterentwicklung beachten

Neue teure Hintergrundarbeit an den Scheduler anbinden und Abbruch im Wartezustand
erhalten. Limits und Checkpoint-Größen zentral konfigurieren und mit Lasttests prüfen.

## Belege und Validierung

[Scheduler](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/task_scheduler.py), [Vorgaben](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/config/settings.py),
[Scheduler-Tests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_task_scheduler.py),
[Vorrangtests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_background_priority.py), Historie: `cc688bf`, `5ea4954`.

