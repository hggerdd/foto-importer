"""Run potentially slow target and converter probes outside Tk."""

from pathlib import Path
from threading import Thread
from collections.abc import Callable
import tkinter as tk

from ui.dispatcher import UiDispatcher
from core.task_scheduler import TaskScheduler


class ImportPreflight:
    """Check one captured import destination without blocking the UI."""

    def __init__(self, root: tk.Misc, scheduler: TaskScheduler | None = None) -> None:
        self._scheduler = scheduler
        self._dispatcher = UiDispatcher(root)

    def check(
        self,
        target: Path,
        converter: Path | None,
        groups: dict[str, list[Path]],
        callback: Callable[[bool, str | None, list[Path]], None],
    ) -> None:
        def work():
            try:
                if converter is not None and not converter.is_file():
                    raise ValueError(
                        f"Adobe DNG Converter nicht gefunden:\n{converter}"
                    )
                exists = target.exists()
                files = [path for paths in groups.values() for path in paths]
            except (OSError, ValueError) as exc:
                self._dispatcher.post(callback, False, str(exc), [])
            else:
                self._dispatcher.post(callback, exists, None, files)

        lease = self._scheduler.foreground() if self._scheduler else None

        def run():
            try:
                work()
            finally:
                if lease:
                    lease.close()

        Thread(target=run, name="ImportPreflight", daemon=True).start()

    def close(self) -> None:
        self._dispatcher.close()
