"""Read oriented previews and crop faces without developing RAW files."""
from __future__ import annotations

from io import BytesIO
from pathlib import Path
import subprocess

import numpy as np
from PIL import Image, ImageOps

from config.settings import FACE_MAX_EDGE

RAW_EXTENSIONS = {"nef", "nrw", "dng", "arw", "cr2", "cr3"}


def safe_path(root: Path, relpath: str) -> Path:
    path = (root / relpath).resolve()
    if not path.is_relative_to(root.resolve()):
        raise ValueError("Bildpfad liegt außerhalb der Sammlung")
    return path


def load_image(path: Path, *, exiftool: str = "exiftool", orientation: int = 1,
               max_edge: int = FACE_MAX_EDGE) -> Image.Image:
    """Extract embedded RAW JPEG, apply orientation once, close source handles."""
    if path.suffix.lower().lstrip(".") in RAW_EXTENSIONS:
        data = b""
        for tag in ("JpgFromRaw", "PreviewImage", "ThumbnailImage"):
            result = subprocess.run(
                [exiftool, "-b", f"-{tag}", str(path)], capture_output=True,
                timeout=30, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            if result.returncode == 0 and result.stdout.startswith(b"\xff\xd8"):
                data = result.stdout
                break
        if not data:
            raise ValueError(f"Kein eingebettetes JPEG: {path.name}")
        source = BytesIO(data)
    else:
        source = path
    with Image.open(source) as image:
        if not image.getexif().get(274) and orientation != 1:
            image.getexif()[274] = orientation
        image = ImageOps.exif_transpose(image).convert("RGB")
        image.thumbnail((max_edge, max_edge), Image.Resampling.LANCZOS)
        return image.copy()


def crop_face(image: Image.Image, bbox, size: int = 256) -> Image.Image:
    """Crop a 1.6x square in the oriented source coordinate frame."""
    x1, y1, x2, y2 = np.asarray(bbox) * [image.width, image.height, image.width, image.height]
    side = max(x2 - x1, y2 - y1) * 1.6
    if side <= 0:
        raise ValueError("Ungültiger Gesichtsausschnitt")
    cx, cy = (x1 + x2) / 2, (y1 + y2) / 2
    return image.crop((round(cx - side / 2), round(cy - side / 2),
                       round(cx + side / 2), round(cy + side / 2))).resize((size, size))
