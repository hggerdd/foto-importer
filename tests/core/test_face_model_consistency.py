"""Opt-in checks with locally installed weights and private labelled photos."""

import os
from pathlib import Path

import numpy as np
import pytest

from config.settings import FACE_MODEL_DIR
from core.face_images import load_image
from core.face_models import ModelRegistry, align_face

REGISTRY = ModelRegistry(FACE_MODEL_DIR)
EMBEDDERS = [m["id"] for m in REGISTRY.list_models() if m["kind"] == "embedder"]


@pytest.mark.parametrize("embedder_id", EMBEDDERS)
def test_real_model_same_person_exceeds_different_person(embedder_id):
    folder = os.environ.get("FACE_REFERENCE_DIR")
    if not folder:
        pytest.skip("Set FACE_REFERENCE_DIR to private labelled person-ID folders")
    if not REGISTRY.path(REGISTRY.models[embedder_id]).exists():
        pytest.skip(f"Install {embedder_id} weights for the model consistency test")
    detector_id = os.environ.get("FACE_TEST_DETECTOR", "yunet")
    detector = REGISTRY.load(detector_id)
    embedder = REGISTRY.load(embedder_id)
    persons = []
    for person in sorted(Path(folder).iterdir()):
        if person.is_dir() and person.name.isdigit():
            paths = [
                p
                for p in sorted(person.iterdir())
                if p.suffix.lower() in (".jpg", ".jpeg", ".png")
            ]
            if len(paths) >= 2:
                persons.append(paths)
    assert len(persons) >= 2, (
        "Need at least two persons with two single-person photos each"
    )
    vectors = []
    for path in (persons[0][0], persons[0][1], persons[1][0]):
        image = np.array(load_image(path))
        faces = detector.detect(image)
        assert len(faces) == 1, f"Expected exactly one detected face in {path}"
        aligned = align_face(image, faces[0].landmarks, embedder.input_spec)
        vector = embedder.embed(np.array([aligned]))[0]
        assert len(vector) == REGISTRY.models[embedder_id]["dim"]
        assert np.linalg.norm(vector) == pytest.approx(1, abs=1e-5)
        vectors.append(vector)
    same = float(vectors[0] @ vectors[1])
    different = float(vectors[0] @ vectors[2])
    assert same > different + 0.05, (embedder_id, same, different)
