# ADR-0064: Gestufte, sichtbare Indizierung mit ehrlichem Fortschritt

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-09
- **Art:** Neue Entscheidung auf Anweisung des Projektverantwortlichen
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0004](0004-vorrang-interaktiver-arbeit.md), [0008](0008-lokale-sqlite-sammlung.md), [0009](0009-stabile-foto-identitaeten.md), [0010](0010-gestufte-bestandspruefung.md) (ergänzt), [0022](0022-indizierte-sammlungsprojektionen.md), [0042](0042-native-bestandsmigrationen.md), [0059](0059-arbeitsportionen-neueste-zuerst.md) (ergänzt)

## Kontext

Beim Indizieren eines Ordners mit mehr als 30.000 Bildern zeigte die Fußleiste nur
„Sammlung indizieren · 34195 gefunden · 356 s“. Ob der Lauf bei 1 % oder kurz vor
dem Ende war, war nicht erkennbar. Der Lauf war außerdem ein einziger Auftrag mit
**einer** Transaktion über alle Dateien: Die Sammlung blieb bis zum Schluss leer,
und ein Abbruch verwarf alles.

Messung (synthetischer Baum, 33.000 Dateien in 300 Ordnern, Windows, Release-Build,
`examples/scan_bench.rs`, Ergebnisse in [validierung.md](../docs/validierung.md)):
Der erste Indexlauf dauerte **726 s**, ein unveränderter zweiter Lauf **351 s**. Die
Kostentreiber je neuer Datei waren

1. `File::open` zum Lesen nur für die Datei-ID (Inode): **≈ 9 ms**. Ein
   Echtzeit-Virenscanner prüft jede erstmals zum Lesen geöffnete Datei; `stat`
   kostet dagegen 0,04 ms.
2. Drei nicht indizierbare Abfragen je neuer Datei (Partner gleicher Dateiname über
   `casefold(stem)`, Verschiebungskandidaten über Inode+Größe und Größe+Hash): jeweils
   ein Vollscan der Tabelle `files`, bei 33.000 Zeilen **13 ms + 2,4 ms + 2,1 ms**
   je Datei, wachsend mit der Zeilenzahl (quadratisch über den Lauf).
3. Das Schreiben mit Triggern (`browse_files_insert` baut `metadata_photos` und
   `face_sources` neu auf): ≈ 0,15 ms je Datei, bei 30.000 Dateien ≈ 5 s. Nicht der
   Haupttreiber.

Verzeichnislauf mit `stat` (0,5 s) und die Sicherung (0,1 s) sind vernachlässigbar.

## Entscheidung

**Fortschritt (`core::progress`, `Control::phase/progress/snapshot`).** Jeder Auftrag
meldet benannte Phasen mit Einheit. `Control::progress(done, total)` speist einen
`Meter`, der nach 0,5-s-Abständen eine exponentiell geglättete Rate bildet. Eine
Restzeit erscheint erst nach vier Messpunkten, davor „Restzeit wird geschätzt …“.
`total = 0` heißt „Gesamtzahl unbekannt“: Es wird nur die Zahl „N gefunden“ und
**kein** Prozentwert gezeigt. Prozent wird abgerundet, 100 % erscheint erst bei
vollständiger Arbeit. Text: „N von M Dateien · X % · Y Dateien/s · noch etwa Z Min.“
Fußleiste und Aufgabenansicht (jeweils mit Fortschrittsbalken, Phasenname, Laufzeit)
sowie Metadaten-, Prüfsummen- und Gesichtsanalyse nutzen dieselbe Struktur.

**Indexlauf in vier benannten Schritten:** (1) Ordner lesen (reiner Verzeichnislauf ohne
Datenbank, abbrechbar, Gesamtzahl unbekannt; er liefert danach die **genaue** Gesamtzahl,
eine getrennte Vorab-Zählung würde den Lauf nur verdoppeln), (2) Bestand vergleichen,
(3) Dateien eintragen mit Prozent/Rate/Restzeit, (4) Abschließen (fehlende Dateien
prüfen, Sicherung schreiben).

**Stufe 1 in Stapeln (`config::SCAN_BATCH` = 500 Dateien je Transaktion).** Jeder Stapel
wird committet; die Galerie zeigt Stapel schon während des Laufs (Aktualisierung höchstens
alle vier Sekunden und nur bei neuen Stapeln, Leser blockieren den Schreiber nicht,
[0008](0008-lokale-sqlite-sammlung.md); die Revisionsprüfung der Abfragen bleibt). Die
Stapelgröße ist durch Messung begründet (siehe Validierung): Commits kosten wenig, der
Schutz vor Verlust und die Sichtbarkeit wachsen mit kleineren Stapeln.

**Invarianten bei Abbruch und Unterbrechung.** Nur der vollständig durchgelaufene
letzte Schritt markiert Dateien als fehlend (Regeln von [0010](0010-gestufte-bestandspruefung.md)
unverändert: Lesefehler, mehr als 20 % ohne Bestätigung). Ein abgebrochener oder
fehlgeschlagener Lauf behält die bereits eingetragenen Stapel, markiert **nichts** als
fehlend, schließt den Scan-Eintrag mit `cancelled`/`failed` und gleicht den Fotostatus ab.
Der nächste Lauf findet eingetragene Dateien über den Pfad: Unveränderte Zeilen erhalten
nur die Besuchsmarke (`last_seen_scan`, löst die Projektions-Trigger nicht aus), IDs und
Foto-Zuordnungen bleiben. Ein Lauf ohne Abbruch liefert dasselbe Inventar wie vorher
(vergleichend geprüft).

