# ADR-0032: Metadaten im Ordnerbaum mit quellbezogenen Gesichtsrahmen

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-30
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0012](0012-lesende-metadatenansicht.md),
  [0015](0015-fortsetzbare-gesichtspipeline.md),
  [0021](0021-uninteressierte-gesichtsgruppen.md),
  [0022](0022-indizierte-sammlungsprojektionen.md),
  [0025](0025-sichtbare-gesichtsvorschauen.md)

> **Ergänzung 2026-10-30:** [ADR-0033](0033-gesichtskorrekturen-am-bild.md) erweitert die
> lesende Ansicht um explizite Einzelaktionen zum Löschen und Benennen von Gesichtern.
> Foto-Dateien bleiben unverändert; die sonstige Lesesemantik bleibt bestehen.

## Kontext

Die flache Metadatenliste verliert die ursprüngliche Ordnerstruktur.
Personeninformationen sollen zusammen mit anderen Metadaten sichtbar werden.
Größere Vorschauen und eine vergrößerbare Bildansicht dürfen bei großen
Sammlungen keine vollständigen Dateilisten oder Bildbestände in Python laden.

## Entscheidung

Die Metadatenansicht startet im Ordnerbaum; die flache Liste bleibt umschaltbar.
Direkte Unterordner und Fotos werden erst beim Öffnen einer Ebene aus der
vorhandenen metadata_photos-Projektion abgefragt. Jede Seite enthält höchstens
200 Einträge, Ordner zuerst. Die Foto-Identität und bevorzugte Metadatenvariante
aus ADR-0012/0022 bleiben maßgeblich; ein RAW/JPEG-Paar erscheint einmal.
Filter gelten über alle Ebenen, Diagramme und Kennzahlen für die Gesamtauswahl.

Nur das ausgewählte Foto wird decodiert. Normale Vorschauen sind auf 960 × 640
Pixel begrenzt. Ein Klick öffnet ein eigenes Fenster und lädt höchstens
1920 × 1280 Pixel nach. Getrennte Worker halten Tk frei; Generationsnummern,
Zweigrevisionen und Zielordnerprüfung verhindern verspätete Ergebnisse.
Filterwechsel und Schließen brechen wartende Arbeit ab.

Gesichtsmetadaten enthalten aktuelle Vorkommen (retained=0), einschließlich
unbekannter und ignorierter Gesichter. Namen und Zuordnungsstatus bleiben
unabhängig vom Bildschalter sichtbar. Historisch zurückbehaltene Vorkommen
werden nicht als zusätzliche aktuelle Gesichter dargestellt. Gesichtsrahmen
und Namen sind standardmäßig eingeschaltet; beide Bildansichten teilen den
Schalter.

Vorschauen bevorzugen die Quelldatei der vorhandenen Gesichtserkennung.
EXIF-Ausrichtung wird einmal angewandt; normalisierte Rahmen beziehen sich
ausschließlich auf diese Bildkoordinaten und die tatsächlich angezeigte Variante.
Bei nicht passender Quelle bleibt der Metadateneintrag mit Hinweis erhalten,
ohne einen möglicherweise falschen Rahmen zu zeichnen. Es erfolgt weder
Gesichtserkennung noch eine Änderung von Zuordnungen oder Datenbankschema.

Weitere Metadaten sind standardmäßig offen. Personen stehen direkt unter dem
Bild; Kennzahlen und kleinere Kamera-/ISO-Diagramme gemeinsam oberhalb des Baums.
Vorschauhöhe und verschiebbare Bereichsgrenze berücksichtigen die Fenstergröße.

## Alternativen

- Vollständiger Dateibaum im Speicher oder ein Dateisystem-Scan: unnötiger
  Speicher- und Dateizugriff beim Start.
- Alle Fotos eines Ordners laden: unbeschränkte Ergebnisgrößen bei großen Alben.
- Rahmen beliebig auf RAW/JPEG-Varianten übertragen: unterschiedliche Ausschnitte
  oder Drehungen können Namen an falschen Stellen erzeugen.
- Nur bestätigte Personen zeigen: unbekannte und ignorierte Vorkommen würden
  als bereits vorhandene Metadaten verborgen.

## Folgen und Grenzen

Keine Migration und keine zusätzlichen Pakete. Bestehende Sammlungen werden
lesend geöffnet. Leere oder durch Filter ausgeschlossene Ordner erscheinen nicht.
Bei Varianten in verschiedenen Ordnern repräsentiert der bevorzugte Fotopfad
weiterhin die Aufnahme. Große Ordnerebenen verlangen Blättern. Benutzer können
mehrere Zweige aufklappen; jedes Nachladen bleibt begrenzt, Bildspeicher bleibt
auf die Auswahl und eine optionale große Vorschau beschränkt.

Ordnernamen werden in SQLite über relative Pfade abgeleitet. Für Filter und
Ordneraggregation kann SQLite vorhandene Projektionszeilen scannen; die
Ergebnismengen in Python bleiben begrenzt. Eine materialisierte Ordnerprojektion
wäre bei später belegten Laufzeitproblemen gesondert zu entscheiden.
Kleine Fenster zeigen zusätzliche Metadaten über den scrollbaren rechten Bereich.

## Bei Weiterentwicklung beachten

Foto-Zählung, Variantenidentität, Quellbezug und Ausrichtung gemeinsam testen.
Gesichtsmetadaten nicht mit einem neuen Erkennungslauf koppeln. Unterordner
niemals rekursiv auf Vorrat laden. Beim Filtern, Blättern und Schließen
ausstehende Zweige und große Vorschauen invalidieren.

## Belege und Validierung

- [Abfragen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/metadata_browser.py), [Ordnerseiten](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/metadata_tree.py)
- [Ansicht](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/metadata_view.py), [Zweige](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/metadata_folder_tree.py),
  [Rahmen und Großansicht](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/metadata_preview.py)
- [Daten-/Vorschautests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_metadata_browser.py),
  [echte Tk-Tests](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/ui/test_metadata_view.py)
- [Bedienung und reproduzierbarer Screenshot](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/docs/metadata-visualization.md)

