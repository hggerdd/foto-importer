"""Decode just the latest viewport with one running and one pending request."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path
from threading import Condition, Event, Thread

from PIL import Image

from core.task_scheduler import ForegroundLease, TaskScheduler
from core.thumbnails import ThumbnailLoader

ResultCallback = Callable[[int, int, Image.Image | None], None]


@dataclass
class PreviewRequest:
    generation: int
    paths: list[tuple[int, Path]]
    size: tuple[int, int]
    callback: ResultCallback
    lease: ForegroundLease | None
    cancel: Event = field(default_factory=Event)


class PreviewWorker:
    """Keep decoding and filesystem access off Tk, with bounded queued work."""

    def __init__(
        self, loader: ThumbnailLoader, scheduler: TaskScheduler | None = None
    ) -> None:
        self._loader = loader
        self._scheduler = scheduler
        self._condition = Condition()
        self._pending: PreviewRequest | None = None
        self._active: PreviewRequest | None = None
        self._closed = False
        self._thread = Thread(target=self._run, name="Preview", daemon=True)
        self._thread.start()

    def request(
        self,
        generation: int,
        paths: list[tuple[int, Path]],
        size: tuple[int, int],
        callback: ResultCallback,
    ) -> None:
        """Replace queued work; a cancelled decoder finishes only its current file."""
        lease = self._scheduler.foreground() if self._scheduler else None
        request = PreviewRequest(generation, paths, size, callback, lease)
        with self._condition:
            previous = self._pending
            if self._active:
                self._active.cancel.set()
            self._pending = None if self._closed else request
            closed = self._closed
            self._condition.notify()
        if previous and previous.lease:
            previous.lease.close()
        if closed and lease:
            lease.close()

    def cancel(self) -> None:
        """Discard pending work immediately without joining an active decoder."""
        with self._condition:
            previous, self._pending = self._pending, None
            if self._active:
                self._active.cancel.set()
        if previous and previous.lease:
            previous.lease.close()

    def _run(self) -> None:
        while True:
            with self._condition:
                self._condition.wait_for(
                    lambda: self._closed or self._pending is not None
                )
                if self._closed:
                    return
                request = self._active = self._pending
                self._pending = None
            try:
                for index, path in request.paths:
                    if request.cancel.is_set():
                        break
                    try:
                        thumbnail = self._loader.load(path, request.size)
                    except Exception:
                        thumbnail = None
                    if request.cancel.is_set():
                        break
                    request.callback(request.generation, index, thumbnail)
            finally:
                if request.lease:
                    request.lease.close()
                with self._condition:
                    self._active = None

    def close(self) -> None:
        """Stop accepting requests; no blocking joins on the UI thread."""
        with self._condition:
            self._closed = True
        self.cancel()
        with self._condition:
            self._condition.notify()
