import gc
import tkinter as tk

import pytest


@pytest.fixture(autouse=True)
def offline_dng_update_check(monkeypatch):
    """Keep UI tests independent of Adobe availability and local installs."""
    monkeypatch.setattr('ui.dng_update_controller.installed_version', lambda path: '17.1')
    monkeypatch.setattr('ui.dng_update_controller.latest_version', lambda: '18.7')


@pytest.fixture(scope="session")
def tk_root():
    """Use a single Tcl interpreter, owned and finalized by the test thread."""
    gc.collect()
    root = tk.Tk()
    root.withdraw()
    yield root
    root.destroy()
