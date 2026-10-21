"""Compute import summaries without filesystem access or UI dependencies."""

from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from threading import Event

PREVIEW_EXTENSIONS = {".jpg", ".jpeg", ".png", ".gif", ".bmp", ".tiff", ".tif"}


@dataclass
class SelectionSummary:
    """Counts for checked days and all previewable images for the focused day."""

    counts: dict[str, int]
    focused_counts: dict[str, int]
    previews: list[Path]
    preview_total: int


def summarize_selection(
    groups: dict[str, list[Path]],
    selected: set[str],
    focused: str | None,
    cancel: Event,
) -> SelectionSummary | None:
    """Count extensions on a worker; stop promptly for superseded selections."""
    counts, focused_counts = Counter(), Counter()
    previews = []
    preview_total = 0
    for day in selected | ({focused} if focused else set()):
        for path in groups.get(day, []):
            if cancel.is_set():
                return None
            extension = path.suffix.lower().lstrip(".") or "no_extension"
            if day in selected:
                counts[extension] += 1
            if day == focused:
                focused_counts[extension] += 1
                if path.suffix.lower() in PREVIEW_EXTENSIONS:
                    preview_total += 1
                    previews.append(path)
    return SelectionSummary(dict(counts), dict(focused_counts), previews, preview_total)


def folder_name_error(name: str) -> str | None:
    """Validate one Windows folder component before joining it to the target."""
    if not name:
        return "Bitte einen Ordnernamen eingeben."
    if name in {".", ".."} or any(c in '<>:"/\\|?*' or ord(c) < 32 for c in name):
        return "Der Ordnername enthält unzulässige Zeichen."
    if name.endswith((".", " ")):
        return "Der Ordnername darf nicht mit Punkt oder Leerzeichen enden."
    reserved = {"CON", "PRN", "AUX", "NUL"} | {
        f"{prefix}{number}" for prefix in ("COM", "LPT") for number in range(1, 10)
    }
    if name.split(".")[0].upper() in reserved:
        return "Dieser Ordnername ist unter Windows reserviert."
    return None
