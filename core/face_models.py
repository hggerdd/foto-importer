"""Manifest-driven CPU detectors and embedders. Never download at inference time."""

from __future__ import annotations

import hashlib
import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

import cv2
import numpy as np


@dataclass(frozen=True)
class Detection:
    bbox: np.ndarray
    score: float
    landmarks: np.ndarray


@dataclass(frozen=True)
class InputSpec:
    size: tuple[int, int] = (112, 112)
    color: str = "RGB"
    mean: float = 127.5
    std: float = 127.5
    align: str = "arcface_5pt"
    template: tuple = ()


class FaceDetector(Protocol):
    id: str
    version: str

    def detect(self, img_rgb: np.ndarray) -> list[Detection]: ...


class FaceEmbedder(Protocol):
    id: str
    version: str
    dim: int
    metric: str
    input_spec: InputSpec

    def embed(self, aligned_faces: np.ndarray) -> np.ndarray: ...


def normalize(vectors: np.ndarray) -> np.ndarray:
    values = np.asarray(vectors, dtype=np.float32)
    norms = np.linalg.norm(values, axis=-1, keepdims=True)
    if not np.isfinite(values).all() or np.any(norms < 1e-8):
        raise ValueError("Ungültiges oder leeres Embedding")
    return np.ascontiguousarray(values / norms, dtype=np.float32)


def align_face(rgb: np.ndarray, landmarks: np.ndarray, spec: InputSpec) -> np.ndarray:
    """Least-squares five-point similarity fit; output remains RGB."""
    template = np.array(
        spec.template
        or (
            (38.2946, 51.6963),
            (73.5318, 51.5014),
            (56.0252, 71.7366),
            (41.5493, 92.3655),
            (70.7299, 92.2041),
        ),
        dtype=np.float64,
    )
    if not spec.template:
        if spec.align != "arcface_5pt":
            raise ValueError("Alignment benötigt ein explizites 5-Punkt-Template")
        template *= np.array(spec.size) / 112
    points = np.asarray(landmarks, dtype=np.float64)
    if points.shape != (5, 2) or not np.isfinite(points).all():
        raise ValueError("Fünf gültige Landmarks erforderlich")
    # x'=a*x-b*y+tx; y'=b*x+a*y+ty; no reflection or random RANSAC.
    design = np.zeros((10, 4))
    design[0::2] = np.column_stack(
        (points[:, 0], -points[:, 1], np.ones(5), np.zeros(5))
    )
    design[1::2] = np.column_stack(
        (points[:, 1], points[:, 0], np.zeros(5), np.ones(5))
    )
    params, _, rank, _ = np.linalg.lstsq(design, template.ravel(), rcond=None)
    if rank < 4:
        raise ValueError("Degenerierte Landmarks")
    a, b, tx, ty = params
    return cv2.warpAffine(rgb, np.array(((a, -b, tx), (b, a, ty))), spec.size)


def prepare_input(faces: np.ndarray, spec: InputSpec) -> np.ndarray:
    pixels = np.asarray(faces, dtype=np.float32)
    if spec.color == "BGR":
        pixels = pixels[..., ::-1]
    return np.ascontiguousarray(((pixels - spec.mean) / spec.std).transpose(0, 3, 1, 2))


