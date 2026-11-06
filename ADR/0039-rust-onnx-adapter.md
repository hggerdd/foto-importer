# ADR-0039: Rust-Adapter für lokale ONNX-Inferenz

- **Status:** Akzeptiert für die experimentelle Rust-Portierung
- **Erfasst am / Entscheidungsdatum:** 2026-11-04
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0014](0014-lokale-austauschbare-gesichtsmodelle.md), [0015](0015-fortsetzbare-gesichtspipeline.md), [0035](0035-rust-desktop.md)
- **Löst ab:** OpenCV-Laufzeitwahl aus 0014 ausschließlich im Rust-Port; lokale Verarbeitung, Hashprüfung und getrennte Modellräume bleiben verbindlich.

## Kontext

Eine Python-/OpenCV-Brücke erfüllt den gewünschten Rust-Betrieb nicht. ONNX
Runtime kann dieselben lokalen Gewichte ohne Python ausführen. Neue Decoder-
und Alignmentimplementierungen sind damit jedoch nicht automatisch gleichwertig.

## Entscheidung

ORT mit dynamisch geladener lokaler ONNX-Runtime-DLL, explizite CPU-Sessions,
zwei Inferenzthreads ohne Spinning. Manifeste bleiben mit Lizenz und SHA-256
maßgeblich. YuNet-Ausgaben und SCRFD-Distanzköpfe werden in Rust decodiert;
SFace und ArcFace erhalten ausgerichtete Fünf-Punkt-Ausschnitte.
Modellschlüssel tragen zusätzlich `:rust-ort-v1`. Bestehende Python-Vektoren
werden weder umgedeutet noch in neue Modellräume kopiert. Bei neuer Analyse
werden Nutzerlabels nur innerhalb derselben Bildquelle über IoU erhalten;
nicht wiedergefundene bestätigte Gesichter bleiben historisch erhalten.

## Alternativen

Rust-OpenCV benötigt zusätzliche native Bibliotheken und eine C++-Toolchain.
Python als Unterprozess würde die Laufzeitmigration nicht erfüllen. Gleiche
Schlüssel ohne Konsistenznachweis könnten falsche Referenzvergleiche ermöglichen.

## Folgen und Grenzen

Vorhandene Personenentscheidungen sind lesbar. Neue Referenzen im Rust-Raum
benötigen passende neue Embeddings. Der Runtime-Pfad ist nach Initialisierung
prozessweit gebunden; eine Änderung benötigt einen Neustart. Modellgewichte
und DLLs werden lokal bereitgestellt und nicht ins Git aufgenommen.
Seit [ADR-0044](0044-onnx-runtime-download.md) kann der Nutzer die Runtime
per Klick beziehen. Der Startvorgang und die Inferenz bleiben lokal.
Der Adapter ist ohne repräsentative private Bildtests keine Zusage gleicher
Detektionsqualität. Der Paritätsbericht nennt offene Algorithmen und Abläufe.

## Bei Weiterentwicklung beachten

Konsistenz separat gegen OpenCV messen; Hash, Kanalreihenfolge, Ausrichtung,
Dimension und Schlüsseländerungen regressionsprüfen. Keine automatischen Downloads.

## Belege und Validierung

[Adapter](../src/core/inference.rs), [Pipeline](../src/core/faces.rs),
[Tests](../tests/faces.rs). Decoder-/SFace-Spezifikation:
[OpenCV-Detektor](https://github.com/opencv/opencv/blob/4.x/modules/objdetect/src/face_detect.cpp),
[OpenCV-Recognizer](https://github.com/opencv/opencv/blob/4.x/modules/objdetect/src/face_recognize.cpp).
