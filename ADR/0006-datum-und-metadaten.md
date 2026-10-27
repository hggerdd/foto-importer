# ADR-0006: Datumsquelle wählbar, Sammlungsmetadaten über ExifTool

- **Status:** Akzeptiert
- **Erfasst am:** 2026-10-27
- **Art:** Rückblickende Bestandsentscheidung; ursprüngliches Datum nicht rekonstruiert

## Kontext

Dateisystemzeiten und tatsächliche Aufnahmezeiten können abweichen. RAW-Paarung
benötigt zudem Kamera-MakerNotes, die nicht jeder Bilddecoder liefert.

## Entscheidung

Der Quellscan bietet Dateisystemdatum oder bevorzugtes EXIF-/IPTC-Datum mit
Dateisystem-Fallback. Für unterstützte normale Bilder liest `FileManager` mit
Pillow; unter Windows nutzt der Dateisystemmodus `st_ctime`, sonst `st_mtime`.
Die Sammlungsanreicherung liest EXIF/MakerNotes dagegen mit externem ExifTool
in Batches und bewahrt zusätzlich dessen JSON. ExifTool wird ausschließlich lesend genutzt.

## Alternativen

Nur Dateisystemzeiten würden Aufnahmeinformationen verlieren. RAW-Metadaten nur
über Pillow auszulesen deckt die vorhandenen Anforderungen nicht ab. Die beiden
Lesepfade zu vereinheitlichen wäre eine neue Abhängigkeits-/Fallbackentscheidung.

## Folgen und Grenzen

Ein Quellscan kann ohne ExifTool funktionieren; Sammlungsmetadaten, identitätsbasierte
Importprüfung und eingebettete RAW-Vorschauen benötigen das separat installierte Werkzeug.
Die Datumsoption des Quellscans ist keine Zusage vollständiger RAW-Metadatenunterstützung.

## Bei Weiterentwicklung beachten

Zeitstempel nicht stillschweigend umdeuten. Plattform-Fallbacks, fehlende Metadaten
und MakerNotes-Verfügbarkeit erhalten; Fotometadaten nicht als Nebeneffekt schreiben.

## Belege und Validierung

[FileManager](../core/file_manager.py), [ExifTool-Leser](../core/collection_metadata.py),
[Datumstests](../tests/core/test_file_manager.py), [Index-Tests](../tests/core/test_collection_index.py).
