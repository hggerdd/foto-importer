# Drei Konzepte und ihre kritische Bewertung

Stand 2026-11-04. Entwurfsbewertung, keine durchgeführte Nutzerstudie.

## A · Geführter Import

`Quelle → Tage wählen → Ziel prüfen → Import → Sammlung`

Ein großer nächster Schritt und eine Fortschrittsleiste führen durch den Import.
Die Galerie erscheint bei der Auswahl; Metadaten und Personen folgen danach.

* Einstieg: wenig gleichzeitige Entscheidungen, gut für gelegentliche Nutzung.
* Effizienz: wiederholte Importe erfordern Rücksprünge; parallel laufende Aufgaben
  und dauerhafte Sammlungsarbeit passen schlecht in eine lineare Geschichte.
* Fehlerprävention: eigener Prüfschritt gut; rückwirkende Änderungen können jedoch
  bereits geprüfte Angaben ungültig machen.
* Barrierefreiheit: klare Fokusreihenfolge; Screenreader müssen Schrittwechsel erkennen.
* Wartbarkeit: Schritte gut isolierbar, aber zunehmend verzweigte Zustandsmaschine.
* Kleine Fenster: gut, weil nur ein Schritt sichtbar ist.

## B · Bildzentrierte Bibliothek

`Alben | großes Fotomosaik | ausblendbare Details`

Import als ausklappbare Seitenleiste, Personen als Filterchips über dem Raster.

* Visuelle Wirkung: maximale Bildfläche, vertraut aus Fotoanwendungen.
* Effizienz: ausgezeichnetes Sichten; Importkonfiguration und technische Aufgaben
  werden hinter Symbolen oder ausklappbaren Flächen leicht übersehen.
* Fehlerprävention: Zielstruktur ist während des Sichtens nicht dauerhaft sichtbar.
* Barrierefreiheit: große Bildziele, aber bildbasierte Navigation braucht gute
  Namen, Tastaturzugriff und sichtbare Auswahlzustände.
* Wartbarkeit: Filter können zu einer schwer verständlichen Kombination aus
  Bibliothekszustand und Importzustand wachsen.
* Kleine Fenster: Detailbereich verdrängt Galerie, häufiges Auf-/Zuklappen nötig.

## C · Gemeinsamer Arbeitsbereich (gewählt)

```text
┌ Navigation ┬ Arbeitsbereich                       ┬ Kontext ┐
│ Import     │ Titel · Status · wichtigste Aktion   │ Auswahl │
│ Sammlung   │ Filter / Tage / Suchkriterien        │ Vorschau│
│ Personen   │                                      │ Details │
│ Aufgaben   │ Galerie oder Benennungsgruppen       │ Ziel    │
│ Optionen   │                                      │         │
├────────────┴──────────────────────────────────────┴─────────┤
│ Aktiver Auftrag · Fortschritt · Abbrechen                  │
└────────────────────────────────────────────────────────────┘
```

* Einstieg: mehr sichtbare Struktur als A; leere Zustände nennen genau den
  nächsten sinnvollen Schritt. Fachliche Optionen werden schrittweise gezeigt.
* Effizienz: feste Orte für Import, Sammlung, Personen und Einstellungen;
  ein Aufgabenbereich bleibt bei der Navigation sichtbar.
* Fehlerprävention: Auswahlanzahl, Zielbaum und Kopierprinzip vor dem Start;
  Speichern muss erfolgreich sein, bevor eine Gruppe verschwindet.
* Barrierefreiheit: Text statt reiner Symbolnavigation, hoher Kontrast,
  mindestens 36 Punkte hohe Hauptaktionen, sichtbare Tastaturfokussierung.
  Egui/AccessKit ersetzt keinen belegten Windows-Screenreader-Test.
* Wartbarkeit: Ansichten teilen Galerie, Vorschau, Aufgaben- und Fehlerdarstellung;
  Services bleiben ohne UI testbar. Risiko: eine große App-Zustandsstruktur;
  deshalb Domainlogik in getrennten Modulen halten.
* Kleine Fenster: anpassbare Galerie, scrollbarere Seitenbereiche; unter 980×680
  keine zugesicherte Bedienung. Eine spätere einklappbare Navigation wäre sinnvoll.

## Vergleich

Ordinaler Entwurfsvergleich (1 schwach, 5 stark), keine Messdaten:

| Gesichtspunkt | A | B | C |
|---|---:|---:|---:|
| Erstimport | 5 | 3 | 4 |
| Wiederholte Arbeit | 2 | 4 | 5 |
| Importziel verständlich | 5 | 2 | 5 |
| Metadaten und Personen | 2 | 5 | 5 |
| Kleine Fenster | 5 | 3 | 3 |
| Erweiterbarkeit | 3 | 3 | 4 |

C passt zur tatsächlichen Breite des Bestands. Die Wahl ist revidierbar nach
Beobachtung realer Aufgaben: Erstimport, zweiter Import während laufender Kopie,
Suche nach X und Y ohne Z, Gruppenkorrektur, Abbruch und Wiederaufnahme.

## Gestaltungsregeln

Dunkles, ruhiges Graphit, petrolfarbene Hauptaktionen, warme Akzentfarbe für
offene Arbeit. Typografische Hierarchie statt vieler Rahmen. Fotos erhalten
neutrale Bildflächen; Dateinamen und Zustände bleiben lesbar. Status zeigt
gespeicherte Resultate und Fehler, keine erfundenen Erfolgsmeldungen.
Einstellungen enthalten Werkzeug- und Modellpfade, keine Pflichtdialoge im Import.
