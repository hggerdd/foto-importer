# Arbeitsweise

## Entscheidung

- **ADRs:** Themen-ADRs nach den [Regeln im Index](README.md#regeln). Sie beschreiben den
  geltenden Stand; die Historie liegt in Git.
- **Prüfungen vor jedem Commit:** `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- **Offline-Tests und echte Messungen sind getrennt.** `cargo test` nutzt temporäre
  Sammlungen, synthetische Bilder und Vektoren und braucht weder Netz noch Modelle.
  Echte Modelle, Adobe, echte Sammlungen und Leistungsmessungen laufen gesondert
  (`examples/`, opt-in-Tests) und werden mit Datum und Datengrundlage in
  [docs/validierung.md](../docs/validierung.md) festgehalten.
- **Echte Sammlungen** werden für Diagnose und Messung nur lesend geöffnet, nie in Tests
  und nie für Testmigrationen.
- **Branches:** Neue Arbeit beginnt auf einem `codex/`-Branch; `main` ist der stabile Stand.
- **Unteragenten** arbeiten nur im Projektordner: Arbeitskopie unter
  `worktrees/<aufgabe>` (`git worktree add worktrees/<aufgabe> -b codex/<aufgabe> main`),
  Build-Ausgaben über `CARGO_TARGET_DIR` unter `worktrees/.target/<aufgabe>`,
  Zwischendateien im eigenen Worktree unter `tmp/` (also `worktrees/<aufgabe>/tmp/`). Alles
  davon ist von Git ausgeschlossen (`/worktrees/`, `/tmp/`). Nach dem Merge werden Worktree,
  Branch und Build-Ordner entfernt.
- **Python-Referenz** ist nur der lokale Ordner `C:\ttt\foto-importer`: Verhaltensreferenz,
  kein Laufzeitbestandteil. Ein Branch mit der Python-Version existiert nicht mehr.
  Bekannte Abweichungen stehen in [docs/paritaet.md](../docs/paritaet.md) (historischer
  Stand).

## Grenzen

Synthetische Tests belegen keine Erkennungsgüte. Nicht Gemessenes wird als nicht gemessen
benannt; es gibt keine zugesicherte Präzision.

## Beim Ändern beachten

Für jeden behobenen Fehler eine Regression ergänzen. Zahlen in ADRs und Doku nur aus
nachvollziehbaren Messungen übernehmen.
