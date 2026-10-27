"""Read photo-level metadata summaries and paginated views of a collection."""

from __future__ import annotations

import io
import subprocess
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageOps

from config.settings import (
    METADATA_PAGE_SIZE,
    METADATA_PREVIEW_SIZE,
    METADATA_PREVIEW_TIMEOUT_SECONDS,
)
from core.collection_index import CollectionIndex, normalized
from core.thumbnails import ThumbnailLoader


@dataclass(frozen=True)
class MetadataFilter:
    search: str = ""
    camera: str = ""
    year: str = ""
    sort: str = "date"
    descending: bool = True
    page: int = 0


PHOTO_ROWS = """
WITH candidates AS (
    SELECT f.id AS file_id, f.photo_id, f.relpath, f.name, f.ext, f.size,
           f.meta_state, m.dt_original, m.model, m.lens, m.iso,
           NULLIF(TRIM(m.model),'') AS camera,
           ROW_NUMBER() OVER (PARTITION BY f.photo_id ORDER BY
               (f.meta_state='done') DESC, (m.dt_original IS NOT NULL) DESC,
               f.ext IN ('nef','nrw','dng') DESC, f.id) AS preference
    FROM files f LEFT JOIN meta m ON m.file_id=f.id
    WHERE f.status='present' AND f.ext!='xmp' AND f.photo_id IS NOT NULL
), photos AS (SELECT * FROM candidates WHERE preference=1)
"""