class ModelRegistry:
    """Read manifests, verify artifacts, and cache sessions per worker thread."""

    def __init__(self, folder: Path) -> None:
        self.folder = Path(folder).resolve()
        self.models: dict[str, dict] = {}
        self.errors: list[str] = []
        self._loaded = {}
        for path in sorted(self.folder.glob("*.json")):
            try:
                model = json.loads(path.read_text(encoding="utf-8"))
                self._validate(model)
                if model["id"] in self.models:
                    raise ValueError("Doppelte Modell-ID")
                self.models[model["id"]] = model
            except (OSError, ValueError, KeyError, TypeError) as exc:
                self.errors.append(f"{path.name}: {exc}")

    def _validate(self, model: dict) -> None:
        for key in (
            "id",
            "version",
            "kind",
            "runtime",
            "file",
            "sha256",
            "license",
            "source_url",
        ):
            if not isinstance(model[key], str) or not model[key]:
                raise ValueError(f"Ungültiges Feld: {key}")
        if not re.fullmatch(r"[a-zA-Z0-9_.-]+", model["id"]):
            raise ValueError("Ungültige Modell-ID")
        if not re.fullmatch(r"[a-f0-9]{64}", model["sha256"]):
            raise ValueError("SHA-256 muss 64 Hex-Zeichen enthalten")
        if model["kind"] not in ("detector", "embedder"):
            raise ValueError("Unbekannter Modelltyp")
        if model["runtime"] not in ("onnxruntime", "opencv_yunet", "opencv_sface"):
            raise ValueError("Nicht unterstützte Runtime")
        self.path(model)
        spec = InputSpec(
            **(
                model.get("input", {})
                | {"size": tuple(model.get("input", {}).get("size", [112, 112]))}
            )
        )
        if (
            spec.color not in ("RGB", "BGR")
            or spec.std <= 0
            or min(spec.size) <= 0
            or not np.isfinite([spec.mean, spec.std]).all()
        ):
            raise ValueError("Ungültige Eingabespezifikation")
        if spec.template and np.asarray(spec.template).shape != (5, 2):
            raise ValueError("Alignment-Template muss 5x2 sein")
        if model["kind"] == "embedder":
            if model.get("metric", "cosine") != "cosine" or int(model["dim"]) < 1:
                raise ValueError("Ungültiger Embedding-Raum")
            thresholds = model["thresholds"]
            if not -1 <= thresholds["suggest"] <= thresholds["auto"] <= 1:
                raise ValueError("Ungültige Schwellen")

    def path(self, model: dict) -> Path:
        path = (self.folder / model["file"]).resolve()
        if not path.is_relative_to(self.folder):
            raise ValueError("Modellpfad liegt außerhalb des Modellordners")
        return path

    def key(self, model_id: str) -> str:
        model = self.models[model_id]
        return f"{model_id}@{model['version']}:{model['sha256']}"

    def list_models(self) -> list[dict]:
        return [
            model
            | {
                "installed": self.path(model).is_file(),
                "size": self.path(model).stat().st_size
                if self.path(model).is_file()
                else model.get("size"),
                "speed": model.get("speed", "nicht gemessen"),
            }
            for model in self.models.values()
        ]

    def verify(self, model_id: str) -> Path:
        model = self.models[model_id]
        path = self.path(model)
        if not path.is_file():
            raise FileNotFoundError(f"Modell fehlt: {path}. Siehe models/README.md.")
        with path.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        if digest != model["sha256"]:
            raise ValueError(f"SHA-256 stimmt nicht: {model_id}")
        return path

    def load(self, model_id: str):
        key = self.key(model_id)
        if key not in self._loaded:
            model = self.models[model_id]
            path = self.verify(model_id)
            if model["runtime"] == "opencv_yunet":
                adapter = YuNet(model, path)
            elif model["runtime"] == "opencv_sface":
                adapter = SFace(model, path)
            else:
                adapter = (
                    OnnxDetector(model, path)
                    if model["kind"] == "detector"
                    else OnnxEmbedder(model, path)
                )
            self._loaded[key] = adapter
        return self._loaded[key]


class Adapter:
    def __init__(self, model: dict, path: Path) -> None:
        self.model = model
        self.id, self.version = model["id"], model["version"]
        self.dim = model.get("dim", 0)
        self.metric = "cosine"
        self.input_spec = InputSpec(**model.get("input", {}))


class YuNet(Adapter):
    def __init__(self, model: dict, path: Path) -> None:
        super().__init__(model, path)
        self.net = cv2.FaceDetectorYN.create(str(path), "", (320, 320), 0.5)

    def detect(self, img_rgb: np.ndarray) -> list[Detection]:
        self.net.setInputSize((img_rgb.shape[1], img_rgb.shape[0]))
        _, rows = self.net.detect(cv2.cvtColor(img_rgb, cv2.COLOR_RGB2BGR))
        return (
            []
            if rows is None
            else [
                Detection(
                    np.array((r[0], r[1], r[0] + r[2], r[1] + r[3])),
                    float(r[14]),
                    r[4:14].reshape(5, 2).copy(),
                )
                for r in rows
            ]
        )


