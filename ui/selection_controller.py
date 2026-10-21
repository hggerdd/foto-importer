"""Cancellable summary computation, independent of the Tk event loop."""

from concurrent.futures import ThreadPoolExecutor
from threading import Event
from collections.abc import Callable
from pathlib import Path
import tkinter as tk

from core.import_plan import SelectionSummary, summarize_selection
from ui.dispatcher import UiDispatcher
from core.task_scheduler import TaskScheduler


class SelectionController:
    """Only publish the latest selection and cancel superseded computation."""

    def __init__(self, root: tk.Misc, scheduler: TaskScheduler | None = None) -> None:
        self._scheduler = scheduler
        self._dispatcher = UiDispatcher(root)
        self._executor = ThreadPoolExecutor(
            max_workers=1, thread_name_prefix="Selection"
        )
        self._cancel = Event()
        self._future = None
        self._generation = 0

    def cancel(self) -> None:
        self._generation += 1
        self._cancel.set()
        if self._future:
            self._future.cancel()

    def request(
        self,
        groups: dict[str, list[Path]],
        selected: set[str],
        focused: str | None,
        limit: int,
        callback: Callable[[SelectionSummary], None],
    ) -> None:
        """Capture list references; the importer never mutates their contents."""
        self.cancel()
        generation = self._generation
        cancel = self._cancel = Event()
        snapshot = {
            day: groups[day]
            for day in selected | ({focused} if focused else set())
            if day in groups
        }

        def work():
            result = summarize_selection(snapshot, selected, focused, limit, cancel)
            if result is not None:
                self._dispatcher.post(deliver, result, key="summary")

        def deliver(result):
            if generation == self._generation:
                callback(result)

        lease = self._scheduler.foreground() if self._scheduler else None
        try:
            self._future = self._executor.submit(work)
        except BaseException:
            if lease:
                lease.close()
            raise
        if lease:
            self._future.add_done_callback(lambda _: lease.close())

    def close(self) -> None:
        self.cancel()
        self._executor.shutdown(wait=False, cancel_futures=True)
        self._dispatcher.close()
