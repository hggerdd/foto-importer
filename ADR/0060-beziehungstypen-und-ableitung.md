# ADR-0060: Beziehungstypen, Richtungsunabhängigkeit und Ableitung

- **Status:** Akzeptiert; die Ableitung und die Rolle von Geschwister/Cousin sind teilweise abgelöst durch [ADR-0061](0061-stammbaum-als-normalisiertes-modell.md)
- **Erfasst am:** 2026-11-08
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-08
- **Bezieht sich auf:** [0019](0019-personen-und-beziehungsgraph.md) (wird ergänzt), [0042](0042-native-bestandsmigrationen.md), [0049](0049-gewichteter-beziehungsgraph.md)

## Kontext

Bisher kannten Sammlungen Eltern/Kind, Partner und Geschwister. Die Eingabe
„Von – Typ – Nach“ war richtungsabhängig und ohne Geschlechtsformen; Cousins,
Freunde und Bekannte fehlten; abgeleitete Verwandtschaft war nur eine Textliste.
Gewünscht sind neue Typen, Eingabe aus beiden Richtungen mit demselben
gespeicherten Zustand, nachvollziehbare Widerspruchsmeldungen und eine
systematische Ableitung. Schema 6 und bestehende IDs müssen unverändert bleiben.

## Entscheidung

**Katalog (eine Quelle der Wahrheit):** `src/core/relations.rs` (`CATALOG`).
Er erzeugt die Zeilen von `relation_types` (`seed_sql()`):

| ID | Name | Umkehrtyp | symmetrisch |
|---|---|---|---|
| 1 | `elternteil_von` | 2 | nein |
| 2 | `kind_von` | 1 | nein |
| 3 | `partner_von` | 3 | ja |
| 4 | `geschwister_von` | 4 | ja |
| 5 | `cousin_von` | 5 | ja |
| 6 | `freund_von` | 6 | ja |
| 7 | `bekannter_von` | 7 | ja |

**Kanonische Speicherung:** Jede Beziehung steht einmal in `relations`.
Eltern: `from_person` ist Elternteil von `to_person` (Typ 1). „Kind von“ wird
nie neu geschrieben, sondern beim Eingeben vertauscht; ältere Zeilen mit Typ 2
werden beim Lesen umgedeutet. Symmetrische Typen: `from_person < to_person`.
„x ist Mutter von y“ und „y ist Tochter von x“ führen deshalb zum identischen
Datensatz; wiederholte Eingabe aus einer der Richtungen ist ein No-op.
Das Lesen (`load_edges`/`normalize`) toleriert nicht kanonische Altzeilen und
Dubletten; `remove` löscht auch diese Formen.

**Beschriftung:** Rolle (`Role`) + Geschlecht der Person, die die Rolle trägt
(Attribut `gender`, Freitext; „weiblich/w/female“ und „männlich/m/male“ werden
erkannt, alles andere ist neutral): Mutter/Vater/Elternteil, Tochter/Sohn/Kind,
Partnerin/Partner/Partner/in, Schwester/Bruder/Geschwisterteil,
Cousine/Cousin/Cousin/Cousine, Freundin/Freund/Freund/in,
Bekannte/Bekannter/Bekannte/r. Die Beschriftung hängt nur vom Geschlecht des
Subjekts ab („x ist …“), nicht vom Geschlecht des Objekts; `label(type_id,
Direction, Gender)` und `Role::label` bilden das ab.

**Widerspruchsregeln** (`validate_add`, reine Funktion, Meldungen auf Deutsch):

1. Selbstbezug ist verboten.
2. Eltern-Zyklen sind verboten (auch die gegenläufige Eltern-Kante).
3. Je Personenpaar sind Eltern-Kind, Partner, Geschwister und Cousin
   gegenseitig ausgeschlossen, mit der einen Ausnahme Partner + Cousin
   (historisch und real möglich). Freund und Bekannter schließen sich aus
   (Freund ist die stärkere Angabe; vorhandene Entscheidung nicht stillschweigend ändern).
   Freund/Bekannter sind mit Familienbeziehungen vereinbar.
4. Partner, Geschwister und Cousin sind zwischen direkten Vorfahren und
   Nachkommen verboten; eine neue Eltern-Kante darf ein vorhandenes
   Partner/Geschwister/Cousin-Paar nicht zu Vorfahr/Nachkomme machen.
5. Cousin ist verboten, wenn die beiden bereits abgeleitet Geschwister sind;
   Geschwister ist verboten, wenn sie abgeleitet Cousins sind.

Nicht geprüft werden weitere Genealogie-Widersprüche (z. B. mehr als zwei
Eltern, Altersangaben); mehr als zwei Eltern bleiben erlaubt.

> Abgelöst durch ADR 0061: `derive` und die Zwei-Schritte-Tabelle entfallen; ein allgemeiner
> Verwandtschaftsrechner (`kinship`) berechnet alle Grade. Bestehen bleiben Katalog 1–7 (ID 8 neu),
> Freund/Bekannter, Eingabe aus beiden Richtungen, Merge-Regeln, Beschriftung nach Geschlecht des Subjekts.
> Geschwister und Cousin sind nur noch Behelf.

