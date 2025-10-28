"""Settings management for camera organizer."""
from __future__ import annotations

import json
from pathlib import Path
from typing import Any


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
                with open(self.config_path, 'r', encoding='utf-8') as f:
                    return json.load(f)
            except (json.JSONDecodeError, OSError):
                return self._get_defaults()
        return self._get_defaults()

    def _get_defaults(self) -> dict[str, Any]:
        """Get default settings."""
        return {
            'last_source_folder': '',
            'last_target_folder': '',
            'preview_count': 10,
            'window_geometry': '1200x800',
        }

    def save(self) -> None:
        """Save settings to file."""
        try:
            with open(self.config_path, 'w', encoding='utf-8') as f:
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
        return self.get('last_source_folder', '')

    @last_source_folder.setter
    def last_source_folder(self, value: str) -> None:
        """Set last used source folder."""
        self.set('last_source_folder', value)

    @property
    def last_target_folder(self) -> str:
        """Get last used target folder."""
        return self.get('last_target_folder', '')

    @last_target_folder.setter
    def last_target_folder(self, value: str) -> None:
        """Set last used target folder."""
        self.set('last_target_folder', value)

    @property
    def preview_count(self) -> int:
        """Get preview count."""
        return self.get('preview_count', 10)

    @preview_count.setter
    def preview_count(self, value: int) -> None:
        """Set preview count."""
        self.set('preview_count', value)

    @property
    def window_geometry(self) -> str:
        """Get window geometry."""
        return self.get('window_geometry', '1200x800')

    @window_geometry.setter
    def window_geometry(self, value: str) -> None:
        """Set window geometry."""
        self.set('window_geometry', value)
