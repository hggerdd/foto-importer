"""Shared subprocess fixtures for converter tests."""
import subprocess
import sys

import pytest


@pytest.fixture
def fake_converter(tmp_path, monkeypatch):
    """Launch a real child process with Adobe's argument shape, without RAWs."""
    script = tmp_path / "fake converter.py"
    script.write_text('''
from pathlib import Path
import sys
import time

assert sys.argv[1] == "-d"
destination, source = Path(sys.argv[2]), Path(sys.argv[3])
mode = source.read_text()
source.with_suffix(".started").touch()
if mode == "hang":
    while True:
        time.sleep(0.05)
if mode == "fail":
    print("Unsupported camera", flush=True)
    sys.exit(7)
if mode != "missing":
    (destination / (source.stem + ".dng")).write_bytes(
        b"" if mode == "empty" else b"converted " + source.name.encode()
    )
''', encoding="utf-8")
    executable = tmp_path / "Adobe DNG Converter.exe"
    executable.touch()
    launch = subprocess.Popen
    processes, commands = [], []

    def popen(command, **kwargs):
        commands.append(command)
        process = launch([sys.executable, str(script), *command[1:]], **kwargs)
        processes.append(process)
        return process

    monkeypatch.setattr("core.dng_worker.subprocess.Popen", popen)
    return executable, processes, commands
