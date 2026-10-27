# Spezifikation: Gesichtserkennung für die Foto-Import-App

> Zielgruppe: Coding-Agent (Claude Code / Codex). Dieses Dokument beschreibt Ziel, Architektur und Lösungsansatz. Vor der Umsetzung die bestehende Codebasis lesen und Technologie-Entscheidungen (Frontend-Framework, Job-System, Pfade) daran anpassen. Abweichungen begründen.

---

## 1. Ziel

Die App soll in allen Bildern der Sammlung **Gesichter erkennen, sie Personen zuordnen und aus Korrekturen lernen**.

- Gesichter in vorhandenen und neu importierten Bildern finden.
- Für jedes Gesicht ein Embedding berechnen.
- Unbekannte Gesichter **clustern**, damit der Nutzer ganze Gruppen auf einmal benennen kann.
- Neue Bilder beim Import **schnell automatisch zuordnen** bzw. Vorschläge machen.
- Aus Bestätigungen und Ablehnungen **lernen**.
- **Personen sind nur IDs.** Name, Geburtsjahr usw. sind Attribute.
- Beziehungen zwischen Personen als **Graph** abbilden, z. B. Anna → Elternteil von → Otto.
- **Modelle sind austauschbar** und im Frontend auswählbar.
- Alles läuft **lokal, ohne GPU, ohne Cloud**.

### Nicht-Ziele (vorerst)
- Keine Echtzeit- oder Videoerkennung.
- Kein Schreiben in Lightroom-Kataloge.
- Keine Weitergabe biometrischer Daten nach außen.

---

## 2. Kontext und bestehende Infrastruktur

- Im Wurzelordner der Sammlung liegt eine SQLite-Datei (`.myfotomanager`) mit einem Dateiindex. Erwartet wird eine Tabelle `files(id, relpath, name, ext, size, mtime, ...)`. Die Gesichtsfunktion **erweitert diese Datenbank** um eigene Tabellen und referenziert `files.id`.
- Pfade sind **relativ zum Sammlungs-Wurzelordner**.
- RAW-Dateien (NEF/DNG) haben ein eingebettetes JPEG-Vorschaubild. **Dieses für die Erkennung nutzen**, nicht das RAW entwickeln (z. B. `exiftool -b -JpgFromRaw` oder `rawpy`).
- JPG und NEF desselben Fotos sind Paare. **Ein Foto nur einmal analysieren** und das Ergebnis dem Paar zuordnen, siehe Abschnitt 9.

---

## 3. Architekturüberblick

```
            ┌─────────────────────────────────────────┐
Frontend ── │ API: Modelle wählen, Cluster benennen,  │
            │ Vorschläge bestätigen, Personen/Graph   │
            └───────────────┬─────────────────────────┘
                            │
            ┌───────────────▼─────────────────────────┐
            │ FaceService                             │
            │  ├─ ModelRegistry (Detector, Embedder)  │
            │  ├─ Pipeline (detect → align → embed)   │
            │  ├─ Matcher (kNN, Schwellen, Priors)    │
            │  ├─ Clusterer (HDBSCAN o. ä.)           │
            │  └─ PersonGraph                         │
            └───────────────┬─────────────────────────┘
                            │
            ┌───────────────▼─────────────────────────┐
            │ Job-Queue (Hintergrund / über Nacht)    │
            │ SQLite (.myfotomanager)                 │
            └─────────────────────────────────────────┘
```

**Detektor und Embedder sind getrennte, austauschbare Komponenten.** Sie haben unterschiedliche Lebenszyklen, siehe Abschnitt 8.

---

## 4. Modell-Abstraktion (Kernanforderung: Flexibilität)

### 4.1 Interfaces

