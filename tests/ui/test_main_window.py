import time
import tkinter as tk

from PIL import Image
import pytest

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


@pytest.mark.parametrize("mode", ["ok", "fail"])
def test_copy_then_dng_preserves_successful_copy(
    tmp_path, monkeypatch, tk_root, fake_converter, mode,
):
    executable, _, _ = fake_converter
    settings = Settings(str(tmp_path / "settings.json"))
    settings.set('convert_to_dng', True)
    settings.set('dng_converter_path', str(executable))
    monkeypatch.setattr("ui.main_window.Settings", lambda: settings)
    source, target = tmp_path / "photo.NEF", tmp_path / "target"
    source.write_text(mode)
    root = tk.Toplevel(tk_root)
    root.withdraw()
    app = MainWindow(root)
    errors = []
    monkeypatch.setattr("ui.main_window.messagebox.showerror", lambda *args: errors.append(args))
    try:
        app.file_manager.files_by_date = {"2026-10-21": [source]}
        app._populate_date_list()
        app.date_list.listbox.selection_set(0)
        app.target_folder = target
        app.custom_name_var.set("album")
        app._execute_copy()
        # A change to the selected target must not redirect the in-flight job.
        app.target_folder = tmp_path / "other target"
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            root.update()
            entry = app.progress_manager.progress_bars.get("DNG: album")
            if entry and str(entry.cancel_button.cget("text")) == "Dismiss":
                break
            time.sleep(0.005)
        else:
            pytest.fail("Conversion result did not reach the UI")
        assert (target / "album" / "nef" / "photo.NEF").read_text() == mode
        assert (target / "album" / "photo.dng").exists() == (mode == "ok")
        assert ("completed" if mode == "ok" else "failed") in entry.label.cget("text")
        assert not app._pending_groups
        assert app.date_list.is_empty()
        assert not errors
        entry.cancel_button.invoke()
        assert not app.progress_manager.has_progress_bar("DNG: album")
    finally:
        app._on_closing()
