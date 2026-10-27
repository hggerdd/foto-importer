# ADR-0016: Namen aus bestätigten Referenzen vorschlagen

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert
- **Bezieht sich auf:** [ADR-0014](0014-lokale-austauschbare-gesichtsmodelle.md), [ADR-0017](0017-kuratierter-referenzdatensatz.md)

## Kontext

Neue und bereits analysierte Fotos sollen Namensvorschläge erhalten und aus
Bestätigungen/Korrekturen profitieren, ohne automatische Fehler selbst zu verstärken.

## Entscheidung

Vortrainierte Embeddings werden per Kosinusvergleich gegen bestätigte hochwertige
Referenzen abgeglichen. Bis zu drei beste Ähnlichkeiten ergeben den Personenscore.
Modellbezogene Schwellen trennen unbekannt, vorgeschlagen und automatisch zugeordnet.
Eine gemeinsame Optimierung verhindert dieselbe Person mehrfach pro Foto.
Ablehnungen werden gespeichert; Nutzerlabels werden nie automatisch überschrieben.
Geburtsjahre können unplausible Zuordnungen ausschließen; Ko-Okkurrenz beeinflusst
nur den Graubereich. Nach Korrekturen erfolgt erneuter Abgleich.

## Alternativen

Ein einzelner Personenmittelwert bildet Alter/Pose schlechter ab. Auto-Labels als
Trainingsreferenz können Fehler fortpflanzen. Eigenes Netz-Finetuning ist nicht implementiert.

## Folgen und Grenzen

Das Lernen erweitert/kuratiert Referenzen, es verändert nicht die Netzgewichte.
NumPy-Suche hinter einem Interface genügt zunächst. Auto-Zuordnungen erscheinen
weiter zur Prüfung. Automatische Importanalyse ist opt-in; Nachtjobs benötigen die laufende App.

## Bei Weiterentwicklung beachten

Bestätigungen und Ablehnungen dauerhaft von Maschinenvorschlägen unterscheiden.
Neue Klassifikatoren, Suchindizes oder geänderte Auto-Semantik brauchen Begründung
und Evaluation, besonders mit unbekannten Personen als Negativbeispielen.

## Belege und Validierung

[Matcher](../core/face_algorithms.py), [Referenzen/Rematch](../core/face_service.py),
[Importanbindung](../ui/collection_controller.py), [Regressionstests](../tests/core/test_faces.py),
[Evaluation](../scripts/evaluate_faces.py).
