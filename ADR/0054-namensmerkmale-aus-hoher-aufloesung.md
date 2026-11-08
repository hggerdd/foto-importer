# ADR-0054: Namensmerkmale kleiner, klar erkannter Gesichter aus höherer Auflösung

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0015](0015-fortsetzbare-gesichtspipeline.md), [0050](0050-empfindlichere-gesichtspraesenz.md), [0051](0051-trainierter-namensklassifikator.md)

## Kontext

Die Analyse verkleinert Fotos auf 1600 Pixel an der längeren Kante und
bestimmt Gesichtsgröße und Qualität (`score × min(Größe/80, 1)`) in diesem
Bild. Namensmerkmale entstehen ab Qualität 0,5. Ein klar erkanntes Gesicht
(Detektion über 0,9) mit 95 bis 110 Pixeln in einem 4528 Pixel breiten Foto
hat im verkleinerten Bild nur etwa 35 bis 40 Pixel und fällt unter die
Grenze. Es erschien dann weder unter „Benennen“ noch bei Vorschlägen. In einer
geprüften Sammlung betraf das etwa 50 von rund 320 unbenannten Gesichtern.

## Entscheidung

Gesichter mit Detektion mindestens 0,8, aber Qualität unter 0,5, werden aus
einer auf 3200 Pixel begrenzten Fassung desselben Fotos nachgerechnet. Größe
und Qualität werden dort mit derselben Formel bestimmt, die Schwelle 0,5 bleibt.
Reicht die Qualität, entstehen Namensmerkmale, das Gesicht wird gruppiert und
erhält Vorschläge.

Bei neuen Analysen geschieht das im selben Durchlauf. Bereits analysierte
Fotos werden beim nächsten „Fotos analysieren“ gezielt nachgerechnet, ohne
erneute Gesichtserkennung. Gespeicherte Landmarken werden dafür auf das größere
Bild umgerechnet. Pro Foto und Analysestand wird dies höchstens einmal
versucht (`face_jobs`, Stufe `hires`). Fehler brechen die Analyse nicht ab.
Detektionen unter 0,8 bleiben reine Präsenztreffer.

## Alternativen

Die Schwelle zu senken würde winzige Gesichter ungeprüft zulassen. Alle Fotos
grundsätzlich mit 3200 Pixeln zu erkennen verdoppelt Speicher und Zeit der
Erkennung ohne Nutzen für große Gesichter. Eine erneute vollständige
Gesichtserkennung ist für den Nachlauf nicht nötig.

## Folgen und Grenzen

Betroffene Fotos werden einmal zusätzlich geladen. RAW-Dateien liefern nur
die eingebettete Vorschau; ist sie nicht größer als das Analysebild, ändert
sich nichts. Ob die Treffsicherheit bei Gesichtern um 60 Pixel gut genug ist,
ist nicht gemessen. Gruppen und Vorschläge bleiben von dir zu prüfen.

## Bei Weiterentwicklung beachten

Auto-Zuordnung nutzt weiter nur Gesichter mit Qualität ab 0,5. Schwellen und
Größe 3200 nicht ohne Messung auf echten Sammlungen ändern.

## Belege und Validierung

[Nachberechnung](../src/core/faces.rs), [Skalierungstest](../src/core/inference.rs),
[Echtlauf auf Sammlungskopie](../tests/real_hires.rs) (ignoriert, benötigt
lokale Modelle und eine private Sammlung).
