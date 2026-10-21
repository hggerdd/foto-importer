# Foto-Importer

Tkinter-Anwendung zum Gruppieren von Kamera-Dateien nach Datum, mit
Bildvorschau und Kopieren in Unterordner je Dateiendung. Als Datumsquelle
stehen Dateisystem-Zeitstempel und Foto-Metadaten zur Verfügung.

```powershell
uv sync
uv run python main.py
```

## Performance

Die Optimierung verwendet beim rekursiven Scan `os.scandir`, um bereits vom
Betriebssystem gelieferte Dateiinformationen wiederzuverwenden. Unpassende
Dateiendungen werden früh aussortiert; ein Abbruch wird auch während der
Verzeichnissuche geprüft.

Worker übergeben Ergebnisse über eine Queue an Tkinter. Fortschrittsmeldungen
werden zusammengefasst und alle 50 ms vom UI-Thread abgeholt. Damit wartet der
Scan oder Kopiervorgang nicht mehr pro Datei auf Tkinter. Abschluss- und
Fehlermeldungen bleiben erhalten.

Vorschaubilder werden von einem einzelnen Hintergrund-Worker geladen.
Veraltete Vorschauaufträge werden abgebrochen; nur der UI-Thread erzeugt
Tkinter-Bilder. Ein LRU-Cache hält bis zu 128 kleine Vorschaubilder und prüft
Dateigröße sowie Änderungs- und Erstellungs-/Metadatenzeit auf Änderungen.
JPEG-Decodierung nutzt die reduzierte Auflösung des Decoders; EXIF-Rotation
wird berücksichtigt. Die Grenzwerte `UI_POLL_INTERVAL_MS` und
`THUMBNAIL_CACHE_SIZE` stehen zentral in `config/settings.py`; bestehende
Benutzereinstellungen bleiben gültig.

Beim Kopieren gleichnamiger Dateien wird der nächste freie Nummernsuffix
weitergezählt. Vorhandene Zieldateien werden weiterhin geprüft und behalten.
Der Fortschritt zählt erst fertig kopierte Dateien.

### Lokaler Vergleich mit Sicherungscommit `b0d08d7`

Windows, identische synthetische Testdaten auf dem lokalen Laufwerk, Median
aus fünf Scan-Durchläufen bzw. drei Durchläufen für die übrigen Messungen:

| Messung | Vorher | Nachher | Faktor |
| --- | ---: | ---: | ---: |
| Scan von 6.000 Dateien ohne UI | 0,5092 s | 0,1898 s | 2,68× |
| Scan von 6.000 Dateien mit Tk-Fortschrittsübergabe | 1,4530 s | 0,1983 s | 7,33× |
| Acht JPEG-Vorschauen, je 24 MP, leerer Anwendungscache | 1,6575 s | 1,6770 s | 0,99× |
| 400 gleiche Dateinamen kopieren, leere Dateien | 3,1189 s | 0,5599 s | 5,57× |

Die acht bereits gecachten Vorschaubilder wurden in rund 0,0004 s geladen.
Die erstmalige Vorschauerzeugung ist in dieser Messung etwa gleich schnell,
blockiert aber die Oberfläche nicht mehr. Der UI-Scan-Benchmark verwendet
echte Tkinter-Aufrufe mit einem leeren Fortschritts-Callback, ohne Widgets
neu zu zeichnen. Die Kopiermessung untersucht Namenskollisionen, nicht den
Datendurchsatz großer Dateien. Speicherkarte, USB-Verbindung, Ziellaufwerk
und Betriebssystemcache beeinflussen die Ergebnisse im echten Einsatz.

Vergleich wiederholen (nutzt temporäre Testdaten und liest den alten Code
direkt aus Git):

```powershell
uv run python -m benchmarks.performance --baseline b0d08d7
```

## Tests

```powershell
uv run python -m pytest
```

Die Tests prüfen Datumsermittlung, Scan-Abbruch, wiederverwendete
Dateiinformationen, Dateinamenskollisionen, Kopierfehler, Cache-Verhalten,
UI-Thread-Zustellung sowie den Ablauf Scan → Vorschau → Kopieren mit echten
Tkinter-Widgets. Für die GUI-Tests wird eine grafische Sitzung benötigt.
