"""Settings management for camera organizer."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any


UI_POLL_INTERVAL_MS = 50
BACKGROUND_COPY_JOBS = 1
BACKGROUND_DNG_JOBS = 1
BACKGROUND_COPY_CHUNK_BYTES = 1024 * 1024
FOREGROUND_SETTLE_SECONDS = 0.15
THUMBNAIL_CACHE_SIZE = 128
PREVIEW_PREFETCH_ROWS = 2
METADATA_PAGE_SIZE = 200
METADATA_SEARCH_DELAY_MS = 250
METADATA_PREVIEW_SIZE = (480, 285)
METADATA_PREVIEW_TIMEOUT_SECONDS = 10
FACE_MODEL_DIR = Path(__file__).resolve().parents[1] / "models"
FACE_MAX_EDGE = 1600
FACE_BATCH_SIZE = 32
FACE_MIN_QUALITY = 0.5
FACE_GROUPS_PER_PAGE = 8
FACE_GROUP_PREVIEWS = 8
FACE_GROUP_THUMBNAIL_SIZE = 104
FACE_NAMING_UNDO_LIMIT = 20
FACE_DEFAULTS = {
    "active_detector": "yunet",
    "active_embedder": "sface",
    "min_score": 0.8,
    "min_quality": FACE_MIN_QUALITY,
    "reference_limit": 100,
    "validation_fraction": 0.2,
    "seed": 42,
    "auto_import": False,
    "nightly": False,
}
DNG_CONVERTER_PATH = (
    r"C:\Program Files\Adobe\Adobe DNG Converter\Adobe DNG Converter.exe"
)
DNG_DOWNLOAD_URL = "https://adobe.com/go/dng_converter_win/"
DNG_UPDATE_TIMEOUT_SECONDS = 5


class Settings:
    """Manages application settings with persistent storage."""

    def __init__(self, config_file: str = "camera_organizer_settings.json") -> None:
        """Initialize settings manager.

        Args:
            config_file: Name of the settings file (stored in user's home directory)
        """
        self.config_path: Path = Path.home() / config_file
        self.settings: dict[str, Any] = self._load_settings()

    def _load_settings(self) -> dict[str, Any]:
        """Load settings from file."""
        if self.config_path.exists():
            try:
                with open(self.config_path, "r", encoding="utf-8") as f:
                    return json.load(f)
            except (json.JSONDecodeError, OSError):
                return self._get_defaults()
        return self._get_defaults()

    def _get_defaults(self) -> dict[str, Any]:
        """Get default settings."""
        return {
            "last_source_folder": "",
            "last_target_folder": "",
            "window_geometry": "1500x980",
            "date_source": "filesystem",
            "convert_to_dng": False,
            "dng_converter_path": DNG_CONVERTER_PATH,
        }

    def save(self) -> None:
        """Save settings to file."""
        try:
            with open(self.config_path, "w", encoding="utf-8") as f:
                json.dump(self.settings, f, indent=2)
        except OSError as e:
            print(f"Failed to save settings: {e}")

    def get(self, key: str, default: Any = None) -> Any:
        """Get a setting value.

        Args:
            key: Setting key
            default: Default value if key not found

        Returns:
            Setting value or default
        """
        return self.settings.get(key, default)

    def set(self, key: str, value: Any) -> None:
        """Set a setting value.

        Args:
            key: Setting key
            value: Setting value
        """
        self.settings[key] = value
        self.save()

    @property
    def last_source_folder(self) -> str:
        """Get last used source folder."""
        return self.get("last_source_folder", "")

    @last_source_folder.setter
    def last_source_folder(self, value: str) -> None:
        """Set last used source folder."""
        self.set("last_source_folder", value)

    @property
    def last_target_folder(self) -> str:
        """Get last used target folder."""
        return self.get("last_target_folder", "")

    @last_target_folder.setter
    def last_target_folder(self, value: str) -> None:
        """Set last used target folder."""
        self.set("last_target_folder", value)

    @property
    def window_geometry(self) -> str:
        """Get window geometry."""
        return self.get("window_geometry", "1200x800")

    @window_geometry.setter
    def window_geometry(self, value: str) -> None:
        """Set window geometry."""
        self.set("window_geometry", value)

    @property
    def date_source(self) -> str:
        """Get preferred date source."""
        return self.get("date_source", "filesystem")

    @date_source.setter
    def date_source(self, value: str) -> None:
        """Set preferred date source."""
        self.set("date_source", value)
