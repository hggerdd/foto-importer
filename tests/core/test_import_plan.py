"""Import summaries and Windows destination name validation."""

from pathlib import Path
from threading import Event

import pytest

from core.import_plan import folder_name_error, summarize_selection


def test_checked_days_and_focused_preview_are_independent():
    groups = {
        "2026-10-20": [Path("a.JPG"), Path("b.nef"), Path("c.MOV")],
        "2026-10-19": [Path("d.jpg"), Path("e.JPG"), Path("f.nef")],
    }
    result = summarize_selection(groups, {"2026-10-20"}, "2026-10-19", 1, Event())
    assert result.counts == {"jpg": 1, "nef": 1, "mov": 1}
    assert result.focused_counts == {"jpg": 2, "nef": 1}
    assert result.previews == [Path("d.jpg")]
    assert result.preview_total == 2


def test_summary_cancellation_and_empty_selection():
    cancel = Event()
    cancel.set()
    assert (
        summarize_selection({"day": [Path("a.jpg")]}, {"day"}, None, 12, cancel) is None
    )
    result = summarize_selection({}, set(), None, 12, Event())
    assert result.counts == result.focused_counts == {}
    assert result.previews == []


@pytest.mark.parametrize(
    "name",
    [
        "",
        "..",
        "../escape",
        "C:\\elsewhere",
        "a/b",
        "a:b",
        "CON",
        "nul.jpg",
        "COM9",
        "trailing.",
        "trailing ",
        "line\nbreak",
    ],
)
def test_invalid_folder_names(name):
    assert folder_name_error(name)


@pytest.mark.parametrize(
    "name", ["2026-10-20_Seeausflug", "Urlaub & Familie", "Frühling 2026"]
)
def test_valid_folder_names(name):
    assert folder_name_error(name) is None
