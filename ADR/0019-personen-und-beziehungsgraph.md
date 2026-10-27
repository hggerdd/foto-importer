# ADR-0019: Personen als IDs und Verwandtschaft aus Grundbeziehungen ableiten

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0008](0008-lokale-sqlite-sammlung.md)

## Kontext

Namen ändern sich und können mehrfach vorkommen. Abgeleitete Familienbeziehungen
können widersprüchlich werden, wenn jede Verwandtschaft separat gespeichert wird.

## Entscheidung

Personen haben stabile IDs; Name, Geburtsjahr, Geschlecht und Notiz sind Attribute.
Gesichtszuordnungen referenzieren die ID. Zusammenführungen setzen `merged_into`
und hängen Zuordnungen um, statt die alte Personidentität hart zu löschen.
Gespeichert werden kanonische Elternbeziehungen sowie symmetrische Partner- und
bei Bedarf explizite Geschwisterbeziehungen. Kindbeziehungen werden invertiert,
weitere Verwandtschaft wird daraus abgeleitet. Geschlechtsspezifische Begriffe gehören zur Anzeige.

## Alternativen

Namen als Schlüssel würden Umbenennungen und Gleichnamigkeit verwechseln. Separate
gespeicherte Großeltern-/Cousin-Kanten würden redundante Daten und Pflegeaufwand erzeugen.

## Folgen und Grenzen

Selbstbeziehungen und Abstammungszyklen werden verhindert; mehr als zwei Eltern
erzeugen eine Warnung. Konfliktträchtige Merges werden abgewiesen. Undo darf eine
gerade erzeugte ungenutzte Person entfernen; das ist keine Zusammenführung historischer Identitäten.

## Bei Weiterentwicklung beachten

Neue Beziehungstypen und Merge-Regeln validieren, Nutzerlabels und Pins erhalten.
Gleichnamige Personen ausdrücklich auswählbar halten; Bezeichnungen nicht mit Identität verwechseln.

## Belege und Validierung

[Graph](../core/person_graph.py), [Personen/Merge](../core/face_service.py),
[Schema](../core/face_store.py), [Graph-Tests](../tests/core/test_faces.py),
[Namensauflösung/Undo](../tests/core/test_face_naming.py).
