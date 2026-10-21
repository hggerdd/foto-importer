"""Background copy worker for file operations."""

from __future__ import annotations

import os
import shutil
import threading
from enum import Enum
from pathlib import Path
from collections import defaultdict
from collections.abc import Callable
from contextlib import nullcontext

from config.settings import BACKGROUND_COPY_CHUNK_BYTES
from core.task_scheduler import TaskScheduler, WaitingCallback, background_thread


class JobState(Enum):
    """Lifecycle states for a copy job."""

    QUEUED = "queued"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"


class CopyJobRegistry:
    """Thread-safe registry that tracks active copy jobs."""

    def __init__(self) -> None:
        self._jobs: dict[str, CopyJob] = {}
        self._lock = threading.Lock()

    def register(self, job: CopyJob) -> None:
        """Register a new job with the registry."""
        with self._lock:
            self._jobs[job.custom_name] = job

    def unregister(self, job_name: str) -> None:
        """Remove a job from the registry."""
        with self._lock:
            self._jobs.pop(job_name, None)

    def get(self, job_name: str) -> CopyJob | None:
        """Retrieve a job by name."""
        with self._lock:
            return self._jobs.get(job_name)

    def active_job_count(self) -> int:
        """Return the count of currently running jobs."""
        with self._lock:
            return sum(1 for job in self._jobs.values() if job.is_alive())

    def jobs(self) -> dict[str, CopyJob]:
        """Return a snapshot of all tracked jobs."""
        with self._lock:
            return dict(self._jobs)


class CopyWorker:
    """Handles background file copy operations."""

    def __init__(self, scheduler: TaskScheduler | None = None) -> None:
        self._registry = CopyJobRegistry()
        self._scheduler = scheduler

    def copy_files(
        self,
        files: list[Path],
        target_folder: Path,
        custom_name: str,
        on_complete: Callable[[str], None] | None = None,
        on_error: Callable[[str, str], None] | None = None,
        on_progress: Callable[[int, int], None] | None = None,
        on_status_change: Callable[[str, JobState], None] | None = None,
        on_waiting: WaitingCallback | None = None,
    ) -> CopyJob:
        """Queue a copy job and start it on a background thread."""

        def _state_handler(job_name: str, state: JobState) -> None:
            if on_status_change:
                on_status_change(job_name, state)
            if state in {JobState.COMPLETED, JobState.FAILED, JobState.CANCELLED}:
                self._registry.unregister(job_name)

        job = CopyJob(
            files=files,
            target_folder=target_folder,
            custom_name=custom_name,
            on_complete=on_complete,
            on_error=on_error,
            on_progress=on_progress,
            on_state_change=_state_handler,
            scheduler=self._scheduler,
            on_waiting=on_waiting,
        )

        self._registry.register(job)
        job.start()
        return job

    def cancel_job(self, job_name: str) -> bool:
        """Request cancellation of a running job."""
        job = self._registry.get(job_name)
        if not job:
            return False
        return job.cancel()

    def get_job(self, job_name: str) -> CopyJob | None:
        """Return a job by name."""
        return self._registry.get(job_name)

    def get_active_job_count(self) -> int:
        """Get number of active copy jobs."""
        return self._registry.active_job_count()

    def list_jobs(self) -> dict[str, JobState]:
        """Return current job states."""
        return {name: job.state for name, job in self._registry.jobs().items()}


