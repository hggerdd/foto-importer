# ADR-0065: Beziehungen zentral speichern und abgleichen

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-09
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-09
- **Bezieht sich auf:** [0019](0019-personen-und-beziehungsgraph.md), [0042](0042-native-bestandsmigrationen.md), [0060](0060-beziehungstypen-und-ableitung.md), [0061](0061-stammbaum-als-normalisiertes-modell.md), [0062](0062-zentraler-personenbestand.md), [0063](0063-personentabelle-und-tippfilter.md); setzt „Beziehungen (Phase 2)“ aus 0062 um

## Kontext

Der zentrale Bestand (ADR 0062) führt Personen mit globaler ID (`gid`), aber keine Beziehungen.
Wer mehrere Sammlungen pflegt, trägt dieselbe Familie mehrfach ein; Rückfrage-Entscheidungen
(„kein Elternteil“, „keine Freundschaft“, ADR 0061) gehen zwischen Sammlungen verloren. Gewünscht:
Beziehungen sollen zentral gespeichert werden können. Randbedingungen: Sammlungsschema bleibt
unverändert (ADR 0042); Originale und Nutzerentscheidungen werden nie verändert oder gelöscht;
Beziehungen sind keine Biometrie; die Validierung aus ADR 0060/0061 (Zyklen, Widersprüche) gilt
auch zentral.

## Blickwinkel vor der Umsetzung

- **Datenverlust:** nie überschreiben, nie still löschen. Konflikte werden gemeldet und nur auf
  ausdrückliche Wahl geändert.
- **Identität:** zentral zählen nur `gid`s, nie lokale IDs; Aliase aus zentralen
  Zusammenführungen werden beim Lesen aufgelöst.
- **Konsistenz:** jede lokale Neuanlage läuft durch dieselbe Prüfung wie von Hand
  (`relations::validate_add`, hier über den inkrementellen `Validator`).
- **Mehrsammlungs-Konflikte:** sie sind der Normalfall und werden als Rückfrage sichtbar.
- **Kompatibilität:** Sammlungen unverändert; Zentraldatei von Schema 1 auf 2 nur durch neue Tabellen.
- **Datenschutz:** keine Gesichtsbezüge, keine Bilder, keine Embeddings in den neuen Tabellen.
- **Leistung:** 5.000 Personen und rund 10.000 Beziehungen in unter zwei Sekunden (Debug-Build).
- **Rückbau:** Einstellung `central_relations` aus: der Abgleich fasst keine Beziehung an; lokale
  Daten bleiben vollständig, zentrale Tabellen bleiben unberührt liegen.

## Entscheidung

**Schema 2 (additiv).** `PRAGMA user_version=2`. Neue Tabellen, `people` und `refs` bleiben unverändert:

- `rels(a, kind, b, created)`, Schlüssel `(a,kind,b)`: kanonisch wie `relations::Edge`, aber in
  `gid`s. Eltern: `a` ist Elternteil von `b`; symmetrische Typen: `a < b` (Textvergleich).
  `kind` ist die Katalog-ID aus ADR 0060 (1 Elternteil, 3 Partner, 8 früherer Partner, 6 Freund,
  7 Bekannter, 4 Geschwister, 5 Cousin). Die Behelfstypen Geschwister/Cousin werden mitgeführt,
  weil sie ausdrückliche Nutzerangaben sind (Eltern unbekannt) und sonst verloren gingen; sie laufen
  beim Anlegen durch dieselbe Prüfung wie lokal (Redundantes wird nicht doppelt gespeichert).
- `rel_tombstones(a, kind, b, deleted)`: Grabsteine bewusst gelöschter Beziehungen.
- `rel_decisions(kind, a, b)`: `parent` = (Kind, Elternteil) abgelehnt, `friend` = Paar sortiert
  abgelehnt; Spiegel von `gaps::Decisions`.

