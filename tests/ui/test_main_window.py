import time
import tkinter as tk

from PIL import Image

from config.settings import Settings
from ui.main_window import MainWindow


def test_scan_preview_copy_and_close(tmp_path, monkeypatch, tk_root):
    """Exercise real Tk widgets and worker callbacks with isolated settings."""
    settings = Settings(str(tmp_path / "settings.json"))
    monkeypatch.setattr("ui.main_window.Settings", lambda: settings)
    source, target = tmp_path / "source", tmp_path / "target"
    source.mkdir()
    photo = source / "photo.jpg"
    Image.new("RGB", (200, 100), "red").save(photo)
    root = tk.Toplevel(tk_root)
    root.withdraw()
    app = MainWindow(root)
    errors = []
    monkeypatch.setattr("ui.main_window.messagebox.showerror", lambda *args: errors.append(args))

    def wait_for(condition):
        deadline = time.monotonic() + 5
        while not condition() and time.monotonic() < deadline:
            root.update()
            time.sleep(0.005)
        assert condition()

    try:
        app._on_source_selected(source)
        wait_for(lambda: app.date_list.listbox.size() == 1)
        app.date_list.listbox.selection_set(0)
        app._on_date_selection_changed(app.date_list.get_selected_dates())
        wait_for(lambda: len(app.preview_widget.image_references) == 1)
        app._on_target_selected(target)
        app.custom_name_var.set("album")
        assert str(app.execute_button.cget("state")) == "normal"
        app._execute_copy()
        wait_for(lambda: not app._pending_groups)
        assert (target / "album" / "jpg" / photo.name).read_bytes() == photo.read_bytes()
        assert app.date_list.is_empty()
        assert not errors
    finally:
        app._on_closing()
    assert (tmp_path / "settings.json").exists()
