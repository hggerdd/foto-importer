# ADR-0040: Reproduzierbarer Validierungssplit im Rust-Modellraum

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0039](0039-rust-onnx-adapter.md)

## Kontext

Die Referenzkuratierung muss ohne NumPy reproduzierbar bleiben. Ein neuer
Modellraum nach ADR 0039 besitzt noch keinen bestehenden Rust-Validierungssplit.

## Entscheidung

Medoid-, Ausreißer-, Serien-, Duplikat-, Jahresquota- und qualitätsgewichtete
Farthest-Point-Auswahl bleiben erhalten. Höchstens 2048 nach Jahr/Qualität
verteilte Kandidaten plus explizite Pins, insgesamt höchstens 100 Auswahlen.
Der Split verwendet die stabile BLAKE3-Reihenfolge aus Seed und Gesichts-ID
anstelle NumPy-PCG. Nach Möglichkeit werden verschiedene Tage zurückgehalten.
Bei mindestens drei Auswahlen bleiben mindestens eine Referenz und zwei
Validierungsbeispiele; kleine Mengen werden nicht künstlich aufgeteilt.

## Alternativen

NumPy-PCG bitgenau nachzubilden erweitert den Port ohne fachlichen Nutzen im
getrennten Modellraum. Eine nicht deterministische Zufallsquelle erschwert
Fehlerreproduktion. Bereits gespeicherte andere Modellräume bleiben unangetastet.

## Folgen und Grenzen

Die konkreten Validierungs-IDs können von Python abweichen. Der gehaltene Split
und die begrenzte Kandidatenmenge erlauben keine pauschale Präzisionsgarantie.
Pins zählen gegen das Limit; zu viele Pins erzeugen einen sichtbaren Fehler.
Gemeinsame Personenoptimierung nutzt Maximum-Weight-Zuordnung mit unbekannten
Plätzen; Referenzscores bleiben Mittelwerte der besten drei Vergleiche.

## Bei Weiterentwicklung beachten

Seed und stabile IDs erhalten. Keine Auto-Labels als Referenzen verwenden;
Validierungsbilder nicht im Referenzscore mitzählen. Neue Algorithmen separat messen.

## Belege und Validierung

[Auswahl und Zuordnung](../src/core/matching.rs),
[Streaming und dauerhafter Cursor](../src/core/recognition.rs),
[Regressionen](../tests/matching.rs).
