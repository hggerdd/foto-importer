"""File management utilities for camera organizer."""
from __future__ import annotations

from enum import Enum
from pathlib import Path
from datetime import datetime
from collections import defaultdict
import os
from collections.abc import Callable
from itertools import islice
from threading import Event

from PIL import Image, ExifTags

EXIF_DATE_TAGS = {
    "DateTimeOriginal",
    "DateTimeDigitized",
    "DateTime",
}

EXIF_TAG_LOOKUP = {value: key for key, value in ExifTags.TAGS.items()}

IMAGE_METADATA_EXTENSIONS = {
    '.jpg', '.jpeg', '.tiff', '.tif', '.png', '.bmp',
}


class ScanCancelledError(RuntimeError):
    """Raised when a running scan is cancelled."""


class DateSource(Enum):
    """Preferred date source for grouping logic."""

    FILESYSTEM = "filesystem"
    METADATA = "metadata"

    @classmethod
    def from_value(cls, value: str) -> DateSource:
        """Resolve enum from a persisted value with graceful fallback."""
        try:
            return cls(value)
        except ValueError:
            return cls.FILESYSTEM


class FileManager:
    """Manages file scanning and organization by date."""

    # Common camera file extensions
    SUPPORTED_EXTENSIONS = {
        '.jpg', '.jpeg', '.png', '.gif', '.bmp', '.tiff', '.tif',  # Images
        '.nef', '.cr2', '.cr3', '.arw', '.dng', '.raw', '.orf',    # RAW formats
        '.mp4', '.mov', '.avi', '.mkv', '.m4v',                     # Videos
    }

    def __init__(self) -> None:
        self.source_folder: Path | None = None
        self.files_by_date: dict[str, list[Path]] = {}
        self.date_source: DateSource = DateSource.FILESYSTEM

    def set_source_folder(self, folder_path: str) -> dict[str, list[Path]]:
        """Set the source folder and scan for files."""
        source_folder = Path(folder_path)
        files_by_date = self._scan_files(source_folder)
        self.source_folder = source_folder
        self.files_by_date = files_by_date
        return self.files_by_date

    def gather_files_by_date(
        self,
        folder_path: Path,
        *,
        on_progress: Callable[[int, int], None] | None = None,
        cancel_event: Event | None = None,
    ) -> dict[str, list[Path]]:
        """Collect files by date without mutating manager state."""
        return self._scan_files(
            folder_path,
            on_progress=on_progress,
            cancel_event=cancel_event,
        )

    def apply_scan_results(
        self,
        source_folder: Path,
        files_by_date: dict[str, list[Path]]
    ) -> None:
        """Apply externally gathered scan results to the manager state."""
        self.source_folder = source_folder
        self.files_by_date = files_by_date

    def set_date_source(self, source: DateSource) -> None:
        """Change the active date source for future scans."""
        self.date_source = source

    def _scan_files(
        self,
        source_folder: Path | None,
        *,
        on_progress: Callable[[int, int], None] | None = None,
        cancel_event: Event | None = None,
    ) -> dict[str, list[Path]]:
        """Scan a folder and group files by creation date."""
        if not source_folder or not source_folder.exists():
            return {}

        files_by_date: defaultdict[str, list[Path]] = defaultdict(list)

        if cancel_event and cancel_event.is_set():
            raise ScanCancelledError()

        # DirEntry reuses timestamps returned by directory enumeration on Windows.
        # Check cancellation during discovery, including unsupported files.
        date_source = self.date_source
        candidates: list[tuple[Path, os.stat_result]] = []
        directories = [source_folder]
        while directories:
            directory = directories.pop()
            try:
                with os.scandir(directory) as entries:
                    for entry in entries:
                        if cancel_event and cancel_event.is_set():
                            raise ScanCancelledError()
                        try:
                            if entry.is_dir(follow_symlinks=False):
                                directories.append(Path(entry.path))
                            elif (
                                os.path.splitext(entry.name)[1].lower()
                                in self.SUPPORTED_EXTENSIONS and entry.is_file()
                            ):
                                candidates.append((Path(entry.path), entry.stat()))
                        except OSError as exc:
                            print(f"Failed to inspect {entry.path}: {exc}")
            except OSError as exc:
                if directory == source_folder:
                    raise
                print(f"Failed to scan {directory}: {exc}")

        total_files = len(candidates)
        if on_progress:
            on_progress(0, total_files)

        processed = 0

        # Process collected files while checking for cancellation
        for file_path, stat_result in candidates:
            if cancel_event and cancel_event.is_set():
                raise ScanCancelledError()

            date_str = self._resolve_date_for_file(
                file_path, stat_result, date_source=date_source
            )
            if date_str:
                files_by_date[date_str].append(file_path)

            processed += 1
            if on_progress:
                on_progress(processed, total_files)

        # Sort files within each date
        for date_str in files_by_date:
            files_by_date[date_str].sort()

        return dict(sorted(files_by_date.items()))

    def get_date_groups(self) -> list[str]:
        """Get list of date groups (sorted)."""
        return list(self.files_by_date.keys())

    def get_files_for_date(self, date_str: str) -> list[Path]:
        """Get all files for a specific date."""
        return self.files_by_date.get(date_str, [])

    def get_image_files_for_preview(self, date_str: str, limit: int = 10) -> list[Path]:
        """Get image files for preview (limited count)."""
        files = self.get_files_for_date(date_str)

        # Filter to only image extensions (not videos)
        image_extensions = {'.jpg', '.jpeg', '.png', '.gif', '.bmp', '.tiff', '.tif'}
        image_files = (f for f in files if f.suffix.lower() in image_extensions)
        return list(islice(image_files, max(0, limit)))

    def get_file_count(self, date_str: str) -> int:
        """Get total file count for a date."""
        return len(self.files_by_date.get(date_str, []))

    def remove_date_group(self, date_str: str) -> None:
        """Remove a date group from the list (after copying)."""
        if date_str in self.files_by_date:
            del self.files_by_date[date_str]

    def _resolve_date_for_file(
        self, file_path: Path, stat_result: os.stat_result | None = None,
        *, date_source: DateSource | None = None,
    ) -> str | None:
        """Determine the grouping date for a file."""
        if (date_source or self.date_source) is DateSource.METADATA:
            metadata_date = self._get_metadata_date(file_path)
            if metadata_date:
                return metadata_date

        return self._get_filesystem_date(file_path, stat_result)

    def _get_filesystem_date(
        self, file_path: Path, stat_result: os.stat_result | None = None,
    ) -> str | None:
        """Fetch date based on filesystem timestamps."""
        try:
            stat_result = stat_result or file_path.stat()
            if os.name == 'nt':  # Windows
                timestamp = stat_result.st_ctime
            else:
                # On Unix, st_ctime is metadata change time, use st_mtime
                timestamp = stat_result.st_mtime
            return datetime.fromtimestamp(timestamp).strftime('%Y-%m-%d')
        except OSError as exc:
            print(f"Failed to read filesystem timestamp for {file_path}: {exc}")
            return None

    def _get_metadata_date(self, file_path: Path) -> str | None:
        """Attempt to derive capture date from photo metadata."""
        if file_path.suffix.lower() not in IMAGE_METADATA_EXTENSIONS:
            return None

        try:
            with Image.open(file_path) as img:
                metadata_date = self._extract_exif_date(img)
                if metadata_date:
                    return metadata_date

                metadata_date = self._extract_iptc_date(img)
                if metadata_date:
                    return metadata_date
        except Exception as exc:  # noqa: BLE001 - any metadata failure falls back
            print(f"Failed to read metadata for {file_path}: {exc}")
            return None

        return None

    def _extract_exif_date(self, image: Image.Image) -> str | None:
        """Extract capture date from EXIF tags."""
        try:
            exif = image.getexif()
        except AttributeError:
            return None

        if not exif:
            return None

        for tag_name in EXIF_DATE_TAGS:
            tag_id = EXIF_TAG_LOOKUP.get(tag_name)
            if tag_id is None:
                continue

            raw_value = exif.get(tag_id)
            if not raw_value:
                continue

            parsed = self._parse_exif_datetime(str(raw_value))
            if parsed:
                return parsed

        return None

    def _extract_iptc_date(self, image: Image.Image) -> str | None:
        """Extract capture date from IPTC tags (APP13)."""
        try:
            iptc_info = image.getiptcinfo()  # type: ignore[attr-defined]
        except Exception:
            return None

        if not iptc_info:
            return None

        date_bytes = iptc_info.get((2, 55))  # DateCreated
        if not date_bytes:
            return None

        try:
            date_str = date_bytes.decode('utf-8', errors='ignore')
        except AttributeError:
            date_str = str(date_bytes)

        return self._parse_iptc_date(date_str)

    def _parse_exif_datetime(self, value: str) -> str | None:
        """Parse EXIF datetime string into YYYY-MM-DD."""
        for fmt in ("%Y:%m:%d %H:%M:%S", "%Y:%m:%d"):
            try:
                return datetime.strptime(value, fmt).strftime("%Y-%m-%d")
            except ValueError:
                continue
        return None

    def _parse_iptc_date(self, value: str) -> str | None:
        """Parse IPTC date string into YYYY-MM-DD."""
        sanitized = value.strip()
        if not sanitized:
            return None

        candidates = [
            ("%Y%m%d", sanitized),
            ("%Y-%m-%d", sanitized),
        ]

        for fmt, candidate in candidates:
            try:
                return datetime.strptime(candidate, fmt).strftime("%Y-%m-%d")
            except ValueError:
                continue
        return None
