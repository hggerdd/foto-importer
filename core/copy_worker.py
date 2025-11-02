"""Background copy worker for file operations."""
from __future__ import annotations

import shutil
import threading
from enum import Enum
from pathlib import Path
from collections import defaultdict
from collections.abc import Callable


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

    def __init__(self) -> None:
        self._registry = CopyJobRegistry()

    def copy_files(
        self,
        files: list[Path],
        target_folder: Path,
        custom_name: str,
        on_complete: Callable[[str], None] | None = None,
        on_error: Callable[[str, str], None] | None = None,
        on_progress: Callable[[int, int], None] | None = None,
        on_status_change: Callable[[str, JobState], None] | None = None,
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
        """Execute the copy operation."""
        if self._cancel_event.is_set():
            self._set_state(JobState.CANCELLED)
            return

        self._set_state(JobState.RUNNING)

        try:
            main_folder = self.target_folder / self.custom_name
            main_folder.mkdir(parents=True, exist_ok=True)

            files_by_ext: defaultdict[str, list[Path]] = defaultdict(list)
            for file_path in self.files:
                ext = file_path.suffix.lower().lstrip('.')
                if not ext:
                    ext = 'no_extension'
                files_by_ext[ext].append(file_path)

            total_files = len(self.files)
            current_file = 0

            for ext, ext_files in files_by_ext.items():
                if self._cancel_event.is_set():
                    self._set_state(JobState.CANCELLED)
                    return

                ext_folder = main_folder / ext
                ext_folder.mkdir(exist_ok=True)

                for file_path in ext_files:
                    if self._cancel_event.is_set():
                        self._set_state(JobState.CANCELLED)
                        return

                    current_file += 1
                    if self.on_progress:
                        self.on_progress(current_file, total_files)

                    dest_path = ext_folder / file_path.name
                    counter = 1
                    original_dest = dest_path
                    while dest_path.exists():
                        dest_path = original_dest.with_stem(f"{original_dest.stem}_{counter}")
                        counter += 1

                    shutil.copy2(file_path, dest_path)

            self._set_state(JobState.COMPLETED)

            if self.on_complete:
                self.on_complete(self.custom_name)

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
