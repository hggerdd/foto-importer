"""Exercise cooperative pauses with real copy bytes and converter processes."""

from pathlib import Path
from threading import Event
import time

from core.copy_worker import CopyJob, JobState
from core.dng_worker import DngJob
from core.task_scheduler import TaskScheduler


def test_copy_pauses_inside_a_large_file_then_resumes(tmp_path, monkeypatch):
    import core.copy_worker as copy_module

    monkeypatch.setattr(copy_module, "BACKGROUND_COPY_CHUNK_BYTES", 64)
    source = tmp_path / "photo.jpg"
    source.write_bytes(bytes(range(256)) * 4)
    scheduler = TaskScheduler(settle_seconds=0)
    first_chunk, release_write, paused = Event(), Event(), Event()
    original = Path.open
    written = []

    class SlowOutput:
        def __init__(self, handle):
            self.handle = handle

        def __enter__(self):
            return self

        def __exit__(self, *args):
            self.handle.close()

        def write(self, chunk):
            result = self.handle.write(chunk)
            written.append(len(chunk))
            if len(written) == 1:
                first_chunk.set()
                assert release_write.wait(3)
            return result

    def open_file(path, mode="r", *args, **kwargs):
        handle = original(path, mode, *args, **kwargs)
        return SlowOutput(handle) if mode == "xb" else handle

    monkeypatch.setattr(Path, "open", open_file)
    job = CopyJob(
        [source],
        tmp_path / "target",
        "album",
        scheduler=scheduler,
        on_waiting=lambda reason: paused.set() if reason else None,
    )
    lease = None
    job.start()
    try:
        assert first_chunk.wait(1)
        lease = scheduler.foreground()
        release_write.set()
        assert paused.wait(1)
        assert written == [64]  # No next block while interactive work owns the gate.
        lease.close()
        job.join(3)
        assert job.state is JobState.COMPLETED
        assert (
            tmp_path / "target" / "album" / "jpg" / source.name
        ).read_bytes() == source.read_bytes()
        assert (
            tmp_path / "target" / "album" / "jpg" / source.name
        ).stat().st_mtime_ns == source.stat().st_mtime_ns
    finally:
        release_write.set()
        if lease:
            lease.close()
        job.cancel()
        job.join(3)


def test_cancel_during_chunk_pause_removes_only_partial_copy(tmp_path, monkeypatch):
    source = tmp_path / "photo.jpg"
    source.write_bytes(b"photo" * 100)
    scheduler = TaskScheduler(settle_seconds=0)
    paused = Event()
    original_checkpoint = scheduler.checkpoint
    lease = None
    calls = 0

    def checkpoint(cancel, on_waiting=None):
        nonlocal calls, lease
        calls += 1
        # Gate after creating the output: do not depend on byte timings.
        destination = tmp_path / "target" / "album" / "jpg" / "photo.jpg"
        if lease is None and destination.exists():
            lease = scheduler.foreground()
        return original_checkpoint(cancel, on_waiting)

    monkeypatch.setattr(scheduler, "checkpoint", checkpoint)
    job = CopyJob(
        [source],
        tmp_path / "target",
        "album",
        scheduler=scheduler,
        on_waiting=lambda reason: paused.set() if reason else None,
    )
    job.start()
    try:
        assert paused.wait(2)
        assert job.cancel()
        job.join(2)
        assert job.state is JobState.CANCELLED
        assert not (tmp_path / "target" / "album" / "jpg" / "photo.jpg").exists()
        assert source.read_bytes() == b"photo" * 100
    finally:
        job.cancel()
        if lease:
            lease.close()
        job.join(2)


def test_queued_copy_can_cancel_without_creating_target(tmp_path):
    scheduler = TaskScheduler(settle_seconds=0)
    lease = scheduler.foreground()
    waiting = Event()
    job = CopyJob(
        [],
        tmp_path / "target",
        "album",
        scheduler=scheduler,
        on_waiting=lambda reason: waiting.set() if reason else None,
    )
    job.start()
    try:
        assert waiting.wait(1)
        job.cancel()
        job.join(2)
        assert job.state is JobState.CANCELLED
        assert not (tmp_path / "target").exists()
    finally:
        lease.close()
        job.join(2)


def test_converter_waits_for_analysis_and_only_one_folder_runs(
    tmp_path, fake_converter
):
    executable, processes, _ = fake_converter
    scheduler = TaskScheduler(settle_seconds=0)
    lease = scheduler.foreground()
    jobs, waiting = [], [Event(), Event()]
    for index in range(2):
        folder = tmp_path / str(index)
        (folder / "nef").mkdir(parents=True)
        (folder / "nef" / "photo.nef").write_text("hang")
        jobs.append(
            DngJob(
                folder,
                executable,
                lambda *_: None,
                lambda *_: None,
                scheduler=scheduler,
                on_waiting=lambda reason, i=index: waiting[i].set() if reason else None,
            )
        )
    try:
        for job in jobs:
            job.start()
        assert all(event.wait(1) for event in waiting)
        assert not processes
        lease.close()
        deadline = time.monotonic() + 3
        while not processes and time.monotonic() < deadline:
            time.sleep(0.005)
        assert len(processes) == 1
        jobs[0].cancel()
        jobs[1].cancel()
    finally:
        lease.close()
        for job in jobs:
            job.cancel()
            job.join(3)
    assert all(not job.is_alive() for job in jobs)
    assert all(process.poll() is not None for process in processes)


def test_new_foreground_work_prevents_next_adobe_file(tmp_path, fake_converter):
    executable, processes, _ = fake_converter
    scheduler = TaskScheduler(settle_seconds=0)
    folder = tmp_path / "album"
    (folder / "nef").mkdir(parents=True)
    for name in ("first", "second"):
        (folder / "nef" / f"{name}.nef").write_text("ok")
    paused = Event()
    leases = []

    def progress(current, total):
        if current == 1:
            leases.append(scheduler.foreground())

    job = DngJob(
        folder,
        executable,
        progress,
        lambda *_: None,
        scheduler=scheduler,
        on_waiting=lambda reason: paused.set() if reason else None,
    )
    job.start()
    try:
        assert paused.wait(3)
        assert len(processes) == 1
        assert (folder / "first.dng").exists()
        assert not (folder / "second.dng").exists()
        leases[0].close()
        job.join(3)
        assert job.state is JobState.COMPLETED
        assert len(processes) == 2
    finally:
        for lease in leases:
            lease.close()
        job.cancel()
        job.join(3)
