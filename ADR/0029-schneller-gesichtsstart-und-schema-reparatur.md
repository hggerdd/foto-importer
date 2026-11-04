# ADR-0029: Status zuerst lesen und unvollständige Skalierungsstände reparieren

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-29
- **Art:** Neue Entscheidung nach Untersuchung einer betroffenen Sammlung
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0008](0008-lokale-sqlite-sammlung.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md), [0027](0027-sichtbarer-erkennungsstand.md)

## Kontext

Eine Sammlung mit 1329 Gesichtern meldete Schema v5, hatte aber unter anderem
keine `face_group_dirty`- und `face_sources`-Tabelle sowie fehlende Referenz-/
Queue-Indizes und Trigger. Frühere Zwischenstände wurden durch die Versionsnummer
allein nicht vollständig erfasst. Frisch erstellte Testdatenbanken deckten das
Upgrade nicht ab. Der bisherige Seitenstart bereitete Gruppen vor dem Status vor;
ein Fehler ließ dadurch Statuskarten im Ladezustand. Ein bereits laufender Scan
stellte zudem jeden lesenden Ansichtsauftrag hinter seinen vollständigen Abschluss.

## Entscheidung

Schema v6 ergänzt die konkret fehlenden abgeleiteten Objekte und installiert die
anwendungseigenen Projektions-/Gruppentrigger aus gemeinsamen Definitionen erneut.
Das geschieht in einer Transaktion beim normalen Schreiber-Upgrade. Fehlende
Gesichtsquellen werden aus vorhandenen Dateien rekonstruiert. Quelltabellen,
Labels, Embeddings, Referenzauswahl, Ausschlüsse, Ablehnungen und vorhandene
Matching-Queues bleiben erhalten. Bereits vollständige v5-Schemata sind ebenfalls
upgradefähig. Leser dürfen v5 weiterhin öffnen; sie führen keine Migration aus.

Der UI-Snapshot ist lesend und baut keine Gruppen auf. Status und bereits gültige
Gruppen werden innerhalb eines Lesesnapshots geliefert. Eine unpassende oder noch
nicht reparierte Gruppenkonfiguration liefert stattdessen den Vorbereitungsstatus.
Ein separater, auf einen Worker begrenzter Leser darf den Status auch bei laufender
Analyse lesen; er verändert weder das Busy-Flag des Scans noch dessen Auftrag.
Revisionen verwerfen veraltete Antworten. Fehler ersetzen die Ladebeschriftung.

Die erste Gruppenvorbereitung hat Vorrang vor einem noch offenen globalen Abgleich.
Weitere Arbeit bleibt begrenzt und fortsetzbar nach ADR 0024. Ein konfigurations-
bedingter Neuaufbau ersetzt nur die Queue und Gruppen dieses Embedders und reiht
nur aktuell geeignete Gesichter ein. Benennung/Undo erledigen weiterhin einen
begrenzten Gruppenschritt vor ihrer Ergebnisanzeige. Nach erledigter Arbeit wird
kein überflüssiger Hintergrundschritt mit anschließendem Neurendern gestartet;
neue interaktive Aufträge lösen einen noch wartenden Wartungstimer ab.

## Alternativen

Nur eine neue Ladebeschriftung würde die fehlenden Objekte nicht reparieren.
Ein Neubau der Datenbank gefährdet Nutzerarbeit und ist unnötig. Das nachträgliche
Ändern eines alten Migrationskörpers ohne neue Versionsnummer erreicht bestehende
Datenbanken nicht zuverlässig. Den Status hinter einer vollständigen Analyse zu
serialisieren widerspricht einem schnell lesbaren Arbeitsstand.

## Folgen und Grenzen

Ein zusätzlicher Leser kann parallel zum Schreiber laufen; alle Tk-Operationen
bleiben im UI-Thread. Während Hintergrundarbeit ist der gelesene Stand ein
konsistenter Zwischenstand. Ohne verfügbaren Schreiber bleiben Reparatur und neue
Gruppen ausstehend; vorhandene Statusdaten sind trotzdem lesbar. Einmaliger
Indexaufbau kann bei großen Beständen Zeit beanspruchen, blockiert aber nicht den
ersten Status. Die beobachtete Teilstruktur wird repariert; beliebige Beschädigung
oder fehlende Quelltabellen sind keine stillschweigend unterstützte Wiederherstellung.

## Bei Weiterentwicklung beachten

Veröffentlichte Schemaänderungen benötigen eine neue Version. Tests müssen neben
Neuinstallationen die vorhandenen Zwischenstände abbilden. Status-Latenz getrennt
von Gruppen-/Abgleichdauer testen. Keine Nutzer-Datenbank für Testmigrationen öffnen;
Diagnose und Validierung erfolgen lesend bzw. auf einer bewahrten lokalen Kopie.

## Belege und Validierung

[Versioniertes Upgrade](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_schema_repair.py),
[gemeinsame Triggerdefinitionen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/scaling_schema.py),
[lesende Gruppenabfrage](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_groups.py),
[Controller](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_controller.py),
[1329-Gesichter-Upgrade und Quellprüfsummen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_collection_scaling.py),
[blockierter Gruppenaufbau/laufender Scan](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_face_view.py).

Am 29.10.2026 auf einer lokalen Kopie der betroffenen Sammlung gemessen:
Status-Snapshot 0,0137 s, Upgrade 0,0658 s, erster Gruppenblock 0,0084 s,
anschließender Seiten-Snapshot 0,0063 s mit acht Gruppen. Das sind Kern-/
Datenbankzeiten, keine garantierte komplette GUI- oder Bilddekodierdauer.
Quellinhalte wurden vor/nach Upgrade per Prüfsumme verglichen; die Original-
Sammlung wurde ausschließlich lesend geöffnet. Die lokale Reproduktion bleibt
außerhalb Git unter `tmp/face-startup-repro-20261029` erhalten.

Der bereits wartende Abgleich der lokalen Kopie schloss anschließend in zehn
Schritten nach 0,2333 s ab, ausschließlich mit vorhandenen Vektoren und ohne
Modellinferenz. [Messwerte](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/evidence/face-startup-repair.json).

Zusätzlich wurde am selben Tag eine lokale verwaiste Schreibsperre festgestellt:
Windows meldete für den eingetragenen Prozess ERROR_INVALID_PARAMETER (87).
Die unveränderte Sperrdatei wurde in der lokalen Reproduktion gesichert und erst
nach erneuter Host-/Prozess-/Dateiidentitätsprüfung entfernt. Dabei wurde weder
ein Prozess beendet noch die Originaldatenbank geschrieben oder migriert.
[UI-Sichtprüfung mit synthetischen Fotos](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/screenshots/face-startup.png).