**Migration.** Beim Öffnen (`Central::open`) mit Schreibrecht, automatisch und idempotent: Ist die
Version kleiner als 2, wird (nur bei Version 1) best-effort eine Sicherung `central.sqlite.v1.bak`
per `VACUUM INTO` angelegt, dann in einer `IMMEDIATE`-Transaktion `CREATE TABLE IF NOT EXISTS`
und `user_version=2` gesetzt. Scheitert die Transaktion (gesperrt, schreibgeschützt), ändert sich
nichts. Eine neuere Version wird weiterhin abgelehnt und nicht verändert. **Ältere Programmstände**,
die nur Schema 1 kennen, lehnen eine Version-2-Datei mit ihrer Versionsmeldung ab und lassen sie
unangetastet; wer zurückwill, nutzt die Sicherung (Beziehungen und Personen, die danach entstanden,
fehlen dort).

**Abgleich (`central::sync_relations`).** Läuft am Ende von `central::sync` (Knopf „Mit zentralem
Personenbestand abgleichen“, Benennen/Anlegen, Auflösen von Rückfragen) bei gesetzter Einstellung
`central_relations` (Standard an, wirkt nur bei `central_enabled`). Ein Fehler hier entwertet den
Personenabgleich nicht, sondern steht als Hinweis im Bericht. Beide Seiten werden über `gid`
verglichen (lokale ID ↔ `gid` über das Attribut `global_id`, Alias aufgelöst):

1. *Zentral → lokal* (eine kurze `IMMEDIATE`-Transaktion): fehlende zentrale Beziehungen zwischen
   Personen, die lokal beide existieren, werden angelegt, wenn die Prüfung sie annimmt. Was die
   Prüfung ablehnt (Zyklus, Selbstbezug, Widerspruch zu lokalen Fakten), wird nicht angelegt, sondern
   als `RelConflict` gemeldet. Fehlt eine der Personen lokal, wird übersprungen (Zähler); `adopt`
   zieht die Beziehungen der übernommenen Personen sofort nach (mit Prüfung).
2. *Lokal → zentral* (eine kurze Transaktion): lokale Beziehungen zwischen verknüpften Personen,
   die zentral fehlen, werden ergänzt, außer es gibt einen Grabstein (dann Rückfrage) oder sie
   widersprechen dem zentralen Graphen (gezählt als „nicht zentral übernommen“). Reihenfolge ist
   gegenüber ADR 0062 bewusst umgekehrt (erst lokal anlegen, dann zentral ergänzen), weil das
   Ergänzen vom Ergebnis des lokalen Prüflaufs abhängt; beide Schritte sind idempotent, ein
   Abbruch dazwischen heilt der nächste Abgleich.
3. *Entscheidungen*: `declined`/`declined_friends` werden als Mengenvereinigung in beide
   Richtungen abgeglichen (lokale IDs ↔ `gid`); Ablehnungen gehen nie verloren, nur Paare mit
   beiden Personen lokal werden lokal übernommen.

**Löschen.** `central::remove_relation_synced` (UI „Beziehung entfernen“) löscht lokal und schreibt,
wenn zentrale Beziehungen aktiv und beide Personen verknüpft sind, einen Grabstein (und löscht die
zentrale Zeile). Der Grabstein verhindert das Wiederanlegen beim Abgleich jeder Sammlung. Eine
andere Sammlung, die die Beziehung noch lokal hat, **löscht nie automatisch**: die lokale Kante
bleibt und erscheint bei jedem Abgleich als Rückfrage („lokal behalten und zentral wiederherstellen“
oder „lokal entfernen“). Bewusstes Neuanlegen (`add_relation_synced`) hebt den Grabstein auf.
Gründe: Eine Löschung ist eine Meinung einer Sammlung, nicht die Wahrheit der anderen; stilles
Löschen wäre Datenverlust.

**Konflikte und Wahl.** `RelConflict` nennt lokale Seite, zentrale Seite (als Sätze, z. B. „Anna ist
Mutter von Ben“ gegen „zentral: Anna und Ben sind Partner“) und Grund. `central::resolve_relation`
kennt drei Wahlen: *Lokal behalten und zentral ersetzen* (zentrale Kante bekommt einen Grabstein,
lokale wird zentral angelegt), *Zentral übernehmen* (lokale Kanten des Paares entfernen, zentrale
anlegen; nur wenn die Prüfung das erlaubt, sonst Fehlermeldung ohne Änderung) und *Keine von beiden*
(beide Seiten entfernt, beide Grabsteine). Nichts davon geschieht ohne Klick. Anmerkung: Nach den
Regeln aus ADR 0060 sind z. B. Partner und Freund für dasselbe Paar **kein** Widerspruch
(`kinds_conflict`); Widersprüche sind etwa Eltern-Kind gegen Partner, Gegenrichtung der Eltern-Kante,
Freund gegen Bekannter und Zyklen über mehrere Personen.

