# Release und Downloads

## Entscheidung

- **Portables Paket:** `scripts/build-release.ps1` baut die Windows-EXE im Release-Modus und
  erzeugt ein neues, datiertes ZIP mit EXE, Modellmanifesten, Dokumentation und ADRs. Alte
  Pakete werden nicht überschrieben. Kein Installer, keine Signierung.
- **Nicht gebündelt:** ExifTool, Adobe DNG Converter, ONNX Runtime und Modellgewichte.
- **Ressourcen** werden zuerst neben der EXE gesucht, beim Entwickeln im Projektordner.
  Eingestellte absolute Pfade haben Vorrang. Einstellungen bleiben im Benutzerprofil.
- **ONNX Runtime auf Klick:** festgelegtes CPU-Paket 1.22.0 für Windows x64 von Microsofts
  GitHub-Release, mit geprüfter Größe und SHA-256, entpackt in einen neuen Ordner unter den
  Arbeitsplatzeinstellungen. Der DLL-Pfad wird erst nach vollständigem Entpacken übernommen.
  Ohne Administratorrechte; die manuelle Auswahl bleibt.
- **Standardmodelle auf Klick:** YuNet und SFace; vorhandene passende Gewichte werden
  verwendet, fehlende von OpenCV Zoo geladen und per SHA-256 geprüft. Gewichte und Manifeste
  kommen gemeinsam in einen neuen Ordner; er wird erst nach erfolgreicher Prüfung übernommen.
- **Netz nur nach Klick,** mit einer Ausnahme: die tägliche Adobe-Versionsprüfung
  (siehe [Import](import.md)). Beim Start wird nichts geladen.

## Grenzen

Nur Windows x64. Nach dem Wechsel einer bereits geladenen Runtime ist ein Neustart nötig.
Alte Runtime- und Modellordner werden nicht aufgeräumt.

## Beim Ändern beachten

Paketversion, Größe und Prüfsumme gemeinsam aktualisieren und mit dem Rust-Binding prüfen.
Lizenzhinweise und Prüfsummen erhalten. Downloads und Entpacken bleiben auf Workern und
abbrechbar. Offline-Regressionen und echte Downloads getrennt testen.
