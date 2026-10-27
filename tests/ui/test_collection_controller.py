"""Exercise real Tk collection controls and import filtering with worker queues."""

import time
import tkinter as tk

from config.settings import Settings
from core.collection_index import CollectionIndex
from ui.collection_controller import CollectionController
from ui.date_list_widget import DateListWidget


def pump(root, condition):
    deadline = time.monotonic() + 10
    while not condition() and time.monotonic() < deadline:
        root.update()
        time.sleep(0.005)
    assert condition()


def test_collection_scan_report_and_missing_confirmation(
    tmp_path, tk_root, monkeypatch
):
    root = tk.Toplevel(tk_root)
    root.withdraw()
    settings = Settings(str(tmp_path / "settings.json"))
    collection = tmp_path / "collection"
    collection.mkdir()
    (collection / "one.jpg").write_bytes(b"one")
    (collection / "two.jpg").write_bytes(b"two")
    answers = iter([True, True])  # first indexing, then high missing percentage
    monkeypatch.setattr(
        "ui.collection_controller.messagebox.askyesno",
        lambda *args, **kwargs: next(answers),
    )
    controller = CollectionController(root, settings, lambda: collection)
    try:
        controller.open_window()
        controller.run("quick_scan")
        pump(root, lambda: not controller.busy)
        assert "Neu: 2" in controller.report.get("1.0", "end")
        assert "2 Dateien" in controller.status_var.get()
        (collection / "one.jpg").unlink()
        controller.run("quick_scan")
        pump(root, lambda: not controller.busy)
        assert "Fehlend: 1" in controller.report.get("1.0", "end")
        assert "1 fehlend" in controller.status_var.get()
    finally:
        controller.window.destroy()
        controller.close()
        root.destroy()


def test_import_filter_and_scoped_update(tmp_path, tk_root, monkeypatch):
    root = tk.Toplevel(tk_root)
    root.withdraw()
    collection = tmp_path / "collection"
    collection.mkdir()
    (collection / "one.jpg").write_bytes(b"one")
    card = tmp_path / "one.jpg"
    card.write_bytes(b"one")
    new = tmp_path / "new.jpg"
    new.write_bytes(b"different")
    monkeypatch.setattr(
        "core.collection_index.read_metadata",
        lambda paths, *_: [{"SourceFile": str(path)} for path in paths],
    )
    with CollectionIndex.open(collection) as index:
        index.quick_scan()
    settings = Settings(str(tmp_path / "settings.json"))
    controller = CollectionController(root, settings, lambda: collection)
    result = []
    try:
        controller.check_import(collection, [card, new], result.append)
        pump(root, lambda: bool(result))
        assert result == [[new]]
        folder = collection / "new"
        folder.mkdir()
        (folder / new.name).write_bytes(new.read_bytes())
        controller.after_copy(collection, folder)
        pump(root, lambda: not controller.busy)
        with CollectionIndex.open(collection) as index:
            assert index.status()["files"] == 2
        settings.set("collection_skip_imported", False)
        result.clear()
        controller.check_import(collection, [card], result.append)
        pump(root, lambda: bool(result))
        assert result == [[card]]
    finally:
        controller.close()
        root.destroy()


def test_day_import_counts_and_dimming(tk_root):
    widget = DateListWidget(tk_root)
    try:
        widget.populate({"2026-10-20": 2, "2026-10-21": 3})
        widget.set_imported_counts({"2026-10-20": 2, "2026-10-21": 1})
        assert "2 von 2 importiert" in widget.tree.item("2026-10-20", "text")
        assert widget.tree.item("2026-10-20", "tags") == ("imported",)
        assert not widget.tree.item("2026-10-21", "tags")
    finally:
        widget.destroy()
