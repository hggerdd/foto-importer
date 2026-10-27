"""Read Windows file IDs in directory batches instead of opening every file."""

from __future__ import annotations

import ctypes
import os
import struct
from pathlib import Path


def directory_file_ids(folder: Path) -> dict[str, int]:
    """Return volume-local IDs via FILE_ID_BOTH_DIR_INFO (Windows only).

    GetFileInformationByHandleEx resumes directory enumeration in 64 KiB batches.
    The temporary mapping covers just one folder and is discarded by the caller.
    Unsupported filesystems raise OSError so callers can use DirEntry.inode().
    """
    if os.name != "nt":
        raise OSError("Windows directory IDs unavailable")
    from ctypes import wintypes

    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    create = kernel.CreateFileW
    create.argtypes = [
        wintypes.LPCWSTR,
        wintypes.DWORD,
        wintypes.DWORD,
        ctypes.c_void_p,
        wintypes.DWORD,
        wintypes.DWORD,
        wintypes.HANDLE,
    ]
    create.restype = wintypes.HANDLE
    query = kernel.GetFileInformationByHandleEx
    query.argtypes = [wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
    query.restype = wintypes.BOOL
    close = kernel.CloseHandle
    close.argtypes = [wintypes.HANDLE]
    close.restype = wintypes.BOOL
    # FILE_LIST_DIRECTORY, share read/write/delete, OPEN_EXISTING, BACKUP_SEMANTICS.
    handle = create(str(folder), 1, 7, None, 3, 0x02000000, None)
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    result = {}
    buffer = ctypes.create_string_buffer(65536)
    try:
        while query(handle, 10, buffer, len(buffer)):
            offset = 0
            data = buffer.raw
            while True:
                following = struct.unpack_from("<I", buffer, offset)[0]
                length = struct.unpack_from("<I", buffer, offset + 60)[0]
                file_id = struct.unpack_from("<Q", buffer, offset + 96)[0]
                name = data[offset + 104 : offset + 104 + length].decode("utf-16-le")
                result[name] = file_id
                if not following:
                    break
                offset += following
        error = ctypes.get_last_error()
        if error != 18:  # ERROR_NO_MORE_FILES
            raise ctypes.WinError(error)
    finally:
        close(handle)
    return result
