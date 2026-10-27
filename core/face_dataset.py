"""Reproducible dataset export and held-out matching calibration."""
from __future__ import annotations

from collections import defaultdict
import json
import os
from pathlib import Path
import tempfile

import numpy as np


def atomic_write(path: Path, content: bytes) -> None:
    """Replace an artifact atomically on the same filesystem."""
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=".tmp-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(content)
        os.replace(temporary, path)
    finally:
        Path(temporary).unlink(missing_ok=True)


def export_dataset(service) -> dict:
    """Export the DB selection using IDs only; synchronize owned files safely."""
    from io import BytesIO
    root = service.index.root / ".myfotomanager_faces"
    if root.is_symlink():
        raise ValueError("Exportordner darf kein Symlink sein")
    root.mkdir(exist_ok=True)
    # Disallow junctions/symlinks in the generated tree before deleting/writing.
    for path in root.rglob("*"):
        if path.is_symlink() or not path.resolve().is_relative_to(root.resolve()):
            raise ValueError("Verknüpfungen im Exportordner sind nicht erlaubt")
    expected = {root / "manifest.json"}
    rows = service.db.execute("SELECT * FROM training_set WHERE embedder_id=? ORDER BY person_id,face_id", (service.embedder_key,)).fetchall()
    entries = []
    for selection in rows:
        face = service.face_rows("f.id=?", (selection["face_id"],))[0]
        folder = root / "persons" / f"{selection['person_id']:06d}" / selection["split"]
        stem = f"f_{face['id']:07d}"
        image_path, meta_path = folder / (stem + ".jpg"), folder / (stem + ".json")
        expected.update((image_path, meta_path))
        metadata = {key: face[key] for key in ("id", "photo_id", "relpath", "quality", "captured_at", "source_fingerprint")}
        metadata["face_id"] = metadata.pop("id")
        metadata["bbox"] = service._bbox(face)
        metadata["landmarks"] = json.loads(face["landmarks"])
        metadata["coordinate_frame"] = "oriented_source_normalized"
        metadata["crop"] = {"scale": 1.6, "size": 256, "aligned": False}
        content = json.dumps(metadata, sort_keys=True, indent=2).encode()
        if not image_path.exists() or not meta_path.exists() or meta_path.read_bytes() != content:
            buffer = BytesIO()
            service.thumbnail(face["id"]).save(buffer, format="JPEG", quality=95)
            atomic_write(image_path, buffer.getvalue())
            atomic_write(meta_path, content)
        entries.append({"person_id": selection["person_id"], "face_id": face["id"], "split": selection["split"]})
    manifest = {"version": 1, "embedder_id": service.embedder_key,
                "N": service.options["reference_limit"], "seed": service.options["seed"],
                "validation_fraction": service.options["validation_fraction"],
                "thresholds": service.registry.models[service.options["active_embedder"]]["thresholds"] |
                    {k: service.options[k] for k in ("auto", "suggest", "duplicate") if k in service.options},
                "entries": entries}
    atomic_write(root / "manifest.json", json.dumps(manifest, sort_keys=True, indent=2).encode())
    # The directory is exclusively a generated export. Remove only files after
    # every expected output succeeded; source photos are outside this directory.
    for path in sorted(root.rglob("*"), reverse=True):
        if path.is_file() and path not in expected:
            path.unlink()
        elif path.is_dir() and not any(path.iterdir()):
            path.rmdir()
    if os.name == "nt":
        import ctypes
        ctypes.windll.kernel32.SetFileAttributesW(str(root), 2)
    return {"faces": len(entries), "path": str(root)}


def calibrate(service) -> dict:
    """Measure held-out top-three kNN; thresholds are proposals, never auto-applied."""
    rows = {row["id"]: row for row in service.vectors("assignment='user' AND ignored=0")}
    selections = service.db.execute("SELECT * FROM training_set WHERE embedder_id=?", (service.embedder_key,)).fetchall()
    ref = [rows[s["face_id"]] for s in selections if s["split"] == "ref" and s["face_id"] in rows]
    val = [rows[s["face_id"]] for s in selections if s["split"] == "val" and s["face_id"] in rows]
    if not ref or not val or len({r["person_id"] for r in ref}) < 2:
        return {"available": False, "reason": "Mindestens zwei Personen mit Referenz- und Validierungsbildern benötigt."}
    vectors = np.array([r["vector"] for r in ref])
    people = np.array([r["person_id"] for r in ref])
    genuine, impostor, predictions = [], [], []
    for row in val:
        similarities = vectors @ row["vector"]
        scores = {int(p): float(np.sort(similarities[people == p])[-3:].mean()) for p in set(people)}
        truth = row["person_id"]
        if truth in scores:
            genuine.append(scores[truth])
        impostor.extend(score for p, score in scores.items() if p != truth)
        best = max(scores, key=scores.get)
        predictions.append((truth, best, scores[best]))
    curve = []
    for threshold in np.linspace(-1, 1, 201):
        accepted = [(truth, best) for truth, best, score in predictions if score >= threshold]
        correct = sum(truth == best for truth, best in accepted)
        curve.append({"threshold": round(float(threshold), 3), "precision": correct / len(accepted) if accepted else None,
                      "recall": correct / len(predictions), "accepted": len(accepted)})
    options = [r for r in curve if r["accepted"] and r["precision"] >= 0.98]
    proposal = max(options, key=lambda r: (r["recall"], r["threshold"])) if options else None
    per_person = defaultdict(lambda: {"total": 0, "correct": 0})
    for truth, best, score in predictions:
        stats = per_person[truth]
        stats["total"] += 1
        stats["correct"] += int(best == truth)
    return {"available": True, "embedder_id": service.embedder_key, "samples": len(val),
            "genuine": genuine, "impostor": impostor, "curve": curve,
            "proposed_auto": proposal["threshold"] if proposal else None,
            "per_person": dict(per_person),
            "note": "Empirische Schätzung auf kleinem Datensatz, keine Garantie für 98 % Precision."}
