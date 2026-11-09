# ADR-0058: Personengraph als Netzwerk im Reiter „Graph“

- **Status:** Akzeptiert
- **Erfasst am:** 2026-11-08
- **Art:** Neue Entscheidung
- **Entscheidungsdatum:** 2026-11-08
- **Bezieht sich auf:** [0019](0019-personen-und-beziehungsgraph.md), [0038](0038-gemeinsamer-arbeitsbereich.md), [0045](0045-fotofokus-in-der-sammlung.md), [0049](0049-gewichteter-beziehungsgraph.md) (ergänzt und löst die Darstellung teilweise ab)

## Kontext

ADR 0049 zeigt im modalen Fenster eine Person mit höchstens zwölf direkten
Verbindungen. Wer wissen will, welche Gruppen in der Sammlung zusammengehören,
sieht nur einen Ausschnitt und muss den Mittelpunkt mehrfach wechseln. Das
Fenster verdeckt außerdem den Personenbereich, aus dem man den Graphen braucht.

## Entscheidung

**Ort.** Der Personenbereich erhält neben „Personen & Beziehungen“ den Reiter
„Graph“. Die Hauptfläche zeigt den Graphen, ein festes Seitenpanel (300 Punkt)
rechts enthält Personensuche und -liste, Details der ausgewählten Person und
die Legende. Das folgt ADR 0038 (Arbeitsbereich mit Kontext rechts) und
ADR 0045 (große Fläche, Details daneben). Ein eigener Reiter statt eines
Fensters behält Zoom, Auswahl und Ebenen beim Wechsel der Reiter. Der Button
„Graph“ in der Personenliste springt in den Reiter und wählt die Person.

**Inhalt.** Alle Personen sind Knoten, auch ohne Fotos. Ein kräftebasiertes
Layout (Fruchterman-Reingold) ordnet sie: Verbundene liegen nah, getrennte
Gruppen bleiben durch schwache Schwerkraft beieinander, ein Mindestabstand
verhindert Überlagerung. Das Layout ist eine reine, deterministische Funktion
(`src/core/graph_layout.rs`): Startpositionen auf einer Spirale in der
Reihenfolge „meiste Fotos zuerst“, keine Zufallszahlen, begrenzte
Iterationszahl (80–300 je nach Knotenzahl, exakte Abstoßung O(n²)).

**Kanten.** Zwei Ebenen, einzeln ausschaltbar:
(a) eingetragene Beziehungen nach Typ: Eltern → Kind orange mit Pfeil, Partner
gestrichelt, Geschwister gepunktet; jeder weitere Typ aus `relation_types`
wird mit seinem Namen datengetrieben in einer neutralen Farbe und eigener
Strichart dargestellt und in der Legende aufgeführt. Die Typen unterscheiden
sich also nicht nur durch Farbe.
(b) gemeinsame Fotos wie in 0049 gezählt (eindeutige Fotos, bestätigte
Zuordnungen, keine historischen Gesichter, keine zusammengeführten Personen),
Linienstärke logarithmisch nach Anzahl. Damit kein Linienknäuel entsteht,
blendet ein Schwellwert „ab Fotos“ schwache Verbindungen aus; der Standard
lässt etwa vier Fotolinien je Person übrig. Beide Ebenen fließen immer in
das Layout ein, damit es sich beim Umschalten nicht verändert.

**Knoten.** Kreis mit Initialen, Radius nach Fotozahl (Wurzelskala),
stabile Farbe je Personen-ID. Beschriftung bei Auswahl, Hover und bei
Nachbarn immer, sonst ab Zoomstufe bzw. bei höchstens 40 Personen, mit
Kollisionsprüfung. Kleine Porträts (`faces::person_portrait`) sind
nicht umgesetzt, weil sie pro Person Gesichtsvektoren lesen würden.

**Interaktion.** Mausrad und Pinch zoomen zum Zeiger, Ziehen verschiebt,
Klick wählt aus und hebt Nachbarn hervor (übrige abgeblendet), Doppelklick
zentriert. Das Panel bietet „Fotos anzeigen“ (Sammlungsfilter wie bisher),
„Zentrieren“ und je Verbindung „Fotos“ für das Paar. Schaltflächen „+“, „−“
und „Einpassen“ ersetzen das Mausrad.

