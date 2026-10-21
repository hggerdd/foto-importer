"""Check DNG updates in a worker and deliver results on Tk's thread."""

from __future__ import annotations

from collections.abc import Callable
from datetime import date
from pathlib import Path
import threading
import tkinter as tk

from config.settings import Settings
from core.dng_updates import installed_version, latest_version, version_tuple
from ui.dispatcher import UiDispatcher


class DngUpdateController:
    """Persist daily attempts, including failures, across application starts."""

    def __init__(
        self, root: tk.Misc, settings: Settings, on_result: Callable[[str, bool], None]
    ) -> None:
        self._dispatcher = UiDispatcher(root)
        self._settings = settings
        self._on_result = on_result
        self._request_id = 0

    def start(self, converter: Path) -> None:
        """Reserve today's attempt before doing background I/O."""
        today = date.today().isoformat()
        self._request_id += 1
        cached = self._settings.get("dng_update_check", {})
        if not isinstance(cached, dict):
            cached = {}
        check_online = cached.get("date") != today
        if check_online:
            cached = {"date": today, "latest": None}
            self._settings.set("dng_update_check", cached)
        threading.Thread(
            target=self._check,
            args=(converter, cached.copy(), check_online, self._request_id),
            daemon=True,
            name="DngUpdateCheck",
        ).start()

    def _check(
        self, converter: Path, cached: dict, check_online: bool, request_id: int
    ) -> None:
        try:
            installed = installed_version(converter)
        except (OSError, ValueError):
            installed = None
        if check_online:
            try:
                cached["latest"] = latest_version()
            except (OSError, ValueError):
                cached["latest"] = None
        self._dispatcher.post(
            self._finished, installed, cached, check_online, request_id
        )

    def _finished(
        self, installed: str | None, cached: dict, save: bool, request_id: int
    ) -> None:
        if save:
            self._settings.set("dng_update_check", cached)
        if request_id != self._request_id:
            return
        if not save:
            cached = self._settings.get("dng_update_check", cached)
        latest = cached.get("latest")
        try:
            newer = bool(
                installed
                and latest
                and version_tuple(latest) > version_tuple(installed)
            )
            if latest:
                version_tuple(latest)
        except (TypeError, ValueError):
            latest, newer = None, False
        text = f"DNG Converter: {installed or 'not found / version unavailable'}"
        if newer:
            text += f" — Update available: {latest}"
        elif latest and installed:
            text += f" — Up to date (Adobe: {latest})"
        elif latest:
            text += f" — Adobe: {latest}"
        else:
            text += " — Update check unavailable; next attempt tomorrow"
        self._on_result(text, newer)

    def close(self) -> None:
        """Discard pending results without waiting for the network."""
        self._dispatcher.close()
