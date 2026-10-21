"""Deliver worker results without calling Tk from background threads."""
from __future__ import annotations

from collections.abc import Callable
from threading import Lock
import sys
import tkinter as tk

from config.settings import UI_POLL_INTERVAL_MS


class UiDispatcher:
    """Poll queued callbacks on Tk's thread, coalescing progress updates."""

    def __init__(self, root: tk.Misc) -> None:
        self._root = root
        self._lock = Lock()
        self._pending: dict[object, tuple[Callable, tuple]] = {}
        self._closed = False
        self._after_id = root.after(UI_POLL_INTERVAL_MS, self._drain)

    def post(self, callback: Callable, *args: object, key: object = None) -> None:
        """Queue a callback; a shared key keeps only the latest update."""
        with self._lock:
            if not self._closed:
                self._pending[object() if key is None else key] = (callback, args)

    def _drain(self) -> None:
        with self._lock:
            pending = self._pending
            self._pending = {}
        try:
            for callback, args in pending.values():
                if self._closed:
                    break
                try:
                    callback(*args)
                except Exception:
                    self._root.report_callback_exception(*sys.exc_info())
        finally:
            if not self._closed:
                self._after_id = self._root.after(UI_POLL_INTERVAL_MS, self._drain)

    def close(self) -> None:
        """Stop polling and discard callbacks when the UI closes."""
        with self._lock:
            self._closed = True
            self._pending.clear()
        self._root.after_cancel(self._after_id)
