# Architekturentscheidungen

Elf Themen-ADRs beschreiben den **geltenden Stand** der Rust-Anwendung: was entschieden
ist, wo die Grenzen liegen und was bei Änderungen erhalten bleiben muss. Die Historie
(70 nummerierte ADRs bis 2026-11-15, einschließlich der Python-Referenz) liegt in Git:
`git show c448382:ADR/README.md`.

| Thema | Inhalt |
|---|---|
| [Arbeitsweise](arbeitsweise.md) | ADR-Pflege, Prüfungen, Agentenregeln |
| [Anwendung und Nebenläufigkeit](anwendung.md) | Rust/egui, UI und Core, Worker, Vorrang, Einstellungen, Portionen, Navigation, Diagnose-Log |
| [Import](import.md) | Kopieren ohne Ersetzen, Datumsquelle, DNG, Adobe-Versionsprüfung |
| [Sammlung und Daten](sammlung-daten.md) | SQLite, Sperre, Migration, Foto-Identität, Indizierung, Indizes |
| [Sammlungsansicht](sammlungsansicht.md) | Suche, Galerie, Bilddetails, Ordnerbaum, Vorschau-Cache |
| [Gesichtserkennung](gesichtserkennung.md) | Modelle, Analyse, Referenzen, Abgleich, Gruppenbildung |
| [Gesichter benennen und prüfen](gesichter-benennen.md) | Reiter, Statuszeile, Gruppenkarten, Ablehnen, Ignorieren, Rücknahme |
| [Personen und Beziehungen](personen.md) | Personen-IDs, Beziehungskatalog, Verwandtschaft, Tabelle, Stammbaum, Graph |
| [Zentraler Personenbestand](zentral.md) | Zentraldatei, Abgleich von Personen und Beziehungen |
| [Zeitreise](zeitreise.md) | Gesichter der gewählten Person über die Zeit, Export |
| [Release und Downloads](release.md) | Portables Paket, ONNX Runtime, Modellgewichte |

## Regeln

1. Vor einer Änderung den betroffenen Themen-ADR lesen. Mehr ist nicht nötig.
2. Ändert sich eine Entscheidung, wird der Themen-ADR **in derselben Änderung
   umgeschrieben**. Er beschreibt immer den Stand des Codes. Begründung und alter Stand
   stehen im Commit, nicht in der Datei.
3. Kein neuer ADR für Bedien- und Layoutdetails, Leistungsverbesserungen ohne neue
   Regel oder einzelne Funktionen. Ein neues Thema bekommt nur dann eine Datei, wenn es
   in keines der vorhandenen passt.
4. Jeder Themen-ADR bleibt kurz: Entscheidung, Grenzen, „Beim Ändern beachten“.
   Messwerte nur, wenn sie eine Schwelle begründen, mit Datum und Datengrundlage.
5. Beschlossenes, das noch nicht umgesetzt ist, steht in einem eigenen Abschnitt
   „Beschlossen, noch nicht umgesetzt“ und wandert beim Umsetzen nach oben.

## Frühere Nummern

Kommentare im Code nennen noch alte Nummern. Zuordnung:

| Nummern | Themen-ADR |
|---|---|
| 0001, 0020, 0070 | [Arbeitsweise](arbeitsweise.md) |
| 0002, 0003, 0004, 0013, 0030, 0035, 0038, 0059 | [Anwendung](anwendung.md) |
| 0005, 0006, 0007, 0036, 0041 | [Import](import.md) |
| 0008, 0009, 0010, 0022, 0029, 0037, 0042, 0064 | [Sammlung und Daten](sammlung-daten.md) |
| 0011, 0012, 0028, 0032, 0033, 0045, 0047, 0052, 0057, 0067 | [Sammlungsansicht](sammlungsansicht.md) |
| 0014, 0015, 0016, 0017, 0023, 0024, 0027, 0039, 0040, 0050, 0051, 0053, 0054 | [Gesichtserkennung](gesichtserkennung.md) |
| 0018, 0021, 0025, 0026, 0031, 0034, 0048, 0055, 0056, 0066, 0068 | [Gesichter benennen](gesichter-benennen.md) |
| 0019, 0049, 0058, 0060, 0061, 0063 | [Personen](personen.md) |
| 0062, 0065 | [Zentraler Bestand](zentral.md) |
| 0069 | [Zeitreise](zeitreise.md) |
| 0043, 0044, 0046 | [Release](release.md) |
