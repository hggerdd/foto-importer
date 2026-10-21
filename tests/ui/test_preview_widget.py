from threading import Event, get_ident
import time

import pytest
from PIL import Image

from ui.preview_widget import PreviewWidget


@pytest.fixture
def root(tk_root):
    return tk_root


def pump_until(root, condition):
    deadline = time.monotonic() + 5
    while not condition() and time.monotonic() < deadline:
        root.update()
        time.sleep(0.005)
    assert condition(), "UI callback did not arrive"


def test_loading_does_not_block_ui_and_discards_stale_results(root, tmp_path, monkeypatch):
    widget = PreviewWidget(root)
    owner = get_ident()
    started, release = Event(), Event()

    def slow_load(path, size):
        assert get_ident() != owner
        if path.name == "old.jpg":
            started.set()
            assert release.wait(timeout=5)
        return Image.new("RGB", (10, 10))

    monkeypatch.setattr(widget._loader, "load", slow_load)
    try:
        widget.load_previews([tmp_path / "old.jpg"])
        assert started.wait(timeout=5)
        heartbeat = []
        root.after(0, lambda: heartbeat.append(True))
        root.update()
        assert heartbeat  # Decoder is still blocked, while Tk remains responsive.
        widget.load_previews([tmp_path / "new.jpg"])
        release.set()
        pump_until(root, lambda: len(widget.image_references) == 1)
        labels = widget.scrollable_frame.winfo_children()[0].winfo_children()
        assert labels[-1].cget("text") == "new.jpg"
        widget.clear_previews()
        assert not widget.image_references
    finally:
        release.set()
        widget.destroy()


def test_corrupt_image_does_not_stop_remaining_previews(root, tmp_path):
    bad, good = tmp_path / "bad.jpg", tmp_path / "good.jpg"
    bad.write_bytes(b"invalid")
    Image.new("RGB", (20, 20)).save(good)
    widget = PreviewWidget(root)
    widget.load_previews([bad, good])
    pump_until(root, lambda: len(widget.scrollable_frame.winfo_children()) == 2)
    assert len(widget.image_references) == 1
    widget.destroy()