```python
class FaceDetector(Protocol):
    id: str                 # z. B. "scrfd_10g"
    version: str
    def detect(self, img_rgb: np.ndarray) -> list[Detection]: ...
    # Detection: bbox (x1,y1,x2,y2, in Original-Pixeln), score, landmarks (5 Punkte)

class FaceEmbedder(Protocol):
    id: str                 # z. B. "arcface_r100_buffalo_l", "adaface_ir101"
    version: str
    dim: int                # z. B. 512
    metric: Literal["cosine"]
    input_spec: InputSpec   # Größe (112x112), Farbreihenfolge (RGB/BGR),
                            # Normalisierung, Alignment-Template
    def embed(self, aligned_faces: np.ndarray) -> np.ndarray: ...
    # liefert L2-normalisierte float32-Vektoren [N, dim]
```

### 4.2 Registry und Manifest

Jedes Modell wird über ein **Manifest** beschrieben (JSON oder YAML), nicht im Code fest verdrahtet:

```json
{
  "id": "arcface_r100_buffalo_l",
  "kind": "embedder",
  "version": "1.0",
  "runtime": "onnxruntime",
  "file": "models/buffalo_l/w600k_r50.onnx",
  "sha256": "...",
  "dim": 512,
  "input": {"size": [112, 112], "color": "RGB", "mean": 127.5, "std": 127.5, "align": "arcface_5pt"},
  "thresholds": {"auto": 0.55, "suggest": 0.40},
  "license": "non-commercial research",
  "source_url": "..."
}
```

- Die Registry scannt beim Start einen Modellordner und listet verfügbare Modelle.
- **API-Endpunkt** `GET /face/models`: liefert Detektoren und Embedder mit Name, Lizenz, Status (installiert/fehlt), Größe und Geschwindigkeitsschätzung.
- **Frontend:** Auswahl des aktiven Detektors und Embedders pro Sammlung. Die Einstellung wird in der Datenbank gespeichert (`face_settings`).
- Bevorzugte Runtime ist **ONNX Runtime**, damit PyTorch nicht Pflicht wird. PyTorch-Modelle nach ONNX exportieren. PyTorch nur optional als Adapter.
- Die Prüfsumme (sha256) beim Laden kontrollieren.

### 4.3 Startmodelle

| Rolle | Modell | Hinweis |
|---|---|---|
| Detektor | SCRFD (InsightFace) | schnell, mit Landmarks |
| Detektor (Alternative) | YuNet (OpenCV) | klein, lizenzunkritisch |
| Embedder | ArcFace `buffalo_l` (InsightFace) | Standard, gute Basis |
| Embedder | AdaFace (CVLFace) | besser bei kleinen/unscharfen Gesichtern |
| Embedder (Alternative) | SFace (OpenCV) | schwächer, lizenzunkritisch |

**Lizenz:** Viele vortrainierte Gewichte (InsightFace, AdaFace) sind nur für nicht-kommerzielle Nutzung freigegeben. Die Lizenz im Manifest und in der UI anzeigen.

**Achtung, Farbreihenfolge:** InsightFace-Modelle erwarten RGB, das AdaFace-Original erwartet BGR. Dies **nur über `input_spec` steuern**, nie im Pipeline-Code hart kodieren. Diese Fehlerquelle ist häufig und schwer zu bemerken, weil das Modell trotzdem plausible, aber schlechtere Vektoren liefert.

---

## 5. Datenmodell (SQLite)

