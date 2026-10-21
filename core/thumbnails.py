"""Load small, detached thumbnails with a bounded memory cache."""
from __future__ import annotations

from collections import OrderedDict
from pathlib import Path

from PIL import Image, ImageOps

from config.settings import THUMBNAIL_CACHE_SIZE


class ThumbnailLoader:
    """Cache thumbnails for one worker; invalidate when a file changes."""

    def __init__(self, capacity: int = THUMBNAIL_CACHE_SIZE) -> None:
        self.capacity = capacity
        self._cache: OrderedDict[tuple, Image.Image] = OrderedDict()

    def load(self, path: Path, size: tuple[int, int]) -> Image.Image:
        """Read and resize an image, closing its source file before returning."""
        stat = path.stat()
        key = (path, size, stat.st_mtime_ns, stat.st_size, stat.st_ctime_ns)
        if key in self._cache:
            self._cache.move_to_end(key)
            return self._cache[key].copy()

        with Image.open(path) as image:
            # JPEG decoders can skip most pixels when only a thumbnail is needed.
            # Use a square so EXIF rotation also fits the requested dimensions.
            edge = max(size)
            image.draft("RGB", (edge, edge))
            image = ImageOps.exif_transpose(image)
            image.thumbnail(size, Image.Resampling.LANCZOS)
            thumbnail = image.copy()

        if self.capacity > 0:
            self._cache[key] = thumbnail
            while len(self._cache) > self.capacity:
                self._cache.popitem(last=False)
        return thumbnail.copy()
