# ADR-0007: Optionale DNG-Konvertierung über Adobe als externen Prozess

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0004](0004-vorrang-interaktiver-arbeit.md), [ADR-0005](0005-nichtdestruktiver-import.md)

## Kontext

Nach dem Import sollen NEFs optional zusätzlich als DNG vorliegen, ohne eine
eigene RAW-Konvertierung in die GUI einzubauen oder den Import davon abhängig zu machen.

## Entscheidung

Ein Job verarbeitet NEFs des importierten Ordners mit dem separat installierten
Adobe DNG Converter, dateiweise und ohne Shell. Ausgabe wird zunächst in einem
temporären Unterordner erzeugt und nach erfolgreichem Prozessende geprüft übernommen.
Vorhandene DNGs und RAWs bleiben erhalten; Abbruch beendet den betreffenden Prozess.
Die Versionsprüfung läuft separat, mit höchstens einem Onlineversuch pro lokalem
Kalendertag einschließlich gecachter Fehler; sie installiert keine Updates.

## Alternativen

Eigene RAW-Entwicklung oder Konvertierung im GUI-Prozess würde mehr Format-/Laufzeitlogik
erfordern. Ein Converterfehler darf keine bereits gelungene Kopie rückgängig machen.

## Folgen und Grenzen

Adobe und dessen Pfad bleiben eine optionale externe Voraussetzung. Ausgabeprüfung
prüft Erfolg und nicht leere Datei, nicht die vollständige DNG-Semantik. Die App
ist nicht insgesamt netzwerkfrei: die Versionsabfrage ist von der lokalen Fotoanalyse getrennt.

## Bei Weiterentwicklung beachten

Prozessaufgaben ohne sichtbares Konsolenfenster starten, sauberes Beenden und
Teildatei-Bereinigung erhalten. Eine vollautomatische Installation wäre neu zu entscheiden.

## Belege und Validierung

[DNG-Worker](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/dng_worker.py), [Versionscontroller](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/dng_update_controller.py),
[Prozesstests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_dng_worker.py), [Versionstests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_dng_updates.py).

