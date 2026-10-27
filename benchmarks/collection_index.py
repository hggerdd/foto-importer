"""Benchmark real inventory scans on an isolated generated collection."""

import argparse
import json
import tempfile
import time
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from core.collection_index import CollectionIndex


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--files", type=int, default=100_000)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="foto-index-benchmark-") as temporary:
        root = Path(temporary)
        for offset in range(args.files):
            folder = root / f"{offset // 1000:03}"
            folder.mkdir(exist_ok=True)
            (folder / f"DSC_{offset:06}.jpg").write_bytes(b"benchmark")
        with CollectionIndex.open(root) as index:
            results = {}
            for label in ("initial", "unchanged"):
                start = time.perf_counter()
                result = index.quick_scan()
                elapsed = time.perf_counter() - start
                results[label] = dict(
                    seconds=elapsed,
                    files_per_second=args.files / elapsed,
                    status=result.status,
                    stats=result.stats,
                )
            print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
