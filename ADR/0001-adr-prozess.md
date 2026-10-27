# ADR-0001: Architekturentscheidungen dokumentieren und beachten

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-27
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen

## Kontext

Die Anwendung hat sich vom Dateiimport zu Sammlung, Metadaten und Gesichtserkennung
entwickelt. Begründungen liegen bislang verteilt in Code, Spezifikationen, README
und Gesprächsverlauf. Veraltete Agentenhinweise behaupten unter anderem, es gebe
noch keine Tests oder große Module. Das erschwert konsistente Weiterentwicklung.

## Entscheidung

Wesentliche Entscheidungen erhalten jeweils einen nummerierten ADR unter `ADR/`.
Vor Änderungen sind der Index und betroffene akzeptierte ADRs zu lesen und bei
Entwurf, Implementierung und Prüfung zu berücksichtigen. Neue Entscheidungen und
begründete Ablösungen werden mit der jeweiligen Änderung dokumentiert.
AGENTS.md und CLAUDE.md verankern diesen Ablauf für beide Agenten.

## Alternativen

Nur README, Kommentare oder Chat-Protokolle weiterzuführen würde Entscheidungsgrund,
Gültigkeit und Ablösungen weiterhin verstreuen. Ein eigener externer Dienst ist
für dieses Repository nicht erforderlich.

## Folgen und Grenzen

Es entsteht etwas Dokumentationsaufwand, dafür eine versionierte Entscheidungshistorie.
Nicht jede lokale Codeänderung benötigt einen ADR. Die rückblickenden ADRs belegen
den heutigen Stand, ohne unbekannte historische Entscheidungsdaten zu erfinden.

## Bei Weiterentwicklung beachten

Die [Arbeitsweise](README.md#arbeitsweise-ab-jetzt) und [Vorlage](TEMPLATE.md)
verwenden. Widersprüche durch explizite Ablösung lösen, alte Begründungen erhalten.

## Belege und Validierung

[AGENTS.md](../AGENTS.md), [CLAUDE.md](../CLAUDE.md), [ADR-Index](README.md).
Für diese reine Dokumentationsänderung werden Struktur, lokale Links und
Konsistenz der Agentenregeln geprüft; das Programmverhalten wird nicht geändert.