```sql
-- Einstellungen
face_settings(key TEXT PRIMARY KEY, value TEXT);   -- active_detector, active_embedder

-- Ein Foto = eine Analyseeinheit (JPG/NEF/DNG-Paar)
photos(id INTEGER PRIMARY KEY, identity_key TEXT UNIQUE);  -- z. B. serial+shutter_count
photo_files(photo_id, file_id, role);                      -- role: preview_source, raw, jpg, dng

-- Erkennungen (abhängig vom Detektor)
faces(
  id INTEGER PRIMARY KEY,
  photo_id INTEGER NOT NULL,
  detector_id TEXT NOT NULL,
  bbox_x1 REAL, bbox_y1 REAL, bbox_x2 REAL, bbox_y2 REAL,   -- normalisiert 0..1
  landmarks BLOB,
  det_score REAL,
  quality REAL,              -- eigene Qualitätsbewertung (Größe, Schärfe, Pose)
  person_id INTEGER NULL,
  assignment TEXT,           -- 'user' | 'auto' | 'suggested' | NULL
  confidence REAL,
  assigned_at TEXT
);

-- Embeddings (abhängig vom Embedder), mehrere Modelle parallel möglich
face_embeddings(
  face_id INTEGER, embedder_id TEXT, dim INTEGER, vec BLOB,  -- float32, L2-normalisiert
  PRIMARY KEY (face_id, embedder_id)
);

face_rejections(face_id INTEGER, person_id INTEGER, PRIMARY KEY (face_id, person_id));

-- Kuratierter, balancierter Referenz-/Trainingsdatensatz (siehe Abschnitt 14)
training_set(
  person_id INTEGER, face_id INTEGER, embedder_id TEXT,
  split TEXT,                -- 'ref' | 'val'
  selected_at TEXT, reason TEXT,   -- 'fps' | 'year_bucket' | 'manual_pin'
  pinned INTEGER DEFAULT 0,  -- vom Nutzer fest aufgenommen
  PRIMARY KEY (person_id, face_id, embedder_id)
);

-- Personen
persons(id INTEGER PRIMARY KEY, created_at TEXT, merged_into INTEGER NULL);
person_attrs(person_id INTEGER, key TEXT, value TEXT);  -- name, birth_year, sex, note

-- Graph
relation_types(id INTEGER PRIMARY KEY, name TEXT, inverse_id INTEGER NULL, symmetric INTEGER);
relations(from_person INTEGER, type_id INTEGER, to_person INTEGER,
          PRIMARY KEY (from_person, type_id, to_person));

-- Verarbeitungsstand
face_jobs(photo_id INTEGER, stage TEXT, model_id TEXT, status TEXT, error TEXT, updated_at TEXT);
```

**Prinzipien:**
- Die Zuordnung zu einer Person hängt am **Gesicht** (`faces.person_id`), nicht am Embedding. Dadurch überlebt sie einen Embedder-Wechsel.
- Bounding Boxes **normalisiert speichern** (0..1). So sind sie unabhängig davon, ob aus Vorschau oder Original erkannt wurde.
- Personen werden nie hart gelöscht, wenn sie zusammengeführt werden: `merged_into` setzen.
- Migrationen versionieren (`PRAGMA user_version`).

---

## 6. Pipeline

1. **Bildquelle:** eingebettetes JPEG, auf maximal etwa 1600 px lange Kante skalieren. **EXIF-Orientierung anwenden.** Sonst werden hochkant aufgenommene Gesichter liegend nicht erkannt.
2. **Detektion:** aktiver Detektor, Mindestscore konfigurierbar.
3. **Qualitätsfilter:** Gesichter unter etwa 40 px Breite (auf das Original bezogen), mit niedrigem Score, starker Unschärfe oder extremer Pose bekommen niedrige `quality`. Sie werden **gespeichert, aber nicht automatisch zugeordnet** und nicht als Referenz genutzt.
4. **Alignment:** 5-Punkt-Ähnlichkeitstransformation auf das Template des Embedders, gemäß `input_spec`.
5. **Embedding:** in Batches, z. B. 32 Gesichter, normalisiert als float32-BLOB speichern.
6. **Matching:** siehe Abschnitt 7.

**Performance:**
- Gesichter bildübergreifend batchen. Modelle einmal laden und im Speicher halten.
- Dekodierung und Inferenz über eine Producer/Consumer-Queue entkoppeln: Bilder in Threads laden, Inferenz in einer ONNX-Session mit `intra_op_num_threads` auf die Kernzahl gesetzt.
- Datenbank-Schreibvorgänge bündeln (eine Transaktion pro Batch, WAL-Modus).
- Fortsetzbar arbeiten: Der Status in `face_jobs` erlaubt Abbruch und Wiederaufnahme. Bereits verarbeitete Fotos überspringen.

---

