# ADR-0010: Inventarisierung, Metadaten und Integritätsprüfung trennen

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0008](0008-lokale-sqlite-sammlung.md), [ADR-0009](0009-stabile-foto-identitaeten.md)

## Kontext

Große Sammlungen sollen schnell wiederholt gescannt werden. Alles bei jedem Lauf
zu dekodieren oder vollständig zu hashen wäre teuer; unvollständige Scans dürfen
Dateien nicht irrtümlich als verschwunden markieren.

## Entscheidung

Schnellscan nutzt Verzeichnis-/Stat-Informationen und gestufte Identitäts-/Hashhilfen.
Metadatenanreicherung ist ein eigener fortsetzbarer Ablauf. Vollständige Prüfung
berechnet BLAKE3; bei Abweichung wird der gespeicherte Vergleichswert nicht einfach
überschrieben. Import-Duplikatprüfung bevorzugt Kameraidentität; schwache Name-/Größen-
Treffer werden zusätzlich per schnellem Inhaltshash geprüft.

## Alternativen

Immer vollständiges Hashen wäre langsamer. Nur Name/Größe zu vergleichen könnte
andere Inhalte verwechseln. Ein fehlender Eintrag beweist bei Abbruch keinen Verlust.

## Folgen und Grenzen

Leere/unzugängliche Wurzeln, Zugriffsfehler und abgebrochene Scans markieren
unbesuchte Dateien nicht pauschal als fehlend. Über 20 % fehlende Einträge erfordern
Bestätigung. „Wahrscheinlich importiert“ bleibt ein schwächerer Status und ist konfigurierbar.

## Bei Weiterentwicklung beachten

Schutz gegen Massenverluste und sichtbare Unsicherheit erhalten. Neue Hash-/Heuristik-
Strategien mit Kollisionen, veränderten Dateien und wiederholten Prüfungen testen.

## Belege und Validierung

[Scan/Verify/Importcheck](../core/collection_index.py),
[Regressionsfälle](../tests/core/test_collection_index.py),
[Betriebshinweise](../docs/collection-index.md), Historie: `556ea81`, `e3db3f9`.
