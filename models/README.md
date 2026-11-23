# Lokale Gesichtsmodelle

Import, Sammlung und Metadatensuche funktionieren ohne Gesichtsmodelle.
Für die Gesichtsanalyse unter **Personen** oder **Einstellungen** auf
**ONNX Runtime herunterladen** klicken. Die App lädt das offizielle
Windows-x64-Paket von Microsoft (rund 72 MB) und wählt die DLL automatisch.
Installierte Dateien liegen unter `%APPDATA%\FotoImporter\runtime`.
Unter **Runtime und Modellordner manuell auswählen** lassen sich eine vorhandene
Windows-DLL `onnxruntime.dll` und der Ordner mit den JSON-Manifesten auswählen.
Weitere DLLs aus derselben Distribution gehören neben die Haupt-DLL.

Unter **Personen** oder **Einstellungen** auf **YuNet und SFace installieren**
klicken. Die App verwendet vorhandene passende Gewichte aus dem konfigurierten
Modellordner, dem Release oder dem lokalen Entwicklungsordner. Fehlende Dateien
lädt sie von OpenCV Zoo (zusammen rund 39 MB). Nach der SHA-256-Prüfung speichert
sie die Gewichte und Manifeste unter `%APPDATA%\FotoImporter\models` und wählt
den neuen Ordner. Der Pfad bleibt bei einem Release-Wechsel gültig.

Das genauere Vergleichsmodell ArcFace (`arcface_buffalo_l`, InsightFace buffalo_l,
`w600k_r50.onnx`) richtet **Gesichter → Automatik & Werkzeuge → Genauer vergleichen →
Genaueres Modell einrichten** ein: Die App übernimmt vorhandene Gewichte oder streamt das
GitHub-Archiv `buffalo_l.zip` (rund 280 MB), speichert nur `w600k_r50.onnx` (174 MB) nach
SHA-256-Prüfung und legt mit YuNet und SFace einen neuen Modellordner an. Die Gewichte sind
nur für nicht kommerzielle Nutzung lizenziert.

SCRFD kann über **Modellordner wählen** manuell eingerichtet
werden. Die Manifeste nennen Bezugsquelle, Datei, Prüfsumme und Lizenz. Das
jeweilige ONNX-Gewicht am angegebenen Pfad in `weights/` ablegen. Bei
Archiven genügt die angegebene Datei. Die InsightFace-Gewichte sind laut
Manifest auf nichtkommerzielle Forschung beschränkt. Das Release-Paket
enthält diese Anleitung und die Manifeste.

Rust und Python speichern Vektoren in getrennten Modellräumen. Nach einem Wechsel
zu Rust muss die Sammlung für Rust neu analysiert werden, bevor Gruppierung und
automatische Erkennung Rust-Vektoren nutzen können. Bestätigte Personen bleiben erhalten.