## 7. Erkennen, Clustern, Lernen

### 7.1 Matching neuer Gesichter
- Referenzmenge: der **kuratierte Datensatz** (`training_set`, split `ref`) für den aktiven Embedder, siehe Abschnitt 14. Er ist balanciert, frei von Beinahe-Duplikaten und klein, also schnell. Fallback, solange er noch nicht aufgebaut ist: alle `user`-Gesichter hoher Qualität.
- **kNN statt Mittelwert pro Person.** Menschen verändern sich über Jahre (Kinder, Bart, Brille), mehrere Beispiele bilden das ab. Score: z. B. Mittel der Top-3-Ähnlichkeiten pro Person.
- Ablehnungen (`face_rejections`) beachten.
- **Drei Zonen**, Schwellen aus dem Manifest, im UI überschreibbar:
  - ≥ `auto` → automatisch zuordnen (`assignment='auto'`)
  - ≥ `suggest` → Vorschlag (`assignment='suggested'`)
  - darunter → unbekannt
- **Constraint:** Eine Person kommt pro Foto höchstens einmal vor. Bei Konflikten die Zuordnung pro Foto optimieren, z. B. mit dem ungarischen Algorithmus.
- Suche: numpy-Brute-Force über eine im Speicher gehaltene Matrix reicht bis weit über 100.000 Gesichter. Die Suche hinter ein Interface legen, damit FAISS oder sqlite-vec später austauschbar sind.

### 7.2 Clustering unbekannter Gesichter
- HDBSCAN (oder agglomerativ mit Kosinus-Schwelle) über unbekannte Gesichter guter Qualität.
- Cluster sind **temporär und dürfen sich ändern**. Nur Nutzerentscheidungen sind dauerhaft.
- Ein Hintergrundjob läuft periodisch neu, z. B. nachts.

### 7.3 Lernen aus Nutzeraktionen
| Aktion | Wirkung |
|---|---|
| Cluster benennen | alle Gesichter → `user`, neue oder bestehende Person |
| Vorschlag bestätigen | → `user` |
| Vorschlag ablehnen | Eintrag in `face_rejections`, Gesicht wird unbekannt |
| Gesicht verschieben | alte Person als Ablehnung, neue Person `user` |
| Personen zusammenführen | `merged_into`, Gesichter umhängen |

- Nach Nutzeraktionen betroffene `auto`- und `suggested`-Zuordnungen **neu bewerten**. Nutzerzuordnungen (`user`) werden nie automatisch überschrieben.

### 7.4 Priors aus Graph und Metadaten (Phase 2)
- **Alters-Plausibilität:** Liegt das Aufnahmedatum vor dem Geburtsjahr der Person, ist sie ausgeschlossen.
- **Ko-Okkurrenz:** Personen, die oft gemeinsam auf Fotos sind, erhöhen die Wahrscheinlichkeit von Vorschlägen im Graubereich.
- Priors **nur im Graubereich** anwenden, nie eine klare Embedding-Entscheidung umdrehen.

---

## 8. Modellwechsel (schwieriges Thema)

**Embedder-Wechsel:**
- Embeddings verschiedener Modelle sind **nicht vergleichbar**, auch nicht bei gleicher Dimension.
- Beim Wechsel einen Hintergrundjob starten, der für alle Gesichter Embeddings mit dem neuen Modell berechnet. Die alten bleiben erhalten, damit ein Zurückwechseln sofort möglich ist.
- Personen-Zuordnungen bleiben gültig, weil sie am Gesicht hängen.
- Bis der Job fertig ist: Matching weiter mit dem alten Modell oder UI-Hinweis „Neuindizierung läuft (x %)“.
- Schwellen sind **pro Modell** und kommen aus dem Manifest.

