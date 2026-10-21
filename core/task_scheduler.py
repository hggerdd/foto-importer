"""Give interactive analysis precedence over bounded background execution."""

from __future__ import annotations

from collections import deque
from collections.abc import Callable, Iterator
from contextlib import contextmanager
import os
from threading import Condition, Event
from time import monotonic

from config.settings import (
    BACKGROUND_COPY_JOBS,
    BACKGROUND_DNG_JOBS,
    FOREGROUND_SETTLE_SECONDS,
)

WaitingCallback = Callable[[str | None], None]


class ForegroundLease:
    """Release a foreground reservation exactly once, including cancelled futures."""

    def __init__(self, scheduler: TaskScheduler) -> None:
        self._scheduler = scheduler
        self._closed = False

    def close(self) -> None:
        with self._scheduler._condition:
            if self._closed:
                return
            self._closed = True
            self._scheduler._foreground -= 1
            self._scheduler._resume_at = monotonic() + self._scheduler._settle_seconds
            self._scheduler._condition.notify_all()


class TaskScheduler:
    """Reserve foreground immediately; queue background lanes in FIFO order.

    Waiting happens exclusively on workers. No file access, sleeps or joins occur
    on the Tk thread. Existing OS I/O and running Adobe files cannot be preempted.
    """

    def __init__(
        self,
        copy_jobs: int = BACKGROUND_COPY_JOBS,
        dng_jobs: int = BACKGROUND_DNG_JOBS,
        settle_seconds: float = FOREGROUND_SETTLE_SECONDS,
    ) -> None:
        if copy_jobs < 1 or dng_jobs < 1 or settle_seconds < 0:
            raise ValueError(
                "Worker limits must be positive and settle time nonnegative"
            )
        self._condition = Condition()
        self._foreground = 0
        self._resume_at = 0.0
        self._settle_seconds = settle_seconds
        self._limits = {"copy": copy_jobs, "dng": dng_jobs}
        self._active = {"copy": 0, "dng": 0}
        self._queues = {lane: deque() for lane in self._limits}

    def foreground(self) -> ForegroundLease:
        """Reserve before launching work so background cannot jump the queue."""
        with self._condition:
            self._foreground += 1
            self._condition.notify_all()
        return ForegroundLease(self)

    def _foreground_busy(self) -> bool:
        return self._foreground > 0 or monotonic() < self._resume_at

    def _wait(
        self,
        cancel: Event,
        ready: Callable[[], bool],
        acquired: Callable[[], None],
        on_waiting: WaitingCallback | None,
    ) -> None:
        previous = None
        try:
            while True:
                with self._condition:
                    if cancel.is_set():
                        raise InterruptedError
                    if not self._foreground_busy() and ready():
                        acquired()
                        return
                    reason = (
                        "Analyse/Vorschau hat Vorrang"
                        if self._foreground_busy()
                        else "Wartet auf freien Ausführungsplatz"
                    )
                if on_waiting and reason != previous:
                    on_waiting(reason)
                previous = reason
                with self._condition:
                    self._condition.wait(timeout=0.05)
        finally:
            if previous is not None and on_waiting:
                on_waiting(None)

    def checkpoint(
        self, cancel: Event, on_waiting: WaitingCallback | None = None
    ) -> None:
        """Yield between bounded chunks, remaining cancellable while paused."""
        self._wait(cancel, lambda: True, lambda: None, on_waiting)

    @contextmanager
    def background_slot(
        self, lane: str, cancel: Event, on_waiting: WaitingCallback | None = None
    ) -> Iterator[None]:
        """Limit simultaneous copy/converter jobs without limiting queued jobs."""
        token = object()
        acquired = False
        with self._condition:
            self._queues[lane].append(token)

        def ready() -> bool:
            return (
                self._queues[lane][0] is token
                and self._active[lane] < self._limits[lane]
            )

        def reserve() -> None:
            nonlocal acquired
            self._queues[lane].popleft()
            self._active[lane] += 1
            acquired = True
            self._condition.notify_all()

        try:
            self._wait(cancel, ready, reserve, on_waiting)
            yield
        finally:
            with self._condition:
                if acquired:
                    self._active[lane] -= 1
                else:
                    self._queues[lane].remove(token)
                self._condition.notify_all()


@contextmanager
def background_thread(enabled: bool = True) -> Iterator[None]:
    """Lower this worker's Windows CPU, memory and I/O scheduling priority.

    Best effort: unsupported platforms retain cooperative scheduling. Never
    lower the whole GUI process or change another thread's priority.
    """
    kernel = None
    handle = None
    entered = False
    if enabled and os.name == "nt":
        import ctypes
        from ctypes import wintypes

        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetCurrentThread.argtypes = []
        kernel.GetCurrentThread.restype = wintypes.HANDLE
        kernel.SetThreadPriority.argtypes = [wintypes.HANDLE, ctypes.c_int]
        kernel.SetThreadPriority.restype = wintypes.BOOL
        handle = kernel.GetCurrentThread()
        entered = bool(kernel.SetThreadPriority(handle, 0x00010000))
    try:
        yield
    finally:
        if entered:
            kernel.SetThreadPriority(handle, 0x00020000)
