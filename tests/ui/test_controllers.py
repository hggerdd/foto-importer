from threading import Thread, get_ident
import time

from core.copy_worker import CopyWorker
from core.file_manager import FileManager
from ui.controllers import CopyJobController, SourceScanController
from ui.dispatcher import UiDispatcher
from ui.dng_controller import DngController


class FakeRoot:
    """Reject any attempt to access Tk from a worker."""

    def __init__(self):
        self.owner = get_ident()
        self.pending = {}
        self.sequence = 0

    def after(self, delay, callback):
        assert get_ident() == self.owner
        self.sequence += 1
        self.pending[self.sequence] = callback
        return self.sequence

    def after_cancel(self, token):
        assert get_ident() == self.owner
        self.pending.pop(token, None)

    def report_callback_exception(self, *args):
        raise AssertionError(args)

    def poll(self):
        pending, self.pending = self.pending, {}
        for callback in pending.values():
            callback()


def test_progress_is_coalesced_and_completion_order_preserved():
    root = FakeRoot()
    dispatcher = UiDispatcher(root)
    received = []

    def worker():
        for current in range(10000):
            dispatcher.post(received.append, current, key="progress")
        dispatcher.post(received.append, "done")

    thread = Thread(target=worker)
    thread.start()
    thread.join(timeout=5)
    assert not thread.is_alive()
    assert not received
    root.poll()
    assert received == [9999, "done"]
    dispatcher.close()
    dispatcher.post(received.append, "too late")
    assert not root.pending


def test_scan_callbacks_run_on_ui_thread_and_ignore_obsolete_results(tmp_path):
    (tmp_path / "photo.jpg").touch()
    root, manager = FakeRoot(), FileManager()
    controller = SourceScanController(root, manager)
    success, errors, progress = [], [], []
    for _ in range(2):
        controller.scan(
            tmp_path, lambda: None, lambda *args: success.append(args),
            errors.append, on_progress=lambda *args: progress.append(args),
        )
        controller._active_thread.join(timeout=5)
        assert not controller._active_thread.is_alive()
    root.poll()
    assert len(success) == 1
    assert not errors
    assert progress == [(1, 1)]
    assert manager.source_folder == tmp_path
    controller.close()


def test_copy_callbacks_run_on_ui_thread(tmp_path):
    class Progress:
        def __init__(self):
            self.values = []

        def add_progress_bar(self, *args, **kwargs):
            pass

        def update_progress(self, *args):
            assert get_ident() == root.owner
            self.values.append(args)

        def remove_progress_bar(self, *args):
            pass

    source = tmp_path / "photo.jpg"
    source.write_bytes(b"data")
    root, worker, progress = FakeRoot(), CopyWorker(), Progress()
    controller = CopyJobController(root, worker, progress)
    completed, errors = [], []
    controller.start_job(
        "album", [source] * 5, tmp_path / "target",
        on_status=lambda _: None, on_completed=completed.append,
        on_failed=lambda *args: errors.append(args),
        on_cancelled=lambda _: None,
    )
    # A completed worker may already have removed itself from the registry.
    for job in worker._registry.jobs().values():
        job.join(timeout=5)
        assert not job.is_alive()
    root.poll()
    assert completed == ["album"]
    assert not errors
    assert progress.values[-1] == ("album", 5, 5)
    controller.close()


def test_dng_callbacks_run_on_ui_thread_and_keep_result(tmp_path, fake_converter):
    class Progress:
        def __init__(self):
            self.values, self.results = [], []

        def add_progress_bar(self, *args, **kwargs):
            assert get_ident() == root.owner

        def remove_progress_bar(self, *args):
            assert get_ident() == root.owner

        def update_progress(self, *args):
            assert get_ident() == root.owner
            self.values.append(args)

        def finish_progress(self, *args):
            assert get_ident() == root.owner
            self.results.append(args)

    executable, _, _ = fake_converter
    folder = tmp_path / "album"
    (folder / "nef").mkdir(parents=True)
    (folder / "nef" / "photo.nef").write_text("ok")
    root, progress = FakeRoot(), Progress()
    controller = DngController(root, progress)
    controller.start_job("album", folder, executable, lambda _: None)
    job = controller._jobs["album"]
    job.join(timeout=5)
    assert not job.is_alive()
    assert not progress.values
    assert controller.is_active("album")
    root.poll()
    assert not controller.is_active("album")
    assert progress.values[-1] == ("DNG: album", 1, 1)
    assert "completed" in progress.results[0][1]
    controller.close()


def test_closing_dng_controller_reaps_child_without_ui_callbacks(tmp_path, fake_converter):
    class Progress:
        def add_progress_bar(self, *args, **kwargs):
            pass

        def remove_progress_bar(self, *args):
            pass

        def update_progress(self, *args):
            raise AssertionError("UI update after close")

        def finish_progress(self, *args):
            raise AssertionError("UI update after close")

    executable, processes, _ = fake_converter
    folder = tmp_path / "album"
    (folder / "nef").mkdir(parents=True)
    (folder / "nef" / "photo.nef").write_text("hang")
    root = FakeRoot()
    controller = DngController(root, Progress())
    controller.start_job("album", folder, executable, lambda _: None)
    job = controller._jobs["album"]
    try:
        deadline = time.monotonic() + 5
        while not (folder / "nef" / "photo.started").exists():
            assert time.monotonic() < deadline
            time.sleep(0.01)
        assert not job.daemon  # Interpreter shutdown must allow cleanup.
    finally:
        controller.close()
        job.join(timeout=5)
    assert not job.is_alive()
    assert all(process.poll() is not None for process in processes)
    root.poll()
    assert not root.pending
    assert not list(folder.glob(".dng-*"))