**Detektor-Wechsel (schwieriger):**
- Ein neuer Detektor erzeugt neue Bounding Boxes, also neue Gesichter.
- Alte und neue Gesichter pro Foto per **IoU-Matching** zuordnen (z. B. IoU ≥ 0,5) und `person_id` übernehmen.
- Gesichter, die nur der alte Detektor fand und die eine Nutzerzuordnung haben, **nicht löschen**, sondern behalten und kennzeichnen.
- Empfehlung: Detektor-Wechsel in der UI als „fortgeschritten“ markieren und vorher eine Datenbank-Sicherung anlegen.

---

## 9. Weitere schwierige Themen

1. **JPG/NEF/DNG-Paare:** Pro Foto nur einmal erkennen. Identität über Seriennummer + Auslösezähler bzw. Aufnahmezeit inklusive Subsekunden. Die Bounding Box gilt für alle Dateien des Paars. Achtung: Ein JPEG kann beschnitten sein (Bildformat-Einstellung). Normalisierte Koordinaten beziehen sich deshalb immer auf die Quelle, aus der erkannt wurde.
2. **EXIF-Orientierung** und unterschiedliche Vorschaugrößen bei Umrechnung der Koordinaten.
3. **Schwellenkalibrierung:** Die Standardwerte sind nur Startpunkte. Ein Kalibrierungs-Tool bauen, das an bestätigten Paaren derselben und verschiedener Personen eine Verteilung zeigt und Schwellen vorschlägt.
4. **Kinder und Alterung:** Das größte Fehlerrisiko. kNN mit vielen Beispielen über verschiedene Jahre hilft, Alters-Priors helfen zusätzlich.
5. **Ähnliche Geschwister, Zwillinge:** häufige Verwechslungen. Lieber Vorschläge als automatische Zuordnung, also die `auto`-Schwelle konservativ setzen.
6. **Fehlerfortpflanzung:** Automatische Zuordnungen dürfen nicht unkontrolliert zu Referenzen werden, sonst „driftet“ eine Person. Standard: nur `user`-Zuordnungen als Referenz.
7. **Fotos von Fotos, Poster, Bildschirme:** Sie werden als Gesichter erkannt. Eine Option „ignorieren“ pro Gesicht vorsehen.
8. **Gelöschte oder verschobene Dateien:** Gesichter an `photos` hängen, nicht direkt an Pfaden. Beim Rescan über `identity_key` neu verknüpfen.
9. **Nebenläufigkeit:** Hintergrundjob und UI schreiben in dieselbe SQLite-Datei. WAL-Modus, kurze Transaktionen, `busy_timeout` setzen. SQLite auf einem Netzlaufwerk vermeiden.
10. **Datenschutz:** Embeddings sind biometrische Daten. Nur lokal, keine Telemetrie. Eine Funktion „Alle Gesichtsdaten löschen“ vorsehen.

---

## 10. Personen-Graph

- **Nur Grundrelationen speichern:** `elternteil_von` (Inverse: `kind_von`), `partner_von` (symmetrisch).
- **Ableiten statt speichern:** Geschwister, Großeltern, Onkel/Tante, Cousins per rekursivem CTE oder in Python. So entstehen keine Widersprüche.
- **Ausnahme:** `geschwister_von` explizit erlauben, wenn die Eltern nicht im Graphen sind. Die Ableitung muss beide Quellen zusammenführen.
- Geschlechtsspezifische Bezeichnungen (Bruder/Schwester, Mutter/Vater) **nur in der Anzeige** aus `person_attrs.sex` ableiten, nicht als eigene Relationstypen.
- **Validierung:** keine Zyklen bei `elternteil_von`, höchstens zwei Eltern (Warnung, kein harter Fehler), keine Selbstreferenz.

Beispiel Großeltern:
```sql
WITH RECURSIVE vorfahren(p, tiefe) AS (
  SELECT from_person, 1 FROM relations WHERE to_person = :person AND type_id = :eltern
  UNION ALL
  SELECT r.from_person, v.tiefe + 1
  FROM relations r JOIN vorfahren v ON r.to_person = v.p
  WHERE r.type_id = :eltern AND v.tiefe < 10)
SELECT p FROM vorfahren WHERE tiefe = 2;
```

---

## 11. API und Frontend