class CopyJob(threading.Thread):
    """Individual copy job running in a separate thread."""

    def __init__(
        self,
        files: list[Path],
        target_folder: Path,
        custom_name: str,
        on_complete: Callable[[str], None] | None = None,
        on_error: Callable[[str, str], None] | None = None,
        on_progress: Callable[[int, int], None] | None = None,
        on_state_change: Callable[[str, JobState], None] | None = None,
        *,
        scheduler: TaskScheduler | None = None,
        on_waiting: WaitingCallback | None = None,
    ) -> None:
        super().__init__(daemon=True, name=f"CopyJob-{custom_name}")
        self.files: list[Path] = files
        self.target_folder: Path = target_folder
        self.custom_name: str = custom_name
        self.on_complete: Callable[[str], None] | None = on_complete
        self.on_error: Callable[[str, str], None] | None = on_error
        self.on_progress: Callable[[int, int], None] | None = on_progress
        self.on_state_change: Callable[[str, JobState], None] | None = on_state_change

        self._cancel_event = threading.Event()
        self._state_lock = threading.Lock()
        self._state: JobState = JobState.QUEUED
        self.error: str | None = None
        self._scheduler = scheduler
        self._on_waiting = on_waiting

        self._emit_state(JobState.QUEUED)

    def cancel(self) -> bool:
        """Signal the job to stop."""
        with self._state_lock:
            if self._state in {JobState.COMPLETED, JobState.FAILED, JobState.CANCELLED}:
                return False

        if self._cancel_event.is_set():
            return False

        self._cancel_event.set()
        if not self.is_alive():
            # Thread not yet running; emit cancelled immediately.
            self._set_state(JobState.CANCELLED)
        return True

    @property
    def state(self) -> JobState:
        """Return the current job state."""
        with self._state_lock:
            return self._state

    def run(self) -> None:
        """Acquire an execution slot on the worker, never on Tk's thread."""
        try:
            slot = (
                self._scheduler.background_slot(
                    "copy", self._cancel_event, self._on_waiting
                )
                if self._scheduler
                else nullcontext()
            )
            with background_thread(threading.current_thread() is self), slot:
                self._run_copy()
        except InterruptedError:
            self._set_state(JobState.CANCELLED)

    def _checkpoint(self) -> None:
        if self._cancel_event.is_set():
            raise InterruptedError
        if self._scheduler:
            self._scheduler.checkpoint(self._cancel_event, self._on_waiting)

    def _copy_file(self, file_path: Path, destination: Path) -> None:
        """Copy in yielding chunks; remove only our partial output on failure."""
        created = False
        try:
            with file_path.open("rb") as source, destination.open("xb") as output:
                created = True
                while True:
                    self._checkpoint()
                    chunk = source.read(BACKGROUND_COPY_CHUNK_BYTES)
                    if not chunk:
                        break
                    self._checkpoint()
                    output.write(chunk)
            self._checkpoint()
            shutil.copystat(file_path, destination)
        except BaseException:
            if created:
                destination.unlink(missing_ok=True)
            raise

    def _run_copy(self) -> None:
        """Execute the copy operation with cooperative priority checkpoints."""
        if self._cancel_event.is_set():
            self._set_state(JobState.CANCELLED)
            return

        self._set_state(JobState.RUNNING)

        try:
            self._checkpoint()
            main_folder = self.target_folder / self.custom_name
            main_folder.mkdir(parents=True, exist_ok=True)

            files_by_ext: defaultdict[str, list[Path]] = defaultdict(list)
            for file_path in self.files:
                self._checkpoint()
                ext = file_path.suffix.lower().lstrip(".")
                if not ext:
                    ext = "no_extension"
                files_by_ext[ext].append(file_path)

            total_files = len(self.files)
            current_file = 0

            for ext, ext_files in files_by_ext.items():
                if self._cancel_event.is_set():
                    self._set_state(JobState.CANCELLED)
                    return

                ext_folder = main_folder / ext
                ext_folder.mkdir(exist_ok=True)
                next_suffix: dict[str, int] = {}

                for file_path in ext_files:
                    if self._cancel_event.is_set():
                        self._set_state(JobState.CANCELLED)
                        return

                    original_dest = ext_folder / file_path.name
                    name_key = os.path.normcase(file_path.name)
                    counter = next_suffix.get(name_key, 0)
                    dest_path = (
                        original_dest.with_stem(f"{original_dest.stem}_{counter}")
                        if counter
                        else original_dest
                    )
                    while dest_path.exists():
                        counter += 1
                        dest_path = original_dest.with_stem(
                            f"{original_dest.stem}_{counter}"
                        )

                    self._checkpoint()
                    self._copy_file(file_path, dest_path)
                    next_suffix[name_key] = counter + 1
                    current_file += 1
                    if self.on_progress:
                        self.on_progress(current_file, total_files)

            self._set_state(JobState.COMPLETED)

            if self.on_complete:
                self.on_complete(self.custom_name)

        except InterruptedError:
            self._set_state(JobState.CANCELLED)
        except Exception as exc:
            self.error = str(exc)
            self._set_state(JobState.FAILED)
            if self.on_error:
                self.on_error(self.custom_name, str(exc))

    def _emit_state(self, state: JobState) -> None:
        if self.on_state_change:
            self.on_state_change(self.custom_name, state)

    def _set_state(self, state: JobState) -> None:
        with self._state_lock:
            if self._state == state:
                return
            self._state = state
        self._emit_state(state)
