"""Explicitly install trusted model artifacts; inference itself stays offline."""

from __future__ import annotations

import argparse
import hashlib
import sys
import tempfile
import urllib.request
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from config.settings import FACE_MODEL_DIR
from core.face_dataset import atomic_write
from core.face_models import ModelRegistry


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("models", nargs="*", default=["yunet", "sface"])
    args = parser.parse_args()
    registry = ModelRegistry(FACE_MODEL_DIR)
    downloads = {}
    with tempfile.TemporaryDirectory(prefix="foto-models-") as temporary:
        for model_id in args.models:
            model = registry.models[model_id]
            print(f"{model_id}: {model['license']}", flush=True)
            url = model["download_url"]
            if not url.startswith("https://"):
                raise ValueError("Nur HTTPS-Downloads erlaubt")
            if url not in downloads:
                archive = Path(temporary) / str(len(downloads))
                urllib.request.urlretrieve(url, archive)
                downloads[url] = archive
            artifact = downloads[url]
            if model.get("archive_member"):
                with zipfile.ZipFile(artifact) as archive:
                    data = archive.read(model["archive_member"])
            else:
                data = artifact.read_bytes()
            if hashlib.sha256(data).hexdigest() != model["sha256"]:
                raise ValueError(f"SHA-256 stimmt nicht: {model_id}")
            atomic_write(registry.path(model), data)
            registry.verify(model_id)
            print(f"Installiert: {registry.path(model)}", flush=True)


if __name__ == "__main__":
    main()
