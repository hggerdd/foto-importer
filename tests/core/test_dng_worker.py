import time

import pytest

from core.copy_worker import JobState
from core.dng_worker import DngJob


def make_folder(tmp_path, name="album"):
    folder = tmp_path / name
    (folder / "nef").mkdir(parents=True)
    return folder


def wait_for(predicate):
    deadline = time.monotonic() + 5
    while not predicate() and time.monotonic() < deadline:
        time.sleep(0.01)
    assert predicate()


def test_convert_and_skip_existing_preserving_raws(tmp_path, fake_converter):
    executable, processes, commands = fake_converter
    folder = make_folder(tmp_path, "Urlaub & schöne Fotos")
    source = folder / "nef" / "Foto & schön.NEF"
    source.write_text("ok")
    (folder / "nef" / "old.nef").write_text("ok")
    (folder / "OLD.DNG").write_bytes(b"existing")
    (folder / "nef" / "ignore.jpg").touch()
    progress, finished = [], []
    job = DngJob(folder, executable, lambda *args: progress.append(args),
                 lambda *args: finished.append(args))
    job.start()
    job.join(timeout=5)
    assert not job.is_alive()
    assert job.state is JobState.COMPLETED
    assert len(processes) == 1
    assert commands[0][0] == str(executable)
    assert commands[0][-1] == str(source)
    assert (folder / "Foto & schön.dng").read_bytes().startswith(b"converted")
    assert (folder / "OLD.DNG").read_bytes() == b"existing"
    assert source.read_text() == "ok"
    assert progress == [(0, 2), (1, 2), (2, 2)]
    assert "1 existing DNGs skipped" in finished[0][1]
    assert not list(folder.glob(".dng-*"))


@pytest.mark.parametrize("mode", ["fail", "missing", "empty"])
def test_converter_failure_never_publishes_partial_output(tmp_path, fake_converter, mode):
    executable, _, _ = fake_converter
    folder = make_folder(tmp_path)
    (folder / "nef" / "photo.nef").write_text(mode)
    finished = []
    job = DngJob(folder, executable, lambda *_: None,
                 lambda *args: finished.append(args))
    job.start()
    job.join(timeout=5)
    assert not job.is_alive()
    assert job.state is JobState.FAILED
    assert "photo.nef" in finished[0][1]
    assert not (folder / "photo.dng").exists()
    assert not list(folder.glob(".dng-*"))


def test_cancel_one_folder_leaves_other_process_running(tmp_path, fake_converter):
    executable, processes, _ = fake_converter
    folders = [make_folder(tmp_path, name) for name in ("one", "two")]
    jobs = []
    for folder in folders:
        (folder / "nef" / "photo.nef").write_text("hang")
        job = DngJob(folder, executable, lambda *_: None, lambda *_: None)
        jobs.append(job)
    try:
        for job in jobs:
            job.start()
        wait_for(lambda: all((folder / "nef" / "photo.started").exists()
                             for folder in folders))
        assert len({process.pid for process in processes}) == 2
        jobs[0].cancel()
        jobs[0].join(timeout=5)
        assert jobs[0].state is JobState.CANCELLED
        assert jobs[1].is_alive()
        assert sum(process.poll() is None for process in processes) == 1
    finally:
        for job in jobs:
            job.cancel()
            job.join(timeout=5)
    assert all(process.poll() is not None for process in processes)
    assert all(not list(folder.glob(".dng-*")) for folder in folders)


def test_missing_executable_and_no_nefs(tmp_path):
    folder = make_folder(tmp_path)
    job = DngJob(folder, tmp_path / "missing.exe", lambda *_: None, lambda *_: None)
    job.run()
    assert job.state is JobState.COMPLETED
    (folder / "nef" / "photo.nef").touch()
    job.run()
    assert job.state is JobState.FAILED


def test_cancel_before_start_does_not_launch_converter(tmp_path, fake_converter):
    executable, processes, _ = fake_converter
    folder = make_folder(tmp_path)
    (folder / "nef" / "photo.nef").touch()
    job = DngJob(folder, executable, lambda *_: None, lambda *_: None)
    assert job.cancel()
    job.run()
    assert job.state is JobState.CANCELLED
    assert not processes
