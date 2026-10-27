"""Evaluate local models on labelled single-person photos; never uploads images."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from time import perf_counter

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from config.settings import FACE_MODEL_DIR
from core.face_algorithms import match_photo
from core.face_images import load_image
from core.face_models import ModelRegistry, align_face


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "dataset",
        type=Path,
        help="Subdirectories: numeric person IDs, plus optional 'unknown'",
    )
    parser.add_argument("--detector", default="yunet")
    parser.add_argument("--embedder", default="sface")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    registry = ModelRegistry(FACE_MODEL_DIR)
    detector, embedder = registry.load(args.detector), registry.load(args.embedder)
    refs, labels, validation, errors, durations = [], [], [], [], []
    for folder in sorted(args.dataset.iterdir()):
        if not folder.is_dir() or (
            not folder.name.isdigit() and folder.name != "unknown"
        ):
            continue
        person = int(folder.name) if folder.name.isdigit() else None
        vectors = []
        for path in sorted(folder.iterdir()):
            if path.suffix.lower() not in (".jpg", ".jpeg", ".png", ".nef", ".dng"):
                continue
            try:
                start = perf_counter()
                rgb = np.array(load_image(path))
                detections = detector.detect(rgb)
                if len(detections) != 1:
                    raise ValueError(
                        f"Expected exactly one face, got {len(detections)}"
                    )
                aligned = align_face(rgb, detections[0].landmarks, embedder.input_spec)
                vector = embedder.embed(np.array([aligned]))[0]
                durations.append(perf_counter() - start)
                vectors.append(vector)
            except Exception as exc:  # noqa: BLE001 - include decoder/runtime failures in the report
                errors.append({"file": str(path), "error": str(exc)})
        if person is not None:
            if len(vectors) < 2:
                errors.append(
                    {"person_id": person, "error": "Need at least two photos"}
                )
                continue
            count = max(1, len(vectors) // 2)
            refs.extend(vectors[:count])
            labels.extend([person] * count)
            validation.extend((person, v) for v in vectors[count:])
        else:
            validation.extend((None, v) for v in vectors)
    if not refs or not validation:
        parser.error("No usable reference / validation split")
    thresholds = registry.models[args.embedder]["thresholds"]
    predictions = []
    for truth, vector in validation:
        start = perf_counter()
        predicted, assignment, score = match_photo(
            np.array([vector]),
            np.array(refs),
            labels,
            [set()],
            set(),
            thresholds["auto"],
            thresholds["suggest"],
            [True],
        )[0]
        predictions.append(
            {
                "truth": truth,
                "predicted": predicted,
                "assignment": assignment,
                "score": score,
                "match_ms": (perf_counter() - start) * 1000,
            }
        )
    automatic = [p for p in predictions if p["assignment"] == "auto"]
    correct = sum(p["truth"] == p["predicted"] for p in automatic)
    known = sum(p["truth"] is not None for p in predictions)
    result = {
        "detector": registry.key(args.detector),
        "embedder": registry.key(args.embedder),
        "references": len(refs),
        "validation": len(validation),
        "automatic": len(automatic),
        "auto_precision": correct / len(automatic) if automatic else None,
        "auto_recall": correct / known if known else None,
        "mean_image_seconds": float(np.mean(durations)),
        "predictions": predictions,
        "errors": errors,
    }
    content = json.dumps(result, indent=2)
    if args.output:
        args.output.write_text(content, encoding="utf-8")
    print(content)


if __name__ == "__main__":
    main()
