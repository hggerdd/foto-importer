"""Validate Windows directory buffers against real filesystem identities."""

import os

import pytest

from core.windows_file_ids import directory_file_ids


@pytest.mark.skipif(os.name != "nt", reason="Windows file-ID API")
def test_batched_ids_match_stat_across_buffers(tmp_path):
    for number in range(600):
        (tmp_path / f"Wörthersee_{number:04}_{'x' * 60}.jpg").touch()
    ids = directory_file_ids(tmp_path)
    paths = list(tmp_path.iterdir())
    assert len(ids) >= 600
    assert all(ids[path.name] == path.stat().st_ino for path in paths)
