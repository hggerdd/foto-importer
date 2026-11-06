# Lokale Gesichtsmodelle

Import, Sammlung und Metadatensuche funktionieren ohne Gesichtsmodelle.
Für die Gesichtsanalyse unter **Personen** oder **Einstellungen** auf
**ONNX Runtime herunterladen** klicken. Die App lädt das offizielle
Windows-x64-Paket von Microsoft (rund 72 MB) und wählt die DLL automatisch.
Installierte Dateien liegen unter `%APPDATA%\FotoImporter\runtime`.
Unter **Runtime und Modellordner manuell auswählen** lassen sich eine vorhandene
Windows-DLL `onnxruntime.dll` und der Ordner mit den JSON-Manifesten auswählen.
Weitere DLLs aus derselben Distribution gehören neben die Haupt-DLL.

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
