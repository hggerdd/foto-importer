"""Real Tk review actions exercise the service through its worker boundary."""

import threading
import time
import tkinter as tk

import numpy as np
from PIL import Image

from core.collection_index import CollectionIndex
from core.face_models import Detection, InputSpec, ModelRegistry
from core.face_service import FaceService
from ui.face_controller import FaceController
from ui.face_view import FaceView


def pump(root, condition):
    deadline = time.monotonic() + 10
    while not condition() and time.monotonic() < deadline:
        root.update()
        time.sleep(0.005)
    assert condition()


def test_cluster_assignment_suggestions_ignore_and_person_details(
    tmp_path, tk_root, monkeypatch
):
    thread = threading.get_ident()
    worker_threads = []

    class Model:
        input_spec = InputSpec()

        def detect(self, pixels):
            worker_threads.append(threading.get_ident())
            return [
                Detection(
                    np.array([10, 10, 110, 115]),
                    0.99,
                    np.array([[35, 45], [85, 45], [60, 65], [40, 90], [80, 90]]),
                )
            ]

        def embed(self, faces):
            return np.tile(np.array([1, 0, 0, 0], np.float32), (len(faces), 1))

    monkeypatch.setattr(ModelRegistry, "load", lambda *args: Model())
    pixels = np.random.default_rng(7).integers(0, 256, (128, 128, 3), dtype=np.uint8)
    Image.fromarray(pixels).save(tmp_path / "one.jpg")
    with CollectionIndex.open(tmp_path) as index:
        index.quick_scan()
    window = tk.Toplevel(tk_root)
    window.withdraw()
    controller = FaceController(window, lambda: tmp_path)
    view = FaceView(window, controller)
    view.pack(fill="both", expand=True)
    try:
        controller.run("scan")
        pump(window, lambda: not controller.busy)
        assert len(view.snapshot["rows"]) == 1
        assert worker_threads and all(t != thread for t in worker_threads)
        monkeypatch.setattr(
            "ui.face_view.simpledialog.askstring", lambda *a, **k: "Testperson"
        )
        view.new_person()
        pump(window, lambda: not controller.busy)
        assert view.selected_person() == 1
        face = view.snapshot["rows"][0]["id"]
        view.selected[face].set(True)
        view.assign()
        pump(window, lambda: not controller.busy)
        assert not view.snapshot["rows"]
        view.mode.set("Person")
        view.reload()
        pump(window, lambda: not controller.busy)
        assert view.snapshot["rows"][0]["assignment"] == "user"
        view.person_details()
        view.configure_models()
        window.update()
        assert (
            len(
                [
                    child
                    for child in view.winfo_children()
                    if isinstance(child, tk.Toplevel)
                ]
            )
            == 2
        )
        view.face_action("ignore")
        pump(window, lambda: not controller.busy)
        assert not view.snapshot["rows"]
        with CollectionIndex.open(tmp_path, readonly=True) as index:
            assert FaceService(index).face_rows()[0]["ignored"] == 1
    finally:
        controller.close()
        window.destroy()


def test_target_change_clears_old_faces_and_handles_destroyed_view(tmp_path, tk_root):
    with CollectionIndex.open(tmp_path):
        pass
    window = tk.Toplevel(tk_root)
    window.withdraw()
    controller = FaceController(window, lambda: tmp_path)
    view = FaceView(window, controller)
    try:
        controller.run()
        pump(window, lambda: not controller.busy)
        controller.target_changed()
        pump(window, lambda: not controller.busy)
        view.destroy()
        controller.target_changed()
    finally:
        controller.close()
        window.destroy()
