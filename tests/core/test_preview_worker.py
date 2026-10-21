"""One active decoder and one replaceable request keep scrolling bounded."""

from pathlib import Path
from threading import Event, get_ident

from PIL import Image

from core.preview_worker import PreviewWorker
from core.task_scheduler import TaskScheduler


def test_scrolling_replaces_pending_work_and_releases_priority():
    started, release, finished = Event(), Event(), Event()
    owner = get_ident()
    loaded, results = [], []
    scheduler = TaskScheduler(settle_seconds=0)

    class Loader:
        def load(self, path, size):
            assert get_ident() != owner
            loaded.append(path.name)
            if path.name == "old.jpg":
                started.set()
                assert release.wait(3)
            return Image.new("RGB", size)

    def receive(generation, index, thumbnail):
        results.append((generation, index))
        finished.set()

    worker = PreviewWorker(Loader(), scheduler)
    try:
        worker.request(1, [(0, Path("old.jpg"))], (20, 10), receive)
        assert started.wait(1)
        for index in range(1, 101):
            worker.request(1, [(index, Path(f"{index}.jpg"))], (20, 10), receive)
        assert worker._pending.paths == [(100, Path("100.jpg"))]
        assert scheduler._foreground == 2
        release.set()
        assert finished.wait(2)
    finally:
        release.set()
        worker.close()
        worker._thread.join(2)
    assert loaded == ["old.jpg", "100.jpg"]
    assert results == [(1, 100)]
    assert scheduler._foreground == 0
    assert not worker._thread.is_alive()


def test_closing_cancels_active_and_pending_results_without_waiting():
    started, release = Event(), Event()
    results = []
    scheduler = TaskScheduler(settle_seconds=0)

    class Loader:
        def load(self, path, size):
            started.set()
            assert release.wait(3)
            return Image.new("RGB", size)

    worker = PreviewWorker(Loader(), scheduler)
    try:
        worker.request(
            1, [(0, Path("old.jpg"))], (20, 10), lambda *args: results.append(args)
        )
        assert started.wait(1)
        worker.request(
            2, [(1, Path("new.jpg"))], (20, 10), lambda *args: results.append(args)
        )
        worker.close()
        assert worker._thread.is_alive()  # close returned while I/O is still blocked.
        assert scheduler._foreground == 1
    finally:
        release.set()
        worker.close()
        worker._thread.join(2)
    assert not results
    assert scheduler._foreground == 0
