# Funktionsmatrix und Migrationsgrenzen

Verhaltensreferenz: Python-Commit 32fbe476. Kein Anspruch vollständiger Parität
aus Kompilierung oder gemeinsamen Datenbanktabellen. Der Rust-Port ist experimentell.

| Ablauf | Rust-Stand | Offene Gleichwertigkeit |
|---|---|---|
| Rekursiver Quellscan, Datum, Tagesauswahl | Implementiert | IPTC-Datum und genaue Scanfilter noch abzugleichen |
| Import nach Endungsordnern | Implementiert, getestet | Windows-Erstellzeit/ACL nicht zugesichert |
| Kollision, Abbruch, Teilfehler | Exklusive Veröffentlichung, Fehlerbericht | Prozessabsturz kann temporäre Reste hinterlassen |
| NEF zusätzlich als DNG | Externer Prozess, Staging, Abbruch | Adobe-Echttest und automatische Versionsprüfung offen |
| Sammlung Schema 6 / Lock / Backup | Implementiert, getestet | Historische Migrationen 1–5 werden abgewiesen |
| Schnellscan und stabile IDs | Implementiert, getestet | Umfangreiche Identitäts-/Move-Sonderfälle noch abzugleichen |
| EXIFTool-Anreicherung / starke Identität | Implementiert | RAW-/MakerNotes-Echttest offen |
| BLAKE3 mit unverändertem Vergleichswert | Implementiert, getestet | Lastmessung großer Sammlungen offen |
| Parametrisierte kombinierte Suche | Implementiert, Semantiktests | Filter-/Leistungsparität noch nicht vollständig belegt |
| Bilddetails, RAW-Vorschau, Orientierung | Implementiert | Historischer Ordnerbaum, Varianten-/Overlaydetails offen |
| Lokale Modelle / SHA-256 / CPU | Rust-ORT-Adapter | Eigener Modellraum; kein belegter Ersatz gleicher Präzision |
| Fortsetzbare Fotoanalyse | Jobs pro Quelle/Modell | Modellwechsel-/Alignment-Konsistenz offen |
| Gruppen ≤14 / Benennen / Ignorieren / Undo | Implementiert, getestet | Fokus-/Tab-/14-Bilder-Sichtprüfung offen |
| Top-3-Referenzvorschläge | Kern vorhanden | Einzelgesichts-Buttons noch anzuschließen |
| Referenzauswahl / Fotoabgleich | Experimenteller Kern | Jahresbalance, Pins, gemeinsame Optimierung, Alter/Ko-Okkurrenz noch abzugleichen |
| Personen / Familienbeziehungen | IDs und Zyklusprüfung | Bearbeiten, Merge, Ableitung, Export noch nicht vollständig portiert |
| Biometrie-Export / Kalibrierung / komplette Löschung | Offen | Kein ersetzender Funktionsumfang |
| Automatische Importanalyse / Nachtlauf | Offen | Keine Behauptung eines externen Schedulers |
| Interaktive Priorität / FIFO-Lanes | UI bleibt getrennt, ein UI-Schreiber | Vollständiger ADR-0004-Scheduler noch offen |

Die offenen Punkte sind Aufgaben der laufenden Portierung, keine stillschweigend
entfernten Produktanforderungen. Nur tatsächlich ausgeführte Tests werden im
[Prüfprotokoll](validierung.md) als bestanden angegeben.
