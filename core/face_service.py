"""Local face workflow, resumable jobs and user decisions on stable photo IDs."""
from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import threading

import cv2
import numpy as np

from config.settings import FACE_BATCH_SIZE, FACE_MAX_EDGE, FACE_MODEL_DIR
from core.collection_index import CollectionIndex
from core.face_algorithms import cluster_unknown, match_photo, select_references
from core.face_images import crop_face, load_image, safe_path
from core.face_models import ModelRegistry, align_face, normalize
from core.face_store import save_settings, settings
from core.person_graph import PersonGraph


def timestamp() -> str:
    return datetime.now(timezone.utc).isoformat()


def iou(a, b) -> float:
    a, b = np.array(a), np.array(b)
    intersection = np.maximum(0, np.minimum(a[2:], b[2:]) - np.maximum(a[:2], b[:2])).prod()
    union = np.maximum(0, a[2:] - a[:2]).prod() + np.maximum(0, b[2:] - b[:2]).prod() - intersection
    return float(intersection / union) if union else 0.0


class FaceService:
    """Use only on the owning collection worker; UI callbacks receive plain data."""

    def __init__(self, index: CollectionIndex, registry: ModelRegistry | None = None) -> None:
        self.index, self.db = index, index.db
        self.registry = registry or ModelRegistry(FACE_MODEL_DIR)
        self.options = settings(self.db) if self.db.execute("PRAGMA user_version").fetchone()[0] >= 2 else {}
        self.graph = PersonGraph(self.db)

    @property
    def embedder_key(self) -> str:
        return self.registry.key(self.options["active_embedder"])

    @property
    def detector_key(self) -> str:
        return self.registry.key(self.options["active_detector"])

    def configure(self, **values) -> None:
        """Persist collection choices; snapshot SQLite before a detector switch."""
        self.index._writable()
        allowed = set(self.options) | {"auto", "suggest", "duplicate", "cluster", "outlier", "age_prior", "cooccurrence"}
        if set(values) - allowed:
            raise ValueError("Unbekannte Gesichtseinstellung")
        combined = self.options | values
        for key, kind in (("active_detector", "detector"), ("active_embedder", "embedder")):
            if combined[key] not in self.registry.models or self.registry.models[combined[key]]["kind"] != kind:
                raise ValueError(f"Ungültiges Modell: {combined[key]}")
        thresholds = self.registry.models[combined["active_embedder"]]["thresholds"]
        auto, suggest = combined.get("auto", thresholds["auto"]), combined.get("suggest", thresholds["suggest"])
        if not -1 <= suggest <= auto <= 1:
            raise ValueError("Es muss -1 ≤ Vorschlag ≤ Automatisch ≤ 1 gelten")
        if not 1 <= int(combined["reference_limit"]) <= 10000 or not 0 <= combined["validation_fraction"] < 1:
            raise ValueError("Ungültige Datensatzgröße oder Validierungsquote")
        for key in ("min_quality", "min_score"):
            if not 0 <= combined[key] <= 1:
                raise ValueError(f"Ungültiger Wert: {key}")
        for key in ("duplicate", "cluster", "outlier"):
            if key in combined and not -1 <= combined[key] <= 1:
                raise ValueError(f"Ungültige Schwelle: {key}")
        if combined["active_detector"] != self.options["active_detector"]:
            backup = self.index.root / f".myfotomanager.before-detector-{datetime.now().strftime('%Y%m%d-%H%M%S-%f')}.bak"
            with sqlite3.connect(backup) as destination:
                self.db.backup(destination)
        # Overrides belong to a model, not to a collection-wide embedding space.
        overrides = self.options.get("threshold_overrides", {})
        if combined["active_embedder"] != self.options["active_embedder"]:
            overrides[self.options["active_embedder"]] = {k: self.options[k] for k in ("auto", "suggest", "duplicate", "cluster", "outlier") if k in self.options}
            for key in ("auto", "suggest", "duplicate", "cluster", "outlier"):
                if key not in values:
                    combined.pop(key, None)
            combined.update(overrides.get(combined["active_embedder"], {}) | values)
            combined["threshold_overrides"] = overrides
        with self.db:
            self.db.execute("DELETE FROM face_settings")
            save_settings(self.db, combined)
        self.options = combined

    def persons(self) -> list[dict]:
        result = []
        for row in self.db.execute("SELECT * FROM persons WHERE merged_into IS NULL ORDER BY id"):
            person = dict(row)
            person["attrs"] = dict(self.db.execute("SELECT key,value FROM person_attrs WHERE person_id=?", (row["id"],)))
            person["count"] = self.db.execute("SELECT count(*) FROM faces WHERE person_id=? AND ignored=0", (row["id"],)).fetchone()[0]
            result.append(person)
        return result

    def create_person(self, attrs: dict | None = None) -> int:
        self.index._writable()
        with self.db:
            person = self.db.execute("INSERT INTO persons DEFAULT VALUES").lastrowid
            self.set_attrs(person, attrs or {})
        return person

    def set_attrs(self, person: int, attrs: dict) -> None:
        self.index._writable()
        self.graph._person(person)
        if "birth_year" in attrs and attrs["birth_year"]:
            year = int(attrs["birth_year"])
            if not 1 <= year <= datetime.now().year:
                raise ValueError("Ungültiges Geburtsjahr")
        with self.db:
            self.db.executemany("INSERT OR REPLACE INTO person_attrs VALUES(?,?,?)",
                                [(person, key, str(value)) for key, value in attrs.items()])

    def face_rows(self, where: str = "1", params: tuple = ()) -> list[dict]:
        # where is private SQL from service methods, never user input.
        return [dict(row) for row in self.db.execute(f"""
            SELECT f.*, coalesce(s.relpath,f.source_relpath) AS relpath,
                   coalesce(m.orientation,1) AS orientation
            FROM faces f LEFT JOIN files s ON s.id=f.source_file_id
            LEFT JOIN meta m ON m.file_id=f.source_file_id WHERE {where} ORDER BY f.id
        """, params)]

    def vectors(self, where: str = "1", params: tuple = ()) -> list[dict]:
        rows = self.face_rows(where, params)
        embeddings = {r[0]: np.frombuffer(r[1], dtype="<f4").copy() for r in self.db.execute(
            "SELECT face_id,vec FROM face_embeddings WHERE embedder_id=?", (self.embedder_key,))}
        return [r | {"vector": embeddings[r["id"]]} for r in rows if r["id"] in embeddings]

    def _source(self, photo: int) -> dict | None:
        row = self.db.execute("""SELECT f.*,m.orientation,m.dt_original,m.width,m.height
            FROM files f LEFT JOIN meta m ON m.file_id=f.id WHERE photo_id=? AND status='present'
            AND ext IN ('jpg','jpeg','png','tif','tiff','nef','nrw','dng')
            ORDER BY CASE WHEN ext IN ('jpg','jpeg') THEN 0 WHEN ext IN ('png','tif','tiff') THEN 1 ELSE 2 END,id LIMIT 1""", (photo,)).fetchone()
        return dict(row) if row else None

    def _load(self, row: dict):
        path = safe_path(self.index.root, row["relpath"])
        image = load_image(path, exiftool=self.index.settings["exiftool"], orientation=row.get("orientation") or 1)
        return np.asarray(image)

    def _fingerprint(self, source: dict) -> str:
        path = safe_path(self.index.root, source["relpath"])
        stat = path.stat()
        data = [source["id"], stat.st_size, stat.st_mtime_ns, stat.st_ctime_ns,
                source.get("orientation"), source.get("dt_original")]
        return hashlib.sha256(json.dumps(data).encode()).hexdigest()

    def _job(self, photo: int, stage: str, model: str, status: str,
             fingerprint: str, error: str | None = None) -> None:
        self.db.execute("INSERT OR REPLACE INTO face_jobs VALUES(?,?,?,?,?,?,?)",
                        (photo, stage, model, status, error, timestamp(), fingerprint))

    def scan(self, scope: str = "", cancel: threading.Event | None = None,
             progress=None, scheduler=None) -> dict:
        """Decode in bounded producer threads; infer and commit batches on this worker.

        An interrupted in-memory batch is repeated. Completed DB batches are
        skipped on resume. Changing the model, image or orientation invalidates
        the appropriate stage, including photos containing zero faces.
        """
        self.index._writable()
        cancel = cancel or threading.Event()
        scope = self.index._scope(scope)
        detector = self.registry.load(self.options["active_detector"])
        embedder = self.registry.load(self.options["active_embedder"])
        sources = [self._source(r[0]) for r in self.db.execute(
            "SELECT DISTINCT photo_id FROM files WHERE status='present' AND photo_id IS NOT NULL ORDER BY photo_id")]
        sources = [s for s in sources if s and (not scope or s["relpath"].startswith(scope + "/"))]
        stats = {"done": 0, "skipped": 0, "errors": [], "total": len(sources), "cancelled": False}
        for offset in range(0, len(sources), 8):
            if cancel.is_set():
                break
            if scheduler:
                try:
                    scheduler.checkpoint(cancel)
                except InterruptedError:
                    break
            todo = []
            for source in sources[offset:offset + 8]:
                try:
                    fingerprint = self._fingerprint(source)
                    job = self.db.execute("SELECT status,fingerprint FROM face_jobs WHERE photo_id=? AND stage='embed' AND model_id=?",
                                          (source["photo_id"], self.detector_key + "/" + self.embedder_key)).fetchone()
                    if job and job["status"] == "done" and job["fingerprint"] == fingerprint:
                        stats["skipped"] += 1
                    else:
                        todo.append((source, fingerprint))
                except Exception as exc:
                    stats["errors"].append(f"{source['relpath']}: {exc}")
            # Only eight pending decodes: no unbounded executor.map prefetch.
            pending = []
            with ThreadPoolExecutor(max_workers=2, thread_name_prefix="FaceDecode") as pool:
                futures = [(s, fp, pool.submit(self._load, s)) for s, fp in todo]
                for source, fingerprint, future in futures:
                    if cancel.is_set():
                        break
                    photo = source["photo_id"]
                    try:
                        rgb = future.result()
                        existing = self.face_rows("f.photo_id=?", (photo,))
                        detect_job = self.db.execute("SELECT status,fingerprint FROM face_jobs WHERE photo_id=? AND stage='detect' AND model_id=?", (photo, self.detector_key)).fetchone()
                        reuse = detect_job and tuple(detect_job) == ("done", fingerprint)
                        if reuse:
                            candidates = [r for r in existing if r["source_file_id"] == source["id"] and
                                          (r["detector_id"] == self.detector_key or (r["retained"] and r["assignment"] == "user"))]
                        else:
                            candidates = self._detect(rgb, source, detector, existing, fingerprint)
                        aligned, needs = [], []
                        for face in candidates:
                            cached = face.get("id") and self.db.execute("SELECT 1 FROM face_embeddings WHERE face_id=? AND embedder_id=?",
                                                                         (face["id"], self.embedder_key)).fetchone()
                            if not cached or not reuse:
                                marks = np.array(json.loads(face["landmarks"])) * [rgb.shape[1], rgb.shape[0]]
                                aligned.append(align_face(rgb, marks, embedder.input_spec))
                                needs.append(face)
                        pending.append((source, fingerprint, candidates, aligned, needs, not reuse))
                    except Exception as exc:
                        with self.db:
                            self._job(photo, "embed", self.detector_key + "/" + self.embedder_key, "error", fingerprint, str(exc))
                        stats["errors"].append(f"{source['relpath']}: {exc}")
            if cancel.is_set():
                break
            all_aligned = [a for _, _, _, aligned, _, _ in pending for a in aligned]
            all_vectors = []
            try:
                for start in range(0, len(all_aligned), FACE_BATCH_SIZE):
                    if cancel.is_set():
                        break
                    all_vectors.extend(normalize(embedder.embed(np.array(all_aligned[start:start + FACE_BATCH_SIZE]))))
            except Exception as exc:
                with self.db:
                    for source, fingerprint, *_ in pending:
                        self._job(source["photo_id"], "embed", self.detector_key + "/" + self.embedder_key, "error", fingerprint, str(exc))
                stats["errors"].append(str(exc))
                continue
            if cancel.is_set():
                break
            cursor = 0
            with self.db:
                for source, fingerprint, candidates, _, needs, redetected in pending:
                    if self._fingerprint(source) != fingerprint:
                        stats["errors"].append(f"Während Analyse geändert: {source['relpath']}")
                        cursor += len(needs)
                        continue
                    self._save_faces(source, candidates, redetected)
                    for face in needs:
                        vector = all_vectors[cursor]
                        cursor += 1
                        self.db.execute("INSERT OR REPLACE INTO face_embeddings VALUES(?,?,?,?)",
                                        (face["id"], self.embedder_key, len(vector), vector.astype("<f4").tobytes()))
                    self._job(source["photo_id"], "detect", self.detector_key, "done", fingerprint)
                    self._job(source["photo_id"], "embed", self.detector_key + "/" + self.embedder_key, "done", fingerprint)
                    stats["done"] += 1
            if progress:
                progress(dict(stats))
        stats["cancelled"] = cancel.is_set()
        if not cancel.is_set():
            self._embed_retained(embedder, cancel, stats)
        stats["cancelled"] = cancel.is_set()
        if not cancel.is_set():
            self.rebuild_training()
            self.rematch()
        return stats

    def _embed_retained(self, embedder, cancel, stats) -> None:
        """Reindex preserved user boxes in their own source coordinate frame."""
        rows = self.face_rows("assignment='user' AND ignored=0 AND NOT EXISTS "
            "(SELECT 1 FROM face_embeddings e WHERE e.face_id=f.id AND e.embedder_id=?)", (self.embedder_key,))
        for start in range(0, len(rows), FACE_BATCH_SIZE):
            if cancel.is_set():
                return
            batch, aligned = [], []
            for face in rows[start:start + FACE_BATCH_SIZE]:
                try:
                    rgb = self._load(face)
                    marks = np.array(json.loads(face["landmarks"])) * [rgb.shape[1], rgb.shape[0]]
                    aligned.append(align_face(rgb, marks, embedder.input_spec))
                    batch.append(face)
                except Exception as exc:
                    stats["errors"].append(f"Gesicht #{face['id']}: {exc}")
            if aligned and not cancel.is_set():
                vectors = normalize(embedder.embed(np.array(aligned)))
                if cancel.is_set():
                    return
                with self.db:
                    for face, vector in zip(batch, vectors):
                        self.db.execute("INSERT OR REPLACE INTO face_embeddings VALUES(?,?,?,?)",
                            (face["id"], self.embedder_key, len(vector), vector.astype("<f4").tobytes()))

    def _detect(self, rgb, source, detector, existing, fingerprint) -> list[dict]:
        height, width = rgb.shape[:2]
        result = []
        for detection in detector.detect(rgb):
            bbox = np.clip(np.asarray(detection.bbox) / [width, height, width, height], 0, 1)
            if bbox[2] <= bbox[0] or bbox[3] <= bbox[1]:
                continue
            x1, y1, x2, y2 = (bbox * [width, height, width, height]).astype(int)
            crop = rgb[y1:y2, x1:x2]
            sharpness = min(1.0, cv2.Laplacian(cv2.cvtColor(crop, cv2.COLOR_RGB2GRAY), cv2.CV_64F).var() / 100) if crop.size else 0
            original_width = (bbox[2] - bbox[0]) * (source.get("width") or width)
            marks = np.asarray(detection.landmarks)
            eyes = np.linalg.norm(marks[1] - marks[0])
            nose_offset = abs(marks[2, 0] - (marks[0, 0] + marks[1, 0]) / 2) / max(eyes, 1)
            quality = min(1.0, original_width / 40) * sharpness * max(0, 1 - nose_offset)
            if detection.score < self.options["min_score"] or original_width < 40:
                quality = min(quality, 0.25)
            result.append(dict(photo_id=source["photo_id"], source_file_id=source["id"],
                source_relpath=source["relpath"], detector_id=self.detector_key,
                bbox_x1=float(bbox[0]), bbox_y1=float(bbox[1]), bbox_x2=float(bbox[2]), bbox_y2=float(bbox[3]),
                landmarks=json.dumps((marks / [width, height]).tolist()), det_score=detection.score,
                quality=float(quality), captured_at=source.get("dt_original"), source_fingerprint=fingerprint,
                person_id=None, assignment=None, confidence=None, ignored=0, retained=0))
        # Greedy sorted IoU matches are one-to-one and preserve all explicit decisions.
        matches = sorted(((iou(self._bbox(new), self._bbox(old)), i, j)
                          for i, new in enumerate(result) for j, old in enumerate(existing)
                          if old["source_file_id"] == source["id"]), reverse=True)
        used_new, used_old = set(), set()
        for overlap, i, j in matches:
            if overlap < 0.5:
                break
            if i in used_new or j in used_old:
                continue
            old = existing[j]
            used_new.add(i)
            used_old.add(j)
            result[i].update({k: old[k] for k in ("id", "person_id", "assignment", "confidence", "ignored")})
        # Retained user boxes keep their source coordinate frame; never transfer
        # between a cropped JPEG and a RAW preview using normalized IoU alone.
        for j, old in enumerate(existing):
            if j not in used_old and (old["assignment"] == "user" or old["ignored"]):
                retained = dict(old) | {"retained": 1}
                if old["source_file_id"] == source["id"]:
                    result.append(retained)
        return result

    @staticmethod
    def _bbox(row) -> tuple:
        return tuple(row[k] for k in ("bbox_x1", "bbox_y1", "bbox_x2", "bbox_y2"))

    def _save_faces(self, source, candidates, redetected) -> None:
        if redetected:
            # Other detector jobs no longer describe the boxes currently stored
            # on reused face IDs. Embedder-only switches keep all cached stages.
            self.db.execute("DELETE FROM face_jobs WHERE photo_id=?", (source["photo_id"],))
            self.db.execute("UPDATE faces SET retained=1 WHERE photo_id=?", (source["photo_id"],))
        columns = ("photo_id", "source_file_id", "source_relpath", "detector_id", "bbox_x1", "bbox_y1", "bbox_x2", "bbox_y2",
                   "landmarks", "det_score", "quality", "captured_at", "source_fingerprint", "person_id", "assignment", "confidence", "ignored", "retained")
        for face in candidates:
            values = [face[k] for k in columns]
            if face.get("id"):
                if redetected and not face["retained"]:
                    self.db.execute("DELETE FROM face_embeddings WHERE face_id=?", (face["id"],))
                    self.db.execute("DELETE FROM training_set WHERE face_id=? AND pinned=0", (face["id"],))
                self.db.execute("UPDATE faces SET " + ",".join(k + "=?" for k in columns) + " WHERE id=?", values + [face["id"]])
            else:
                face["id"] = self.db.execute("INSERT INTO faces(" + ",".join(columns) + ") VALUES(" + ",".join("?" for _ in columns) + ")", values).lastrowid

    def rebuild_training(self) -> dict:
        self.index._writable()
        key = self.embedder_key
        threshold = self.options.get("duplicate", self.registry.models[self.options["active_embedder"]]["thresholds"].get("duplicate", 0.92))
        report = {}
        with self.db:
            self.db.execute("DELETE FROM face_review WHERE embedder_id=?", (key,))
            for person in self.persons():
                rows = self.vectors("f.person_id=? AND assignment='user' AND ignored=0 AND quality>=?",
                                    (person["id"], self.options["min_quality"]))
                pins = {r[0] for r in self.db.execute("SELECT face_id FROM training_set WHERE person_id=? AND embedder_id=? AND pinned=1", (person["id"], key))}
                for row in rows:
                    row["pinned"] = row["id"] in pins
                selected = select_references(rows, int(self.options["reference_limit"]), threshold,
                                             self.options["validation_fraction"], self.options["seed"],
                                             self.options.get("outlier", 0.2))
                self.db.execute("DELETE FROM training_set WHERE person_id=? AND embedder_id=?", (person["id"], key))
                for split, ids in (("ref", selected.ref), ("val", selected.val)):
                    self.db.executemany("INSERT INTO training_set VALUES(?,?,?,?,?,?,?)", [
                        (person["id"], face, key, split, timestamp(), "manual_pin" if face in pins else "fps", int(face in pins)) for face in ids])
                self.db.executemany("INSERT OR REPLACE INTO face_review VALUES(?,?,?)", [(face, key, "outlier") for face in selected.outliers])
                report[person["id"]] = {"ref": len(selected.ref), "val": len(selected.val), "review": len(selected.outliers)}
        return report

    def rematch(self) -> None:
        """Reevaluate machine decisions only; rejection and user labels always win."""
        self.index._writable()
        rows = self.vectors("ignored=0 AND (retained=0 OR assignment='user')")
        curated = {r[0] for r in self.db.execute("SELECT face_id FROM training_set WHERE embedder_id=? AND split='ref'", (self.embedder_key,))}
        outliers = {r[0] for r in self.db.execute("SELECT face_id FROM face_review WHERE embedder_id=?", (self.embedder_key,))}
        references = [r for r in rows if r["assignment"] == "user" and r["quality"] >= self.options["min_quality"] and r["id"] not in outliers and r["id"] in curated]
        # Per-person fallback only before a curated set exists for that person.
        curated_people = {r[0] for r in self.db.execute("SELECT DISTINCT person_id FROM training_set WHERE embedder_id=?", (self.embedder_key,))}
        references += [r for r in rows if r["assignment"] == "user" and r["quality"] >= self.options["min_quality"] and r["person_id"] not in curated_people and r["id"] not in outliers]
        thresholds = self.registry.models[self.options["active_embedder"]]["thresholds"]
        rejected = {}
        for face, person in self.db.execute("SELECT * FROM face_rejections"):
            rejected.setdefault(face, set()).add(person)
        births = {r[0]: int(r[1]) for r in self.db.execute("SELECT person_id,value FROM person_attrs WHERE key='birth_year' AND value GLOB '[0-9][0-9][0-9][0-9]'")}
        with self.db:
            self.db.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE assignment IN ('auto','suggested')")
            for photo in sorted({r["photo_id"] for r in rows}):
                faces = [r for r in rows if r["photo_id"] == photo]
                occupied = {r["person_id"] for r in faces if r["assignment"] == "user"}
                queries = [r for r in faces if r["assignment"] != "user" and not r["retained"]]
                if not queries:
                    continue
                forbidden = []
                for face in queries:
                    year = int(face["captured_at"][:4]) if face["captured_at"] else None
                    forbidden.append({p for p, birth in births.items() if year and year < birth} if self.options.get("age_prior", True) else set())
                priors = {}
                if self.options.get("cooccurrence", True) and occupied:
                    for known in occupied:
                        for person, count in self.db.execute("""SELECT b.person_id,count(DISTINCT a.photo_id)
                            FROM faces a JOIN faces b ON b.photo_id=a.photo_id
                            WHERE a.person_id=? AND a.assignment='user' AND b.assignment='user'
                              AND a.ignored=0 AND b.ignored=0 AND b.person_id!=?
                            GROUP BY b.person_id""", (known, known)):
                            priors[person] = max(priors.get(person, 0), min(0.03, count * 0.005))
                decisions = match_photo(np.array([r["vector"] for r in queries]), np.array([r["vector"] for r in references]),
                    [r["person_id"] for r in references], [rejected.get(r["id"], set()) for r in queries], occupied,
                    self.options.get("auto", thresholds["auto"]), self.options.get("suggest", thresholds["suggest"]),
                    [r["quality"] >= self.options["min_quality"] for r in queries], forbidden, priors)
                for face, (person, assignment, confidence) in zip(queries, decisions):
                    self.db.execute("UPDATE faces SET person_id=?,assignment=?,confidence=?,assigned_at=? WHERE id=? AND assignment IS NOT 'user'",
                                    (person, assignment, confidence, timestamp(), face["id"]))

    def clusters(self) -> list[list[int]]:
        rows = self.vectors("person_id IS NULL AND ignored=0 AND retained=0 AND quality>=?", (self.options["min_quality"],))
        return cluster_unknown(rows, self.options.get("cluster", 0.6))

    def assign(self, face_ids: list[int], person: int) -> None:
        """Name a cluster or move faces; reject prior identities and rebuild references."""
        self.index._writable()
        self.graph._person(person)
        face_ids = sorted(set(face_ids))
        with self.db:
            photos = set()
            for face_id in face_ids:
                row = self.db.execute("SELECT * FROM faces WHERE id=?", (face_id,)).fetchone()
                if not row:
                    raise ValueError("Gesicht fehlt")
                if row["photo_id"] in photos or self.db.execute("SELECT 1 FROM faces WHERE photo_id=? AND person_id=? AND assignment='user' AND id!=? AND ignored=0", (row["photo_id"], person, face_id)).fetchone():
                    raise ValueError("Person ist auf diesem Foto bereits zugeordnet")
                photos.add(row["photo_id"])
                if row["person_id"] and row["person_id"] != person:
                    self.db.execute("INSERT OR IGNORE INTO face_rejections VALUES(?,?)", (face_id, row["person_id"]))
                self.db.execute("DELETE FROM training_set WHERE face_id=? AND person_id!=?", (face_id, person))
                self.db.execute("DELETE FROM face_rejections WHERE face_id=? AND person_id=?", (face_id, person))
                self.db.execute("UPDATE faces SET person_id=?,assignment='user',ignored=0,confidence=1,assigned_at=? WHERE id=?", (person, timestamp(), face_id))
        self.rebuild_training()
        self.rematch()

    def confirm(self, face: int) -> None:
        row = self.db.execute("SELECT person_id FROM faces WHERE id=?", (face,)).fetchone()
        if not row or not row[0]:
            raise ValueError("Kein Vorschlag vorhanden")
        self.assign([face], row[0])

    def reject(self, face: int) -> None:
        self.index._writable()
        with self.db:
            self.db.execute("INSERT OR IGNORE INTO face_rejections SELECT id,person_id FROM faces WHERE id=? AND person_id IS NOT NULL", (face,))
            self.db.execute("UPDATE faces SET person_id=NULL,assignment=NULL,confidence=NULL WHERE id=?", (face,))
            self.db.execute("DELETE FROM training_set WHERE face_id=?", (face,))
        self.rebuild_training()
        self.rematch()

    def ignore(self, face: int) -> None:
        self.index._writable()
        with self.db:
            self.db.execute("UPDATE faces SET ignored=1,person_id=NULL,assignment=NULL WHERE id=?", (face,))
            self.db.execute("DELETE FROM training_set WHERE face_id=?", (face,))
        self.rebuild_training()
        self.rematch()

    def pin(self, face: int, enabled: bool = True) -> None:
        self.index._writable()
        row = self.db.execute("SELECT * FROM faces WHERE id=? AND assignment='user' AND ignored=0 AND quality>=?", (face, self.options["min_quality"])).fetchone()
        if not row:
            raise ValueError("Nur bestätigte Gesichter guter Qualität können fixiert werden")
        with self.db:
            self.db.execute("INSERT INTO training_set VALUES(?,?,?,'ref',?,'manual_pin',?) ON CONFLICT(person_id,face_id,embedder_id) DO UPDATE SET pinned=excluded.pinned", (row["person_id"], face, self.embedder_key, timestamp(), int(enabled)))
        self.rebuild_training()
        self.rematch()

    def merge(self, source: int, target: int) -> None:
        self.index._writable()
        self.graph._person(source)
        self.graph._person(target)
        if source == target:
            raise ValueError("Personen sind identisch")
        with self.db:
            # Conflicting confirmed identities must be resolved explicitly first.
            conflict = self.db.execute("SELECT photo_id FROM faces WHERE person_id IN (?,?) AND assignment='user' AND ignored=0 GROUP BY photo_id HAVING count(DISTINCT person_id)>1", (source, target)).fetchone()
            if conflict:
                raise ValueError("Beide Personen sind auf demselben Foto bestätigt")
            edges = [(target if a == source else a, kind, target if b == source else b)
                     for a, kind, b in self.db.execute("SELECT * FROM relations")]
            if any(a == b for a, _, b in edges):
                raise ValueError("Zusammenführen würde eine Selbstbeziehung erzeugen")
            self.graph.validate({(a, b) for a, kind, b in edges if kind == 1})
            self.db.execute("DELETE FROM relations")
            for a, kind, b in edges:
                if kind in (3, 4):
                    a, b = sorted((a, b))
                self.db.execute("INSERT OR IGNORE INTO relations VALUES(?,?,?)", (a, kind, b))
            self.db.execute("UPDATE persons SET merged_into=? WHERE id=?", (target, source))
            self.db.execute("UPDATE faces SET person_id=? WHERE person_id=?", (target, source))
            self.db.execute("INSERT OR IGNORE INTO person_attrs SELECT ?,key,value FROM person_attrs WHERE person_id=?", (target, source))
            self.db.execute("INSERT OR IGNORE INTO face_rejections SELECT face_id,? FROM face_rejections WHERE person_id=?", (target, source))
            self.db.execute("DELETE FROM face_rejections WHERE person_id=?", (source,))
            self.db.execute("DELETE FROM training_set WHERE person_id IN (?,?)", (source, target))
        self.rebuild_training()
        self.rematch()

    def thumbnail(self, face: int, size: int = 256):
        row = self.face_rows("f.id=?", (face,))[0]
        image = load_image(safe_path(self.index.root, row["relpath"]), exiftool=self.index.settings["exiftool"], orientation=row["orientation"])
        return crop_face(image, self._bbox(row), size)

    def person_photos(self, person: int) -> list[dict]:
        return [dict(r) for r in self.db.execute("""SELECT DISTINCT files.* FROM files JOIN faces ON faces.photo_id=files.photo_id
            WHERE faces.person_id=? AND faces.ignored=0 AND files.status='present' ORDER BY files.relpath""", (person,))]

    def status(self) -> dict:
        return {"jobs": [dict(r) for r in self.db.execute("SELECT * FROM face_jobs ORDER BY photo_id")],
                "faces": self.db.execute("SELECT count(*) FROM faces WHERE retained=0 OR assignment='user'").fetchone()[0],
                "persons": len(self.persons()), "settings": self.options}

    def delete_all(self) -> None:
        """Remove face DB data and derived exports; inventory and photos survive."""
        self.index._writable()
        with self.db:
            for table in ("face_embeddings", "face_rejections", "training_set", "face_review", "relations", "person_attrs", "persons", "faces", "face_jobs", "face_settings"):
                self.db.execute(f"DELETE FROM {table}")
        folder = self.index.root / ".myfotomanager_faces"
        if folder.is_symlink():
            folder.unlink()
        elif folder.exists():
            shutil.rmtree(folder)
        # Checkpoint and VACUUM remove deleted vectors from DB/WAL free pages.
        self.db.execute("PRAGMA wal_checkpoint(TRUNCATE)")
        self.db.execute("VACUUM")
        self.options = settings(self.db)
