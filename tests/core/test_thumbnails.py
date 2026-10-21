from PIL import Image
import pytest

from core.thumbnails import ThumbnailLoader


def test_cache_hit_avoids_decoding_and_returns_independent_image(tmp_path, monkeypatch):
    path = tmp_path / "photo.jpg"
    Image.new("RGB", (2000, 1000), "red").save(path)
    loader = ThumbnailLoader()
    first = loader.load(path, (150, 150))
    assert first.size == (150, 75)
    first.paste("blue", (0, 0, 150, 75))
    monkeypatch.setattr(Image, "open", lambda *_: pytest.fail("Cache should avoid decoding"))
    second = loader.load(path, (150, 150))
    assert second.getpixel((0, 0))[0] > 240


def test_cache_is_bounded_and_invalidates_changed_files(tmp_path):
    loader = ThumbnailLoader(capacity=1)
    first, second = tmp_path / "a.png", tmp_path / "b.png"
    Image.new("RGB", (20, 10), "red").save(first)
    Image.new("RGB", (30, 10), "blue").save(second)
    assert loader.load(first, (100, 100)).size == (20, 10)
    assert loader.load(second, (100, 100)).size == (30, 10)
    assert len(loader._cache) == 1
    Image.new("RGB", (40, 10), "green").save(second)
    assert loader.load(second, (100, 100)).size == (40, 10)


def test_exif_rotation(tmp_path):
    path = tmp_path / "rotated.jpg"
    exif = Image.Exif()
    exif[274] = 6
    Image.new("RGB", (200, 100)).save(path, exif=exif)
    assert ThumbnailLoader().load(path, (100, 100)).size == (50, 100)
