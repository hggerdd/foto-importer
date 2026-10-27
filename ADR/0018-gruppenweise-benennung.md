# ADR-0018: Gesichter gruppenweise inline benennen und prüfen

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0003](0003-ui-thread-und-worker.md), [ADR-0016](0016-namensvorschlaege-aus-referenzen.md)

## Kontext

Die ursprüngliche Kachelansicht mit globaler Personenauswahl und vielen Aktionen
wurde vom Nutzer als zu langsam und unübersichtlich abgelehnt. Gewählt wurde ein
Entwurf mit einer Zeile je Gruppe und direkter Namenseingabe.

## Entscheidung

`Benennen` zeigt ähnliche Gesichter in einer Zeile mit eigenem Namensfeld,
Autovervollständigung und Enter-Zuordnung. Alle Gruppenmitglieder sind zunächst
ausgewählt; Ausreißer lassen sich abwählen. Tab wechselt zum nächsten Namensfeld.
`Prüfen` nutzt denselben Ablauf mit vorausgefülltem Namen. Verwaltung und technische
Optionen liegen im Menü. Gruppen bleiben auch über 60 Gesichter zusammen; Vorschauen
und Seiten sind begrenzt. Auswahl und Entwürfe überleben das Durchblättern.

## Alternativen

Einzelkachel-Auswahl und getrenntes Personen-Anlegen erfordern mehr Schritte.
Eine fokussierte Ein-Gesicht-Prüfung war ein anderer Entwurf, wurde nicht als
primäre Arbeitsfläche gewählt. Unbekannte werden temporär per Complete-Link-Kosinus gruppiert.

## Folgen und Grenzen

Abgewählte Gesichter werden beim Zuordnen für diesen Namen abgelehnt, damit sie
nicht sofort automatisch zurückkehren. Neue Person und Labels entstehen zusammen.
Bis zu 20 Zuordnungen sind sitzungsbezogen rückgängig machbar, einschließlich
Ablehnungen/Pins; andere Änderungen oder Sammlungswechsel leeren diesen Verlauf.

## Bei Weiterentwicklung beachten

Schnelles Tippen/Enter, Ausreißer, ganze Gruppen, Fokus und Undo als gemeinsamen
Ablauf testen. Neue technische Funktionen nicht wieder in die Hauptfläche drängen.

## Belege und Validierung

[Workspace](../ui/face_workspace.py), [Zeile](../ui/face_group_row.py),
[Transaktion/Undo](../core/face_naming.py), [Naming-Tests](../tests/core/test_face_naming.py),
[70-Bilder-/Tastaturtests](../tests/ui/test_face_view.py),
[Screenshot](../docs/screenshots/face-group-naming.png), Historie: `212062c`, `b82c45e`.
