"""Background copy worker for file operations."""
from __future__ import annotations

import threading
import shutil
from pathlib import Path
from collections.abc import Callable
from collections import defaultdict


class CopyWorker:
    """Handles background file copy operations."""

    def __init__(self) -> None:
        self.active_jobs: list[CopyJob] = []
        self.completed_jobs: list[CopyJob] = []

    def copy_files(
        self,
        files: list[Path],
        target_folder: Path,
        custom_name: str,
        on_complete: Callable[[str], None] | None = None,
        on_error: Callable[[str, str], None] | None = None,
        on_progress: Callable[[int, int], None] | None = None
    ) -> None:
        """
        Copy files to target folder with organized structure.

        Structure: target_folder/custom_name/extension/files

        Args:
            files: List of file paths to copy
            target_folder: Base target folder
            custom_name: Custom name for the group folder
            on_complete: Callback when copy is complete (receives custom_name)
            on_error: Callback on error (receives custom_name, error message)
            on_progress: Callback for progress updates (receives current, total)
        """
        job = CopyJob(
            files=files,
            target_folder=target_folder,
            custom_name=custom_name,
            on_complete=on_complete,
            on_error=on_error,
            on_progress=on_progress
        )

        self.active_jobs.append(job)
        job.start()

    def get_active_job_count(self) -> int:
        """Get number of active copy jobs."""
        return len([job for job in self.active_jobs if job.is_alive()])


class CopyJob(threading.Thread):
    """Individual copy job running in a separate thread."""

    def __init__(
        self,
        files: list[Path],
        target_folder: Path,
        custom_name: str,
        on_complete: Callable[[str], None] | None = None,
        on_error: Callable[[str, str], None] | None = None,
        on_progress: Callable[[int, int], None] | None = None
    ) -> None:
        super().__init__(daemon=True)
        self.files: list[Path] = files
        self.target_folder: Path = target_folder
        self.custom_name: str = custom_name
        self.on_complete: Callable[[str], None] | None = on_complete
        self.on_error: Callable[[str, str], None] | None = on_error
        self.on_progress: Callable[[int, int], None] | None = on_progress
        self.error: str | None = None

    def run(self) -> None:
        """Execute the copy operation."""
        try:
            # Create main folder
            main_folder = self.target_folder / self.custom_name
            main_folder.mkdir(parents=True, exist_ok=True)

            # Group files by extension
            files_by_ext: defaultdict[str, list[Path]] = defaultdict(list)
            for file_path in self.files:
                ext = file_path.suffix.lower().lstrip('.')
                if not ext:  # Handle files without extension
                    ext = 'no_extension'
                files_by_ext[ext].append(file_path)

            # Copy files
            total_files = len(self.files)
            current_file = 0

            for ext, ext_files in files_by_ext.items():
                # Create extension subfolder
                ext_folder = main_folder / ext
                ext_folder.mkdir(exist_ok=True)

                # Copy each file
                for file_path in ext_files:
                    current_file += 1

                    # Report progress
                    if self.on_progress:
                        self.on_progress(current_file, total_files)

                    # Copy file
                    dest_path = ext_folder / file_path.name

                    # Handle duplicate names
                    counter = 1
                    original_dest = dest_path
                    while dest_path.exists():
                        dest_path = original_dest.with_stem(
                            f"{original_dest.stem}_{counter}"
                        )
                        counter += 1

                    shutil.copy2(file_path, dest_path)

            # Success callback
            if self.on_complete:
                self.on_complete(self.custom_name)

        except Exception as e:
            self.error = str(e)
            if self.on_error:
                self.on_error(self.custom_name, str(e))
