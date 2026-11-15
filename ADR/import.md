# Import

## Entscheidung

- **Originale werden nur gelesen.** Der Import kopiert nach
  `<Ziel>/<Ordnername>/<Endung>/`; nichts wird verschoben oder gelöscht.
- **Vollständige Kopien ohne Ersetzen:** Je Datei entsteht eine temporäre Datei im
  Ziel-Dateisystem (blockweise Kopie mit Abbruch, Übernahme der Änderungszeit, `sync_all`),
  danach wird sie exklusiv unter dem Originalnamen oder einem freien Nummernsuffix
  veröffentlicht (`persist_noclobber`). Vorhandene Zieldateien werden nie ersetzt.
- **Teilfehler:** Fertige Kopien bleiben; ein Auftrag entfernt bei Fehler oder Abbruch nur
  seine eigene temporäre Datei.
- **Ordnername** wird vor dem Start geprüft; der Zielplan ist vor dem Kopieren sichtbar.
- **Datumsquelle wählbar:** Dateisystemdatum oder Aufnahmedatum aus EXIF/IPTC mit Rückfall
  auf das Dateisystem. Zeitstempel werden nicht umgedeutet.
- **Duplikate überspringen (wählbar):** bevorzugt über die Kameraidentität; schwache Treffer
  über Name und Größe werden zusätzlich per schnellem Inhaltshash geprüft.
- **ExifTool** liefert Sammlungsmetadaten und eingebettete RAW-Vorschauen, ausschließlich
  lesend. Der Quellscan funktioniert auch ohne ExifTool.
- **DNG (optional):** Der separat installierte Adobe DNG Converter läuft als eigener Prozess
  je Datei, ohne Shell und ohne Konsolenfenster, in einer eigenen Warteschlange. Die Ausgabe
  entsteht zuerst temporär und wird nach geprüftem Erfolg mit demselben Schutz veröffentlicht.
  Vorhandene DNGs und RAWs bleiben.
- **Adobe-Versionsprüfung:** höchstens ein Versuch je lokalem Kalendertag (auch Fehler werden
  gemerkt), nur eine HEAD-Anfrage an den bekannten Adobe-Link (fünf Sekunden, höchstens acht
  Weiterleitungen, nur HTTPS auf `download.adobe.com`). Die lokale Version wird aus den
  Dateieigenschaften gelesen, ohne den Converter zu starten. Kein Download, keine Installation.

## Grenzen

Keine Garantie bei Stromausfall oder auf Netzlaufwerken. Die DNG-Prüfung stellt Erfolg und
nicht leere Datei fest, nicht die inhaltliche Richtigkeit. Echter Adobe-Lauf und seltene
Kamera-Metadaten sind nicht umfassend geprüft.

## Beim Ändern beachten

Keine automatische Quelllöschung, keine Ersetzung vorhandener Dateien. Temporäre Reste nie
fremden Aufträgen zuordnen oder löschen. Kollision, Wiederholung, Fehler und Abbruch mit
echten Dateisystemtests prüfen.
