# ADR-0024: Fortsetzbarer Abgleich nach Nutzerentscheidungen

- **Status:** Akzeptiert
- **Erfasst am / Entscheidungsdatum:** 2026-10-28
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0016](0016-namensvorschlaege-aus-referenzen.md), [0017](0017-kuratierter-referenzdatensatz.md), [0021](0021-uninteressierte-gesichtsgruppen.md)
- **Löst ab:** [ADR-0017](0017-kuratierter-referenzdatensatz.md)

## Kontext

Vollständiger Neuaufbau für alle Personen plus globale Vektorlisten nach jeder
Benennung blockieren interaktive Arbeit und vervielfachen den Speicherbedarf.

## Entscheidung

Bestätigte, hochwertige Referenzen, Pins, Duplikat-/Ausreißerfilter,
Jahresverteilung und reproduzierbarer Validierungssplit bleiben bestehen.
Der deterministische Neuaufbau betrifft bei UI-Änderungen nur schmutzige Personen.
Er betrachtet höchstens 2048 nach Jahr und Qualität verteilte Kandidaten plus
explizite Pins. Die Referenzobergrenze bleibt wirksam; Auto-Labels lernen nicht.

UI-Benennung, Ignorieren und Undo speichern ihre Entscheidung zuerst.
SQLite hält dirty Personen und einen generationsgebundenen Foto-Cursor für
nachfolgende Arbeit. Ignorieren ohne bestätigte Referenzen startet keinen
unnötigen globalen Abgleich neu. Bestätigungsänderungen registrieren diese Arbeit zusätzlich
über Trigger in derselben Transaktion. Änderungen setzen den Cursor zurück:
Neue Referenzen können jedes Foto beeinflussen, weshalb kein unbewiesener
Filter potenzielle neue Treffer ausschließt.

Worker erledigen eine Person oder 32 Fotos je Schritt. Nutzeraktionen haben
zwischen Schritten Vorrang. Abgleich liest nur den aktuellen Foto-Batch und
streamt Referenzen in Blöcken zu 256 Vektoren. Exakte Top-3-Personenscores,
Foto-Optimierung, Ablehnungen, Alter und Ko-Okkurrenz bleiben erhalten.
Fortschritt und Ergebnisse eines Matching-Schritts werden zusammen bestätigt.
Abbruch und Neustart erhalten unerledigte Arbeit; direkte Core-Aufrufe bleiben
standardmäßig synchron.

## Alternativen

Ein globaler Neuaufbau ignoriert die lokale Ursache der Referenzänderung.
Nur dieselben Fotos neu abzugleichen würde neue Referenztreffer verlieren.
Ein Netz-Finetuning oder geänderte Scores sind dafür nicht erforderlich.

## Folgen und Grenzen

Maschinenvorschläge können bis zum Abschluss der Hintergrundarbeit veraltet sein.
Nutzerlabels werden nie vom Worker überschrieben. Große Pin-Mengen und viele
verschiedene Personen kosten weiterhin Rechenzeit. Der Kandidatenpool ändert
die Auswahl bei sehr großen Personensätzen; echte Präzision bleibt separat
zu evaluieren. Der SQLite-Export bleibt reproduzierbar aus der aktuellen Auswahl.

## Bei Weiterentwicklung beachten

Keine Auto-Labels als Referenzen übernehmen. Top-3-Scores und gemeinsame
Fotoentscheidung bewahren. Cursor nach jeder Referenzänderung neu starten.
Sitzungs-Undo bleibt von abgeleiteten Maschinenvorschlägen unabhängig.

## Belege und Validierung

[Streaming-Abgleich](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_matching.py),
[Auswahl/Queue](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_service.py), [Benennung/Undo](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/core/face_naming.py),
[Worker](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/ui/face_controller.py),
[Wiederanlauf, Abbruch und Scorevergleich](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_collection_scaling.py),
[Erkennungsregressionen](https://github.com/hggerdd/foto-importer/blob/32fbe4761b47f637a79505d783c94a2969ebc599/tests/core/test_faces.py).

