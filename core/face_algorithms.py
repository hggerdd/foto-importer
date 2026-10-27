"""Model-isolated matching, conservative clustering and balanced references."""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from typing import Protocol

import numpy as np
from scipy.optimize import linear_sum_assignment


class VectorSearch(Protocol):
    def similarities(self, queries: np.ndarray) -> np.ndarray: ...


class NumpySearch:
    def __init__(self, references: np.ndarray) -> None:
        self.references = references

    def similarities(self, queries: np.ndarray) -> np.ndarray:
        return queries @ self.references.T


def match_photo(
    queries: np.ndarray,
    references: np.ndarray,
    people: list[int],
    rejections: list[set[int]],
    occupied: set[int],
    auto: float,
    suggest: float,
    eligible: list[bool],
    forbidden=None,
    priors=None,
) -> list[tuple]:
    """Optimize top-three person scores jointly, with an unknown slot per face."""
    result = [(None, None, None)] * len(queries)
    if not len(queries) or not len(references):
        return result
    similarities = NumpySearch(references).similarities(queries)
    all_ids, inverse, counts = np.unique(
        people, return_inverse=True, return_counts=True
    )
    available = np.flatnonzero(~np.isin(all_ids, list(occupied)))
    ids = all_ids[available].tolist()
    scores = np.full((len(queries), len(ids) + len(queries)), suggest - 1e-6)
    ends = np.cumsum(counts) - 1
    # Sort scores within person groups once per query, then gather the last
    # three. Avoid an O(persons * references) boolean scan for every person.
    for row, similarities_row in enumerate(similarities):
        ordered = similarities_row[np.lexsort((similarities_row, inverse))]
        totals = ordered[ends].copy()
        for k in (1, 2):
            totals += np.where(counts > k, ordered[np.maximum(0, ends - k)], 0)
        scores[row, : len(ids)] = (totals / np.minimum(counts, 3))[available]
    for col, person in enumerate(ids):
        for row in range(len(queries)):
            if priors and suggest - 0.03 <= scores[row, col] < auto:
                scores[row, col] = min(
                    auto - 1e-6, scores[row, col] + priors.get(person, 0)
                )
            if (
                not eligible[row]
                or person in rejections[row]
                or (forbidden and person in forbidden[row])
            ):
                scores[row, col] = -2
    rows, cols = linear_sum_assignment(scores, maximize=True)
    for row, col in zip(rows, cols):
        if col < len(ids) and scores[row, col] >= suggest:
            score = float(scores[row, col])
            result[row] = (ids[col], "auto" if score >= auto else "suggested", score)
    return result


def cluster_unknown(rows: list[dict], threshold: float = 0.6) -> list[list[int]]:
    """Greedy complete-link cosine clusters; never put two faces of a photo together.

    Complete-link avoids transitive chains joining different people. Cluster IDs
    are intentionally ephemeral and consist only of the returned face IDs.
    """
    clusters: list[list[dict]] = []
    for row in sorted(rows, key=lambda r: r["id"]):
        for group in clusters:
            if any(item["photo_id"] == row["photo_id"] for item in group):
                continue
            if (
                min(float(row["vector"] @ item["vector"]) for item in group)
                >= threshold
            ):
                group.append(row)
                break
        else:
            clusters.append([row])
    return [[item["id"] for item in group] for group in clusters]


@dataclass
class ReferenceSelection:
    ref: list[int]
    val: list[int]
    outliers: list[int]


def _time(value: str | None) -> datetime | None:
    if not value:
        return None
    try:
        return datetime.fromisoformat(value[:10].replace(":", "-") + value[10:19])
    except ValueError:
        return None


def select_references(
    rows: list[dict],
    limit: int = 100,
    duplicate: float = 0.92,
    validation_fraction: float = 0.2,
    seed: int = 42,
    outlier_threshold: float = 0.2,
) -> ReferenceSelection:
    """Deterministic quality-aware FPS, year quotas, pins and day-separated validation."""
    rows = sorted(rows, key=lambda r: r["id"])
    if not rows:
        return ReferenceSelection([], [], [])
    vectors = np.array([r["vector"] for r in rows])
    # Cosine medoid via summed similarities, without an M x M matrix; then
    # exact nearest neighbours in blocks for outlier/duplicate detection.
    center = vectors.sum(axis=0)
    medoid = int(np.argmax(vectors @ center))
    medoid_scores = vectors @ vectors[medoid]
    nearest = np.full(len(rows), -1.0)
    for start in range(0, len(rows), 128):
        block = vectors[start : start + 128] @ vectors.T
        for offset in range(len(block)):
            block[offset, start + offset] = -1
        k = min(3, max(1, len(rows) - 1))
        nearest[start : start + len(block)] = np.partition(block, -k, axis=1)[
            :, -k:
        ].mean(axis=1)
    pins = [i for i, r in enumerate(rows) if r.get("pinned")]
    if len(pins) > limit:
        raise ValueError("Mehr fixierte Gesichter als das Referenzlimit")
    outliers = [
        i
        for i in range(len(rows))
        if len(rows) >= 4
        and medoid_scores[i] < outlier_threshold
        and nearest[i] < outlier_threshold
        and i not in pins
    ]
    times = [_time(r.get("captured_at")) for r in rows]
    keep = list(pins)
    for i in sorted(
        range(len(rows)), key=lambda i: (-rows[i]["quality"], rows[i]["id"])
    ):
        if i in pins or i in outliers:
            continue
        if any(
            float(vectors[i] @ vectors[j]) > duplicate
            or (
                times[i] and times[j] and abs((times[i] - times[j]).total_seconds()) < 2
            )
            for j in keep
        ):
            continue
        keep.append(i)
    chosen = list(pins)
    distances = np.full(len(rows), 2.0)
    for i in chosen:
        distances = np.minimum(distances, 1 - vectors @ vectors[i])
    years = sorted({times[i].year if times[i] else 0 for i in keep})
    quota = max(1, limit // max(1, len(years)))

    def pick(candidates):
        if not chosen:
            return max(candidates, key=lambda i: (medoid_scores[i], -rows[i]["id"]))
        return max(
            candidates,
            key=lambda i: (
                distances[i] * (0.5 + 0.5 * rows[i]["quality"]),
                -rows[i]["id"],
            ),
        )

    def add(i):
        chosen.append(i)
        np.minimum(distances, 1 - vectors @ vectors[i], out=distances)

    # Round-robin years prevents earlier buckets consuming all available slots.
    for _ in range(quota):
        for year in years:
            options = [
                i
                for i in keep
                if i not in chosen and (times[i].year if times[i] else 0) == year
            ]
            if options and len(chosen) < limit:
                add(pick(options))
    while len(chosen) < min(limit, len(keep)):
        add(pick([i for i in keep if i not in chosen]))
    # Never withhold every example of a person; need at least one reference.
    count = (
        min(max(2, round(len(chosen) * validation_fraction)), len(chosen) - 1)
        if len(chosen) >= 3 and validation_fraction > 0
        else 0
    )
    rng = np.random.default_rng(seed)
    order = list(rng.permutation(chosen))
    val, days = [], set()
    for i in order:
        day = times[i].date() if times[i] else None
        if day not in days and len(val) < count:
            val.append(i)
            days.add(day)
    # If dates cannot be separated, still produce a reproducible validation set.
    val += [i for i in order if i not in val][: count - len(val)]
    return ReferenceSelection(
        [rows[i]["id"] for i in chosen if i not in val],
        [rows[i]["id"] for i in val],
        [rows[i]["id"] for i in outliers],
    )
