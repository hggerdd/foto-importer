# ADR-0070: Unteragenten arbeiten ausschließlich im Projektordner

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-13
- **Art:** Neue Entscheidung (Arbeitsregel)
- **Bezieht sich auf:** [0001](0001-adr-prozess.md), [0005](0005-nichtdestruktiver-import.md), [0020](0020-reproduzierbare-validierung.md)

## Kontext

Parallel arbeitende Unteragenten (Codex/Claude) haben ihre Arbeitskopien, Build-Verzeichnisse
und Zwischendateien an verschiedenen Stellen der Platte angelegt (`C:\ttt\cargo-target-*`,
`%TEMP%`, `tmp\…`, `.claude\worktrees`). Folgen: Die Platte lief mehrfach voll, niemand
konnte sagen, wer den Platz belegt, und Aufräumen war nur mit Rätselraten möglich.
Außerdem wurden versehentlich Programme außerhalb der Sitzung gestartet.

## Entscheidung

1. **Ein Ort:** Jeder Unteragent arbeitet in einer eigenen Git-Arbeitskopie (Worktree) unter
   `<Projektordner>\worktrees\<aufgabe>` auf einem eigenen `codex/`-Branch
   (`git worktree add worktrees/<aufgabe> -b codex/<aufgabe> rust-main`).
   Der Ordner `worktrees/` ist über `.gitignore` ausgeschlossen. Der Hauptordner selbst
   bleibt auf `rust-main` und wird nur für Merges und Releases verwendet.
2. **Build-Ausgaben im Projektordner:** `CARGO_TARGET_DIR=<Projektordner>\worktrees\.target\<aufgabe>`
   (oder ein gemeinsames `…\worktrees\.target\shared`, wenn nur ein Agent gleichzeitig baut).
   Keine Build-Verzeichnisse außerhalb des Projektordners. Platzsparend bauen
   (`-j 1`, bei Bedarf `CARGO_PROFILE_DEV_DEBUG=0`).
3. **Zwischendateien** (Skripte, Messdaten, Demo-Sammlungen, Screenshots vor dem Commit) liegen
   in `<Projektordner>\tmp\<aufgabe>` bzw. im Scratchpad der Sitzung, nie verstreut auf der Platte.
4. **Nie** schreiben Agenten außerhalb des Projektordners, ausgenommen explizit vom Nutzer
   genannte Ziele (z. B. ein Exportordner). Private Sammlungen des Nutzers werden nicht
   gelesen, der echte zentrale Bestand nicht angefasst (ADR 0005, 0020).
5. **Aufräumen:** Wer eine Aufgabe abschließt, nennt im Bericht Worktree, Branch und
   Build-Ordner. Die Hauptsitzung entfernt nach dem Merge Worktree
   (`git worktree remove`), lokalen Branch und `worktrees\.target\<aufgabe>`. Vor dem
   Löschen wird geprüft, dass kein Agent mehr dort baut.
6. **Prozesse:** Agenten beenden nur Prozesse, die sie selbst gestartet haben, und melden
   verbliebene Prozesse.
7. **Plattenplatz:** Vor großen Builds den freien Platz prüfen; bei weniger als 8 GB nicht
   starten, sondern melden.

## Alternativen

Gemeinsames Build-Verzeichnis außerhalb des Projekts spart Zeit, macht aber Zuordnung und
Aufräumen unmöglich und blockiert sich bei parallelen Builds. Agenten im Hauptordner
arbeiten zu lassen führt zu Konflikten im Arbeitsbaum.

## Folgen und Grenzen

Mehr Platzbedarf im Projektordner (jeder parallele Build hat einen eigenen Ordner, etwa
2 bis 10 GB). Die Regel kann durch das Werkzeug nicht technisch erzwungen werden; sie wird
in jedem Auftrag an einen Unteragenten genannt und in `AGENTS.md` verankert. Bereits
laufende Agenten behalten ihre alten Orte bis zum Abschluss.

## Bei Weiterentwicklung beachten

Neue Aufträge an Unteragenten müssen Worktree-Pfad, Branch und `CARGO_TARGET_DIR` nach
dieser Regel enthalten. Die Hauptsitzung räumt nach jedem Merge auf.
