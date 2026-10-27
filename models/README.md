# Lokale Gesichtsmodelle

Die App liest JSON-Manifeste aus diesem Ordner. Gewichte liegen unter `weights/`
und werden nicht ins Git-Repository aufgenommen. Beim Laden wird SHA-256 geprüft.
Die Erkennung selbst benötigt kein Netzwerk und nutzt ausschließlich die CPU.

## Installation

```powershell
uv sync
uv run python scripts/install_face_models.py
```

Dies installiert YuNet und SFace (zusammen etwa 39 MB). Die Gewichte wurden anhand
der SHA-256-IDs im offiziellen OpenCV-Zoo verifiziert. Alternativ:

```powershell
uv run python scripts/install_face_models.py scrfd_10g arcface_buffalo_l
```

Das InsightFace-Archiv hat etwa 289 MB; die App benötigt daraus nur den Detektor
und Embedder. **Die vortrainierten InsightFace-Gewichte sind laut Anbieter nur
für nichtkommerzielle Forschung freigegeben.** Die Lizenz erscheint auch in den
Modelleinstellungen. Der Installer zeigt sie vor dem Download. Eine Offline-
Installation ist möglich: passende ONNX-Dateien an die `file`-Pfade der Manifeste
kopieren; derselbe Hash wird beim Laden kontrolliert.

## Weitere Embedder / AdaFace

Der ONNX-Adapter unterstützt ArcFace und selbst bereitgestellte AdaFace-/CVLFace-
Exporte mit einem NCHW-Bildeingang und Embeddings als erster Ausgabe. Dynamische
und feste Batchgrößen werden unterstützt. PyTorch ist keine Laufzeitabhängigkeit.
AdaFace-Gewichte bzw. ein bestimmter ONNX-Export werden nicht mitgeliefert.

Beispiel für einen bereits lokal erzeugten, vertrauenswürdigen AdaFace-Export:

```powershell
uv run python scripts/register_face_embedder.py D:/Modelle/adaface_ir101.onnx `
  --id adaface_ir101 --version mein-export-v1 `
  --sha256 <erwarteter-SHA256-des-Exports> --dim 512 --color BGR `
  --mean 127.5 --std 127.5 --auto 0.65 --suggest 0.40 `
  --license "Lizenz des verwendeten Checkpoints" `
  --source-url "https://github.com/mk-minchul/CVLface"
```

Die App danach neu starten. Der Registrar überschreibt keine vorhandene ID.
Bei anderen Eingabegrößen oder Alignment-Templates das erzeugte Manifest
entsprechend anpassen. `input.template` erlaubt fünf explizite Zielpunkte,
`input.size` die Ausgabegröße. Die Pipeline liefert RGB; nur der Adapter wendet
`input.color`, `mean` und `std` an. Beim OpenCV-SFace-Adapter übernimmt OpenCV
die interne Vorverarbeitung, sein Eingang ist BGR. AdaFace erwartet im Original
BGR mit auf −1 bis +1 normalisierten Pixeln.

Dimension, Version und Hash gehören zur Identität des Embedding-Raums.
Vektoren verschiedener Modelle werden nie vermischt. Die angegebenen Schwellen
sind Startwerte; vor automatischen Zuordnungen mit eigenen Fotos kalibrieren.

## Quellen

- [YuNet und Modelllizenz](https://github.com/opencv/opencv_zoo/tree/main/models/face_detection_yunet)
- [SFace und Modelllizenz](https://github.com/opencv/opencv_zoo/tree/main/models/face_recognition_sface)
- [InsightFace: Modelle und Nutzungsbedingungen](https://github.com/deepinsight/insightface/tree/master/python-package)
- [AdaFace: offizielle Eingabevorverarbeitung](https://github.com/mk-minchul/AdaFace/blob/master/inference.py)
- [ONNX Runtime: CPU-Threads](https://onnxruntime.ai/docs/performance/tune-performance/threading.html)
