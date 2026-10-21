import gc
import tkinter as tk

import pytest


@pytest.fixture(scope="session")
def tk_root():
    """Use a single Tcl interpreter, owned and finalized by the test thread."""
    gc.collect()
    root = tk.Tk()
    root.withdraw()
    yield root
    root.destroy()