**Ableitung** (`derive`, nicht gespeichert, tiefenbegrenzt auf zwei Schritte,
höchstens 50 Einträge je Rolle, sortiert nach Rolle, dann ID):

| Abgeleitet | Regel |
|---|---|
| Großeltern / Enkel | Eltern der Eltern / Kinder der Kinder |
| Geschwister | gemeinsamer Elternteil; direkt gespeicherte Geschwister werden nicht doppelt abgeleitet |
| Halbgeschwister | gemeinsamer Elternteil, aber jede Seite hat einen weiteren, nicht gemeinsamen Elternteil; ist nur eine Seite mit weiteren Eltern bekannt, gilt „Geschwister“ |
| Onkel/Tante | (Halb-)Geschwister der Eltern, direkt oder abgeleitet |
| Nichte/Neffe | Kinder der (Halb-)Geschwister |
| Cousin/Cousine | Kinder von Onkel/Tante (ohne direkt gespeicherte und ohne Geschwister) |
| Schwiegereltern / Schwiegerkinder | Eltern der Partner / Partner der Kinder |

Nicht abgeleitet: Urgroßeltern, Stiefkinder, Schwäger, Freunde von Freunden.
Abgeleitetes ist in der Oberfläche getrennt und als „abgeleitet“ markiert,
nicht bearbeitbar. Übernehmbar als direkte Beziehung sind nur abgeleitete
Geschwister und Cousins; Halbgeschwister und Verschwägerte nicht, weil der Katalog
den Unterschied nicht ausdrücken kann.

**Zusammenführung:** `plan_merge` hängt die Kanten der Quellperson auf die
Zielperson um, verwirft Selbstbezüge und Dubletten, stellt die kanonische
Reihenfolge wieder her und prüft jede Kante mit `validate_add`; bei
Widerspruch wird die ganze Zusammenführung abgelehnt (ohne Änderung). Besteht
zwischen dem Ziel und einer Person „Bekannter“ und kommt „Freund“ hinzu, ersetzt der Freund
den Bekannten. Die bestehende Abstammungsprüfung bleibt.

**Kompatibilität:** Keine neue Tabelle und keine Schemaänderung. Neue Typen sind nur
Zeilen von `relation_types`; `INSERT OR IGNORE` ergänzt sie beim Anlegen, beim
Upgrade (ADR 0042), beim Öffnen mit Schreibrecht und vor jedem Schreiben
einer Beziehung. Schreibgeschütztes Öffnen verändert nichts; Katalog und
Beschriftung liegen im Code und funktionieren auch ohne die Zeilen. IDs 1–4 und
gespeicherte Beziehungen bleiben gültig.

**Oberfläche:** Im Personenbereich wird die Beziehung als Satz „[Person A] ist
[Rolle] von [Person B]“ eingegeben; die Rollenliste folgt dem Geschlecht von A,
darunter steht der gleichbedeutende Gegensatz. Darunter zeigt die Übersicht je
Person Eltern, Kinder, Partner, Geschwister, Cousins, Freunde und Bekannte (mit
Entfernen) und getrennt die abgeleiteten Beziehungen. Der alte Bericht
„Beziehungen anzeigen“ entfällt.

## Alternativen

Beide Richtungen zu speichern würde Dubletten und Widersprüche erzeugen.
Ableitungen zu speichern würde Redundanz erzeugen (bereits in 0019 verworfen).
Eine neue Tabelle für Typen wäre bei Schema-6-Sammlungen eine stille
Strukturänderung. Beschriftung nur aus der Datenbank wäre bei fehlenden Zeilen unvollständig.

## Folgen und Grenzen

Was bleibt aus 0019: stabile IDs, kanonische Eltern-Kanten, Ableitung statt
Speicherung, Zyklus- und Selbstbezugsprüfung. Was sich ändert: weitere Typen,
strengere Widerspruchsprüfung (Paarregeln, Vorfahrenregeln) auch bei der
Zusammenführung, Geschlechtsformen in der Oberfläche, systematische Ableitung
einschließlich Nichten/Neffen, Halbgeschwister und Schwiegereltern.
Bereits vorhandene widersprüchliche Altdaten werden nicht repariert, sondern
nur bei neuen Änderungen berücksichtigt. Das Geschlecht ist Freitext; unbekannte
Schreibweisen führen zur neutralen Form. Der Beziehungsgraph (ADR 0049) liest weiter
`relation_types` und zeigt neue Typen mit dem Namen aus der Tabelle.

## Bei Weiterentwicklung beachten

Neue Typen erhalten neue IDs im Katalog und eine Beschriftung je Geschlecht; Tests
prüfen Umkehrung und Beschriftung für alle Rollen. IDs nie umdeuten. Regeln
und Ableitungstabelle nur gemeinsam mit Tests ändern.

## Belege und Validierung

[Katalog, Regeln, Ableitung](../src/core/relations.rs),
[Eingabe, Zusammenführung, Altsammlung](../tests/relations.rs),
[Oberfläche](../src/ui/app/people_view.rs),
[Zusammenführung](../src/core/maintenance.rs). Echte Sammlungen wurden nicht geprüft.
