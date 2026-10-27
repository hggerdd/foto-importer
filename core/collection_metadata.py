"""Read metadata in ExifTool batches without modifying source files."""

from __future__ import annotations

import json
import subprocess
from pathlib import Path


def read_metadata(paths: list[Path], executable: str = "exiftool") -> list[dict]:
    """Use an argument-file batch and retain Nikon MakerNotes with -fast."""
    if any("\n" in str(path) or "\r" in str(path) for path in paths):
        raise ValueError("ExifTool argument files cannot represent newline filenames")
    args = "\n".join(str(path.resolve()) for path in paths) + "\n"
    try:
        result = subprocess.run(
            [
                executable,
                "-j",
                "-n",
                "-fast",
                "-G1",
                "-charset",
                "filename=UTF8",
                "-@",
                "-",
            ],
            input=args.encode("utf-8"),
            capture_output=True,
            timeout=120,
            creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        )
    except FileNotFoundError as exc:
        raise RuntimeError(
            "ExifTool fehlt. Bitte exiftool installieren oder den Pfad konfigurieren."
        ) from exc
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    return json.loads(result.stdout.decode("utf-8"))


def metadata_values(raw: dict) -> dict:
    """Flatten known tags, preserving the original grouped JSON separately."""
    tags = {key.rsplit(":", 1)[-1]: value for key, value in raw.items()}

    def get(*names):
        return next((tags[name] for name in names if tags.get(name) is not None), None)

    return dict(
        make=get("Make"),
        model=get("Model"),
        serial=get("SerialNumber", "InternalSerialNumber", "CameraSerialNumber"),
        shutter_count=get("ShutterCount"),
        dt_original=get("DateTimeOriginal", "CreateDate"),
        subsec=get("SubSecTimeOriginal"),
        offset_time=get("OffsetTimeOriginal"),
        width=get("ExifImageWidth", "ImageWidth"),
        height=get("ExifImageHeight", "ImageHeight"),
        orientation=get("Orientation"),
        lens=get("LensID", "LensModel", "Lens"),
        focal=get("FocalLength"),
        aperture=get("FNumber"),
        exposure=get("ExposureTime"),
        iso=get("ISO"),
        lat=get("GPSLatitude"),
        lon=get("GPSLongitude"),
        alt=get("GPSAltitude"),
        rating=get("Rating"),
        label=get("Label"),
        json=json.dumps(raw, ensure_ascii=False),
    )


def identity_key(meta: dict) -> str | None:
    """Return the strongest available identity; timestamps remain unconverted."""
    serial, count = meta.get("serial"), meta.get("shutter_count")
    date, subsec = meta.get("dt_original"), meta.get("subsec")
    if serial and count is not None:
        return f"{serial}:{count}"
    if serial and date:
        return f"{serial}:{date}.{subsec or ''}"
    if (
        all(meta.get(key) is not None for key in ("make", "model", "width", "height"))
        and date
        and subsec
    ):
        return f"{meta['make']}{meta['model']}:{date}.{subsec}:{meta['width']}x{meta['height']}"
    return None
