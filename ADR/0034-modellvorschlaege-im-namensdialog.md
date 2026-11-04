# ADR-0034: Sechs Modellvorschläge im Namensdialog

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-30
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0016](0016-namensvorschlaege-aus-referenzen.md),
  [0024](0024-fortsetzbarer-gesichtsabgleich.md),
  [0033](0033-gesichtskorrekturen-am-bild.md)

## Kontext

Der Dialog zum Benennen eines einzelnen Gesichts verlangt bisher die vollständige
Namenseingabe oder das Durchsuchen der Personenliste. Die vorhandenen
Gesichtsvektoren und bestätigten Referenzen können diesen Schritt verkürzen.

## Entscheidung

Oberhalb des Eingabefelds stehen sechs Vorschlagsplätze in einer Zeile.
Verfügbare Kandidaten sind nach absteigender Ähnlichkeit sortiert; Gleichstände
werden nach Name und Personen-ID aufgelöst. Ein Klick übernimmt die konkrete
Personen-ID und speichert dieselbe explizite Nutzerbestätigung wie die bisherige
Personenauswahl. Ohne Klick wird keine Zuordnung geändert.

Die Bewertung verwendet ausschließlich den gespeicherten Gesichtsvektor des
aktiven Embedders und die bestehende reference_scores-Funktion: Mittelwert der
höchstens drei besten gültigen Referenzähnlichkeiten pro Person. Gültigkeit,
Mindestqualität, Review-/Trainingsausschlüsse und Modelltrennung entsprechen
ADR-0016/0024. Das zu benennende Gesicht wird selbst aus den Referenzen entfernt;
weitere Beispiele derselben Person bleiben verwendbar. Bereits anderweitig
bestätigte Personen auf demselben Foto und explizite Ablehnungen erscheinen
nicht unter den Vorschlagsbuttons.

Ein manuell zu benennendes Gesicht darf selbst unter der Mindestqualität liegen;
diese begrenzt die Referenzauswahl. Die Liste ist eine Ähnlichkeitsrangfolge und
keine kalibrierte Wahrscheinlichkeit oder automatische Zuordnungsentscheidung.
Schwache Kandidaten werden nicht als sichere Erkennung gekennzeichnet.

Es laufen weder Bilddecodierung noch Netzinferenz oder Training. Namen, Vektor und
Referenzen werden auf dem bisherigen Dialog-Worker in einem lesenden Snapshot
abgefragt. Referenzen bleiben in Blöcken zu 256 Vektoren begrenzt; nur der eine
Abfragevektor und Personenbewertungen werden gehalten. Die Namensliste benötigt
keine gesonderten Gesichts-Zählabfragen je Person.

Fehlt der aktuelle Vektor, ist er ungültig oder fehlen gültige Referenzen,
erscheint ein verständlicher Hinweis. Nicht belegte Vorschlagsplätze bleiben
deaktiviert; Personen werden nicht als künstliche Modellvorschläge ergänzt.

Beim Tippen filtert das Namensfeld vorhandene Personen ohne Beachtung der
Groß-/Kleinschreibung und zeigt bis zu fünf Treffer direkt darunter; das
Ausklappfeld enthält alle passenden Namen. Ein Listentreffer kann angeklickt
werden. Enter übernimmt einen eindeutigen Treffer, mehrere Treffer verlangen
eine Auswahl. Eine freie neue Eingabe und die bisherige Übernehmen-Schaltfläche
bleiben erhalten. Gleiche Namen werden wie bisher über IDs unterschieden.

## Alternativen

- Namen nach Häufigkeit oder Aktualität vorschlagen: entspricht nicht der
  angeforderten Modellähnlichkeit.
- Das aktuelle Auto-Label allein anzeigen: liefert keine sechs Alternativen.
- Alle Referenzbilder neu analysieren: unnötige Laufzeit und Speicherlast.
- Ähnlichkeiten als Prozentwahrscheinlichkeit darstellen: ohne Kalibrierung
  fachlich nicht begründet.

## Folgen und Grenzen

Keine Schemaänderung und keine neuen Pakete. Mehr gültige Referenzen können die
Dialogberechnung verlängern; der UI-Thread bleibt frei. Personen ohne geeignete
Beispiele im aktiven Modell sind weiterhin über die Namenssuche erreichbar.
Das gemeinsame Foto-Optimierungsverfahren für automatische Zuordnungen bleibt
unverändert; Buttons zeigen individuell geeignete manuelle Kandidaten.
Alter und Ko-Okkurrenz ändern diese reine Ähnlichkeitsrangfolge nicht.

## Bei Weiterentwicklung beachten

Keine automatischen Labels als Lernreferenzen übernehmen, Modellräume nicht
mischen und die eigene Referenz nicht zur Selbstbestätigung verwenden.
Rangfolge, begrenzte Referenzblöcke, unvollständige Modellstände sowie
personen-ID-genaue Übernahme bei doppelten Namen testen.

## Belege und Validierung

- [Rangfolge und Namensdaten](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/metadata_face_actions.py),
  [gemeinsame Bewertung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_matching.py)
- [Buttons und Namenssuche](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/metadata_face_actions.py),
  [Konfiguration](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/config/settings.py)
- [Rankingtests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_face_name_suggestions.py),
  [echte Tk- und Integrationstests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_metadata_face_actions.py)
- [Bedienung und Screenshot](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/metadata-visualization.md)

