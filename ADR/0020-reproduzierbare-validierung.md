# ADR-0020: Offline-Regressionen von echten Modell- und Leistungsmessungen trennen

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert

## Kontext

Regressionen sollen ohne Kamera, Adobe-Installation, Modell-Download oder private
Fotos prüfbar sein. Solche Tests allein beweisen weder RAW-Kompatibilität noch Erkennungsgenauigkeit.

## Entscheidung

`pytest` unter `tests/core/` und `tests/ui/` ist der reguläre Prüfweg über
`uv run python -m pytest`. Temporäre Sammlungen, synthetische Bilder, kontrollierte
ExifTool-Antworten und Testprozesse isolieren externe Abhängigkeiten. UI-Tests nutzen
echtes Tk und seine Ereignisschleife. Modellkonsistenz mit privaten Referenzbildern
ist opt-in; Evaluation und Benchmarks sind separate Skripte. Vorschau-Skripte
erzeugen Screenshots mit isolierten Einstellungen/Testdaten.

## Alternativen

Nur manuelle Tests wären schwer reproduzierbar. Private Fotos oder Gewichtsdownloads
als Voraussetzung jedes Testlaufs wären fragil. Rein gemockte Widgets würden
Fokus, Tastatur, Queue-Zustellung und Tcl-Lebenszyklus nicht ausreichend prüfen.

## Folgen und Grenzen

GUI-Tests benötigen eine grafische Sitzung. Übersprungene Modelltests bedeuten
fehlende echte Bildvalidierung. Maschinenabhängige Zeiten und Helligkeitsvarianten
sind keine Garantie von 98 % Auto-Precision auf einer privaten Sammlung. Echte
Adobe-/Kameradaten müssen zusätzlich repräsentativ geprüft werden.

## Bei Weiterentwicklung beachten

Für Fehler aussagekräftige Regressionen ergänzen; relevante Prüfungen laufen lassen.
Neue Risiken gezielt prüfen, statt nur Implementierung zu spiegeln. Messbedingungen,
Testdatensatz, übersprungene Prüfungen und nicht belegte Ziele ehrlich benennen.

## Belege und Validierung

[Tests](../tests/conftest.py), [Tk-Fixture](../tests/ui/conftest.py),
[Modelltests](../tests/core/test_face_model_consistency.py), [Evaluation](../scripts/evaluate_faces.py),
[Performancevergleich](../benchmarks/performance.py), [Indexbenchmark](../benchmarks/collection_index.py),
[Vorschau](../scripts/preview_faces.py).