def load_metadata_page(root: Path, filters: MetadataFilter) -> dict:
    """Count captures once even when RAW, JPEG and sidecars share an identity."""
    if not (root / ".myfotomanager").is_file():
        return dict(
            summary=dict(total=0, metadata=0, cameras=0, lenses=0),
            rows=[],
            cameras=[],
            years=[],
            camera_counts=[],
            iso_counts=[0] * 6,
            last_metadata=None,
            indexed=False,
            page=0,
        )
    with CollectionIndex.open(root, readonly=True) as index:
        db = index.db
        db.execute("PRAGMA temp_store=MEMORY")
        db.create_function("casefold", 1, lambda value: str(value or "").casefold())
        # One snapshot keeps the total, charts and page consistent during scans.
        db.execute("BEGIN")
        db.execute(
            "CREATE TEMP TABLE browse_photos AS " + PHOTO_ROWS + " SELECT * FROM photos"
        )
        cameras = [
            row[0]
            for row in db.execute(
                "SELECT DISTINCT camera FROM browse_photos WHERE camera IS NOT NULL ORDER BY camera"
            )
        ]
        years = [
            row[0]
            for row in db.execute(
                "SELECT DISTINCT substr(dt_original,1,4) AS year FROM browse_photos WHERE dt_original GLOB '[0-9][0-9][0-9][0-9]:*' ORDER BY year DESC"
            )
        ]
        where, params = ["1=1"], []
        if filters.search.strip():
            where.append(
                "(instr(casefold(name),?)>0 OR instr(casefold(relpath),?)>0 OR instr(casefold(camera),?)>0 OR instr(casefold(lens),?)>0)"
            )
            params.extend([normalized(filters.search.strip()).casefold()] * 4)
        if filters.camera:
            where.append("camera=?")
            params.append(filters.camera)
        if filters.year:
            where.append("substr(dt_original,1,4)=?")
            params.append(filters.year)
        db.execute(
            "CREATE TEMP TABLE browse_filtered AS SELECT * FROM browse_photos WHERE "
            + " AND ".join(where),
            params,
        )
        summary = dict(
            db.execute("""SELECT COUNT(*) AS total,
            COALESCE(SUM(meta_state='done'),0) AS metadata,
            COUNT(DISTINCT camera) AS cameras,
            COUNT(DISTINCT NULLIF(TRIM(lens),'')) AS lenses FROM browse_filtered""").fetchone()
        )
        camera_counts = [
            tuple(row)
            for row in db.execute("""SELECT COALESCE(camera,'Unbekannt'),COUNT(*) AS count
            FROM browse_filtered GROUP BY camera ORDER BY count DESC, camera IS NULL, camera LIMIT 5""")
        ]
        bins = [0] * 6
        for row in db.execute("""SELECT CASE WHEN iso<=100 THEN 0 WHEN iso<=200 THEN 1
            WHEN iso<=400 THEN 2 WHEN iso<=800 THEN 3 WHEN iso<=1600 THEN 4 ELSE 5 END AS bin,
            COUNT(*) FROM browse_filtered WHERE typeof(iso) IN ('integer','real') AND iso>0 GROUP BY bin"""):
            bins[row[0]] = row[1]
        sort = {
            "name": "name COLLATE NOCASE",
            "date": "dt_original",
            "camera": "camera",
            "iso": "CAST(iso AS REAL)",
        }.get(filters.sort, "dt_original")
        page = min(
            max(0, filters.page), max(0, (summary["total"] - 1) // METADATA_PAGE_SIZE)
        )
        rows = [
            dict(row)
            for row in db.execute(
                f"SELECT * FROM browse_filtered ORDER BY {sort} {'DESC' if filters.descending else 'ASC'},photo_id LIMIT ? OFFSET ?",
                (METADATA_PAGE_SIZE, page * METADATA_PAGE_SIZE),
            )
        ]
        last = db.execute(
            "SELECT MAX(finished_at) FROM scans WHERE kind='meta' AND status='done'"
        ).fetchone()[0]
        return dict(
            summary=summary,
            rows=rows,
            cameras=cameras,
            years=years,
            camera_counts=camera_counts,
            iso_counts=bins,
            last_metadata=last,
            indexed=True,
            page=page,
        )


def load_metadata_detail(root: Path, photo_id: int) -> dict | None:
    """Return metadata, present variants, and a detached image for one capture."""
    with CollectionIndex.open(root, readonly=True) as index:
        rows = [
            dict(row)
            for row in index.db.execute(
                """SELECT f.id AS file_id, f.*, m.*
            FROM files f LEFT JOIN meta m ON m.file_id=f.id
            WHERE f.photo_id=? AND f.status='present' ORDER BY
            (f.ext='xmp'), (f.meta_state='done') DESC, (m.dt_original IS NOT NULL) DESC,
            f.ext IN ('nef','nrw','dng') DESC, f.id""",
                (photo_id,),
            )
        ]
        media = [row for row in rows if row["ext"] != "xmp"]
        if not media:
            return None
        selected = media[0]
        for sidecar in rows:
            if sidecar["ext"] == "xmp":
                for field in ("rating", "label"):
                    if sidecar.get(field) is not None:
                        selected[field] = sidecar[field]
        image = None
        paths = []
        for row in rows:
            path = index._path(row["relpath"])
            if path.is_symlink() or not path.resolve().is_relative_to(index.root):
                continue
            paths.append((row, path))
        # A paired JPEG supplies the preview of a RAW capture without conversion.
        previews = sorted(
            paths,
            key=lambda item: (
                item[0]["ext"] not in ("jpg", "jpeg", "png", "tif", "tiff")
            ),
        )
        for row, path in previews:
            if row["ext"] in ("xmp", "mov", "mp4"):
                continue
            try:
                image = ThumbnailLoader(capacity=0).load(path, METADATA_PREVIEW_SIZE)
            except (OSError, ValueError):
                continue
            if image is not None:
                break
        if image is None:
            raw = next(
                (path for row, path in paths if row["ext"] in ("nef", "nrw", "dng")),
                None,
            )
            if raw:
                image = _raw_preview(raw, index.settings["exiftool"])
        selected["variants"] = [
            dict(name=row["name"], ext=row["ext"], relpath=row["relpath"])
            for row in rows
        ]
        selected["path"] = next(
            (path for row, path in paths if row["file_id"] == selected["file_id"]), None
        )
        selected["image"] = image
        return selected


def _raw_preview(path: Path, executable: str) -> Image.Image | None:
    """Read an embedded RAW preview; never write or convert the original."""
    for tag in ("PreviewImage", "JpgFromRaw"):
        try:
            result = subprocess.run(
                [executable, "-b", f"-{tag}", str(path)],
                capture_output=True,
                timeout=METADATA_PREVIEW_TIMEOUT_SECONDS,
                creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
            )
            if result.returncode == 0 and result.stdout:
                with Image.open(io.BytesIO(result.stdout)) as source:
                    image = ImageOps.exif_transpose(source)
                    image.thumbnail(METADATA_PREVIEW_SIZE, Image.Resampling.LANCZOS)
                    return image.copy()
        except (OSError, ValueError, subprocess.TimeoutExpired):
            return None
    return None
