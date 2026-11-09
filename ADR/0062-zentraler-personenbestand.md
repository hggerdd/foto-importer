# ADR-0062: Zentraler Personenbestand im Benutzerprofil

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-09
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-09
- **Bezieht sich auf:** [0008](0008-lokale-sqlite-sammlung.md), [0013](0013-konfigurationsbereiche.md), [0019](0019-personen-und-beziehungsgraph.md), [0042](0042-native-bestandsmigrationen.md), [0061](0061-stammbaum-als-normalisiertes-modell.md)

## Kontext

Personen leben bisher nur in einer Sammlung. Wer mehrere Sammlungen pflegt, legt dieselben
Personen mehrfach an. Gewünscht ist ein zentraler Bestand auf dem Rechner, der vor jedem
Anlegen einer Person abgeglichen wird, optional auch mit Referenzbildern. Randbedingungen:
Sammlungsschema 6 bekommt nach ADR 0042 keine still nachgerüsteten Tabellen; lokale
`persons.id` und Nutzerentscheidungen müssen erhalten bleiben; Gesichtsausschnitte sind
biometrisch; langsame Arbeit gehört auf Worker.

## Entscheidung

**Datei und Ort.** Eigene SQLite-Datei `central.sqlite` neben `settings.json`
(`Settings::central_path` ändert den Ort, z. B. Netzlaufwerk; `central_enabled` schaltet ab).
`PRAGMA user_version` trägt das Schema (1); eine neuere Version wird abgelehnt, nicht
verändert. WAL, `busy_timeout` 5 s, Schreibzugriffe nur als kurze `BEGIN IMMEDIATE`-Transaktionen;
Verbindungen werden je Vorgang geöffnet und sofort geschlossen. Auf UNC-Pfaden wird statt WAL
ein Rollback-Journal verwendet, weil WAL über Netzfreigaben nicht zuverlässig ist.

**Identität.** Zentral: `people(gid UUID, name, gender, birth_year, note, created_at,
updated_at, merged_into)`. In der Sammlung steht die Verknüpfung als Personenattribut
`global_id` in `person_attrs`; kein Schema-Eingriff, lokale IDs bleiben. `global_distinct`
merkt „andere Person“-Entscheidungen. Alte Sammlungen ohne `global_id` sind gültig und werden
beim Abgleich verknüpft.

**Abgleich (`core::central::sync`).** Nie destruktiv: leere Felder werden in beide Richtungen
ergänzt (zentral nur bedingt `WHERE feld=''`, sicher bei parallelen Schreibern). Gleichnamige
unverknüpfte Person wird verknüpft, wenn kein Feld widerspricht; sonst entsteht eine Rückfrage
(`Conflict`) mit Wahl je Feld („lokal“/„zentral“) und „Andere Person“. „Zuletzt geändert
gewinnt“ gilt bewusst nicht. Mehrdeutige Namenstreffer werden gezählt und nicht verknüpft.
Unverknüpfte lokale Personen ohne Treffer werden zentral angelegt. Zentrale Personen, die
lokal fehlen, erscheinen als Liste „fehlt in dieser Sammlung“ und werden einzeln oder alle über
`adopt` übernommen. Zuerst die zentrale Transaktion, dann die lokale; scheitert die lokale,
heilt der nächste Abgleich (Namenstreffer verknüpft erneut).

**Vor jedem Anlegen.** `name_group_synced`, `add_person_synced` (alle Stellen, an denen
`persons`-Zeilen entstehen: Gruppe benennen, Einzelgesicht benennen, Person hinzufügen):
(1) Abgleich, bei Namenstreffer im Zentralbestand `adopt` statt Neuanlage, (2) lokales Anlegen
über die unveränderten Funktionen, (3) erneuter Abgleich legt die Person zentral an. Alles
läuft im bestehenden Schreib-Worker. Ist die Zentraldatei gesperrt oder fehlt sie, wird lokal
angelegt und eine Warnung gezeigt (`SyncNote.warning`); der nächste Abgleich holt nach.

