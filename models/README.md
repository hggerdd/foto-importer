# Lokale Gesichtsmodelle

Import, Sammlung und Metadatensuche funktionieren ohne Gesichtsmodelle.
Für die Gesichtsanalyse unter **Einstellungen** den Ordner mit den JSON-Manifesten
und die lokale Windows-DLL `onnxruntime.dll` wählen. Weitere DLLs aus derselben
ONNX-Runtime-Distribution gehören neben die Haupt-DLL. Im Projektordner lautet
der Standardpfad `runtime/onnxruntime.dll`.

Die Manifeste nennen Bezugsquelle, Datei, Prüfsumme und Lizenz. Das jeweilige
ONNX-Gewicht unter dem im Manifest angegebenen Pfad in `weights/` ablegen.
Wenn ein Manifest ein Archiv nennt, wird daraus nur die angegebene Datei benötigt.
YuNet und SFace bilden die Standardkombination; SCRFD und ArcFace sind optional.
Die InsightFace-Gewichte sind laut Manifest auf nichtkommerzielle Forschung
beschränkt. Die App prüft lokale Dateien und lädt keine Modelle herunter.
Das Release-Paket enthält nur diese Anleitung und die Manifeste.

Rust und Python speichern Vektoren in getrennten Modellräumen. Nach einem Wechsel
zu Rust muss die Sammlung für Rust neu analysiert werden, bevor Gruppierung und
automatische Erkennung Rust-Vektoren nutzen können. Bestätigte Personen bleiben erhalten.
