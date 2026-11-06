# ADR-0051: Trainierter Namensklassifikator für die Top-3-Vorschläge

- **Status:** Akzeptiert
- **Erfasst am und entschieden am:** 2026-11-06
- **Art:** Neue Entscheidung
- **Bezieht sich auf:** [0016](0016-namensvorschlaege-aus-referenzen.md), [0024](0024-fortsetzbarer-gesichtsabgleich.md), [0040](0040-reproduzierbarer-rust-validierungssplit.md), [0048](0048-erkennungsablauf-im-personenbereich.md)

## Kontext

Namensvorschläge beruhten auf dem Mittel der drei besten Kosinus-Ähnlichkeiten
zu bestätigten Referenzen (0016). Gewünscht sind für ein neues Gesicht die drei
passendsten Personen als Schaltflächen zur schnellen Auswahl. ADR 0016 verlangt
für neue Klassifikatoren Begründung und Evaluation.

## Entscheidung

Aus den Referenzen (`split='ref'`) jedes Embedding-Modells wird eine multinomiale
logistische Regression (Softmax, L2-Regularisierung, Adam, 250 Schritte,
Nullstart, personengleiche Gewichtung) auf normalisierten Vektoren trainiert.
Das Training ist deterministisch, braucht keine neue Abhängigkeit und läuft im
Referenz-Worker (`recognition::rebuild`), nach Bestätigungen automatisch für
geänderte Personen. Personen mit weniger als zwei Referenzen und Sammlungen mit
weniger als zwei Personen erhalten kein Modell; ein veraltetes Modell wird dann entfernt.

Das Modell liegt als JSON in `face_settings` (`rust.classifier:<Embedder>`).
Dadurch ist keine Schemamigration nötig (vgl. 0042).

Der zurückgehaltene Split (`split='val'`) fließt nicht ins Training ein. Auf
ihm werden Top-1 und Top-3 des Modells und des bisherigen Ähnlichkeitsvergleichs
gemessen und im Personenbereich angezeigt.

Das Namensfenster zeigt die drei besten Personen des Modells mit Prozentwert.
Die Kandidaten kommen weiterhin aus dem Ähnlichkeitsvergleich mit allen
Ausschlüssen (Ablehnungen, Person schon im Foto bestätigt, Qualität); das
Modell sortiert nur und normiert über die erlaubten Personen. Ohne Modell oder
Vektor gilt der bisherige Vergleich mit sechs Schaltflächen.

Automatische Zuordnung (`rematch`, Schwellen) bleibt unverändert.

## Alternativen

Ein Netz-Finetuning oder ein MLP braucht deutlich mehr Daten und Rechenzeit.
Die Regression ist auf einigen Dutzend Beispielen je Person stabil und
nachvollziehbar. Den Klassifikator auch für die Auto-Zuordnung zu nutzen, wurde
verschoben, bis Messungen auf echten Sammlungen vorliegen.

## Folgen und Grenzen

Softmax-Prozentwerte sind nicht kalibriert und kennen keine „unbekannte"
Person; das Modell wählt immer unter den bekannten Personen. Nach neuen
Personen ist das Modell bis zum nächsten Referenzlauf unvollständig; diese
Personen fehlen dann in den Modellvorschlägen. Die Messung auf dem Split ist
nur so repräsentativ wie dieser. Echte Präzision auf privaten Sammlungen ist
**nicht gemessen**; Tests nutzen synthetische Cluster.

## Bei Weiterentwicklung beachten

Validierungsbeispiele nie ins Training aufnehmen. Auto-Labels nicht als
Trainingsdaten verwenden (0016). Vor Nutzung in der Auto-Zuordnung unbekannte
Personen als Negativbeispiele evaluieren.

## Belege und Validierung

[Klassifikator](../src/core/classifier.rs), [Vorschläge](../src/core/recognition.rs),
[Tests](../tests/classifier.rs).
