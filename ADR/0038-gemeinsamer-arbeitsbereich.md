# ADR-0038: Gemeinsamer Arbeitsbereich

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0011](0011-begrenzte-bildvorschauen.md), [0025](0025-sichtbare-gesichtsvorschauen.md), [0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md), [0027](0027-sichtbarer-erkennungsstand.md), [0028](0028-kombinierte-bildsuche.md), [0031](0031-inkrementelle-gesichtsgruppen-ansicht.md)

## Kontext

Import, Sammlung, Suche und Personen brauchen zusammenhängende Bedienung.

## Entscheidung

Konzept C aus der dokumentierten Drei-Konzept-Kritik: feste Textnavigation,
gemeinsamer Arbeitsbereich, kontextabhängiger Inspektor, sichtbare Aufgaben.
Primäraktion pro Kontext, leere Zustände mit Einstieg, konkrete Fehler statt
stiller Fallbacks. Importplan vor Start, bestätigte Identitäten getrennt von Vorschlägen.
Gruppenaktionen beziehen sich auf maximal 14 dargestellte Mitglieder.

## Alternativen

Geführter Assistent und reine Fotobibliothek sind in der UX-Dokumentation aus
Einstieg, Effizienz, Sicherheit, Zugänglichkeit, Wartbarkeit und Bildschirmgröße kritisiert.

## Folgen und Grenzen

Mehr gleichzeitige Struktur als ein Assistent. Mindestfenstergröße 980×680.
Entwurfsentscheidung ohne Nutzerstudie; Tastatur- und Screenreader-Prüfung bleibt nötig.

## Bei Weiterentwicklung beachten

Keine versteckten ausgewählten Gruppenmitglieder, kein Verlust von Namensentwürfen
durch Fortschrittsereignisse. Sichtbare Resultate erst nach erfolgreichem Speichern.

## Belege und Validierung

[Konzepte](../docs/ux-konzepte.md), [UI](../src/ui/app.rs), [Sichtprüfung](../docs/validierung.md).
