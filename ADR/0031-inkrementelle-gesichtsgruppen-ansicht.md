# ADR-0031: Bestehende Gesichtsgruppen bei Bestätigung weiterverwenden

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-29
- **Art:** Neue Entscheidung auf Nutzeranweisung
- **Bezieht sich auf:** [0003](0003-ui-thread-und-worker.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0025](0025-sichtbare-gesichtsvorschauen.md), [0026](0026-vollstaendig-sichtbare-gesichtsgruppen.md), [0029](0029-schneller-gesichtsstart-und-schema-reparatur.md)

## Kontext

Jede erfolgreiche Gruppenaktion lieferte einen neuen Snapshot. Die Ansicht
zerstörte daraufhin alle Gruppen-Widgets, verwarf geladene Vorschauen und erzeugte
sämtliche Zeilen/Bildobjekte erneut. Dadurch verursachte selbst eine einzelne
Bestätigung unnötige Vorschauarbeit und verzögerte die nächste Benennung.
Vorhandene Entwürfe mussten außerdem über einen kompletten Neuaufbau gerettet
werden. Bei wiederverwendeten Widgets dürfen wiederholte Event-Bindings keine
zusätzlichen Tcl-Callbacks ansammeln.

## Entscheidung

In Benennen und Prüfen werden Snapshots desselben Modus, derselben Seite und
derselben aktiven Detektor-/Embedderauswahl inkrementell abgeglichen.
Unveränderte Mitgliedslisten behalten ihre FaceGroupRow-Instanz einschließlich
Eingabefeld, Auswahl, decodierten Bildern und Tk-Bildobjekten. Es werden nur
verschwundene/veränderte Gruppen entfernt und tatsächlich neue Gruppen angelegt.
Gruppennummern und Personenlisten werden ohne Neuaufbau aktualisiert. Die Pack-
Reihenfolge ändert sich nur an benötigten Stellen; vorhandene Zeilen bleiben
gemappt. Frei gewordene Plätze werden aus der nächsten begrenzten Seite ergänzt.

Bestätigungen und Ignorieren entfernen eine Gruppe erst nach erfolgreichem
Speichern. Währenddessen wird keine optimistische unsichtbare Änderung behauptet.
Ein Fehler lässt die Gruppe samt Entwurf stehen. Rückgängig kann die betroffene
Gruppe wieder einsetzen; die übrigen Zeilen bleiben bestehen. Der Fokus geht
nach erfolgreicher Aktion zur nächsten Gruppe. Ist deren Namensfeld bereits
fokussiert, bleiben Einfügeposition und Eingabe erhalten. Gewöhnliche Hintergrund-
Snapshots verschieben den bestehenden Eingabefokus nicht.

Eine konstante Rahmenbreite verhindert Größenänderungen allein durch den
Fokuswechsel. Sichtbereichsermittlung erfolgt nach Entfernung, Layout und
Fokus-/Scrollanpassung, damit vorübergehende Zwischenpositionen keine Bilder
unnötig freigeben. Die Vorschaugeneration bleibt bei unveränderten Gesichtern
erhalten; verspätete Antworten für entfernte Gesichter werden verworfen.

Quelle, Fingerprint, Detektor, Ausschnitt, Orientierung und Pfad gehören zum
Vergleich der Vorschauidentität. Bei einer tatsächlichen Änderung wird nur die
betroffene Vorschau invalidiert; alte Decoderantworten sind dann ungültig.
Eine geänderte vorgeschlagene Person verlangt auch die Aktualisierung der
betroffenen Prüfgruppe. Modus-, Seiten-, Modell- und Sammlungswechsel dürfen
weiterhin vollständig neu aufbauen.

Der Sichtbereichsvertrag und die Cachegrenzen aus ADR 0025 bleiben bestehen:
Unsichtbare Tk-Bilder werden freigegeben, nur sichtbare fehlende Vorschauen
nachgeladen. Es entsteht kein unbegrenzter Bildspeicher für die gesamte Sammlung.
ScrollFrame bindet das Mausrad pro vorhandenem Widget nur einmal und bindet
später hinzugefügte Widgets nach; ein WeakSet verhindert zusätzliche Widget-
Lebensdauer durch die Bindungsverwaltung.

Der Rust-Arbeitsbereich übernimmt nach erfolgreichem Speichern die nächste
noch offene Gruppe anhand ihrer Gesichts-IDs aus der bisherigen Liste. Ihre
Namenszeile rückt soweit möglich an die bisherige Bildschirmposition. Wird
die letzte Seite leer, zeigt die Ansicht wieder eine Seite mit Gruppen.

## Alternativen

Ein größerer Bildcache allein verhindert weder Widget-Neubau noch Flackern und
Tcl-Bildarbeit. Vorabladen aller Gruppen widerspricht der Speichergrenze.
Optimistisches Entfernen vor dem Commit würde zusätzliche Rücksetzlogik und
möglicherweise verlorene Benutzerentscheidungen erzeugen. Die Datenbankabfragen
oder die Identitäts-/Gruppierungssemantik zu ändern ist für diesen Fehler unnötig.

## Folgen und Grenzen

Die UI muss Widget-, Bild- und Snapshot-Besitz gemeinsam abgleichen.
Tatsächlich veränderte Gruppenzusammensetzung oder Bildquelle kann weiterhin
einen gezielten Neuaufbau erfordern. Nach Scrollen oder Seitenwechsel dürfen
Vorschauen erneut aus dem begrenzten Cache bzw. der Quelle geladen werden.
Persistenz, 14er-Grenze, Größensortierung und Undo-Sicherheitsprüfung bleiben
unverändert. Es ist keine Schemaänderung nötig.

## Bei Weiterentwicklung beachten

Bei neuen Vorschau-Quellfeldern den Identitätsvergleich ergänzen. Keine neuen
Mitglieder still in eine bestehende geprüfte Auswahl übernehmen. Tests sollen
Widget-/Bildidentität und echte Loader-Aufrufe prüfen, nicht nur das optisch
gleiche Ergebnis. Wiederholte UI-Aktualisierung darf keine Event-Callbacks lecken.

## Belege und Validierung

[Snapshot-Abgleich](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_workspace.py),
[stabile Gruppenkacheln](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_group_row.py),
[idempotentes Mausrad-Binding](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/scroll_frame.py),
[Benennen/Prüfen, Serien, Entwürfe, Fehler und Undo](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_face_view.py),
[Callback-Stabilität](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_scroll_frame.py).

Der Serientest bestätigt sechs aufeinanderfolgende Zuordnungen bei acht
Gruppen auf der geladenen Seite: acht Zeilen beim Einstieg und genau sechs neue
Ersatzzeilen; ein vollständiger Neuaufbau würde 56 Zeilen erzeugen. Die
unverändert sichtbaren Bilder bleiben dieselben Tk-Objekte ohne erneute
Loader-Aufrufe. Das ist eine Aufwandsprüfung, keine pauschale Laufzeitgarantie.

[Sichtprüfung nach der ersten Bestätigung](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/screenshots/incremental-face-groups.png),
reproduzierbar mit [Vorschauskript](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/scripts/preview_faces.py) und Option --confirm-first.

