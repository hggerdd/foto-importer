# ADR-0037: Schema 6 als Kompatibilitätsgrenze

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0008](0008-lokale-sqlite-sammlung.md), [0009](0009-stabile-foto-identitaeten.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0029](0029-schneller-gesichtsstart-und-schema-reparatur.md), [0030](0030-gemeinsame-sammlungs-schreibwarteschlange.md)

## Kontext

Eine neue DB-Struktur würde bestehende IDs und Personen gefährden. Gleichzeitig
ist ein ungeprüfter Port aller historischen Reparaturen riskant.

## Entscheidung

Neue Sammlungen verwenden das aus Referenzcommit 32fbe476 extrahierte Schema 6
einschließlich Triggern und Projektionen. Bestehende Schema-6-Sammlungen werden
ohne ID-Neuvergabe geöffnet. Andere Versionen erhalten einen verständlichen Fehler;
Upgrade zunächst mit der Python-Referenz bzw. später mit geprüften Rust-Migrationen.
Die Host/PID-Sperrdatei bleibt unverändert maßgeblich. Keine fremde Sperre entfernen.
Die UI lässt jeweils einen Schreibauftrag laufen; Leser besitzen eigene Verbindungen.

## Alternativen

Eigenes Schema würde eine verlustfreie bidirektionale Migration verlangen.
Automatische historische Reparaturen ohne Regressionen wären nicht ausreichend belegt.

## Folgen und Grenzen

Dies schränkt historische Upgrades gegenüber Python ausdrücklich ein und ist eine
Migrationsgrenze, keine vollständige Ablösung von ADR 0029. Vor Produktiveinsatz
eine Kopie der Sammlung prüfen. DB-Kompatibilität allein beweist keine Serviceparität.

## Bei Weiterentwicklung beachten

Veröffentlichte Änderungen mit neuer Version und Migrationsfixtures prüfen.
Keine unverträglichen Statuswerte in bestehende Tabellen einführen.

## Belege und Validierung

[Schema](../src/core/schema.sql), [DB-Service](../src/core/collection.rs),
[DB-Tests](../tests/collection.rs), [Funktionsmatrix](../docs/paritaet.md).
