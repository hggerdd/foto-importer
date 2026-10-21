"""Daily cache persistence, offline behavior and worker-to-Tk delivery."""
from datetime import date, timedelta
from pathlib import Path
import threading
import time
import tkinter as tk

import pytest

from config.settings import Settings
from ui.dng_update_controller import DngUpdateController


def wait_for(root, condition):
    deadline = time.monotonic() + 3
    while not condition() and time.monotonic() < deadline:
        root.update()
        time.sleep(0.005)
    assert condition()


def test_daily_check_survives_restart(tmp_path, monkeypatch, tk_root):
    calls, results = [], []
    ui_thread = threading.get_ident()

    def fetch():
        assert threading.get_ident() != ui_thread
        calls.append(True)
        return '18.7'

    def show(text, update):
        assert threading.get_ident() == ui_thread
        results.append((text, update))

    monkeypatch.setattr('ui.dng_update_controller.latest_version', fetch)
    config_file = str(tmp_path / 'settings.json')
    for _ in range(2):
        controller = DngUpdateController(tk_root, Settings(config_file), show)
        previous = len(results)
        try:
            controller.start(Path('converter.exe'))
            wait_for(tk_root, lambda: len(results) > previous)
        finally:
            controller.close()
    assert len(calls) == 1
    assert all(update and '18.7' in text and '17.1' in text for text, update in results)


@pytest.mark.parametrize('failure', [OSError('offline'), ValueError('changed URL')])
def test_failed_attempt_is_not_repeated_today(tmp_path, monkeypatch, tk_root, failure):
    settings = Settings(str(tmp_path / 'settings.json'))
    settings.set('dng_update_check', {
        'date': (date.today() - timedelta(days=1)).isoformat(), 'latest': '18.7',
    })
    calls, results = [], []

    def fetch():
        calls.append(True)
        raise failure

    monkeypatch.setattr('ui.dng_update_controller.latest_version', fetch)
    controller = DngUpdateController(tk_root, settings, lambda *args: results.append(args))
    try:
        controller.start(Path('converter.exe'))
        wait_for(tk_root, lambda: len(results) == 1)
        controller.start(Path('converter.exe'))
        wait_for(tk_root, lambda: len(results) == 2)
        assert len(calls) == 1
        assert 'unavailable' in results[-1][0]
        assert results[-1][1] is False
    finally:
        controller.close()


def test_next_day_refreshes_cache(tmp_path, monkeypatch, tk_root):
    settings = Settings(str(tmp_path / 'settings.json'))
    settings.set('dng_update_check', {
        'date': (date.today() - timedelta(days=1)).isoformat(), 'latest': '17.1',
    })
    results = []
    controller = DngUpdateController(tk_root, settings, lambda *args: results.append(args))
    try:
        controller.start(Path('converter.exe'))
        wait_for(tk_root, lambda: bool(results))
        assert results[0][1] is True
        assert settings.get('dng_update_check') == {
            'date': date.today().isoformat(), 'latest': '18.7',
        }
    finally:
        controller.close()


@pytest.mark.parametrize('installed, latest, expected', [
    ('18.7.0.0', '18.7', 'Up to date'),
    ('18.10', '18.7', 'Up to date'),
    (None, '18.7', 'not found'),
    ('17.1', 'invalid', 'unavailable'),
])
def test_cached_status(tmp_path, monkeypatch, tk_root, installed, latest, expected):
    settings = Settings(str(tmp_path / 'settings.json'))
    settings.set('dng_update_check', {'date': date.today().isoformat(), 'latest': latest})
    monkeypatch.setattr('ui.dng_update_controller.installed_version', lambda path: installed)
    results = []
    controller = DngUpdateController(tk_root, settings, lambda *args: results.append(args))
    try:
        controller.start(Path('converter.exe'))
        wait_for(tk_root, lambda: bool(results))
        assert expected in results[0][0]
        assert results[0][1] is False
    finally:
        controller.close()


def test_close_during_network_check(tmp_path, monkeypatch, tk_root):
    started, release = threading.Event(), threading.Event()
    results = []

    def fetch():
        started.set()
        release.wait(3)
        return '18.7'

    monkeypatch.setattr('ui.dng_update_controller.latest_version', fetch)
    controller = DngUpdateController(
        tk_root, Settings(str(tmp_path / 'settings.json')), lambda *args: results.append(args),
    )
    controller.start(Path('converter.exe'))
    try:
        assert started.wait(1)
        controller.close()
    finally:
        release.set()
    tk_root.update()
    assert not results
