# ADR-0036: Vollständige Kopien exklusiv veröffentlichen

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung
- **Löst ab:** [0005](0005-nichtdestruktiver-import.md), ausschließlich dessen direkte Veröffentlichung unvollständiger Kopien im Rust-Projekt.

## Kontext

Direkte Zieldateien können bei Abbruch teilweise sichtbar bleiben.

## Entscheidung

Pro Datei eine temporäre Datei im Ziel-Dateisystem, blockweise Kopie mit Abbruch,
mtime-Übernahme und sync_all. Anschließend persist_noclobber unter einem freien
Original- oder Suffixnamen. Keine Ersetzung und keine Quelllöschung. Fertige
Dateien bleiben bei weiteren Fehlern erhalten. DNG durchläuft denselben Schutz.

## Alternativen

Direktes Schreiben wie bisher ist einfacher, zeigt aber Teilkopien. Rename mit
Ersetzung gefährdet vorhandene Dateien. Eine Transaktion des gesamten Imports
würde zusätzlichen Platz und komplexe Wiederherstellung verlangen.

## Folgen und Grenzen

Stromausfälle können temporäre Reste hinterlassen. Dateisystem- und SMB-Semantik
begrenzen dauerhafte Crash-Garantien; kein zugesichertes atomisches Gesamtimport.
Windows-Erstellzeit und ACL-Kopie sind noch keine zugesicherte Parität zu copystat.

## Bei Weiterentwicklung beachten

Temporäre Reste nicht automatisch fremden Aufträgen zuordnen oder löschen.
Kollisionen, Wiederholung, Fehler und Abbruch mit echten Dateisystemtests prüfen.

## Belege und Validierung

[Kopieren](../src/core/files.rs), [Regressionen](../tests/import.rs).
