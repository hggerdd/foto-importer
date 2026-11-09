# ADR-0066: Schnelles Speichern von Gesichtsgruppen auch bei 90.000+ Fotos

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-11-10
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0004](0004-vorrang-interaktiver-arbeit.md), [0008](0008-lokale-sqlite-sammlung.md), [0022](0022-indizierte-sammlungsprojektionen.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0042](0042-native-bestandsmigrationen.md), [0064](0064-gestufte-indizierung-mit-fortschritt.md)

## Kontext

Auf großen Sammlungen dauerte „Bestätigen" in „Benennen" und „Vorschläge prüfen" lange, bis
die nächste Gruppe sichtbar war. Gemessen wurde mit einer synthetischen Sammlung
(`examples/face_save_bench.rs`; 90.000 Fotos, 200.000 Gesichter, 40 Personen, 15.000
bestätigt, 20.000 automatisch/vorgeschlagen, 12.030 Gruppen, 5.680 Referenzen, trainiertes
Modell; nie private Daten). Die Speicherung selbst (`name_group`) kostet nur etwa 20 bis
50 ms. Die Zeit entsteht danach beim Neuladen der Seite:

1. `recognition::ranked` las für **jedes** Gesicht alle Referenzvektoren neu aus SQLite
   (rund 230 ms Release je Gesicht, 24 Gesichter je Seite, dazu Klassifikator und Porträts je
   Gruppe erneut): etwa 5,4 s je Seite.
2. Reiter „review", „confirmed" und „ignored" zählten und paginierten mit Fensterfunktionen über
   alle Gesichter des Reiters; `status()` las die ganze Gesichtstabelle, `people()` zählte
   korreliert je Person.
3. Der Abgleich nach einer Bestätigung (`maintain` → `rematch`) setzt wegen ADR 0024 bei
   Foto 0 an und benötigte durch Punkt 1 etwa eine Sekunde je Foto; jede Folgespeicherung
   bricht ihn ab (Abbruch dauert 20 bis 170 ms und blockiert nicht).
4. Der Speicherauftrag war als Hintergrundauftrag markiert und wartete dadurch an seinem
   ersten Checkpoint auf laufende Lesevorgänge und 150 ms Ruhezeit (ADR 0004), also auf das
   Neuladen der vorigen Seite.

## Entscheidung

- **Referenzen je Lesestand einmal laden:** `recognition::References` hält die
  gesichtsunabhängigen Referenzvektoren; Ausschlüsse je Gesicht (eigenes Gesicht, Ablehnungen,
  Personen desselben Fotos) werden je Aufruf angewendet. `SuggestionCache` teilt Referenzen,
  Klassifikator und Porträts über alle Gruppen einer Seite. Der Zwischenspeicher lebt nur
  einen Aufruf (einen konsistenten Lesestand) und kann nie veraltete Zuordnungen zeigen. Ein
  prozessweiter Cache wurde bewusst nicht gebaut: ohne Änderungszähler in der Sammlung
  (neue Tabellen oder Trigger sind für Schema 6 nach ADR 0042 nicht zulässig) wäre die
  Invalidierung nicht beweisbar. `rematch` lädt die Referenzen einmal je Lauf und merkt sich
  die Ko-Okkurrenz-Prioren je bekannter Person; jede Nutzeraktion bricht den Lauf ab.
  Ergebnisse sind bitgleich zum bisherigen Verfahren (Test).
- **Seiten ohne Fensterfunktionen:** Teilgruppen entstehen aus den Zählungen je Person
  (`ceil(n/28)` Teile, Sortierung größte zuerst, Person, Teil); je Gruppe wird nur ihre Seite
  per `LIMIT/OFFSET` gelesen. Die Prädikate stehen wörtlich wie in den Teilindizes.
- **Zähler:** `status()` nutzt für jede Zählung einen Teilindex; `people()` zählt in einer
  Gruppierung über `ix_faces_user_counts`; „analysierte Fotos" ohne Join über alle Fotos.
- **Zusatzindizes** (rein additiv, nur abgeleitete Daten): `ix_faces_user_counts`
  (`person_id` bei `assignment='user' AND retained=0`), `ix_faces_ignored`,
  `ix_faces_retained`. Neue Sammlungen erhalten sie aus `schema.sql`; bestehende Schema-6-
  Sammlungen beim Öffnen **mit Schreibrecht** per `CREATE INDEX IF NOT EXISTS`
  (`migrations::ensure_indexes`, wiederholbar, Aufbau auf 200.000 Gesichtern rund 0,3 s, daher
  ohne eigene Fortschrittsanzeige und ohne Sicherung, da nur abgeleitete Daten entstehen).
  Schreibgeschütztes Öffnen verändert nichts. Kein Versionssprung, keine Tabellen, Trigger
  bleiben unverändert. Wirkung: `status()` und `people()` sinken von 296 bzw. 213 ms auf unter
  60 bzw. 3 ms.
- **Lesende Verbindungen** setzen `mmap_size` (512 MB), weil jede Lesung mit leerem Seitencache
  beginnt: das Lesen der Referenzen fiel von 1,4 s (kalt) auf 60 ms. Schreibende Verbindungen
  bleiben unverändert.
- **Nutzerschreibaufträge** (`naming`, `undo`, `mutation`) sind keine Hintergrundaufträge mehr
  und warten nicht auf Lesevorgänge; Wartung und lange Aufträge geben weiter nach (ADR 0004),
  und jeder Nutzerschreibauftrag stoppt die Wartung weiterhin vor dem Start.

## Alternativen

Prozessweiter Referenz-Cache mit Fingerabdruck (Summe über Personen/Gesichter): schneller um
etwa 50 ms je Seite, aber Kollisionen nicht ausgeschlossen. Ein Revisionszähler per Trigger
wäre ein neues Schemaobjekt (ADR 0042). `synchronous=NORMAL` und WAL waren schon aktiv bzw.
bringen beim Speichern (20 ms) nichts. Vorschläge nur beim Anzeigen jeder Gruppe zu berechnen
würde die Anzeige verzögern.

## Folgen und Grenzen

Ergebnisse, Vorschläge und Zähler bleiben identisch (Gleichheitstests gegen die bisherigen
Abfragen). `mmap` kann auf Netzlaufwerken oder bei während des Betriebs entfernten Medien
heikel sein; es betrifft nur lesende Verbindungen. Nicht erreicht: der Reiter „persons" und
„ignored" brauchen weiterhin 130 bis 300 ms; die Wartung (Referenzen 7 s, Abgleich über alle
Fotos) bleibt rechenintensiv und läuft nur zwischen Nutzeraktionen. Zentrale Synchronisierung
(`name_group_synced`) wurde nicht verändert. Echte Modelle und echte Sammlungen wurden nicht
gemessen; die Zahlen gelten für die synthetische Sammlung.

## Bei Weiterentwicklung beachten

Der Zwischenspeicher darf nicht über Lesestände hinweg leben, solange es keinen Änderungszähler
gibt. Neue Zählungen müssen zu den Teilindex-Prädikaten passen (`EXPLAIN QUERY PLAN`). Neue
Zusatzindizes in `schema.sql` und `migrations::ADDITIVE_INDEXES` eintragen (Test prüft Gleichheit).

## Belege und Validierung

[Benchmark](../examples/face_save_bench.rs), [Gleichheits- und Invalidierungstests](../tests/face_save_performance.rs),
[Messreihe](../docs/validierung.md#gesichtsgruppen-speichern-adr-0066), Implementierung in
`src/core/recognition.rs`, `src/core/faces.rs`, `src/core/migrations.rs`, `src/ui/app.rs`.