**Endpunkte (Vorschlag):**
- `GET /face/models`, `PUT /face/settings` (aktiver Detektor/Embedder, Schwellen)
- `POST /face/jobs/scan` (ganze Sammlung / Ordner / neue Importe), `GET /face/jobs/status`
- `GET /face/clusters` (unbekannte Gesichter gruppiert, mit Vorschaubildern)
- `POST /face/clusters/{id}/assign` (Person neu oder bestehend)
- `GET /face/suggestions`, `POST /face/faces/{id}/confirm|reject|move|ignore`
- `GET/POST /persons`, `POST /persons/{id}/merge`, `PUT /persons/{id}/attrs`
- `GET/POST/DELETE /persons/{id}/relations`, `GET /persons/{id}/derived-relations`
- `GET /persons/{id}/photos`

**UI-Ansichten:**
1. **Modell-Einstellungen:** Auswahl, Lizenzhinweis, Status der Neuindizierung.
2. **Unbekannte Personen:** Cluster als Kachelraster, benennen per Eingabe mit Autovervollständigung, einzelne Ausreißer per Klick entfernen.
3. **Vorschläge prüfen:** schnelle Bestätigung per Tastatur (Ja/Nein/Überspringen).
4. **Personenseite:** alle Fotos, Attribute, Beziehungen, abgeleitete Verwandtschaft.
5. **Import-Integration:** Nach dem Kopieren läuft die Erkennung im Hintergrund, in der Tagesansicht erscheinen Namen und Vorschläge.
6. **Gesichts-Thumbnails** aus der Bounding Box mit etwas Rand erzeugen und cachen.

---

## 12. Umsetzungsphasen

1. **Grundlage:** Schema und Migrationen, ModelRegistry mit Manifesten, ein Detektor und ein Embedder (SCRFD + ArcFace), Pipeline über eine Sammlung, fortsetzbare Jobs.
2. **Benennen:** Clustering, Cluster-UI, Personen, Zuordnungen.
3. **Erkennen:** kuratierter Datensatz (Abschnitt 14), kNN-Matching darauf, drei Zonen, Vorschlags-UI, Lernen aus Ablehnungen, Import-Integration.
4. **Flexibilität:** zweiter Embedder (AdaFace), Modellwechsel mit Neuindizierung, Kalibrierungs-Tool.
5. **Graph:** Relationen, Ableitungen, Validierung, Anzeige.
6. **Priors:** Alters-Plausibilität und Ko-Okkurrenz.

---

## 13. Tests und Akzeptanzkriterien

- **Unit-Tests:** Alignment (bekannte Landmarks → erwartete Ausgabe), Koordinatenumrechnung bei EXIF-Orientierung, Graph-Ableitungen (Geschwister, Großeltern), Zyklenerkennung.
- **Modell-Konsistenz:** Für jedes Modell im Manifest einen Test mit Referenzbildern. Zwei Bilder derselben Person müssen eine deutlich höhere Ähnlichkeit haben als zwei verschiedene. Das deckt falsche Farbreihenfolge oder Normalisierung auf.
- **Kleines Testset** mit eigenen, bekannten Fotos: Precision und Recall der automatischen Zuordnung messen. **Ziel: Precision ≥ 98 % bei `auto`.** Falsche automatische Zuordnungen sind schlimmer als fehlende.
- **Performance-Ziel:** Etwa 0,3 s pro Bild auf der CPU, inklusive Erkennung. Der Abgleich eines neuen Gesichts dauert unter 50 ms bei 100.000 Referenzen.
- **Robustheit:** Ein Job lässt sich jederzeit abbrechen und fortsetzen. Ein Modellwechsel und zurück verliert keine Nutzerzuordnung.
- **Datensatz:** Keine Person hat mehr als N Gesichter. Kein Paar innerhalb einer Person liegt über der Duplikat-Schwelle. Der Ordner ist aus der Datenbank vollständig reproduzierbar. Ein Neuaufbau mit gleichen Daten und gleichem Seed ergibt dieselbe Auswahl.

