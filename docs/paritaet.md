# Funktionsmatrix und überprüfte Grenzen

Referenz: Python-Commit `32fbe4761b47f637a79505d783c94a2969ebc599`.
Stand: 05.11.2026. „Implementiert“ bezeichnet vorhandenen Code, keine pauschale
Zusicherung identischer Ergebnisse auf jeder privaten Sammlung.

| Ablauf | Rust-Stand und Belege | Prüfgrenze |
|---|---|---|
| Rekursiver Quellscan, EXIF/IPTC-/Dateisystemdatum | `files.rs`, Datumstests | Weitere Kamera-IPTC-Varianten nicht evaluiert |
| Tagesauswahl, vollständige Galerie, große Vorschau | Sichtbereichsraster, 32-MiB-/128-Cache; 100.000-Dateien-Cachetest | Kein kompletter 500.000-Fotos-UI-Benchmark |
| Sicherer Import nach Endungsordnern | Vollständiges Staging, exklusiver freier Zielname, Abbruch/Teilfehler | Keine Stromausfall-/NAS-Dauerhaftigkeitsgarantie; Dateiattribute nicht vollständig wie copystat |
| Vorprüfung und Duplikatüberspringen | Kameraidentität, Name/Größe plus BLAKE3-Sampling; Prozessfixture | ExifTool-/MakerNotes-Echttest mit realen RAWs offen |
| DNG | Separater Adobe-Prozess, Staging, Abbruch, tägliche Versionsprüfung | Echter Adobe-Converterlauf nicht ausgeführt |
| Schema 1–6, Sperren, Sicherungen | Modellierte Altstände und unvollständiges v5 getestet; IDs/Labels/Vektoren/Pins erhalten | Keine beliebige beschädigte Datenbank repariert |
| Inventar, Verschiebungen, fehlende Dateien | Windows-Datei-ID und bekannte vollständige Hashes, explizite Auflösung und Bereinigung | Ungehashte mehrdeutige Datenträgerwechsel bleiben prüfpflichtig |
| Metadatenanreicherung und stabile Identität | ExifTool, starke Kameraidentität, Zusammenführung mit Redirect | Umfangreiche reale MakerNotes-Kombinationen offen |
| Integrität | BLAKE3, Vergleichswerte bei Abweichung unverändert | Gesonderte Lastmessung großer realer Bestände offen |
| Kombinierte Suche | Alle/mindestens eine/keine Personen, nur diese, Datum, Monate, Metadaten-UND/ODER, XMP-Priorität | Breite Semantik vorhanden; kein vollständiger Differentialtest aller Python-Fälle |
| Ergebnisbegrenzung und Ordnernavigation | 48 Fotos, 200 direkte Unterordner pro Seite; abbrechbare SQL-Abfragen | Drilldown ersetzt den gleichzeitig aufgeklappten Tk-Ordnerbaum |
| Bilddetails und Gesichtsaktionen | Orientierte Quelle, passende Rahmen/Namen, Benennen/Löschen, Referenzaktionen | Variantenrahmen nur bei gleicher Quelle; große private Gruppen nicht visuell evaluiert |
| Lokale Inferenz | YuNet/SFace und SCRFD/ArcFace mit SHA-256 und CPU ausgeführt | Synthetische Smoke-Tests, kein Nachweis gleicher realer Präzision wie OpenCV |
| Wiederaufnahme und Modellwechsel | Modell-/Quelljobs, eigene Rust-Räume, IoU-Erhaltung, historische Nutzerlabels | Umfangreiche Hin-/Zurückwechsel mit privaten Gesichtern offen |
| Gruppen und Rücknahme | Complete Link, höchstens 14 sichtbare Mitglieder, Größe vor Pagination, Ablehnungen und Pins | Langzeittest mit echter Tastatur-/Screenreader-Nutzung offen |
| Namensvorschläge und Lernen | Trainierter Softmax-Klassifikator für Top-3-Buttons (ADR 0051, nur synthetisch getestet), Top-3-Scores, sechs Ähnlichkeitsvorschläge als Fallback, Pins/Ausschlüsse, FPS mit Jahresbalance, Maximum-Weight-Fotozuordnung, Alter/Ko-Okkurrenz | Split bewusst anders reproduziert (ADR 0040); keine Qualitätsgarantie |
| Personen und Beziehungen | Stabile IDs, Bearbeiten, Fotofilter, Merge, kanonische Kanten, Zyklusprüfung; Ableitung der Python-Referenz (zwei Schritte) ersetzt durch den allgemeinen Verwandtschaftsrechner (Rust, [ADR 0061](../ADR/0061-stammbaum-als-normalisiertes-modell.md)): nur Eltern-Kind, Partner, frühere Partner, Freund, Bekannter gespeichert; Geschwister, Großeltern bis Ururgroßeltern, Onkel/Tante, Nichten/Neffen, Cousins aller Grade, Stief- und Schwiegerfamilie berechnet; Geschwister/Cousin nur als Behelf; Rückfragen bei fehlenden Eltern; Person ohne Foto anlegen; Stammbaumansicht mit Verwandtschaftsprüfung; Eingabe als Satz aus beiden Richtungen mit Geschlechtsformen | Parität zur Python-Referenz bei Beziehungen nicht geprüft (bewusste Abweichung: mehr Ableitung, Behelfstypen); Stammbaum nicht mit Screenreader oder Nutzern geprüft; Schwippschwager und mehrstufige Verschwägerung nicht benannt |
| Export, Validierung, Biometrielöschung | Atomare Ausschnitte/Manifest, Held-out-Auswertung, Bereinigung bekannter Backups | Auswertungsbericht kompakter als der Python-Bericht; historische fremde Exporte bleiben außerhalb der Kontrolle |
| Automatik | Sammlungsbezogen opt-in nach Import / 03:00 bei laufender App; Metadaten optional vorher | Kein Dienst bei geschlossener App; keine externe Aufgabenplanung |
| Nebenläufigkeit | Ein UI-Schreiber pro kanonischer Sammlung, FIFO-Kopier-/DNG-Lanes, priorisierte Leser/Vorschauen, Revisionen | Laufende einzelne Inferenz-/OS-Aufrufe nicht sofort präemptierbar |

## Bewusste Unterschiede

Die neue Oberfläche folgt dem kritisierten und gewählten Konzept C. Originale,
Foto-/Personen-IDs und Nutzerentscheidungen behalten Vorrang. Rust ersetzt die
Laufzeit; das Python-Projekt bleibt unangetastet. Dateien werden vor Veröffentlichung
vollständig geschrieben (ADR 0036). Neue Vektoren besitzen getrennte Schlüssel
(ADR 0039), der Validierungssplit eine eigene reproduzierbare Reihenfolge (0040).
Historische DBs werden inzwischen nativ migriert; ADR 0042 ersetzt die anfängliche
Schema-6-Grenze aus 0037.

Noch **nicht als vollständig nachgewiesen** gelten: reale Kamera-/Adobe-Parität,
exakte OpenCV-Alignment-/Detektionsgleichheit, sämtliche seltenen Bestandsfälle,
private Präzisionsmessungen und große reale UI-/NAS-Lastläufe. Einzelne technische
Auswertungsfelder, freie Sortier-/Diagrammansichten und die alte Bauminteraktion
sind nicht identisch nachgebildet. Diese Grenzen sind kein stilles Erfolgsversprechen.

Ausgeführte Prüfungen: [Validierung](validierung.md). Entwurf: [Drei UX-Konzepte](ux-konzepte.md).