**Zusammenführen.** `merged_into` lokal bleibt. `merge_people_synced` vermerkt zentral
`merged_into` als Alias (Datensatz bleibt, Referenzbilder wandern zur Zielperson); andere
Sammlungen folgen dem Alias beim Abgleich. Keine Löschung.

**Referenzbilder (Phase 2, Kernstück umgesetzt).** Tabelle `refs(gid, model_key, jpeg)`;
Schreiben nur mit Opt-in `central_images` (Standard aus), höchstens 128 KiB und 64 Ausschnitte
je Person, Modellschlüssel Pflicht. Embeddings werden nicht gespeichert: sie sind modellabhängig
und ließen sich nur sauber mit Modellschlüssel wiederverwenden; Ausschnitte genügen zum
Neu-Einbetten. „Biometrie entfernen“ bietet eine Option, die zentralen Ausschnitte mit
`secure_delete` und `VACUUM` zu löschen (hinter derselben Bestätigung „BIOMETRIE“). Zentrale
Personen (Name, Jahr) bleiben, weil sie keine Biometrie sind. Die Anbindung, die Ausschnitte
aus einer Sammlung automatisch zentral ablegt und beim Neu-Einbetten nutzt, ist offen.

**Beziehungen (Phase 2, nicht umgesetzt).** Eltern/Partner/Freund zentral zu führen hätte
Konfliktregeln für Graphen (Zyklen, Widersprüche, ADR 0061) und Rückfragen zwischen
Sammlungen zur Folge. Phase 1 bleibt ohne; Beziehungen verbleiben in der Sammlung.

## Alternativen

Zentrale Daten in einer „Hauptsammlung“ koppelt Sammlungen und erzwingt Sperren. Eine
Schemaerweiterung der Sammlung widerspricht ADR 0042. JSON-Datei ohne Transaktionen verliert
Änderungen bei parallelen Instanzen. Automatisches „letzte Änderung gewinnt“ würde Nutzer-
entscheidungen still überschreiben.

## Folgen und Grenzen

- Ein lokal geänderter Name führt nach Verknüpfung zu einer Rückfrage statt zu stiller
  Weitergabe; es gibt keinen Basisstand für Drei-Wege-Vergleiche.
- „Rückgängig“ nach dem Anlegen entfernt die lokale Person; die zentrale bleibt und erscheint
  anderswo als „fehlt“.
- Bei vielen tausend Personen wird je Abgleich die ganze Liste gelesen (Test mit 3000 Personen
  in Sekunden); Namenssuche nutzt einen In-Memory-Index.
- Netzlaufwerke: nur mit Rollback-Journal, parallele Schreiber auf SMB bleiben nur durch
  SQLite-Dateisperren geschützt; gemappte Laufwerksbuchstaben werden nicht als Netz erkannt
  und nutzen WAL (nicht verifiziert).
- Rückbau: Zentralnutzung abschalten verändert keine Sammlung; `global_id` bleibt als
  harmloses Attribut.
- Schreibgeschützte Sammlungen werden nie geschrieben; Übersicht „fehlt“ ist lesend möglich.
- Echte Mehrinstanz-Läufe über Netzlaufwerk sind nicht geprüft.

## Bei Weiterentwicklung beachten

Nie Nutzerfelder ohne Wahl überschreiben; nie Originalfotos kopieren; Referenzbilder nur mit
Opt-in und mit Modellschlüssel; zentrale Schreibsperren kurz halten, UI nie blockieren.
Schemaänderung der Zentraldatei braucht Versionssprung und Migration mit Sicherung.

## Belege und Validierung

[central.rs](../src/core/central.rs), [central_view.rs](../src/ui/app/central_view.rs),
[Tests](../tests/central.rs): zwei Sammlungen, Duplikatfreiheit, Konflikt, Alias,
schreibgeschützt, fehlende/gesperrte Datei, Opt-in, Biometrie-Bereinigung, parallele Schreiber.
