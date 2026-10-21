from pathlib import Path

from core.copy_worker import CopyJob, JobState


def test_duplicates_preserve_existing_files_and_report_completed_copies(tmp_path, monkeypatch):
    source = tmp_path / "photo.jpg"
    source.write_bytes(b"new")
    target = tmp_path / "target"
    destination = target / "album" / "jpg"
    destination.mkdir(parents=True)
    (destination / "photo.jpg").write_bytes(b"original")
    (destination / "photo_2.jpg").write_bytes(b"existing")
    progress = []
    checks = []
    exists = Path.exists

    def count_exists(path):
        checks.append(path)
        return exists(path)

    def on_progress(current, total):
        assert len(list(destination.iterdir())) == current + 2
        progress.append((current, total))

    monkeypatch.setattr(Path, "exists", count_exists)
    job = CopyJob([source] * 20, target, "album", on_progress=on_progress)
    job.run()
    assert job.state is JobState.COMPLETED
    assert (destination / "photo.jpg").read_bytes() == b"original"
    assert (destination / "photo_2.jpg").read_bytes() == b"existing"
    assert (destination / "photo_21.jpg").read_bytes() == b"new"
    assert progress[-1] == (20, 20)
    assert len(checks) < 50  # Linear lookup rather than restarting at suffix 1.


def test_cancellation_after_first_copy(tmp_path):
    source = tmp_path / "photo.jpg"
    source.write_bytes(b"photo")
    job = CopyJob([source] * 3, tmp_path / "target", "album")
    job.on_progress = lambda *_: job.cancel()
    job.run()
    assert job.state is JobState.CANCELLED
    assert len(list((tmp_path / "target" / "album" / "jpg").iterdir())) == 1


def test_copy_failure_does_not_report_success(tmp_path):
    errors, progress = [], []
    job = CopyJob(
        [tmp_path / "missing.jpg"], tmp_path / "target", "album",
        on_error=lambda *args: errors.append(args),
        on_progress=lambda *args: progress.append(args),
    )
    job.run()
    assert job.state is JobState.FAILED
    assert errors
    assert not progress
