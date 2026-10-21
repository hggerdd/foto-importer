"""Version comparisons and Adobe's download redirect contract."""
from pathlib import Path

import pytest

from core.dng_updates import (
    installed_version, latest_version, version_from_download_url, version_tuple,
)


def test_numeric_versions_and_padding():
    assert version_tuple('18.10') > version_tuple('18.7')
    assert version_tuple('17.1') == version_tuple('17.1.0.0')
    assert version_tuple('18.7.1') > version_tuple('18.7')
    with pytest.raises(ValueError):
        version_tuple('unknown')


@pytest.mark.parametrize('url', [
    'https://example.org/pub/adobe/dng/win/AdobeDNGConverter_x64_18_7.exe',
    'https://download.adobe.com/unexpected.exe',
    'http://download.adobe.com/pub/adobe/dng/win/AdobeDNGConverter_x64_18_7.exe',
])
def test_unexpected_download_url(url):
    with pytest.raises(ValueError):
        version_from_download_url(url)


def test_latest_uses_head_without_reading_installer(monkeypatch):
    class Response:
        def __enter__(self):
            return self

        def __exit__(self, *args):
            pass

        def geturl(self):
            return ('https://download.adobe.com/pub/adobe/dng/win/'
                    'AdobeDNGConverter_x64_18_7_1.exe')

    def open_request(request, timeout):
        assert request.get_method() == 'HEAD'
        assert 0 < timeout <= 10
        return Response()

    monkeypatch.setattr('core.dng_updates.urlopen', open_request)
    assert latest_version() == '18.7.1'


def test_missing_converter(tmp_path):
    assert installed_version(tmp_path / 'missing.exe') is None


def test_file_without_version_metadata(tmp_path):
    executable = tmp_path / 'fake.exe'
    executable.write_text('not an executable')
    assert installed_version(executable) is None