**Verschiebungs- und Partnerabfragen im Speicher (`core::known_files`).** Die
Verschiebungskandidaten (Zeilen, deren Pfad der Lauf nicht besucht hat) und die
Partnerzuordnung (Stamm → Foto je Familienordner) werden einmal je Lauf geladen und mit
den Änderungen des Laufs fortgeschrieben. Ergebnis und Reihenfolge entsprechen den
früheren Einzelabfragen; es braucht keine neuen Indizes (**keine Schemaänderung**,
[0042](0042-native-bestandsmigrationen.md)).

**Dateiattribute ohne Leserecht.** Die Datei-ID wird mit Attributzugriff
(`FILE_READ_ATTRIBUTES`) gelesen. Das ändert keine Datei und umgeht das Prüfen jeder
Datei durch den Virenscanner.

**Weitere Stufen als eigene Hintergrundläufe.** Metadaten (ExifTool, Foto-Identität und
Zusammenführung nach [0009](0009-stabile-foto-identitaeten.md)) und BLAKE3-Prüfung sind
schon eigene, abbrechbare, fortsetzbare Läufe in Portionen
([0059](0059-arbeitsportionen-neueste-zuerst.md)). Ist „Bei automatischen Läufen zuerst
Metadaten lesen“ aktiv, folgt der Metadatenlauf automatisch auf einen Indexlauf
(niedrige Priorität, weicht interaktiver Arbeit, [0004](0004-vorrang-interaktiver-arbeit.md)).
Fußleiste (im Leerlauf) und Aufgabenansicht nennen, was noch aussteht („Metadaten:
1.200 von 34.195 gelesen“). Neue BLAKE3-Werte werden je 100 Dateien statt je Datei
gespeichert; vor einem Abbruch Berechnetes bleibt erhalten.

## Alternativen

*Getrennte Vorab-Zählung:* nach Messung unnötig, der Verzeichnislauf (0,5 s für 33.000
Dateien) liefert die genaue Zahl; auf langsamen Netzlaufwerken kostet eine zweite
Zählung nur Zeit. *Streaming* (Datenbank schreiben, während noch gelaufen wird): sofortige
Sichtbarkeit, aber unbekannte Gesamtzahl und Konflikte mit der festen Sortierung, die die
ID-Vergabe bestimmt. *Neue Indizes* (`stem`, `inode`, `size`): lösen die Vollscans, wären
aber eine Schemaänderung bestehender Schema-6-Sammlungen, die [0042](0042-native-bestandsmigrationen.md)
ausschließt. *Trigger entfernen und Projektionen am Laufende aufbauen:* spart ≈ 5 s je
30.000 Dateien, setzt aber eine Schemaänderung voraus und macht die Galerie während des
Laufs leer; deshalb nur Empfehlung.

## Folgen und Grenzen

Der Indexlauf ist um den Faktor ≈ 50 schneller (726 s auf 14 s; unveränderte Neuindizierung
351 s auf 8 s). Teilweise Läufe hinterlassen einen benutzbaren Bestand. Einschränkungen:
Die 9 ms je Datei hängen vom Virenscanner ab und wurden nur auf dem Entwicklungsrechner
gemessen. Unveränderte Zeilen werden nicht mehr durch die Trigger neu projiziert; eine
zuvor beschädigte Projektion heilt ein Neuindizieren daher nicht mehr nebenbei. Die
Restzeit der BLAKE3-Prüfung zählt Dateien, nicht Bytes, und schwankt bei sehr
unterschiedlichen Dateigrößen. Nach einem Absturz bleibt ein Scan-Eintrag auf `running`
und der Fotostatus wird erst vom nächsten Lauf abgeglichen. Der Metadatenfortschritt zählt
die Portion (Standard 500), nicht die ganze Sammlung.

## Bei Weiterentwicklung beachten

Fehlend nur nach vollständigem Lauf markieren. Neue lange Läufe melden Phasen über
`Control::phase` und `progress`. Trigger auf `files` feuern bei jedem `UPDATE` einer
gelisteten Spalte, auch ohne Wertänderung; unveränderte Zeilen nur über nicht gelistete
Spalten anfassen. Bei einer späteren Schemamigration: Indizes auf `files(inode,size)` und
`files(stem)` sowie Projektionsaufbau am Laufende neu bewerten.

## Belege und Validierung

[Fortschritt](../src/core/progress.rs), [Control](../src/core/jobs.rs),
[Indexlauf](../src/core/collection.rs), [Bestand im Speicher](../src/core/known_files.rs),
[Tests Fortschritt](../tests/progress.rs), [Tests Stapel/Abbruch](../tests/scan_batches.rs),
[Benchmark](../examples/scan_bench.rs). Messungen und Sichtprüfung:
[validierung.md](../docs/validierung.md). Echte Sammlungen wurden nicht verwendet.
