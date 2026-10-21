"""Convert a folder's NEF files using independent Adobe child processes."""

from __future__ import annotations

import os
from pathlib import Path
import subprocess
import tempfile
import threading
from collections.abc import Callable
from contextlib import nullcontext

from core.task_scheduler import TaskScheduler, WaitingCallback, background_thread

from core.copy_worker import JobState


class DngJob(threading.Thread):
    """Run Adobe outside the UI process; preserve RAWs and existing DNGs."""

    def __init__(
        self,
        folder: Path,
        converter: Path,
        on_progress: Callable[[int, int], None],
        on_finished: Callable[[JobState, str], None],
        *,
        scheduler: TaskScheduler | None = None,
        on_waiting: WaitingCallback | None = None,
    ) -> None:
        # Keep Python alive briefly during shutdown so cancellation can reap
        # Adobe and remove staging files after the Tk window is destroyed.
        super().__init__(daemon=False, name=f"DngJob-{folder.name}")
        self.folder = folder
        self.converter = converter
        self._scheduler = scheduler
        self._on_waiting = on_waiting
        self.on_progress = on_progress
        self.on_finished = on_finished
        self._cancel_event = threading.Event()
        self.state = JobState.QUEUED
        self.skipped = 0

    def cancel(self) -> bool:
        """Request termination of this folder's current converter process."""
        if self.state in {JobState.COMPLETED, JobState.FAILED, JobState.CANCELLED}:
            return False
        if self._cancel_event.is_set():
            return False
        self._cancel_event.set()
        return True

    def run(self) -> None:
        """Queue execution independently of the UI and yield to analysis."""
        try:
            slot = (
                self._scheduler.background_slot(
                    "dng", self._cancel_event, self._on_waiting
                )
                if self._scheduler
                else nullcontext()
            )
            with background_thread(threading.current_thread() is self), slot:
                self._run_conversion()
        except InterruptedError:
            self.state = JobState.CANCELLED
            self.on_finished(self.state, "Conversion cancelled while waiting")

    def _checkpoint(self) -> None:
        self._check_cancelled()
        if self._scheduler:
            self._scheduler.checkpoint(self._cancel_event, self._on_waiting)

    def _run_conversion(self) -> None:
        """Convert serially per folder; never publish incomplete DNGs."""
        self.state = JobState.RUNNING
        message = ""
        try:
            self._checkpoint()
            self.folder = self.folder.resolve()
            self.converter = self.converter.resolve()
            nef_folder = self.folder / "nef"
            files = (
                sorted(
                    (
                        path
                        for path in nef_folder.iterdir()
                        if path.is_file() and path.suffix.lower() == ".nef"
                    ),
                    key=lambda path: path.name.lower(),
                )
                if nef_folder.is_dir()
                else []
            )
            self.on_progress(0, len(files))
            if files and not self.converter.is_file():
                raise FileNotFoundError(
                    f"Adobe DNG Converter not found: {self.converter}"
                )

            existing = {path.name.casefold() for path in self.folder.iterdir()}
            for current, source in enumerate(files, 1):
                self._checkpoint()
                destination = self.folder / f"{source.stem}.dng"
                if destination.name.casefold() in existing:
                    self.skipped += 1
                else:
                    self._convert(source, destination)
                    existing.add(destination.name.casefold())
                self.on_progress(current, len(files))
            self._check_cancelled()
            self.state = JobState.COMPLETED
            message = (
                f"{len(files) - self.skipped} converted, "
                f"{self.skipped} existing DNGs skipped"
            )
        except InterruptedError:
            self.state = JobState.CANCELLED
            message = "Conversion cancelled; copied files and finished DNGs retained"
        except Exception as exc:
            self.state = JobState.FAILED
            message = str(exc)
        self.on_finished(self.state, message)

    def _check_cancelled(self) -> None:
        if self._cancel_event.is_set():
            raise InterruptedError

    def _convert(self, source: Path, destination: Path) -> None:
        self._checkpoint()
        # Stage output so failures/termination never leave a final partial DNG.
        with tempfile.TemporaryDirectory(prefix=".dng-", dir=self.folder) as temporary:
            staging = Path(temporary)
            with (staging / "converter.log").open("w+b") as log:
                flags = 0
                if os.name == "nt":
                    flags = subprocess.CREATE_NO_WINDOW | subprocess.IDLE_PRIORITY_CLASS
                process = subprocess.Popen(
                    [str(self.converter), "-d", str(staging), str(source)],
                    cwd=self.folder,
                    stdin=subprocess.DEVNULL,
                    stdout=log,
                    stderr=subprocess.STDOUT,
                    creationflags=flags,
                )
                try:
                    while True:
                        self._check_cancelled()
                        try:
                            returncode = process.wait(timeout=0.1)
                            break
                        except subprocess.TimeoutExpired:
                            continue
                finally:
                    if process.poll() is None:
                        process.kill()
                    process.wait()
                self._check_cancelled()
                outputs = [
                    path
                    for path in staging.iterdir()
                    if path.is_file() and path.suffix.lower() == ".dng"
                ]
                if (
                    returncode != 0
                    or len(outputs) != 1
                    or outputs[0].stat().st_size == 0
                ):
                    log.seek(0, os.SEEK_END)
                    log.seek(max(0, log.tell() - 2000))
                    detail = log.read().decode(errors="replace").strip()
                    raise RuntimeError(
                        f"{source.name}: converter exit code {returncode}; "
                        f"expected one non-empty DNG. {detail}"
                    )
            self._checkpoint()
            # Exclusive creation protects outputs added by another program too.
            try:
                output = destination.open("xb")
            except FileExistsError:
                self.skipped += 1
                return
            try:
                with output, outputs[0].open("rb") as converted:
                    while chunk := converted.read(1024 * 1024):
                        self._checkpoint()
                        output.write(chunk)
            except BaseException:
                destination.unlink(missing_ok=True)
                raise