---

## 14. Kuratierter, balancierter Referenz- und Trainingsdatensatz

### 14.1 Problem
Die Verteilung in der Sammlung ist stark unausgewogen: Eine Person (der Sohn) kommt ≥ 10-mal häufiger vor als alle anderen, oft in Serien mit nahezu identischen Bildern. Wird alles als Referenz genutzt:
- dominiert diese Person das kNN-Matching, und Unbekannte werden ihr eher zugeordnet,
- tragen Beinahe-Duplikate keine Information, verzerren aber Statistiken und Schwellen,
- wird das Matching unnötig langsam.

### 14.2 Ziel
Ein fester, reproduzierbarer Datensatz pro Person:
- **höchstens N Gesichter** (Standard 100, konfigurierbar),
- **maximal unterschiedlich**: Pose, Licht, Alter, Mimik,
- nur **verlässliche Labels**,
- **inkrementell erweiterbar** um neue Personen und neue Gesichter,
- als **Ordner exportiert**, benannt nach Personen-ID, nie nach Namen.

### 14.3 Auswahl-Algorithmus (pro Person, pro Embedder)

1. **Kandidaten:** nur `assignment='user'`, `quality` ≥ Schwelle, nicht `ignored`. Keine `auto`-Zuordnungen, sonst verstärken sich Fehler selbst.
2. **Ausreißer entfernen (wichtig):** Medoid der Person bestimmen und Kandidaten mit sehr geringer Ähnlichkeit zum Medoid **und** zu ihren k nächsten Nachbarn ausschließen. Solche Ausreißer sind meist falsch beschriftet, verdeckt oder fehlerhaft ausgeschnitten. Stattdessen in einer UI-Liste „Bitte prüfen“ anzeigen.
   **Hintergrund:** Eine Auswahl nach maximalem Abstand bevorzugt genau diese Ausreißer. Ohne diesen Schritt füllt sich der Datensatz mit Fehlern.
3. **Beinahe-Duplikate entfernen:** Kandidaten mit Kosinus-Ähnlichkeit über einer Duplikat-Schwelle (Startwert ~0,92, modellabhängig, gehört ins Manifest) zusammenfassen und je Gruppe das Gesicht mit der höchsten `quality` behalten. Zusätzlich günstig: gleiche Serie, also Aufnahmezeit < 2 s auseinander, wie Duplikate behandeln.
4. **Zeitliche Streuung sichern:** Kandidaten nach Aufnahmejahr in Buckets einteilen und pro Bucket ein Mindestkontingent reservieren, z. B. `max(3, N / Anzahl_Jahre)`. Das ist für Kinder entscheidend, damit alle Altersstufen vertreten sind.
5. **Diversitätsauswahl:** Die restlichen Plätze per **Farthest-Point-Sampling (greedy k-Center)** füllen. Start mit dem Medoid, dann wiederholt das Gesicht hinzufügen, dessen Abstand zum nächsten bereits gewählten Gesicht am größten ist. Dabei nach `quality` gewichten, z. B. `abstand * (0.5 + 0.5 * quality)`.
6. **Vom Nutzer fixierte Gesichter** (`pinned`) werden immer aufgenommen und zählen auf N.
7. **Validierungs-Split:** Etwa 20 % der gewählten Gesichter, mindestens 2, nach Möglichkeit aus anderen Aufnahmetagen, werden als `split='val'` markiert. Sie dienen nicht als Referenz, sondern zur Messung von Genauigkeit und zur **Kalibrierung der Schwellen** (Abschnitt 9, Punkt 3). Weil der Datensatz balanciert ist, sind die Messwerte aussagekräftig.

Komplexität: Farthest-Point-Sampling ist O(N · M) pro Person (M Kandidaten). Für einige tausend Gesichter dauert das Millisekunden. Kein Approximationsindex nötig.