**Berechnung.** `relationship_graph::network` läuft auf einem Worker
(Art „relationship-graph“, lesend, abbrechbar) in einer Lesetransaktion.
Das Ergebnis trägt eine Revision; nur die zum aktuellen Start gehörige
Revision wird übernommen. Nach Schreibaufträgen und Benennungen wird das Netz
als veraltet markiert und neu berechnet; Auswahl und Ausschnitt bleiben.
Die Oberfläche zeichnet nur.

**Grenzen.** Höchstens 600 Personen (die mit den meisten Fotos) und 6000
Kanten (eingetragene zuerst, dann nach Fotozahl) werden dargestellt; die
Symbolleiste nennt die Zahl der fehlenden Personen und Verbindungen.

**Barrierefreiheit.** Die Liste im Seitenpanel ist die Tastaturalternative:
Personen sind per Tab erreichbar und auswählbar, die Details nennen
Beziehungen und Fotozahlen als Text. Die Zeichenfläche hat eine
Textbeschreibung. Der Typ einer Beziehung wird nie nur über Farbe
vermittelt.

## Alternativen

Eine Beibehaltung des Fensters mit Ego-Graph blieb ohne Überblick über
Gruppen. Ein reiner Kreis- oder Rasterlayout zeigt keine Nähe. Eine
Barnes-Hut-Näherung wäre bei mehreren tausend Personen nötig, bei 600
Knoten genügt die exakte Berechnung (Test: 500 Knoten unter 20 s im
Debug-Build, im Release deutlich schneller). Echtzeit-Simulation im
UI-Thread wurde verworfen (AGENTS.md: langsame Arbeit auf Worker).

## Folgen und Grenzen

Zusammenhänge zwischen vielen Personen werden sichtbar. Das Layout ist
nach Datenänderung nicht kontinuierlich: neue Personen können die Karte
neu anordnen. Bei sehr großen Sammlungen fehlen die Personen mit
wenigsten Fotos. Von ADR 0049 bleiben Zählregeln, Linienstärke und die
Unterscheidung von eingetragenen und fotografisch belegten Verbindungen;
abgelöst sind Ego-Darstellung mit zwölf Nachbarn, Modalfenster und der
Wechsel des Mittelpunkts. `relationship_graph::for_person` bleibt als
Referenzabfrage und wird von den Tests verwendet. Eine Prüfung mit
Screenreader oder Nutzern steht aus.

## Bei Weiterentwicklung beachten

Zählregeln müssen mit der Sammlungssuche übereinstimmen (Test vorhanden).
Neue Beziehungstypen erscheinen automatisch neutral; für feste Farben
`Recorded` und `palette` erweitern. Layout nur deterministisch ändern. Bei
mehr als 600 Personen Barnes-Hut oder Gitter einführen und `MAX_NODES` neu
bewerten.

## Belege und Validierung

[Layout](../src/core/graph_layout.rs), [Netzabfrage](../src/core/relationship_graph.rs),
[Ansicht](../src/ui/app/relationship_view.rs),
[Layouttests](../tests/graph_layout.rs), [Netztests](../tests/relationship_graph.rs).
Sichtprüfung mit synthetischen Demodaten (`examples/demo_faces.rs`) siehe
[Prüfprotokoll](../docs/validierung.md).

## Ergänzung 2026-11-09: Geschwister aus dem Stammbaum

Geschwister und Halbgeschwister erscheinen im Graphen als gepunktete Linie, auch wenn sie nicht eingetragen sind. Sie werden im Netzwerk-Worker aus gemeinsamen Eltern berechnet (`kinship::Family::relationship`, Schritte 1/1) und nicht gespeichert. Eine direkt eingetragene Geschwisterkante wird nicht zusätzlich abgeleitet. Die Linie zählt wie eine eingetragene Beziehung für Ebene und Layout; höchstens 20.000 Geschwisterpaare werden geprüft.
