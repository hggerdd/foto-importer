"""Real Tk review actions exercise the service through its worker boundary."""

import json
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
        face = view.snapshot["rows"][0]["id"]
        row = view.group_rows[0]
        assert row.selected_ids() == [face]
        row.name.set("Testperson")
        row._enter()
        pump(window, lambda: not controller.busy)
        assert not view.snapshot["rows"]
        assert controller.can_undo
        controller.undo_naming()
        pump(window, lambda: not controller.busy)
        assert len(view.group_rows) == 1
        assert not view.snapshot["persons"]
        view.group_rows[0].name.set("Testperson")
        view.group_rows[0].submit()
        pump(window, lambda: not controller.busy)
        view.person.set(next(iter(view.people)))
        assert view.selected_person() is not None
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
        view.selected[face].set(True)
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


def populate_groups(root, sizes):
    count = sum(sizes)
    for i in range(count):
        Image.new("RGB", (80, 80), "#d5ac87").save(root / f"{i:04}.jpg")
    with CollectionIndex.open(root) as index:
        index.quick_scan()
        service = FaceService(index)
        service.create_person({"name": "Anna"})
        files = list(index.db.execute("SELECT * FROM files ORDER BY id"))
        offset = 0
        for group, size in enumerate(sizes):
            vector = np.zeros(128, dtype="<f4")
            vector[group] = 1
            for file in files[offset : offset + size]:
                with index.db:
                    face = index.db.execute(
                        """INSERT INTO faces(photo_id,source_file_id,source_relpath,detector_id,
                        bbox_x1,bbox_y1,bbox_x2,bbox_y2,quality,captured_at,landmarks)
                        VALUES(?,?,?,'test',.1,.1,.9,.9,.9,'2020:01:01 12:00:00',?)""",
                        (
                            file["photo_id"],
                            file["id"],
                            file["relpath"],
                            json.dumps(
                                [
                                    [0.3, 0.3],
                                    [0.7, 0.3],
                                    [0.5, 0.5],
                                    [0.3, 0.7],
                                    [0.7, 0.7],
                                ]
                            ),
                        ),
                    ).lastrowid
                    index.db.execute(
                        "INSERT INTO face_embeddings VALUES(?,?,128,?)",
                        (face, service.embedder_key, vector.tobytes()),
                    )
            offset += size


def test_large_group_outliers_autocomplete_enter_next_focus_and_undo(tmp_path, tk_root):
    populate_groups(tmp_path, [70, 3])
    window = tk.Toplevel(tk_root)
    window.geometry("1450x950")
    controller = FaceController(window, lambda: tmp_path)
    view = FaceView(window, controller)
    view.pack(fill="both", expand=True)
    try:
        controller.run()
        pump(window, lambda: not controller.busy)
        assert (
            len(view.group_rows) == 2
        )  # A group must not be split at the old 60-face limit.
        row = view.group_rows[0]
        assert len(row.faces) == 70
        assert sum(bool(face.get("image")) for face in row.faces) == 8
        excluded = row.faces[0]["id"]
        row.toggle(excluded)
        row.name.set("An")
        row.entry.focus_force()
        window.update()
        assert row.choices.winfo_manager() == "pack"
        assert row.matches[0]["attrs"]["name"] == "Anna"
        view.preview_group(row, 8)
        pump(window, lambda: not controller.busy)
        row = view.group_rows[0]
        assert excluded not in row.selected_ids()
        assert row.name.get() == "An"
        assert row.offset == 8
        assert all(row.faces[i].get("image") is not None for i in range(8, 16))
        row.entry.focus_force()
        window.update()
        row.entry.event_generate("<KeyPress-Down>")
        window.update()
        row.entry.event_generate("<KeyPress-Return>")
        pump(window, lambda: not controller.busy)
        assert len(view.snapshot["persons"]) == 1
        assert len(view.snapshot["rows"]) == 4
        assert view._active_key == view.group_rows[1].key
        with CollectionIndex.open(tmp_path, readonly=True) as index:
            service = FaceService(index)
            assert service.face_rows("f.id=?", (excluded,))[0]["person_id"] is None
            assert (
                index.db.execute(
                    "SELECT count(*) FROM faces WHERE assignment='user'"
                ).fetchone()[0]
                == 69
            )
        controller.undo_naming()
        pump(window, lambda: not controller.busy)
        assert len(view.group_rows[0].faces) == 70
        assert not controller.can_undo
        assert view._active_key == view.group_rows[0].key
        with CollectionIndex.open(tmp_path, readonly=True) as index:
            assert (
                index.db.execute(
                    "SELECT count(*) FROM faces WHERE person_id IS NOT NULL"
                ).fetchone()[0]
                == 0
            )
    finally:
        controller.close()
        window.destroy()


def test_group_pagination_keeps_drafts_and_tab_moves_between_names(tmp_path, tk_root):
    populate_groups(tmp_path, [1] * 10)
    window = tk.Toplevel(tk_root)
    window.geometry("1450x950")
    controller = FaceController(window, lambda: tmp_path)
    view = FaceView(window, controller)
    view.pack(fill="both", expand=True)
    try:
        controller.run()
        pump(window, lambda: not controller.busy)
        assert len(view.group_rows) == 8
        row = view.group_rows[0]
        row.name.set("Otto")
        row.entry.focus_force()
        window.update()
        row.entry.event_generate("<KeyPress-Tab>")
        window.update()
        assert view._active_key == view.group_rows[1].key
        assert window.focus_get() == view.group_rows[1].entry
        view.turn_page(1)
        pump(window, lambda: not controller.busy)
        assert len(view.group_rows) == 2
        assert view.snapshot["pages"] == 2
        view.turn_page(-1)
        pump(window, lambda: not controller.busy)
        assert view.group_rows[0].name.get() == "Otto"
    finally:
        controller.close()
        window.destroy()


def test_review_groups_prefill_names_confirm_selection_and_undo(tmp_path, tk_root):
    populate_groups(tmp_path, [4])
    with CollectionIndex.open(tmp_path) as index:
        FaceService(index).assign([1], 1)
    window = tk.Toplevel(tk_root)
    window.withdraw()
    controller = FaceController(window, lambda: tmp_path)
    view = FaceView(window, controller)
    view.pack(fill="both", expand=True)
    view.mode.set("Vorschläge")
    try:
        controller.run()
        pump(window, lambda: not controller.busy)
        row = view.group_rows[0]
        assert row.review and row.name.get() == "Anna" and row.person_id == 1
        assert len(row.faces) == 3
        row.toggle(row.faces[0]["id"])
        row._enter()
        pump(window, lambda: not controller.busy)
        assert not view.snapshot["rows"]
        with CollectionIndex.open(tmp_path, readonly=True) as index:
            assert (
                index.db.execute(
                    "SELECT count(*) FROM faces WHERE assignment='user'"
                ).fetchone()[0]
                == 3
            )
            assert (
                index.db.execute(
                    "SELECT count(*) FROM faces WHERE person_id IS NULL"
                ).fetchone()[0]
                == 1
            )
        controller.undo_naming()
        pump(window, lambda: not controller.busy)
        assert len(view.group_rows[0].faces) == 3
        with CollectionIndex.open(tmp_path, readonly=True) as index:
            assert (
                index.db.execute(
                    "SELECT count(*) FROM faces WHERE assignment='user'"
                ).fetchone()[0]
                == 1
            )
    finally:
        controller.close()
        window.destroy()