### 14.4 Inkrementelle Aktualisierung
- **Neue Person:** Sobald sie genug bestätigte Gesichter hat (mindestens etwa 3), wird ihr Datensatz komplett nach 14.3 gebaut.
- **Neues bestätigtes Gesicht, Person unter N:** aufnehmen, wenn es kein Beinahe-Duplikat ist.
- **Person bei N:** Tauschkandidat ist das nicht fixierte Gesicht mit dem kleinsten Abstand zu seinem nächsten Nachbarn im Set. Tauschen, wenn das neue Gesicht einen größeren Mindestabstand zum Set hätte. Mindestkontingente der Jahres-Buckets beachten.
- **Ablehnen, Verschieben, Zusammenführen, Ignorieren:** betroffene Einträge sofort entfernen bzw. umhängen und die Lücke beim nächsten Lauf auffüllen.
- **Nächtlicher Vollneuaufbau** mit festem Zufalls-Seed. Er korrigiert die Drift der inkrementellen Schritte.
- **Embedder-Wechsel:** Der Datensatz wird pro Embedder geführt, weil die Abstände modellabhängig sind. Nach der Neuindizierung (Abschnitt 8) automatisch neu aufbauen.

### 14.5 Ordnerstruktur (Export)

```
<Sammlung>/.myfotomanager_faces/
  manifest.json                # Erstellzeit, embedder_id, N, Schwellen, Seed, Version
  persons/
    000017/
      ref/
        f_0004521.jpg          # Gesichtsausschnitt
        f_0004521.json         # face_id, photo_id, relpath, bbox, landmarks, quality, Datum
      val/
        f_0009310.jpg
        f_0009310.json
    000023/
      ...
```

- **Die Datenbank ist die Quelle der Wahrheit,** der Ordner ein reproduzierbarer Export. Er darf jederzeit gelöscht und neu erzeugt werden. Änderungen im Ordner werden **nicht** zurückgelesen. Nur IDs verwenden, keine Namen, auch nicht in Dateinamen oder JSON.
- **Ausschnitte nicht vorausgerichtet speichern,** sondern mit Rand (etwa 1,6-fache Bounding Box, Kantenlänge 256 px) plus Landmarks im JSON. So kann jedes Modell mit seinem eigenen Alignment-Template arbeiten, siehe `input_spec`.
- **Synchronisation** per Abgleich DB ↔ Ordner: fehlende Dateien schreiben, überzählige löschen. Atomar schreiben, also erst in eine temporäre Datei und dann umbenennen.
- **Datenschutz:** Der Ordner enthält Gesichter, also biometrisch relevante Daten. Er ist versteckt, wird aus Backups optional ausgeschlossen und von „Alle Gesichtsdaten löschen“ mit entfernt.

### 14.6 Wofür der Datensatz genutzt wird
1. **Referenzmenge für das Matching** (Abschnitt 7.1): balanciert und schnell.
2. **Schwellenkalibrierung und Evaluation** über den `val`-Split, pro Modell. Das Kalibrierungs-Tool zeigt Precision und Recall pro Person.
3. **Optional, Phase 6+:** Training eines **leichten Klassifikators über den Embeddings**, etwa logistische Regression oder ein kleines MLP. Unbekannte, nicht zugeordnete Gesichter dienen als Negativklasse, damit die Erkennung offen bleibt (open-set) und nicht jedes Gesicht einer bekannten Person zuordnet.
   **Nicht empfohlen:** Feintuning des Embedding-Netzes selbst. Mit einigen hundert Identitäten droht Overfitting, und die allgemeine Erkennungsqualität sinkt. Falls gewünscht, dann nur ein kleiner Adapter auf eingefrorenen Embeddings und immer gegen den `val`-Split messen.

### 14.7 UI
- Pro Person: Ansicht des Datensatzes (ref/val), Anzahl, Abdeckung der Jahre, Gesichter fixieren oder entfernen.
- Liste „Bitte prüfen“: Ausreißer aus 14.3, Schritt 2.
- Einstellungen: N, Duplikat-Schwelle, Validierungsanteil, Button „Datensatz neu aufbauen“.