class SFace(Adapter):
    def __init__(self, model: dict, path: Path) -> None:
        super().__init__(model, path)
        self.net = cv2.FaceRecognizerSF.create(str(path), "")

    def embed(self, aligned_faces: np.ndarray) -> np.ndarray:
        # OpenCV owns SFace normalization; the manifest specifies channel order.
        rows = [
            self.net.feature(
                np.ascontiguousarray(
                    face[..., ::-1] if self.input_spec.color == "BGR" else face
                )
            ).reshape(-1)
            for face in aligned_faces
        ]
        return normalize(np.array(rows))


class OnnxAdapter(Adapter):
    def __init__(self, model: dict, path: Path) -> None:
        super().__init__(model, path)
        import onnxruntime as ort

        options = ort.SessionOptions()
        options.intra_op_num_threads = os.cpu_count() or 1
        options.add_session_config_entry("session.intra_op.allow_spinning", "0")
        self.net = ort.InferenceSession(
            str(path), sess_options=options, providers=["CPUExecutionProvider"]
        )
        self.input = self.net.get_inputs()[0]


class OnnxEmbedder(OnnxAdapter):
    def embed(self, aligned_faces: np.ndarray) -> np.ndarray:
        tensor = prepare_input(aligned_faces, self.input_spec)
        fixed = self.input.shape[0]
        batch = fixed if isinstance(fixed, int) and fixed > 0 else len(tensor)
        rows = []
        for start in range(0, len(tensor), batch):
            part = tensor[start : start + batch]
            count = len(part)
            if count < batch:
                part = np.concatenate(
                    (part, np.repeat(part[-1:], batch - count, axis=0))
                )
            rows.append(self.net.run(None, {self.input.name: part})[0][:count])
        result = normalize(np.concatenate(rows).reshape(len(tensor), -1))
        if result.shape[1] != self.dim:
            raise ValueError("Embedding-Dimension entspricht nicht dem Manifest")
        return result


class OnnxDetector(OnnxAdapter):
    """SCRFD distance-head decoder with NMS and five-point landmarks."""

    def detect(self, img_rgb: np.ndarray) -> list[Detection]:
        width, height = self.model.get("input", {}).get("size", [640, 640])
        ratio = min(width / img_rgb.shape[1], height / img_rgb.shape[0])
        resized = cv2.resize(
            img_rgb, (round(img_rgb.shape[1] * ratio), round(img_rgb.shape[0] * ratio))
        )
        padded = np.zeros((height, width, 3), np.uint8)
        padded[: resized.shape[0], : resized.shape[1]] = resized
        outputs = self.net.run(
            None, {self.input.name: prepare_input(padded[None], self.input_spec)}
        )
        levels = 3 if len(outputs) == 9 else 5 if len(outputs) == 15 else 0
        if not levels:
            raise ValueError("SCRFD benötigt 9 oder 15 Ausgaben inklusive Landmarks")
        detections = []
        for i, stride in enumerate((8, 16, 32, 64, 128)[:levels]):
            scores = outputs[i].reshape(-1)
            boxes = outputs[i + levels].reshape(-1, 4) * stride
            marks = outputs[i + 2 * levels].reshape(-1, 5, 2) * stride
            grid = (
                np.stack(
                    np.meshgrid(
                        np.arange(width // stride), np.arange(height // stride)
                    ),
                    axis=-1,
                ).reshape(-1, 2)
                * stride
            )
            anchors = len(scores) // len(grid)
            centers = np.repeat(grid, anchors, axis=0)
            for j in np.flatnonzero(scores >= 0.5):
                center, box = centers[j], boxes[j]
                bbox = np.r_[center - box[:2], center + box[2:]] / ratio
                detections.append(
                    Detection(bbox, float(scores[j]), (marks[j] + center) / ratio)
                )
        if not detections:
            return []
        boxes = [
            [
                float(d.bbox[0]),
                float(d.bbox[1]),
                float(d.bbox[2] - d.bbox[0]),
                float(d.bbox[3] - d.bbox[1]),
            ]
            for d in detections
        ]
        keep = cv2.dnn.NMSBoxes(boxes, [d.score for d in detections], 0.5, 0.4)
        return [detections[int(i)] for i in np.asarray(keep).ravel()]
