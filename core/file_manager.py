"""File management utilities for camera organizer."""
from __future__ import annotations

from pathlib import Path
from datetime import datetime
from collections import defaultdict
import os


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

    def set_source_folder(self, folder_path: str) -> dict[str, list[Path]]:
        """Set the source folder and scan for files."""
        self.source_folder = Path(folder_path)
        self.files_by_date = self._scan_files()
        return self.files_by_date

    def _scan_files(self) -> dict[str, list[Path]]:
        """Scan source folder and group files by creation date."""
        if not self.source_folder or not self.source_folder.exists():
            return {}

        files_by_date: defaultdict[str, list[Path]] = defaultdict(list)

        # Recursively find all supported files
        for file_path in self.source_folder.rglob('*'):
            if not file_path.is_file():
                continue

            # Check if file extension is supported
            if file_path.suffix.lower() not in self.SUPPORTED_EXTENSIONS:
                continue

            # Get creation date
            try:
                # Try to get creation time, fall back to modification time
                if os.name == 'nt':  # Windows
                    creation_time = file_path.stat().st_ctime
                else:  # Unix-like systems
                    # On Unix, st_ctime is metadata change time, use st_mtime
                    creation_time = file_path.stat().st_mtime

                date_str = datetime.fromtimestamp(creation_time).strftime('%Y-%m-%d')
                files_by_date[date_str].append(file_path)
            except Exception as e:
                print(f"Error reading file {file_path}: {e}")
                continue

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
        image_files = [f for f in files if f.suffix.lower() in image_extensions]

        return image_files[:limit]

    def get_file_count(self, date_str: str) -> int:
        """Get total file count for a date."""
        return len(self.files_by_date.get(date_str, []))

    def remove_date_group(self, date_str: str) -> None:
        """Remove a date group from the list (after copying)."""
        if date_str in self.files_by_date:
            del self.files_by_date[date_str]
