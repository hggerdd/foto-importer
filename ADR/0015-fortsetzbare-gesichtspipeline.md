# ADR-0015: Gesichter quellbezogen und modellabhängig verarbeiten

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0009](0009-stabile-foto-identitaeten.md), [ADR-0014](0014-lokale-austauschbare-gesichtsmodelle.md)

## Kontext

Eine Aufnahme hat mehrere Varianten, RAW-Vorschauen haben eigene Abmessungen,
und EXIF-Orientierung verändert das Koordinatensystem. Modellwechsel und Abbruch
dürfen bestätigte Personen nicht verlieren.

## Entscheidung

Die Pipeline analysiert eine bevorzugte Quelle pro Foto: JPEG oder eingebettetes
RAW-JPEG, orientiert und begrenzt verkleinert. Detektion, Qualitätsbewertung,
5-Punkt-Alignment und normalisierte float32-Embeddings laufen im Hintergrund.
Bounding Boxes und Landmarks gehören zur orientierten Quelle, deren Datei-ID/Pfad
gespeichert wird. Jobs mit Modellschlüssel und Quellfingerprint erlauben Fortsetzung,
auch für Fotos ohne Gesicht. Batches begrenzen Decode und Inferenz.

## Alternativen

Jede Variante separat zu analysieren wäre doppelte Arbeit. RAW-Entwicklung wäre
teurer. Reine Pixelkoordinaten ohne Quellbezug würden bei Vorschauwechseln falsch.

## Folgen und Grenzen

Embedderwechsel ergänzt Vektoren und erhält alte Räume. Detektorwechsel benötigt
Sicherung und IoU-Zuordnung innerhalb derselben Quelle; nicht wiedergefundene
Nutzerlabels bleiben markiert erhalten. Abbruch kann den laufenden Batch wiederholen.
Unterschiedlich beschnittene Quellen werden nicht allein über normierte IoU gleichgesetzt.

## Bei Weiterentwicklung beachten

Labels am Gesicht, nicht am Embedding halten. Quell-/Modellinvalidierung, alle
EXIF-Orientierungen, leere Detektionen und Hin-/Zurückwechsel regressionsprüfen.

## Belege und Validierung

[Bildquellen](../core/face_images.py), [Pipeline](../core/face_service.py),
[Schema](../core/face_store.py), [Regressionstests](../tests/core/test_faces.py).