**Zusammenführen.** Zentral hängt `Central::merge` Beziehungen, Grabsteine und Entscheidungen in
derselben Transaktion von der Quell- auf die Ziel-`gid` um (`relations::plan_merge`; bei einem
Widerspruch im Plan wird nur umgehängt, geprüft wird erst beim lokalen Anlegen). Dubletten und
Selbstbezüge entfallen. Lesen löst Aliase ohnehin auf (Beziehungen, die ein langsamer Schreiber
noch auf die alte `gid` legt, werden richtig behandelt). Lokal bleibt `maintenance::merge_people`.

**Leistung.** `relations::Validator` entscheidet wie `validate_add`, hält aber Adjazenzen vor
(Gleichheitstest gegen `validate_add` auf pseudozufälligen Folgen); ohne ihn dauerte das Anlegen von
10.000 Beziehungen rund eine Minute, mit ihm unter einer Sekunde.

**Oberfläche.** Einstellungskarte: Kontrollkästchen „Beziehungen zentral mitführen“. Bereich
„Zentraler Bestand“: Zeile „Beziehungen: N zentral ergänzt, M lokal ergänzt, K Rückfragen/Konflikte,
J übersprungen (Person fehlt)“ und je Konflikt eine Gruppe mit den Knöpfen oben. Die Arbeit läuft im
vorhandenen Schreib-Worker (`run_central`); Konflikte zählen in der Kennzahl am Reiter mit.

## Alternativen

Beziehungen mit lokalen IDs zentral führen: Identität bricht bei jeder Sammlung. Ein gemeinsamer
Graph mit „letzte Änderung gewinnt“: überschreibt Nutzerentscheidungen. Löschungen automatisch
verteilen: Datenverlust bei Fehlbedienung einer Sammlung. Umbau von `people`: Migration mit
Datenrisiko, unnötig.

## Folgen und Grenzen

- Ein Grabstein macht eine Löschung dauerhaft zentral wirksam, bis jemand dieselbe Beziehung wieder
  bewusst anlegt; `gaps::apply` („Ja“ bei Rückfragen) hebt Grabsteine nicht auf, erst der nächste
  Eintrag über die Beziehungsmaske tut es.
- Lokale Beziehungen sind lokal geprüft; zentrale Prüfung gegen den globalen Graphen ist nur bei
  gesammeltem Abgleich vollständig (Zyklen über Personen, die lokal fehlen, werden beim sofortigen
  Anlegen in `add_relation_synced` nicht zentral geprüft; der nächste Abgleich meldet sie).
- Duplikate in einer Sammlung mit derselben `gid` (nach zentraler, noch nicht lokaler Zusammenführung)
  nutzen für den Abgleich die kleinste lokale ID.
- Nicht verifiziert: Mehrinstanz-Läufe über Netzlaufwerk, Bedienung der neuen Knöpfe per Maus
  (nur Zustandstest und Bild), Verhalten bei sehr großen Konfliktzahlen (die Liste ist nicht
  begrenzt).

## Bei Weiterentwicklung beachten

Nie überschreiben und nie still löschen; Beziehungen nie mit Gesichtsdaten koppeln; jede lokale
Neuanlage über die Prüfung aus `relations`; Änderungen am Schema 2 nur additiv mit neuem
Versionssprung; neue Beziehungstypen im Katalog und hier ergänzen.

## Belege und Validierung

[rels.rs](../src/core/central/rels.rs), [central.rs](../src/core/central.rs),
[relations.rs](../src/core/relations.rs), [central_view.rs](../src/ui/app/central_view.rs),
[Tests](../tests/central_relations.rs): zwei Sammlungen, Duplikatfreiheit, Grabstein ohne stilles
Löschen, Zyklus und Widerspruch als Rückfrage, fehlende Person und `adopt`, Vereinigung der
Entscheidungen, Zusammenführen mitten im Baum, Migration v1→v2 mit Sicherung, neuere Version,
gesperrt/schreibgeschützt, Parallelität, 5.000/10.000 in Sekunden, Berichtszähler.
