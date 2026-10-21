"""Inspect the installed converter and Adobe's current Windows release."""
from __future__ import annotations

import ctypes
from ctypes import wintypes
import os
from pathlib import Path
import re
from urllib.parse import urlsplit
from urllib.request import Request, urlopen

from config.settings import DNG_DOWNLOAD_URL, DNG_UPDATE_TIMEOUT_SECONDS


def version_tuple(version: str) -> tuple[int, ...]:
    """Parse a numeric version with padding for reliable comparisons."""
    if not re.fullmatch(r"\d+(?:\.\d+){1,3}", version):
        raise ValueError("Unrecognized converter version")
    parts = tuple(map(int, version.split('.')))
    return parts + (0,) * (4 - len(parts))


def installed_version(converter: Path) -> str | None:
    """Read Windows executable metadata without launching the converter."""
    if os.name != 'nt' or not converter.is_file():
        return None
    api = ctypes.WinDLL('version', use_last_error=True)
    api.GetFileVersionInfoSizeW.argtypes = [wintypes.LPCWSTR, ctypes.POINTER(wintypes.DWORD)]
    api.GetFileVersionInfoSizeW.restype = wintypes.DWORD
    api.GetFileVersionInfoW.argtypes = [wintypes.LPCWSTR, wintypes.DWORD,
                                       wintypes.DWORD, ctypes.c_void_p]
    api.GetFileVersionInfoW.restype = wintypes.BOOL
    api.VerQueryValueW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR,
                                  ctypes.POINTER(ctypes.c_void_p),
                                  ctypes.POINTER(wintypes.UINT)]
    api.VerQueryValueW.restype = wintypes.BOOL
    size = api.GetFileVersionInfoSizeW(str(converter), None)
    if not size:
        return None
    data = ctypes.create_string_buffer(size)
    if not api.GetFileVersionInfoW(str(converter), 0, size, data):
        return None
    pointer, length = ctypes.c_void_p(), wintypes.UINT()
    if not api.VerQueryValueW(data, '\\', ctypes.byref(pointer), ctypes.byref(length)):
        return None
    # VS_FIXEDFILEINFO consists of 13 DWORDs; product version is at indexes 4/5.
    if length.value < 13 * ctypes.sizeof(wintypes.DWORD):
        return None
    info = ctypes.cast(pointer, ctypes.POINTER(wintypes.DWORD))
    if info[0] != 0xFEEF04BD:
        return None
    parts = [info[4] >> 16, info[4] & 0xFFFF, info[5] >> 16, info[5] & 0xFFFF]
    while len(parts) > 2 and parts[-1] == 0:
        parts.pop()
    return '.'.join(map(str, parts))


def latest_version() -> str:
    """Follow Adobe's HEAD redirect without downloading the installer."""
    request = Request(DNG_DOWNLOAD_URL, method='HEAD', headers={
        'Cache-Control': 'no-cache',
    })
    with urlopen(request, timeout=DNG_UPDATE_TIMEOUT_SECONDS) as response:
        return version_from_download_url(response.geturl())


def version_from_download_url(url: str) -> str:
    """Reject unexpected redirects rather than reporting an invented version."""
    parsed = urlsplit(url)
    match = re.fullmatch(
        r'/pub/adobe/dng/win/AdobeDNGConverter_(?:x64|arm64)_(\d+(?:_\d+){1,3})\.exe',
        parsed.path, re.IGNORECASE,
    )
    if parsed.scheme != 'https' or parsed.hostname != 'download.adobe.com' or not match:
        raise ValueError("Adobe download link format changed")
    return match[1].replace('_', '.')
