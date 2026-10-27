# ADR-0014: Gesichtsmodelle lokal über geprüfte Manifeste laden

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert

## Kontext

Biometrische Bilder sollen lokal verarbeitet werden. Detektor und Embedder sollen
unabhängig wechselbar sein; Gewichtslizenz und falsche Farbnormalisierung beeinflussen die Nutzbarkeit.

## Entscheidung

Eine Registry liest JSON-Manifeste mit Rolle, Version, Hash, Lizenz, Pfad und
Eingabespezifikation. SHA-256 wird beim Laden geprüft. Modellidentität enthält
ID, Version und Hash. ONNX Runtime nutzt explizit die CPU; YuNet/SFace haben
OpenCV-Adapter. RGB/BGR, Normalisierung und Alignment werden vom Modell beschrieben.
Kleine YuNet-/SFace-Modelle sind voreingestellt, SCRFD/ArcFace sind installierbare
Alternativen. Gewichte werden separat installiert und nicht ins Git-Repository aufgenommen.

## Alternativen

Cloud-Inferenz, GPU-Pflicht oder PyTorch als Pflichtlaufzeit passen nicht zum lokalen
CPU-Betrieb. Fest verdrahtete Gewichte würden Wechsel, Lizenzanzeige und Prüfbarkeit erschweren.

## Folgen und Grenzen

Kein Foto-/Embedding-Upload und kein automatischer Gewichtsdownload während der
Inferenz. Die explizite Installation benötigt ggf. Netzwerk. InsightFace-Gewichte
haben eingeschränkte Nutzungslizenzen; AdaFace benötigt einen selbst bereitgestellten ONNX-Export.

## Bei Weiterentwicklung beachten

Embedding-Räume nie nach gleicher Dimension gleichsetzen. Neue Adapter mit echten
Konsistenzbildern, korrekter Eingabe und nachvollziehbarer Gewichtslizenz prüfen.

## Belege und Validierung

[Registry/Adapter](../core/face_models.py), [Installationsskript](../scripts/install_face_models.py),
[Registrierung](../scripts/register_face_embedder.py), [Modelldokumentation](../models/README.md),
[Konsistenztests](../tests/core/test_face_model_consistency.py).
