"""Register a user-supplied ONNX embedder, e.g. an exported AdaFace model."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from config.settings import FACE_MODEL_DIR
from core.face_dataset import atomic_write
from core.face_models import ModelRegistry


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("file", type=Path)
    parser.add_argument("--id", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument(
        "--sha256", required=True, help="Expected hash from your trusted export/source"
    )
    parser.add_argument("--license", required=True)
    parser.add_argument("--source-url", required=True)
    parser.add_argument("--dim", type=int, default=512)
    parser.add_argument("--color", choices=("RGB", "BGR"), default="BGR")
    parser.add_argument("--mean", type=float, default=127.5)
    parser.add_argument("--std", type=float, default=127.5)
    parser.add_argument("--auto", type=float, default=0.65)
    parser.add_argument("--suggest", type=float, default=0.4)
    args = parser.parse_args()
    if not re.fullmatch(r"[a-zA-Z0-9_.-]+", args.id):
        parser.error("Invalid model ID")
    manifest_path = FACE_MODEL_DIR / f"{args.id}.json"
    if manifest_path.exists():
        parser.error(
            "Model ID already registered; choose a new ID for a different model/version"
        )
    data = args.file.read_bytes()
    if hashlib.sha256(data).hexdigest() != args.sha256.lower():
        parser.error("SHA-256 mismatch")
    manifest = {
        "id": args.id,
        "kind": "embedder",
        "version": args.version,
        "runtime": "onnxruntime",
        "file": f"weights/{args.id}.onnx",
        "sha256": args.sha256.lower(),
        "dim": args.dim,
        "metric": "cosine",
        "size": len(data),
        "license": args.license,
        "source_url": args.source_url,
        "input": {
            "size": [112, 112],
            "color": args.color,
            "mean": args.mean,
            "std": args.std,
            "align": "arcface_5pt",
        },
        "thresholds": {"auto": args.auto, "suggest": args.suggest, "duplicate": 0.92},
    }
    registry = ModelRegistry(FACE_MODEL_DIR)
    registry._validate(manifest)
    atomic_write(registry.path(manifest), data)
    atomic_write(manifest_path, json.dumps(manifest, indent=2).encode())
    print(
        f"Registered {args.id}; restart the app and run a model consistency check before use."
    )


if __name__ == "__main__":
    main()
