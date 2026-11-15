# Zentraler Personenbestand

## Entscheidung

- **Datei:** `central.sqlite` neben `settings.json` (Ort einstellbar, abschaltbar über
  `central_enabled`). Schema in `PRAGMA user_version` (derzeit 2); eine neuere Version wird
  abgelehnt und nicht verändert. Schreibzugriffe sind kurze `BEGIN IMMEDIATE`-Transaktionen,
  Verbindungen leben je Vorgang. Auf UNC-Pfaden Rollback-Journal statt WAL.
- **Identität:** zentral `people(gid, name, gender, birth_year, note, merged_into, …)`. In der
  Sammlung steht die Verknüpfung als Personenattribut `global_id`; lokale IDs bleiben.
- **Abgleich von Personen** (`central::sync`) ist nie destruktiv. Leere Felder werden in beide
  Richtungen ergänzt. Eine gleichnamige unverknüpfte Person wird verknüpft, wenn kein Feld
  widerspricht; sonst entsteht eine Rückfrage mit Wahl je Feld oder „Andere Person“. Es gilt
  nie „zuletzt geändert gewinnt“. Mehrdeutige Namenstreffer werden nicht verknüpft. Zentrale
  Personen, die lokal fehlen, werden als Liste angeboten und nur auf Klick übernommen.
- **Vor jedem Anlegen einer Person** (Gruppe benennen, Einzelgesicht benennen, Person
  hinzufügen): abgleichen, bei Namenstreffer übernehmen statt neu anlegen, lokal anlegen,
  erneut abgleichen. Ist die Zentraldatei nicht erreichbar, wird lokal angelegt und gewarnt;
  der nächste Abgleich holt nach.
- **Zusammenführen** vermerkt zentral einen Alias; nichts wird gelöscht.
- **Referenzbilder** nur mit Opt-in (`central_images`), höchstens 64 Ausschnitte zu 128 KiB je
  Person, mit Modellschlüssel. Vektoren werden nicht zentral gespeichert. „Biometrie entfernen“
  kann die zentralen Ausschnitte mitlöschen; Personen bleiben, weil sie keine Biometrie sind.
- **Beziehungen** (`central_relations`, Standard an): Tabellen `rels`, `rel_tombstones`,
  `rel_decisions`, kanonisch wie lokal, aber über `gid`.
  - Zentral nach lokal: fehlende Beziehungen werden angelegt, wenn die lokale Prüfung sie
    annimmt; sonst entsteht ein gemeldeter Konflikt.
  - Lokal nach zentral: fehlende werden ergänzt, außer es gibt einen Grabstein oder einen
    Widerspruch.
  - **Löschen** schreibt einen Grabstein. Eine andere Sammlung löscht nie automatisch,
    sondern fragt: lokal behalten und zentral wiederherstellen, oder lokal entfernen.
  - Abgelehnte Rückfragen werden als Mengenvereinigung in beide Richtungen übernommen.
  - Konflikte löst nur der Nutzer: lokal behalten, zentral übernehmen oder keine von beiden.
- **Schema-Upgrade 1 auf 2** geschieht beim Öffnen mit Schreibrecht, additiv, nach einer
  Sicherung `central.sqlite.v1.bak`.

## Grenzen

Kein Basisstand für Drei-Wege-Vergleiche: Ein lokal geänderter Name führt zu einer Rückfrage.
„Rückgängig“ nach dem Anlegen entfernt nur die lokale Person. Der Geburtsmonat der Zeitreise
wird nicht zentral geführt. Mehrere Instanzen über ein Netzlaufwerk sind nicht geprüft; die
Konfliktliste ist nicht begrenzt.

## Beim Ändern beachten

Nie Nutzerfelder ohne Wahl überschreiben, nie still löschen, nie Originalfotos kopieren.
Beziehungen nie mit Gesichtsdaten koppeln. Jede lokale Neuanlage läuft durch die Prüfung aus
`relations`. Schemaänderungen der Zentraldatei nur additiv mit Versionssprung und Sicherung.
Zentrale Sperren kurz halten, die Oberfläche nie blockieren.
